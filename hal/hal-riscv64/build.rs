// ============================================================================
// build.rs — RISC-V
//
// Mirrors hal-x86_64/hal-arm64's build.rs structure. Differences here
// are only in the compiler flags for assembling boot.S, matching
// RV64GC's ABI (per targets/riscv64gc-hal.json's "+m,+a,+f,+d,+c"
// features and "lp64d" ABI name).
// ============================================================================

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set by cargo");

    let linker_script = format!("{manifest_dir}/linker.ld");
    println!("cargo:rerun-if-changed={linker_script}");
    println!("cargo:rustc-link-arg=-T{linker_script}");
}