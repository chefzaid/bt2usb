//! Stack high-water measurement.
//!
//! cortex-m-rt's `paint-stack` feature fills the whole stack region
//! (`_stack_end`..`_stack_start`) with `0xCCCC_CCCC` at reset. The deepest the
//! stack has ever reached is where that paint was first overwritten, so
//! scanning up from the bottom gives the worst-case usage so far. The Embassy
//! thread executor runs every task on this one stack (plus interrupt frames),
//! so this is the number that has to stay well below the region size.

const PAINT: u32 = 0xCCCC_CCCC;

extern "C" {
    static _stack_end: u32;
    static _stack_start: u32;
}

/// `(used, total)` stack bytes: the deepest usage seen since reset and the
/// size of the stack region.
pub fn high_water() -> (usize, usize) {
    // Linker-provided bounds of the stack region; only their addresses are used.
    let bottom = core::ptr::addr_of!(_stack_end) as usize;
    let top = core::ptr::addr_of!(_stack_start) as usize;
    let mut addr = bottom;
    while addr < top {
        // SAFETY: word-aligned reads inside the stack region, which is plain
        // RAM owned by this program. Volatile so the scan isn't optimised on
        // assumptions about memory the compiler doesn't see written.
        if unsafe { core::ptr::read_volatile(addr as *const u32) } != PAINT {
            break;
        }
        addr += 4;
    }
    (top - addr, top - bottom)
}
