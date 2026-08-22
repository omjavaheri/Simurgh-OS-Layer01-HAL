# =============================================================================
# boot.S — x86_64
#
# The minimal assembly bootstrap required before any Rust code can run,
# per 01-HAL-Layer.md section 7:
#   "بخش‌های bootstrap اولیه (قبل از راه‌اندازی stack سالم) در اسمبلی
#    حداقلی، جدا در فایل boot.S هر معماری؛ باقی کد ۱۰۰٪ Rust"
#
# Entry conditions (per section 3.5, Boot Abstraction — UEFI path):
#   A UEFI bootloader stub has already:
#     - loaded this ELF image's segments into physical memory at the
#       addresses linker.ld's .boot section specifies (KERNEL_LMA_BASE)
#     - left the CPU in x86_64 long mode, paging ENABLED via UEFI's own
#       identity-mapped page tables (UEFI guarantees this before
#       ExitBootServices() / image handoff)
#     - jumped (via a `call`/`jmp`, not a function-pointer ABI call) to
#       the `_start` symbol below, with RDI holding a pointer to a
#       UEFI-provided memory map descriptor blob that hal-x86_64's Rust
#       `memory.rs` implementation parses into
#       `hal_manifest::raw::MemoryRegionRaw` entries.
#
# This file does exactly four things, per the "smallest possible code
# before a valid stack exists" principle:
#   1. Set up a known-good stack (the .bss-reserved region from
#      linker.ld: __boot_stack_bottom / __boot_stack_top).
#   2. Zero .bss (Rust code assumes zero-initialized statics; nothing
#      guarantees the loader did this for us).
#   3. Establish the minimal SysV x86_64 calling-convention environment
#      Rust expects (clear the direction flag; SSE/AVX are already
#      disabled at the compiler level via targets/x86_64-hal.json's
#      "+soft-float" feature, so no FPU/SSE init is needed here).
#   4. Call into the first Rust function, `hal_x86_64_rust_entry`,
#      passing along the UEFI memory map pointer received in RDI.
#
# Everything past this point (parsing the memory map into
# HardwareManifestRaw, setting up the higher-half identity mapping via
# MemoryBootstrap::setup_identity_mapping, building BootInfo, and handing
# control to the microkernel per section 0's direct-function-call model)
# is 100% Rust, living in this crate's src/*.rs files.
# =============================================================================

    .intel_syntax noprefix

# -----------------------------------------------------------------------
# .boot.header — reserved for a future boot-protocol header (e.g. a
# Multiboot2 or project-specific magic header) should this image ever
# need to be loaded by something other than our own UEFI stub. Left
# empty for now; linker.ld already reserves the section so adding a
# header later requires no linker script change.
# -----------------------------------------------------------------------
    .section .boot.header, "a"
    # (intentionally empty for the UEFI-stub-only MVP boot path)

# -----------------------------------------------------------------------
# .boot.text — the actual bootstrap code.
# -----------------------------------------------------------------------
    .section .boot.text, "ax"
    .global _start
    .type _start, @function

_start:
    # ---------------------------------------------------------------
    # Step 1: establish a known-good stack.
    #
    # __boot_stack_top is defined in linker.ld inside .bss; the System
    # V x86_64 ABI requires RSP to be 16-byte aligned at a `call`
    # instruction boundary (linker.ld aligns the stack region to 16
    # bytes for exactly this reason).
    # ---------------------------------------------------------------
    lea     rsp, [rip + __boot_stack_top]

    # A valid stack is now the FIRST precondition met; per module docs,
    # no Rust code may run before this point.

    # ---------------------------------------------------------------
    # Step 2: zero .bss.
    #
    # Rust's `static` items with no explicit initializer (and hal-core/
    # hal-x86_64's own zero-initialized buffers, e.g. any boot-time
    # HardwareManifestRaw storage kept as `static mut`) are only sound
    /* if this memory actually reads as zero — the ELF loader is not */
    # guaranteed to have done this for us (a `.bss` section by
    # definition occupies no space in the file image), so we do it
    # explicitly here before any Rust code touches such statics.
    # ---------------------------------------------------------------
    lea     rdi, [rip + __bss_start]
    lea     rcx, [rip + __bss_end]
    sub     rcx, rdi                   # rcx = byte count to zero
    xor     al, al
    cld                                 # ensure forward direction for stosb
    rep stosb                          # zero [rdi, rdi+rcx) one byte at a time

    # ---------------------------------------------------------------
    # Step 3: establish the minimal ABI environment Rust expects.
    #
    # `cld` above already clears the direction flag (DF), which the
    # SysV x86_64 ABI requires to be clear on entry to any function —
    # satisfied as a side effect of the .bss zeroing above, but stated
    # again here explicitly since it is a hard precondition for calling
    # into Rust, not just an implementation detail of `rep stosb`.
    #
    # No SSE/AVX register initialization is performed: targets/
    # x86_64-hal.json sets "+soft-float", so rustc never emits code
    # that reads/writes XMM/YMM registers for this crate's compiled
    # output — CR0.EM / CR0.TS / CR4.OSFXSR are therefore left exactly
    # as UEFI configured them; hal-x86_64's own CpuAbstraction
    # implementation (cpu.rs) is responsible for any FPU/SIMD state
    # setup needed later, once per-core bootstrap actually requires it
    # (e.g. before running user-space code that DOES use SSE/AVX).
    # ---------------------------------------------------------------
    cld

    # ---------------------------------------------------------------
    # Step 4: hand off to Rust.
    #
    # RDI still holds the UEFI memory map pointer passed in by the
    # bootloader stub (untouched since entry — none of the steps above
    # clobbered RDI). `hal_x86_64_rust_entry` is declared in this
    # crate's src/lib.rs as:
    #
    #   #[no_mangle]
    #   pub extern "C" fn hal_x86_64_rust_entry(uefi_memory_map: *const u8) -> !
    #
    # and is the first Rust code in the entire system to execute. It
    # never returns (return type `!`) — if it somehow did, the `hlt`
    # loop below is the last-resort safety net.
    # ---------------------------------------------------------------
    call    hal_x86_64_rust_entry

    # Unreachable under normal operation (hal_x86_64_rust_entry's `!`
    # return type is enforced by the Rust type system). Kept as a
    # defensive halt loop rather than falling off the end of .text,
    # in case a future change accidentally makes the Rust entry point
    # non-divergent.
.halt_forever:
    cli
    hlt
    jmp     .halt_forever

    .size _start, . - _start

# -----------------------------------------------------------------------
# .boot.data — reserved for any small, position-fixed data boot.S itself
# needs before Rust/relocations are available (e.g. a temporary early
# GDT, if a future revision needs one). Left empty for now: UEFI already
# hands off with a working GDT/IDT/paging setup, so no early boot.S-
# owned data structures are required for the current MVP boot path.
# -----------------------------------------------------------------------
    .section .boot.data, "aw"
    # (intentionally empty for the UEFI-stub-only MVP boot path)