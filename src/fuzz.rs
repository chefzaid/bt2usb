//! A dependency-free fuzzing harness for the host tests.
//!
//! Each target in [`fuzz_tests`](crate::fuzz_tests) runs a check over its
//! seed corpus (`tests/fuzz-corpus/`), every truncation of each seed, and then
//! inputs mutated from the seeds by a seeded pseudo-random generator. A check
//! panics on any property it finds broken, and Rust's bounds checks turn an
//! out-of-bounds access into a panic as well; either is reported with the
//! target, the case, the input in hex, and the settings that reproduce it.
//! An input that keeps taking longer than [`MAX_INPUT_TIME`] fails too.
//!
//! The search is random, not coverage-guided: the pinned stable toolchain has
//! no sanitizer coverage for `cargo fuzz`. It runs briefly in every test run
//! and at length on the weekly CI schedule with a new seed
//! (docs/testing.md#parser-fuzzing). `BT2USB_FUZZ_CASES` sets the mutated
//! cases per target and `BT2USB_FUZZ_SEED` the seed, in decimal or `0x` hex.

use std::any::Any;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// Mutated cases per target in an ordinary test run.
const DEFAULT_CASES: u64 = 5_000;
/// The seed of an ordinary test run, so it checks the same inputs each time.
const DEFAULT_SEED: u64 = 0x6274_3275_7362;
/// The longest one input may take, judged by the fastest of three reruns so a
/// test thread the OS descheduled does not fail the run. Every target's work
/// is linear in its input, which takes microseconds.
pub const MAX_INPUT_TIME: Duration = Duration::from_millis(100);
/// Byte values at the boundaries the parsers test: zero, small lengths, the
/// sign bit, and the largest values.
const INTERESTING: [u8; 8] = [0x00, 0x01, 0x02, 0x20, 0x7F, 0x80, 0xFE, 0xFF];

/// One parser under test.
pub struct Target {
    pub name: &'static str,
    /// The seed corpus: one input per line in hex, `#` starts a comment.
    pub corpus: &'static str,
    /// The longest input to generate: the most the firmware can be handed.
    pub max_len: usize,
}

/// xorshift64*: fast, deterministic, and good enough to choose mutations.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // The state must never be zero.
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..n`, for a nonzero `n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}

/// Which input of a run failed, to name in the report.
#[derive(Clone, Copy)]
enum Case {
    Seed { seed: usize, length: usize },
    Mutated(u64),
}

impl fmt::Display for Case {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Case::Seed { seed, length } => write!(f, "seed {seed} cut to {length} bytes"),
            Case::Mutated(case) => write!(f, "mutated case {case}"),
        }
    }
}

/// Run `check` over the target's seeds and mutated inputs; `extra` adds
/// seeds the firmware builds itself, such as its own descriptors.
pub fn run(target: &Target, extra: &[&[u8]], check: impl Fn(&[u8])) {
    let seed = setting("BT2USB_FUZZ_SEED").unwrap_or(DEFAULT_SEED);
    let cases = setting("BT2USB_FUZZ_CASES").unwrap_or(DEFAULT_CASES);
    let mut seeds = parse_corpus(target.corpus);
    seeds.extend(extra.iter().map(|input| input.to_vec()));
    assert!(
        !seeds.is_empty(),
        "fuzz target {} has no seeds",
        target.name
    );

    let attempt = |case: Case, input: &[u8]| {
        let started = Instant::now();
        if let Err(cause) = catch_unwind(AssertUnwindSafe(|| check(input))) {
            panic!(
                "fuzz target {} failed on {case}, input [{}]; reproduce with \
                 BT2USB_FUZZ_SEED={seed:#x} BT2USB_FUZZ_CASES={cases}: {}",
                target.name,
                hex(input),
                reason(cause.as_ref()),
            );
        }
        started.elapsed()
    };
    let mut slowest = (Duration::ZERO, Case::Mutated(0), Vec::new());
    let mut time = |case: Case, input: Vec<u8>| {
        let elapsed = attempt(case, &input);
        if elapsed > slowest.0 {
            slowest = (elapsed, case, input);
        }
    };
    for (index, input) in seeds.iter().enumerate() {
        for length in 0..=input.len() {
            time(
                Case::Seed {
                    seed: index,
                    length,
                },
                input[..length].to_vec(),
            );
        }
    }
    let mut rng = Rng::new(seed ^ name_hash(target.name));
    for case in 0..cases {
        time(
            Case::Mutated(case),
            mutate(&mut rng, &seeds, target.max_len),
        );
    }

    let (elapsed, case, input) = slowest;
    if elapsed > MAX_INPUT_TIME {
        let fastest = (0..3).map(|_| attempt(case, &input)).min();
        assert!(
            fastest.is_some_and(|time| time <= MAX_INPUT_TIME),
            "fuzz target {} took {fastest:?} on {case}, input [{}]",
            target.name,
            hex(&input),
        );
    }
    println!(
        "fuzz target {}: {} seeds, {cases} mutated cases, seed {seed:#x}, slowest input {elapsed:?}",
        target.name,
        seeds.len(),
    );
}

