//! Where the stack guard goes and what the Cortex-M4 MPU is programmed with.
//!
//! The stack grows down from the top of RAM towards the statics, so an
//! overflow would run into `.bss` and corrupt it without a trace. The board
//! shell (`src/stack.rs`) makes the lowest bytes of the stack region a
//! no-access MPU region instead: the first push into it faults, and the
//! HardFault handler reports the overflow while every static is still intact.
//! This module decides the region and the register values and is
//! hardware-free, so the host tests cover it; ADR 0026 records the design.

/// The ARMv7-M MPU's smallest region (bytes).
const MIN_REGION: u32 = 32;

/// `MPU_RASR.XN`: no instruction fetch from the region.
const RASR_XN: u32 = 1 << 28;
/// `MPU_RASR.C` and `MPU_RASR.B` with `TEX = 0`: normal, write-back memory,
/// as the default map treats SRAM. `MPU_RASR.AP` stays 0: no access at any
/// privilege level.
const RASR_NORMAL_MEMORY: u32 = (1 << 17) | (1 << 16);
/// `MPU_RASR.ENABLE`.
const RASR_ENABLE: u32 = 1;

/// `MPU_CTRL`: the MPU on (`ENABLE`), the default memory map for every
/// address outside the guard (`PRIVDEFENA`), and the MPU off inside the
/// HardFault handler (`HFNMIENA` clear), so the handler can stack its frame
/// and run on the guard's bytes.
pub const MPU_CTRL: u32 = (1 << 2) | 1;

/// A no-access MPU region at the bottom of the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackGuard {
    base: u32,
    size: u32,
}

impl StackGuard {
    /// The guard for a stack whose lowest address is `stack_end`: the
    /// `size`-byte region at the first `size`-aligned address at or above it,
    /// since an MPU region's base must be a multiple of its size. `None` when
    /// `size` is not a power of two of at least 32 bytes, or when the region
    /// would end past the address space.
    pub fn place(stack_end: u32, size: u32) -> Option<Self> {
        if !size.is_power_of_two() || size < MIN_REGION {
            return None;
        }
        let base = stack_end.checked_next_multiple_of(size)?;
        base.checked_add(size)?;
        Some(Self { base, size })
    }

    pub const fn base(&self) -> u32 {
        self.base
    }

    /// The first address above the guard. `place` checked that it fits.
    pub const fn top(&self) -> u32 {
        self.base.wrapping_add(self.size)
    }

    pub const fn size(&self) -> u32 {
        self.size
    }

    /// Whether the guard lies below the stack in use, with stack left before
    /// it: the stack pointer is above the guard's top. A fault taken with the
    /// stack pointer at or below the top is an overflow: on the core, the
    /// frame the fault stacks always lands in the guard then, and an emulator
    /// that does not move the stack pointer for a frame it could not stack
    /// leaves it at the top.
    pub const fn below(&self, stack_pointer: u32) -> bool {
        self.top() < stack_pointer
    }

    /// The `MPU_RBAR` value for the region number already in `MPU_RNR`.
    pub const fn rbar(&self) -> u32 {
        self.base
    }

    /// The `MPU_RASR` value: execute-never, no access, normal memory, the
    /// region's size as `SIZE = log2(size) - 1`, and enabled.
    pub const fn rasr(&self) -> u32 {
        let size_field = self.size.trailing_zeros() - 1;
        RASR_XN | RASR_NORMAL_MEMORY | (size_field << 1) | RASR_ENABLE
    }

    /// Whether read-back MPU registers hold this guard: `MPU_CTRL` as
    /// [`MPU_CTRL`], and region 0's `MPU_RBAR` (whose low five bits are the
    /// `VALID` and `REGION` fields, not address bits) and `MPU_RASR` as this
    /// guard's.
    pub const fn is_programmed(&self, ctrl: u32, rbar: u32, rasr: u32) -> bool {
        ctrl == MPU_CTRL && rbar & !(MIN_REGION - 1) == self.base && rasr == self.rasr()
    }
}

/// Words in an encoded [`OverflowRecord`].
pub const OVERFLOW_RECORD_WORDS: usize = 6;

