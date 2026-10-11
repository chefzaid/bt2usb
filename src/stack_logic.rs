//! Where the stack guard goes, what the Cortex-M4 MPU is programmed with, and
//! which faults are stack overflows.
//!
//! The stack grows down from the top of RAM towards the statics, so an
//! overflow would run into `.bss` and corrupt it without a trace. The board
//! shell (`src/stack.rs`) puts a no-access MPU region at the bottom of the
//! stack region instead, on the first guard-sized boundary at or above
//! `_stack_end` (the bytes below it are never used): the first push into it
//! faults, and the HardFault handler reports the overflow while the statics
//! are still intact. This module decides the region, the register values,
//! and which faults are overflows, and is hardware-free, so the host tests
//! cover it; ADR 0026 records the design.

/// The ARMv7-M MPU's smallest region (bytes).
const MIN_REGION: u32 = 32;

/// `MPU_RASR.XN`: no instruction fetch from the region.
const RASR_XN: u32 = 1 << 28;
/// `MPU_RASR.C` and `MPU_RASR.B` with `TEX = 0`: normal memory, write-back
/// with no write-allocate. The attributes never take effect: `MPU_RASR.AP`
/// stays 0, so no access is allowed at any privilege level, and the
/// Cortex-M4 has no cache.
const RASR_NORMAL_MEMORY: u32 = (1 << 17) | (1 << 16);
/// `MPU_RASR.ENABLE`.
const RASR_ENABLE: u32 = 1;

/// `MPU_CTRL`: the MPU on (`ENABLE`), the default memory map for every
/// address outside the guard (`PRIVDEFENA`), and the MPU off while the
/// HardFault handler runs (`HFNMIENA` clear), so the handler's own pushes and
/// calls can use the guard's bytes. The fault's frame is pushed before the
/// handler runs, with the MPU still on, so a frame that lands in the guard is
/// not stored (`CFSR.MSTKERR`).
pub const MPU_CTRL: u32 = (1 << 2) | 1;

/// `CFSR.DACCVIOL`: the MPU refused a data access.
const CFSR_DACCVIOL: u32 = 1 << 1;
/// `CFSR.MSTKERR`: the MPU refused the push of an exception's frame.
const CFSR_MSTKERR: u32 = 1 << 4;
/// `CFSR.MMARVALID`: `MMFAR` holds the address of the refused data access.
const CFSR_MMARVALID: u32 = 1 << 7;
/// `CFSR.STKERR`: a bus error refused the push of an exception's frame.
const CFSR_STKERR: u32 = 1 << 12;

/// The number of regions an `MPU_TYPE` value reports (`DREGION`); 0 means
/// the core has no MPU.
pub const fn mpu_regions(mpu_type: u32) -> u32 {
    (mpu_type >> 8) & 0xFF
}

/// Whether an `MPU_RASR` value turns its region on.
pub const fn region_enabled(rasr: u32) -> bool {
    rasr & RASR_ENABLE != 0
}