/// The inputs in a corpus file.
pub fn parse_corpus(text: &str) -> Vec<Vec<u8>> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .map(|line| line.split_whitespace().collect::<String>())
        .filter(|digits| !digits.is_empty())
        .map(|digits| {
            let nibbles: Vec<u8> = digits
                .chars()
                .map(|digit| digit.to_digit(16).expect("corpus is hex") as u8)
                .collect();
            assert!(
                nibbles.len().is_multiple_of(2),
                "odd number of hex digits: {digits}"
            );
            nibbles
                .chunks(2)
                .map(|pair| pair[0] << 4 | pair[1])
                .collect()
        })
        .collect()
}

/// A new input: usually a seed with a few edits, sometimes random bytes.
fn mutate(rng: &mut Rng, seeds: &[Vec<u8>], max_len: usize) -> Vec<u8> {
    let mut input = if rng.below(16) == 0 {
        (0..rng.below(max_len + 1)).map(|_| rng.byte()).collect()
    } else {
        seeds[rng.below(seeds.len())].clone()
    };
    for _ in 0..=rng.below(8) {
        let len = input.len();
        match rng.below(8) {
            0 if len > 0 => {
                let at = rng.below(len);
                input[at] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let at = rng.below(len);
                input[at] = rng.byte();
            }
            2 if len > 0 => {
                let at = rng.below(len);
                input[at] = INTERESTING[rng.below(INTERESTING.len())];
            }
            3 => input.insert(rng.below(len + 1), rng.byte()),
            4 if len > 0 => {
                input.remove(rng.below(len));
            }
            // Repeat a run of bytes somewhere else, as a duplicated item or
            // structure would.
            5 if len > 0 => {
                let start = rng.below(len);
                let end = start + 1 + rng.below(len - start);
                let at = rng.below(len + 1);
                let copy = input[start..end].to_vec();
                input.splice(at..at, copy);
            }
            6 => input.truncate(rng.below(len + 1)),
            // Splice the tail of another seed onto this one's head.
            _ => {
                let other = &seeds[rng.below(seeds.len())];
                input.truncate(rng.below(len + 1));
                input.extend_from_slice(&other[rng.below(other.len() + 1)..]);
            }
        }
    }
    input.truncate(max_len);
    input
}

/// A number from the environment, in decimal or `0x` hex.
fn setting(name: &str) -> Option<u64> {
    let value = std::env::var(name).ok()?;
    let parsed = match value.strip_prefix("0x") {
        Some(digits) => u64::from_str_radix(digits, 16),
        None => value.parse(),
    };
    Some(parsed.unwrap_or_else(|_| panic!("{name} must be a number, not {value:?}")))
}

/// FNV-1a, so each target draws its own inputs from the shared seed and adding
/// a target leaves the others' inputs unchanged.
fn name_hash(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

fn hex(input: &[u8]) -> String {
    input.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reason(cause: &(dyn Any + Send)) -> &str {
    cause
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| cause.downcast_ref::<&str>().copied())
        .unwrap_or("a panic without a message")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_lines_are_hex_with_comments_and_spacing() {
        let corpus = "# a comment\n0a FF 10\n\n  01 # trailing note\n";
        assert_eq!(
            parse_corpus(corpus),
            vec![vec![0x0A, 0xFF, 0x10], vec![0x01]]
        );
    }

    #[test]
    fn mutated_inputs_are_reproducible_and_bounded() {
        let seeds = vec![vec![1, 2, 3, 4], vec![9; 40]];
        let draw = |seed| {
            let mut rng = Rng::new(seed);
            (0..200)
                .map(|_| mutate(&mut rng, &seeds, 16))
                .collect::<Vec<_>>()
        };
        let inputs = draw(7);
        assert_eq!(inputs, draw(7));
        assert_ne!(inputs, draw(8));
        assert!(inputs.iter().all(|input| input.len() <= 16));
        assert!(inputs.iter().any(|input| !seeds.contains(input)));
    }

    #[test]
    fn a_failing_check_names_the_target_case_and_input() {
        let target = Target {
            name: "example",
            corpus: "00 01\n",
            max_len: 4,
        };
        let failure = catch_unwind(|| run(&target, &[], |input| assert_ne!(input, [0x00])));
        let message = failure.expect_err("the check fails on the seed's first byte");
        let message = reason(message.as_ref());
        assert!(message.contains("fuzz target example failed on seed 0 cut to 1 bytes"));
        assert!(message.contains("input [00]"));
        assert!(message.contains("reproduce with BT2USB_FUZZ_SEED=0x"));
    }
}
