//! The stack: its high-water mark, and the MPU guard at its bottom.
//!
//! cortex-m-rt's `paint-stack` feature fills the whole stack region
//! (`_stack_end`..`_stack_start`) with `0xCCCC_CCCC` at reset. The deepest the
//! stack has ever reached is where that paint was first overwritten, so
//! scanning up from the bottom gives the worst-case usage so far. The Embassy
//! thread executor runs every task on this one stack (plus interrupt frames),
//! so this is the number that has to stay well below the region size.
//!
//! [`enable_guard`] makes the lowest [`STACK_GUARD_BYTES`] of the region a
//! no-access MPU region (placed by [`StackGuard`]), so an overflow faults
//! before it reaches the statics below, and this module's HardFault handler
//! reports it. The MPU is off inside that handler (`MPU_CTRL.HFNMIENA` clear),
//! so the processor can stack the fault's frame in the guard and the handler
//! can run there (docs/adr/0026-mpu-stack-guard.md).

use crate::config::STACK_GUARD_BYTES;
use crate::stack_logic::{OverflowRecord, StackGuard, MPU_CTRL, OVERFLOW_RECORD_WORDS};
use core::sync::atomic::{AtomicU32, Ordering};
use cortex_m::peripheral::MPU;

const PAINT: u32 = 0xCCCC_CCCC;

extern "C" {
    static _stack_end: u32;
    static _stack_start: u32;
}

/// Linker-provided bounds of the stack region; only their addresses are used.
fn bounds() -> (u32, u32) {
    let bottom = core::ptr::addr_of!(_stack_end) as u32;
    let top = core::ptr::addr_of!(_stack_start) as u32;
    (bottom, top)
}

/// The guard this build places at the bottom of the stack, whether or not
/// [`enable_guard`] turned it on.
pub fn guard() -> Option<StackGuard> {
    StackGuard::place(bounds().0, STACK_GUARD_BYTES)
}

/// `(used, total)` stack bytes above the guard: the deepest usage seen since
/// reset and the size of the stack the guard leaves. Never reads the guard,
/// which faults while it is on.
pub fn high_water() -> (usize, usize) {
    let (bottom, top) = bounds();
    let bottom = guard().map_or(bottom, |guard| guard.top()).min(top) as usize;
    let top = top as usize;
    let mut addr = bottom;
    while addr < top {
        // SAFETY: word-aligned reads inside the stack region above the guard,
        // which is plain RAM owned by this program. Volatile so the scan isn't
        // optimised on assumptions about memory the compiler doesn't see
        // written.
        if unsafe { core::ptr::read_volatile(addr as *const u32) } != PAINT {
            break;
        }
        addr += 4;
    }
    (top - addr, top - bottom)
}

/// Why [`enable_guard`] left the MPU off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, defmt::Format)]
pub enum GuardError {
    /// `STACK_GUARD_BYTES` is not a power of two of at least 32 bytes.
    BadSize,
    /// The stack in use already reaches into the guard's bytes.
    StackInUse,
    /// The core reports no MPU regions.
    NoMpu,
}

