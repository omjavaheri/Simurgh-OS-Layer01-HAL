# =============================================================================
# boot.S — RISC-V (RV64GC)
#
# Mirrors hal-x86_64/hal-arm64/boot.S's overall structure (stack, .bss
# zero, minimal ABI setup, jump to Rust). The boot protocol itself is
# entirely different, per 01-HAL-Layer.md section 3.5: this is entered
# via SBI, NOT UEFI.
#
# Entry conditions (per section 3.5's RISC-V row, "SBI + Device Tree"):
# An SBI implementation (OpenSBI, per this crate's linker.ld doc
# comment on the standard QEMU virt "supervisor payload" convention)
# has already:
#   - initialized the hart (RISC-V's term for a hardware thread/core)
#     to S-mode (Supervisor mode — this project's kernel privilege
#     level, per hal-core's PrivilegeLevel::Kernel mapping) — M-mode
#     firmware setup (trap delegation, PMP configuration) is entirely
#     SBI's responsibility and already complete by the time _start
#     runs, unlike ARM64 where THIS crate's boot.S has to perform the
#     EL2->EL1 drop itself.
#   - loaded this image at KERNEL_LMA_BASE (linker.ld) and jumped to
#     `_start` with:
#       a0 (x10) = this hart's hart ID
#       a1 (x11) = physical address of the Device Tree Blob (DTB),
#                  per the RISC-V SBI spec's mandatory boot protocol
#                  (section 3.2: "Device Tree (اجباری طبق مشخصات SBI)")
#
# Per the RISC-V calling convention, a0/a1 are the first two integer
# argument registers — mirroring how x86_64 uses RDI and ARM64 uses X0
# for their single boot-parameter pointer, RISC-V's SBI boot protocol
# conventionally supplies TWO parameters (hart id + DTB pointer) instead
# of one, which this crate's Rust entry point signature reflects (see
# lib.rs's hal_riscv64_rust_entry).
# =============================================================================

    .section .boot.header, "a"
    # (intentionally empty, mirrors the other two architectures)

    .section .boot.text, "ax"
    .global _start
    .type _start, @function

_start:
    # ---------------------------------------------------------------
    # Step 1: establish a known-good stack (mirrors the other two
    # architectures' boot.S step 1). __boot_stack_top from linker.ld.
    #
    # Only hart 0 (the boot hart) proceeds past this point in this
    # MVP phase — section 8's acceptance criteria target single-hart
    # QEMU boot; secondary hart parking (via SBI's HSM extension) is a
    # tracked follow-up mirroring cpu.rs's multi-core deferral on the
    # other two architectures.
    # ---------------------------------------------------------------
    bnez    a0, .park_secondary_hart   # a0 = hart id; only hart 0 continues

    la      sp, __boot_stack_top

    # ---------------------------------------------------------------
    # Step 2: zero .bss (mirrors the other two architectures' step 2).
    # ---------------------------------------------------------------
    la      t0, __bss_start
    la      t1, __bss_end
1:  bgeu    t0, t1, 2f
    sd      zero, 0(t0)
    addi    t0, t0, 8
    j       1b
2:

    # ---------------------------------------------------------------
    # Step 3: minimal ABI environment. The RISC-V ELF psABI has no
    # equivalent of x86_64's direction-flag requirement; no additional
    # setup needed beyond stack/.bss. F/D (hardware float, per
    # targets/riscv64gc-hal.json's "+f,+d") are left exactly as SBI/
    # M-mode firmware configured them (mstatus.FS is set to "Initial"
    # or "Clean" by standard SBI implementations before S-mode entry;
    # this project's context_switch — cpu.rs — is responsible for any
    # further FP state handling, mirroring ARM64's equivalent
    # deferral).
    # ---------------------------------------------------------------

    # ---------------------------------------------------------------
    # Step 4: hand off to Rust. a0 still holds the hart id (unused by
    # the stack-setup/.bss steps above, which only used t0/t1); a1
    # still holds the DTB pointer, per this file's module docs on the
    # SBI boot protocol's two-parameter convention. hal_riscv64_rust_entry
    # is declared in lib.rs as:
    #
    #   #[no_mangle]
    #   pub extern "C" fn hal_riscv64_rust_entry(hart_id: usize, dtb_phys: *const u8) -> !
    # ---------------------------------------------------------------
    call    hal_riscv64_rust_entry

.halt_forever:
    wfi
    j       .halt_forever

.park_secondary_hart:
    # Per this MVP phase's single-hart scope (see Step 1's comment):
    # any hart other than 0 parks itself in a wfi loop rather than
    # proceeding into Rust code that assumes single-hart execution
    # (e.g. every `static mut` table in this crate's cpu.rs/memory.rs,
    # written under a single-threaded-boot-time contract that a second
    # hart running concurrently would violate).
    wfi
    j       .park_secondary_hart

    .size _start, . - _start

    .section .boot.data, "aw"
    # (intentionally empty, mirrors the other two architectures)