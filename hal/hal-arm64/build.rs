// ============================================================================
// build.rs — ARM64
//
// Mirrors hal-x86_64/build.rs's structure exactly (see that file for
// the general rationale). Differences here are only in the compiler
// flags passed to `cc::Build` for assembling boot.S, matching this
// architecture's ABI constraints instead of x86_64's.
// ============================================================================

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set by cargo");

    let linker_script = format!("{manifest_dir}/linker.ld");
    println!("cargo:rerun-if-changed={linker_script}");
    println!("cargo:rustc-link-arg=-T{linker_script}");
}