/// First word of a record whose PC was stacked ("STKP").
const RECORD_WITH_PC: u32 = 0x5354_4B50;
/// First word of a record whose PC was not stacked ("STKN").
const RECORD_WITHOUT_PC: u32 = 0x5354_4B4E;

/// A stack overflow as the HardFault handler saw it. The handler stores it,
/// encoded, in RAM the reset handler leaves alone before it tries to log, so
/// the next boot can report an overflow whose log line never got out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverflowRecord {
    /// The stack pointer the fault was taken with.
    pub stack_pointer: u32,
    /// The guard's lowest address.
    pub guard_base: u32,
    /// The first address above the guard.
    pub guard_top: u32,
    /// The faulting PC, when the fault's frame could be stacked.
    pub pc: Option<u32>,
}

impl OverflowRecord {
    /// The record as stored: a first word that also says whether a PC is
    /// present, the four values (0 for no PC), and a check word, so words
    /// RAM held at power-on are not mistaken for a record.
    pub fn encode(&self) -> [u32; OVERFLOW_RECORD_WORDS] {
        let (tag, pc) = match self.pc {
            Some(pc) => (RECORD_WITH_PC, pc),
            None => (RECORD_WITHOUT_PC, 0),
        };
        let (stack_pointer, base, top) = (self.stack_pointer, self.guard_base, self.guard_top);
        let check = check_word(&[tag, stack_pointer, base, top, pc, 0]);
        [tag, stack_pointer, base, top, pc, check]
    }

    /// The record `words` hold, or `None` when they are not one: a cleared
    /// slot, the RAM's power-on contents, or a record with a changed word.
    pub fn decode(words: [u32; OVERFLOW_RECORD_WORDS]) -> Option<Self> {
        let [tag, stack_pointer, guard_base, guard_top, pc, check] = words;
        let pc = match tag {
            RECORD_WITH_PC => Some(pc),
            RECORD_WITHOUT_PC if pc == 0 => None,
            _ => return None,
        };
        (check == check_word(&words) && guard_base < guard_top).then_some(Self {
            stack_pointer,
            guard_base,
            guard_top,
            pc,
        })
    }
}