/// Turn the guard on: MPU region 0 covers it with no access, and the default
/// memory map covers everything else. Call it once, early in `main`.
pub fn enable_guard() -> Result<StackGuard, GuardError> {
    let guard = guard().ok_or(GuardError::BadSize)?;
    if !guard.below(cortex_m::register::msp::read()) {
        return Err(GuardError::StackInUse);
    }
    // SAFETY: the MPU's register block is always mapped on this core; this
    // only reads its TYPE register.
    let mpu = unsafe { &*MPU::PTR };
    if (mpu._type.read() >> 8) & 0xFF == 0 {
        return Err(GuardError::NoMpu);
    }
    // SAFETY: the region covers only the guard, which nothing uses (the stack
    // pointer is above it, checked above, and no static lies in it), and
    // `PRIVDEFENA` keeps the default map for every other address, so no
    // access the firmware makes changes meaning. The MPU is off while it is
    // reprogrammed.
    unsafe {
        mpu.ctrl.write(0);
        mpu.rnr.write(0);
        mpu.rbar.write(guard.rbar());
        mpu.rasr.write(guard.rasr());
        mpu.ctrl.write(MPU_CTRL);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    Ok(guard)
}

/// [`enable_guard`], logging where the guard went or why it is off.
#[cfg(not(feature = "sim"))]
pub fn enable_guard_logged() -> Option<StackGuard> {
    match enable_guard() {
        Ok(guard) => {
            defmt::info!(
                "stack guard: {=u32} bytes at {=u32:#010x}..{=u32:#010x}",
                guard.size(),
                guard.base(),
                guard.top()
            );
            Some(guard)
        }
        Err(err) => {
            defmt::warn!("stack guard off: {}", err);
            None
        }
    }
}

/// Whether the MPU still holds `guard`, as [`enable_guard`] programmed it.
#[allow(dead_code)] // used by the self-test binary
pub fn guard_is_on(guard: &StackGuard) -> bool {
    // SAFETY: the MPU's register block is always mapped on this core.
    let mpu = unsafe { &*MPU::PTR };
    // SAFETY: this selects region 0, which only this module programs, so the
    // reads below see it; no other code depends on the region number.
    unsafe { mpu.rnr.write(0) };
    guard.is_programmed(mpu.ctrl.read(), mpu.rbar.read(), mpu.rasr.read())
}

/// The last overflow's [`OverflowRecord`], encoded. `.uninit` is the part of
/// RAM the reset handler neither zeroes nor paints (the `0` initializer is
/// never loaded), and the nRF52 product specifications say RAM is never
/// reset, though some reset sources can corrupt it: a record written before a
/// lock-up, a soft reset, or the reset button is still there at the next boot,
/// and the check word rejects anything else. A power cycle loses it.
#[link_section = ".uninit.bt2usb.OVERFLOW_RECORD"]
static OVERFLOW_RECORD: [AtomicU32; OVERFLOW_RECORD_WORDS] =
    [const { AtomicU32::new(0) }; OVERFLOW_RECORD_WORDS];

/// The overflow the previous boot recorded, if any, clearing the record so
/// it is reported once. Call it at boot.
pub fn take_previous_overflow() -> Option<OverflowRecord> {
    let words = core::array::from_fn(|index| {
        OVERFLOW_RECORD
            .get(index)
            .map_or(0, |word| word.swap(0, Ordering::Relaxed))
    });
    OverflowRecord::decode(words)
}

/// `CFSR.MSTKERR`: stacking the fault's frame faulted, so the frame holds
/// whatever the guard held before (its reset paint), not the faulting code's
/// registers. A real overflow always sets it, because the frame goes below a
/// stack pointer that is already at the guard.
const CFSR_MSTKERR: u32 = 1 << 4;

/// Every HardFault ends here, including the one panic-probe raises after
/// printing a panic. A fault taken with the stack pointer at or below the
/// guard's top ([`StackGuard::below`]) is a stack overflow and is reported;
/// then, as cortex-m-rt's default handler does, the core spins until it is
/// reset. The MPU is off in this handler, so it runs on the guard's bytes.
/// cortex-m-rt requires the HardFault handler to be an `unsafe fn`; only the
/// exception calls it.
#[cortex_m_rt::exception]
unsafe fn HardFault(frame: &cortex_m_rt::ExceptionFrame) -> ! {
    let stack_pointer = core::ptr::from_ref(frame) as u32;
    if let Some(guard) = guard().filter(|guard| !guard.below(stack_pointer)) {
        // SAFETY: the SCB's register block is always mapped on this core;
        // this only reads the fault status.
        let stacked = unsafe { (*cortex_m::peripheral::SCB::PTR).cfsr.read() } & CFSR_MSTKERR == 0;
        let record = OverflowRecord {
            stack_pointer,
            guard_base: guard.base(),
            guard_top: guard.top(),
            pc: stacked.then(|| frame.pc()),
        };
        // Stored first: logging can fail (a fault inside a log call leaves
        // the logger taken, and its panic locks the core up), and the next
        // boot then reports the record instead.
        for (slot, word) in OVERFLOW_RECORD.iter().zip(record.encode()) {
            slot.store(word, Ordering::Relaxed);
        }
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        report_overflow(&record);
    }
    loop {
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
    }
}

// The MemManage exception, which an MPU fault becomes instead of a HardFault
// when the exception is enabled, as nothing here does (SHCSR.MEMFAULTENA), or
// when an emulator ignores that enable, as Renode 1.16.1 does. Its handler
// runs at priority 0 with the MPU still on and the stack pointer in the
// guard, so any push would fault again. This one uses no stack: it turns the
// MPU off and calls the `HardFault` handler above, through the entry point
// cortex-m-rt exports for it, with the handler's stack pointer as the frame.
// The firmware runs only on the main stack, and after a frame it could not
// stack Renode's EXC_RETURN names the process stack and its banked MSP is
// stale, so neither cortex-m-rt's trampoline, which picks the stack from
// EXC_RETURN, nor `mrs r0, MSP` would find the frame.
core::arch::global_asm!(
    ".section .text.MemoryManagement, \"ax\"",
    ".global MemoryManagement",
    ".type MemoryManagement, %function",
    ".thumb_func",
    "MemoryManagement:",
    "movw r0, #0xED94", // MPU_CTRL, 0xE000_ED94
    "movt r0, #0xE000",
    "movs r1, #0",
    "str r1, [r0]",
    "dsb",
    "isb",
    "mov r0, sp", // the main stack: handler mode never runs on the other
    "b _HardFault",
    ".size MemoryManagement, . - MemoryManagement",
);

#[cfg(not(feature = "sim"))]
fn report_overflow(record: &OverflowRecord) {
    let OverflowRecord {
        stack_pointer,
        guard_base,
        guard_top,
        pc,
    } = *record;
    match pc {
        Some(pc) => defmt::error!(
            "stack overflow: stack pointer {=u32:#010x}, guard {=u32:#010x}..{=u32:#010x}, PC {=u32:#010x}",
            stack_pointer,
            guard_base,
            guard_top,
            pc
        ),
        None => defmt::error!(
            "stack overflow: stack pointer {=u32:#010x}, guard {=u32:#010x}..{=u32:#010x}, PC not stacked",
            stack_pointer,
            guard_base,
            guard_top
        ),
    }
}

/// [`take_previous_overflow`], logging a record the previous boot left.
#[cfg(not(feature = "sim"))]
pub fn log_previous_overflow() {
    let Some(OverflowRecord {
        stack_pointer,
        guard_base,
        guard_top,
        pc,
    }) = take_previous_overflow()
    else {
        return;
    };
    match pc {
        Some(pc) => defmt::warn!(
            "previous boot: stack overflow: stack pointer {=u32:#010x}, guard {=u32:#010x}..{=u32:#010x}, PC {=u32:#010x}",
            stack_pointer,
            guard_base,
            guard_top,
            pc
        ),
        None => defmt::warn!(
            "previous boot: stack overflow: stack pointer {=u32:#010x}, guard {=u32:#010x}..{=u32:#010x}, PC not stacked",
            stack_pointer,
            guard_base,
            guard_top
        ),
    }
}

/// Write the overflow line, `stack overflow: ...` without a line ending, as
/// the simulation prints it.
#[cfg(feature = "sim")]
pub fn describe_overflow(record: &OverflowRecord, line: &mut impl core::fmt::Write) {
    let _ = write!(
        line,
        "stack overflow: stack pointer {:#010x}, guard {:#010x}..{:#010x}, ",
        record.stack_pointer, record.guard_base, record.guard_top
    );
    let _ = match record.pc {
        Some(pc) => write!(line, "PC {:#010x}", pc),
        None => write!(line, "PC not stacked"),
    };
}

/// The simulation reads its log from UART0, so the report is written there
/// directly: the UART driver's task will never run again.
#[cfg(feature = "sim")]
fn report_overflow(record: &OverflowRecord) {
    let mut line = heapless::String::<128>::new();
    describe_overflow(record, &mut line);
    let _ = line.push_str("\r\n");
    crate::uart_write_blocking(line.as_bytes());
}
