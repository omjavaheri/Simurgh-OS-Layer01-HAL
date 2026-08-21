// ============================================================================
// build.rs
//
// Assembles boot.S (the minimal x86_64 assembly bootstrap, per
// 01-HAL-Layer.md section 7: "بخش‌های bootstrap اولیه... در اسمبلی
// حداقلی، جدا در فایل boot.S هر معماری") and wires the linker script
// (linker.ld) into the final link step for this crate's staticlib
// output.
//
// This file only runs when hal-x86_64 itself is being compiled (i.e.
// via `cargo xbuild-x86_64`, which passes --target targets/x86_64-hal.json)
// — it has no effect on, and is not invoked by, host-target builds of
// hal-core/hal-manifest/hal-direct.
// ============================================================================

fn main() {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set by cargo");

    let boot_asm = format!("{manifest_dir}/src/boot.S");
    let linker_script = format!("{manifest_dir}/linker.ld");

    // Re-run this build script if either the assembly stub or the
    // linker script changes — without these, `cargo` would not know to
    // re-assemble/re-link on edits to files it doesn't otherwise track
    // as Rust source.
    println!("cargo:rerun-if-changed={boot_asm}");
    println!("cargo:rerun-if-changed={linker_script}");

    // Assemble boot.S into an object file and archive it so the Rust
    // linker step picks it up automatically alongside this crate's own
    // compiled Rust code. `cc::Build` in assembler mode shells out to
    // the system assembler (or clang's integrated assembler) with the
    // correct flags for our bare-metal, no-red-zone, static-relocation
    // target — matching targets/x86_64-hal.json's own settings so the
    // hand-written assembly and the Rust-compiled code agree on ABI
    // assumptions.
    cc::Build::new()
        .file(&boot_asm)
        // Freestanding: no libc, no OS-provided startup files — this
        // is bare-metal assembly executed before Rust's own runtime
        // exists.
        .flag("-ffreestanding")
        .flag("-fno-stack-protector")
        // Matches targets/x86_64-hal.json's "disable-redzone": true —
        // boot.S itself must not assume a red zone either, since it
        // runs before any interrupt handling is set up and must remain
        // consistent with the Rust code it hands off to.
        .flag("-mno-red-zone")
        .compile("hal_x86_64_boot");

    // Pass the linker script to the final link step. This must match
    // (not duplicate/conflict with) the `-C link-arg=-T...` entry
    // already present in .cargo/config.toml's [target.x86_64-hal]
    // section — both point at the same linker.ld, from two different
    // required injection points (build.rs for this crate's own build
    // graph, .cargo/config.toml for the top-level binary link Cargo
    // performs). Keeping both is deliberate belt-and-suspenders: a
    // consumer that depends on hal-x86_64 as a library from a
    // differently-configured workspace still gets the correct linker
    // script via this build.rs alone.
    println!("cargo:rustc-link-arg=-T{linker_script}");
}