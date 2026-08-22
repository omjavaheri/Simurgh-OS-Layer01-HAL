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

    let boot_asm = format!("{manifest_dir}/src/boot.S");
    let linker_script = format!("{manifest_dir}/linker.ld");

    println!("cargo:rerun-if-changed={boot_asm}");
    println!("cargo:rerun-if-changed={linker_script}");

    cc::Build::new()
        .file(&boot_asm)
        .flag("-ffreestanding")
        .flag("-fno-stack-protector")
        // Matches targets/riscv64gc-hal.json's RV64GC feature set and
        // lp64d ABI exactly — the assembler must agree with rustc on
        // which extensions (M/A/F/D/C) are available and how doubles
        // are passed, or relocations/instruction encoding will
        // mismatch between hand-written boot.S and rustc-compiled code.
        .flag("-march=rv64gc")
        .flag("-mabi=lp64d")
        .compile("hal_riscv64_boot");

    println!("cargo:rustc-link-arg=-T{linker_script}");
}