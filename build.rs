//! Build script - selects the linker memory layout and copies it into OUT_DIR
//! as `memory.x` so cortex-m-rt's `link.x` (`INCLUDE memory.x`) finds it.
//!
//! IMPORTANT: the source layouts are intentionally NOT named `memory.x`. The
//! linker (rust-lld) resolves `INCLUDE memory.x` from the current directory
//! (the crate root) *before* the `-L` search path, so a `memory.x` in the root
//! would shadow the one we copy here and silently win — which previously made
//! the SoftDevice-free `sim` build link at the SoftDevice offset. Keeping the
//! sources as `memory_sd.x` / `memory_sim.x` ensures OUT_DIR/memory.x is the
//! only `memory.x` and is always the one used.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // The SoftDevice-free `sim` build (Renode) owns the whole device, so it uses
    // a different memory map than the real firmware (which reserves the
    // SoftDevice flash/RAM region). Pick the right source layout by feature.
    let mem_file = if env::var_os("CARGO_FEATURE_SIM").is_some() {
        "memory_sim.x"
    } else {
        "memory_sd.x"
    };

    // Copy the chosen layout to OUT_DIR as `memory.x`.
    fs::copy(mem_file, out_dir.join("memory.x")).unwrap();

    // Tell cargo to look for linker scripts in OUT_DIR.
    println!("cargo:rustc-link-search={}", out_dir.display());

    // Rebuild if either source layout changes...
    println!("cargo:rerun-if-changed=memory_sd.x");
    println!("cargo:rerun-if-changed=memory_sim.x");
    println!("cargo:rerun-if-changed=build.rs");
    // ...and, crucially, re-run when the `sim` feature toggles, otherwise cargo
    // would reuse a previously-copied memory.x with the wrong layout.
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SIM");
}
