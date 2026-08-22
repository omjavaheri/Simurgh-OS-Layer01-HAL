// ============================================================================
// build.rs
//
// Only wires the linker script into the final link step. Boot
// bootstrap assembly is now embedded directly in src/lib.rs via
// global_asm!, so rustc/LLVM assembles it as part of normal
// compilation — no external assembler (clang/gcc) or `cc` crate
// dependency is needed anymore.
// ============================================================================

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set by cargo");

    let linker_script = format!("{manifest_dir}/linker.ld");
    println!("cargo:rerun-if-changed={linker_script}");
    println!("cargo:rustc-link-arg=-T{linker_script}");
}