/// Whether a fault's frame holds the registers of the code that faulted,
/// from the `CFSR` value. When pushing the frame itself faulted (`MSTKERR`
/// in the guard, `STKERR` off the end of RAM), the processor still moved the
/// stack pointer but stored nothing, so the frame's words, the PC among them,
/// are whatever that RAM held before.
pub const fn frame_stacked(cfsr: u32) -> bool {
    cfsr & (CFSR_MSTKERR | CFSR_STKERR) == 0
}

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

    /// Whether `address` is one of the guard's bytes.
    pub const fn contains(&self, address: u32) -> bool {
        self.base <= address && address < self.top()
    }

    /// Whether the guard lies below `address` with stack left before it:
    /// `address` is above the guard's top. `stack::enable_guard` passes the
    /// stack pointer; [`is_overflow`](Self::is_overflow) passes a fault's
    /// frame.
    pub const fn below(&self, address: u32) -> bool {
        self.top() < address
    }

    /// Whether a fault is a stack overflow, from the address of its frame and
    /// the fault status registers (`CFSR`, `MMFAR`): the frame is at or below
    /// the guard's top, or the MPU refused a data access inside the guard.
    ///
    /// On the core, a frame pushed into the guard lies below its top (and was
    /// not stored, `MSTKERR`), and one below the base means a single frame
    /// stepped over the guard. A frame exactly at the top lies wholly above
    /// the guard; it counts for an emulator that leaves the stack pointer
    /// where it was when it cannot push a frame. The refused access catches a
    /// push or store that faults in the guard while the fault's own frame,
    /// 32 bytes below the stack pointer before it, still fits above the
    /// guard, as a store-multiple of 44 bytes or more can.
    pub const fn is_overflow(&self, frame: u32, cfsr: u32, mmfar: u32) -> bool {
        let refused = CFSR_DACCVIOL | CFSR_MMARVALID;
        !self.below(frame) || (cfsr & refused == refused && self.contains(mmfar))
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
    fn the_guard_holds_its_own_bytes_only() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        assert!(guard.contains(0x2000_7000));
        assert!(guard.contains(0x2000_7FFF));
        assert!(!guard.contains(0x2000_8000));
        assert!(!guard.contains(0x2000_6FFF));
    }

    #[test]
    fn the_guard_is_below_only_addresses_above_its_top() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        assert!(guard.below(0x2003_FFF0));
        assert!(guard.below(0x2000_8004));
        assert!(!guard.below(0x2000_8000));
        assert!(!guard.below(0x2000_7FE0));
        assert!(!guard.below(0x2000_6F00));
    }

    /// `CFSR` of a refused data access with `MMFAR` valid.
    const REFUSED_ACCESS: u32 = CFSR_DACCVIOL | CFSR_MMARVALID;

    #[test]
    fn a_frame_at_or_under_the_guard_top_is_an_overflow() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        assert!(
            guard.is_overflow(0x2000_7FE0, CFSR_MSTKERR, 0),
            "a frame pushed into the guard"
        );
        assert!(
            guard.is_overflow(0x2000_8000, 0, 0),
            "an emulator that left the stack pointer at the top"
        );
        assert!(
            guard.is_overflow(0x2000_6F00, 0, 0),
            "a frame that stepped over the guard"
        );
    }

    #[test]
    fn a_refused_access_in_the_guard_is_an_overflow_wherever_the_frame_is() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        // A 44-byte push from 0x2000_8028 faults at 0x2000_7FFC while its
        // fault's frame fits at 0x2000_8008.
        assert!(guard.is_overflow(0x2000_8008, REFUSED_ACCESS, 0x2000_7FFC));
        assert!(guard.is_overflow(0x2000_8008, REFUSED_ACCESS, 0x2000_7000));
        assert!(
            !guard.is_overflow(0x2000_8008, REFUSED_ACCESS, 0x2000_8000),
            "refused above the guard"
        );
        assert!(
            !guard.is_overflow(0x2000_8008, CFSR_DACCVIOL, 0x2000_7FFC),
            "MMFAR not valid"
        );
        assert!(
            !guard.is_overflow(0x2000_8008, CFSR_MMARVALID, 0x2000_7FFC),
            "not a data access"
        );
    }

    #[test]
    fn other_faults_above_the_guard_are_not_overflows() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        // panic-probe's `udf` (CFSR.UNDEFINSTR) with stack to spare.
        assert!(!guard.is_overflow(0x2003_F000, 1 << 16, 0));
        assert!(!guard.is_overflow(0x2000_8008, 0, 0x2000_7FFC));
    }

    #[test]
    fn a_frame_whose_push_faulted_holds_no_pc() {
        assert!(frame_stacked(0));
        assert!(frame_stacked(REFUSED_ACCESS));
        assert!(frame_stacked(1 << 16));
        assert!(!frame_stacked(CFSR_MSTKERR));
        assert!(!frame_stacked(CFSR_STKERR));
        assert!(!frame_stacked(CFSR_MSTKERR | REFUSED_ACCESS));
    }

    #[test]
    fn the_type_register_gives_the_region_count() {
        // The nRF52840's MPU_TYPE: eight regions, unified.
        assert_eq!(mpu_regions(0x0000_0800), 8);
        assert_eq!(mpu_regions(0), 0);
        assert_eq!(mpu_regions(0xFFFF_00FF), 0);
        assert_eq!(mpu_regions(0x00FF_FF00), 0xFF);
    }

    #[test]
    fn only_the_enable_bit_turns_a_region_on() {
        let guard = StackGuard::place(0x2000_7000, 4096).unwrap();
        assert!(region_enabled(guard.rasr()));
        assert!(region_enabled(1));
        assert!(!region_enabled(0));
        assert!(!region_enabled(guard.rasr() & !1));
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
