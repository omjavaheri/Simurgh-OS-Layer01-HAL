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

    let boot_asm = format!("{manifest_dir}/src/boot.S");
    let linker_script = format!("{manifest_dir}/linker.ld");

    println!("cargo:rerun-if-changed={boot_asm}");
    println!("cargo:rerun-if-changed={linker_script}");

    cc::Build::new()
        .file(&boot_asm)
        .flag("-ffreestanding")
        .flag("-fno-stack-protector")
        // ARM64 has no "red zone" concept the way x86_64 does (see
        // targets/aarch64-hal.json's comment on this), so no
        // equivalent flag is needed here.
        // Target this build explicitly at AArch64 general-purpose
        // registers only (matches targets/aarch64-hal.json's
        // "+neon,+fp-armv8" — NEON/FP ARE available on this
        // architecture's ABI, unlike x86_64's soft-float choice, so no
        // float-disabling flag is passed either).
        .flag("-march=armv8-a")
        .compile("hal_arm64_boot");

    println!("cargo:rustc-link-arg=-T{linker_script}");
}