//! Build script - copies the linker script into the output directory
//! so that the linker can find it at link time.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Copy memory.x to OUT_DIR
    fs::copy("memory.x", out_dir.join("memory.x")).unwrap();

    // Tell cargo to look for linker scripts in OUT_DIR
    println!("cargo:rustc-link-search={}", out_dir.display());

    // Rebuild if the linker script changes
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