/// The inverted XOR of every word but the last, each rotated by five bits per
/// position, so two swapped words do not cancel out.
fn check_word(words: &[u32; OVERFLOW_RECORD_WORDS]) -> u32 {
    !words
        .iter()
        .take(OVERFLOW_RECORD_WORDS - 1)
        .zip(0u32..)
        .fold(0, |check, (word, position)| {
            check ^ word.rotate_left(position * 5)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD: OverflowRecord = OverflowRecord {
        stack_pointer: 0x2000_DFC8,
        guard_base: 0x2000_D000,
        guard_top: 0x2000_E000,
        pc: None,
    };

    #[test]
    fn an_overflow_record_survives_encoding() {
        assert_eq!(OverflowRecord::decode(RECORD.encode()), Some(RECORD));
        let with_pc = OverflowRecord {
            pc: Some(0x0003_1238),
            ..RECORD
        };
        assert_eq!(OverflowRecord::decode(with_pc.encode()), Some(with_pc));
        // A PC of 0 is still a PC.
        let pc_zero = OverflowRecord {
            pc: Some(0),
            ..RECORD
        };
        assert_eq!(OverflowRecord::decode(pc_zero.encode()), Some(pc_zero));
    }

    #[test]
    fn an_overflow_record_has_a_fixed_layout() {
        let words = RECORD.encode();
        assert_eq!(
            words[..5],
            [0x5354_4B4E, 0x2000_DFC8, 0x2000_D000, 0x2000_E000, 0]
        );
        assert_eq!(words[5], check_word(&words));
        assert_eq!(
            OverflowRecord {
                pc: Some(7),
                ..RECORD
            }
            .encode()[0],
            0x5354_4B50
        );
    }

    #[test]
    fn anything_but_an_intact_record_decodes_to_none() {
        assert_eq!(OverflowRecord::decode([0; OVERFLOW_RECORD_WORDS]), None);
        assert_eq!(
            OverflowRecord::decode([u32::MAX; OVERFLOW_RECORD_WORDS]),
            None
        );
        let with_pc = OverflowRecord {
            pc: Some(0x0003_1238),
            ..RECORD
        }
        .encode();
        for words in [RECORD.encode(), with_pc] {
            for index in 0..OVERFLOW_RECORD_WORDS {
                for bit in [0, 13, 31] {
                    let mut changed = words;
                    changed[index] ^= 1 << bit;
                    assert_eq!(
                        OverflowRecord::decode(changed),
                        None,
                        "word {index} bit {bit}"
                    );
                }
            }
        }
        // Two swapped words are caught too, which a plain XOR would miss.
        let mut swapped = RECORD.encode();
        swapped.swap(1, 2);
        assert_eq!(OverflowRecord::decode(swapped), None);
    }

    #[test]
    fn a_record_without_a_pc_must_store_zero_for_it() {
        let mut words = RECORD.encode();
        words[4] = 0x1234;
        words[5] = check_word(&words);
        assert_eq!(OverflowRecord::decode(words), None);
    }

    #[test]
    fn a_record_with_an_empty_guard_is_refused() {
        let empty = OverflowRecord {
            guard_top: RECORD.guard_base,
            ..RECORD
        };
        assert_eq!(OverflowRecord::decode(empty.encode()), None);
    }

    #[test]
    fn the_guard_starts_at_the_first_aligned_address_above_the_statics() {
        let guard = StackGuard::place(0x2000_6E24, 4096).unwrap();
        assert_eq!(guard.base(), 0x2000_7000);
        assert_eq!(guard.top(), 0x2000_8000);
        assert_eq!(guard.size(), 4096);
        // An end already on the boundary keeps it.
        assert_eq!(
            StackGuard::place(0x2000_7000, 4096).unwrap().base(),
            0x2000_7000
        );
    }

    #[test]
    fn only_power_of_two_sizes_of_at_least_32_bytes_are_regions() {
        for size in [0, 1, 16, 31, 48, 3000, 4097] {
            assert_eq!(StackGuard::place(0x2000_0000, size), None, "{size}");
        }
        for size in [32, 64, 1024, 4096] {
            assert!(StackGuard::place(0x2000_0000, size).is_some(), "{size}");
        }
    }

    #[test]
    fn a_region_past_the_address_space_is_refused() {
        assert_eq!(StackGuard::place(0xFFFF_F001, 4096), None);
        assert_eq!(StackGuard::place(0xFFFF_F000, 4096), None);
        assert!(StackGuard::place(0xFFFF_E000, 4096).is_some());
    }

    #[test]
    fn a_stack_pointer_under_the_guard_top_has_overflowed() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        assert!(guard.below(0x2003_FFF0));
        assert!(guard.below(0x2000_8004));
        assert!(
            !guard.below(0x2000_8000),
            "no stack left: the next push faults"
        );
        assert!(!guard.below(0x2000_7FFC), "a frame stacked in the guard");
        assert!(!guard.below(0x2000_7000));
        // A frame larger than what was left can skip past the guard.
        assert!(!guard.below(0x2000_6F00));
    }

    #[test]
    fn the_registers_describe_a_no_access_execute_never_region() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        assert_eq!(guard.rbar(), 0x2000_7000);
        // XN, TEX=0 C=1 B=1, AP=0, SIZE=11 (2^12 bytes), ENABLE.
        assert_eq!(guard.rasr(), 0x1003_0017);
        let smallest = StackGuard::place(0x2000_7000, 32).unwrap();
        assert_eq!(smallest.rasr(), 0x1003_0009);
        assert_eq!(MPU_CTRL, 0b101);
    }

    #[test]
    fn read_back_registers_must_match_the_guard() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        let (rbar, rasr) = (guard.rbar(), guard.rasr());
        assert!(guard.is_programmed(MPU_CTRL, rbar, rasr));
        // The low five bits are not address bits.
        assert!(guard.is_programmed(MPU_CTRL, rbar | 0x10, rasr));
        assert!(!guard.is_programmed(0, rbar, rasr), "MPU off");
        assert!(
            !guard.is_programmed(MPU_CTRL | (1 << 1), rbar, rasr),
            "on in HardFault"
        );
        assert!(!guard.is_programmed(MPU_CTRL, rbar + 4096, rasr), "moved");
        assert!(
            !guard.is_programmed(MPU_CTRL, rbar, rasr & !1),
            "region off"
        );
    }
}
