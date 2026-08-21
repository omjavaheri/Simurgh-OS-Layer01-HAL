# =============================================================================
# boot.S — ARM64
#
# Mirrors hal-x86_64/boot.S's four-step structure (stack, .bss zero,
# minimal ABI setup, jump to Rust) — see that file's module docs for
# the shared rationale. Differences here are entirely in what "minimal
# ABI setup" means for AArch64:
#
#   - EL2 -> EL1 drop: UEFI on ARM64 may hand off in EL2 (hypervisor
#     level) rather than EL1 (kernel level), depending on firmware and
#     whether virtualization extensions are in use. This project's
#     kernel runs at EL1 (per 01-HAL-Layer.md section 3.1's PrivilegeLevel
#     ::Kernel mapping to EL1), so boot.S must detect and drop EL2 -> EL1
#     if UEFI left us there — a step x86_64 has no equivalent of.
#   - No GDT/IDT loading here: AArch64's exception vector table (VBAR_EL1)
#     is set up entirely in Rust (cpu.rs's bootstrap_current_core), since
#     — unlike x86_64's IDT, which needs a flat array of gate descriptors
#     built before any Rust heap-free logic can run usefully — AArch64's
#     vector table is a single, fixed-layout 2KB-aligned code block that
#     is more naturally expressed as Rust `global_asm!` in cpu.rs itself
#     (see that file).
#
# Entry conditions (per section 3.5's UEFI path): the same UEFI
# bootloader stub used for x86_64 loads this image and jumps to
# `_start`, with X0 holding the UEFI memory map pointer (AAPCS64's
# first integer argument register, mirroring x86_64's RDI).
# =============================================================================

    .section .boot.header, "a"
    # (intentionally empty, mirrors hal-x86_64/boot.S)

    .section .boot.text, "ax"
    .global _start
    .type _start, %function

_start:
    # ---------------------------------------------------------------
    # Step 0 (ARM64-specific, no x86_64 equivalent): drop EL2 -> EL1
    # if UEFI left us in EL2.
    #
    # Reads CurrentEL (bits 3:2 encode the current exception level) to
    # decide whether the EL2 -> EL1 drop sequence is needed at all —
    # UEFI on a non-virtualized boot commonly hands off directly at
    # EL1, in which case this entire block is skipped.
    # ---------------------------------------------------------------
    mrs     x1, CurrentEL
    and     x1, x1, #0xC
    cmp     x1, #0x8            // CurrentEL == EL2 (0b10 << 2)
    b.ne    1f

    // Configure EL1 to be AArch64 (not AArch32) on the eret below:
    // HCR_EL2.RW (bit 31) = 1.
    mov     x1, #(1 << 31)
    msr     hcr_el2, x1

    // SCTLR_EL1 reset value: MMU/caches off (matches the identity-
    // mapped state UEFI leaves memory in; hal-arm64's own
    // MemoryBootstrap sets up the real mapping later, mirroring
    // hal-x86_64's memory.rs).
    msr     sctlr_el1, xzr

    // SPSR_EL2: what PSTATE eret restores when dropping to EL1 —
    // 0x3C5 = EL1h (use SP_EL1), interrupts masked (D,A,I,F set).
    mov     x1, #0x3C5
    msr     spsr_el2, x1

    // Set ELR_EL2 to the label immediately after this block, so eret
    // resumes execution right where a non-EL2 boot would have started
    // anyway.
    adr     x1, 1f
    msr     elr_el2, x1
    eret

1:
    # ---------------------------------------------------------------
    # Step 1: establish a known-good stack (mirrors hal-x86_64/boot.S
    # step 1). __boot_stack_top is defined in linker.ld.
    # ---------------------------------------------------------------
    adr     x1, __boot_stack_top
    mov     sp, x1

    # ---------------------------------------------------------------
    # Step 2: zero .bss (mirrors hal-x86_64/boot.S step 2).
    # ---------------------------------------------------------------
    adr     x1, __bss_start
    adr     x2, __bss_end
2:  cmp     x1, x2
    b.ge    3f
    str     xzr, [x1], #8
    b       2b
3:

    # ---------------------------------------------------------------
    # Step 3: minimal ABI environment. AAPCS64 has no equivalent of
    # x86_64's direction-flag requirement; no additional setup needed
    # beyond the stack/.bss steps above. FP/NEON (per
    # targets/aarch64-hal.json's "+neon,+fp-armv8") are left exactly as
    # UEFI configured them — CPACR_EL1 trap configuration, if ever
    # needed to restrict FP/NEON use in user-space, is a layer 2/3
    # concern (cpu.rs's set_privilege_level / context_switch own any
    # further FP state handling), not something boot.S needs to touch.
    # ---------------------------------------------------------------

    # ---------------------------------------------------------------
    # Step 4: hand off to Rust. X0 still holds the UEFI memory map
    # pointer (untouched since entry — the EL2 drop sequence above
    # only used X1). hal_arm64_rust_entry is declared in lib.rs as:
    #
    #   #[no_mangle]
    #   pub extern "C" fn hal_arm64_rust_entry(uefi_memory_map: *const u8) -> !
    # ---------------------------------------------------------------
    bl      hal_arm64_rust_entry

.halt_forever:
    wfi
    b       .halt_forever

    .size _start, . - _start

    .section .boot.data, "aw"
    # (intentionally empty, mirrors hal-x86_64/boot.S)