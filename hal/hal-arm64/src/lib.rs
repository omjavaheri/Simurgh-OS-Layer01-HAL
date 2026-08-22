//! ============================================================================
//! hal-arm64
//!
//! The ARM64 implementation of every hal-core trait. Mirrors
//! hal-x86_64/src/lib.rs's structure exactly — see that file's module
//! docs for the shared rationale (submodule layout, panic handler,
//! top-level PlatformHal type, entry-point responsibilities).
//! ============================================================================

#![no_std]
#![no_main]
#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]

use core::panic::PanicInfo;

// ============================================================================
// Boot bootstrap assembly (formerly boot.S), embedded via global_asm!
// — see hal-x86_64/src/lib.rs's equivalent block for the general
// rationale (no external assembler required).
// ============================================================================
core::arch::global_asm!(
    r#"
    .section .boot.header, "a"

    .section .boot.text, "ax"
    .global _start
    .type _start, %function

_start:
    // Step 0: drop EL2 -> EL1 if UEFI left us in EL2.
    mrs     x1, CurrentEL
    and     x1, x1, #0xC
    cmp     x1, #0x8
    b.ne    1f

    mov     x1, #(1 << 31)
    msr     hcr_el2, x1
    msr     sctlr_el1, xzr
    mov     x1, #0x3C5
    msr     spsr_el2, x1
    adr     x1, 1f
    msr     elr_el2, x1
    eret

1:
    // Step 1: establish a known-good stack.
    adr     x1, __boot_stack_top
    mov     sp, x1

    // Step 2: zero .bss.
    adr     x1, __bss_start
    adr     x2, __bss_end
2:  cmp     x1, x2
    b.ge    3f
    str     xzr, [x1], #8
    b       2b
3:

    // Step 4: hand off to Rust. X0 still holds the UEFI memory map
    // pointer.
    bl      hal_arm64_rust_entry

.halt_forever:
    wfi
    b       .halt_forever

    .size _start, . - _start

    .section .boot.data, "aw"
    "#
);

pub mod compute;
pub mod cpu;
pub mod interrupt;
pub mod memory;
pub mod power;
pub mod timer;

#[cfg(feature = "hal-direct-support")]
pub mod direct;

unsafe extern "C" {
    static __kernel_image_phys_start: u8;
    static __kernel_image_phys_end: u8;
    static __boot_stack_bottom: u8;
    static __boot_stack_top: u8;
}

fn linker_symbol_addr(sym: &u8) -> u64 {
    sym as *const u8 as u64
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // TODO(layer 1 diagnostics): same as hal-x86_64's panic_handler —
    // no serial output path exists yet in this phase.
    //
    // SAFETY: masking interrupts (DAIF bits) before halting prevents a
    // timer or IPI from waking this core into a handler that would run
    // against already-inconsistent state, same rationale as
    // hal-x86_64's panic_handler.
    unsafe {
        core::arch::asm!("msr daifset, #0xF");
    }
    loop {
        // SAFETY: `wfi` with interrupts masked is the AArch64
        // equivalent of x86_64's `hlt` — an infinite, side-effect-free
        // halt.
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

/// The ARM64 realization of `hal_core::PlatformHal`. Mirrors
/// hal-x86_64's `X86_64Hal` struct exactly in shape.
pub struct Arm64Hal {
    pub cpu: cpu::Cpu,
    pub memory: memory::Memory,
    pub timer: timer::Timer,
    pub interrupt: interrupt::InterruptCtrl,
    pub compute: compute::ComputeDiscovery,
    pub power: power::PowerThermalImpl,
}

/// Fixed size, in bytes, of one ARM64 saved hardware context. Covers:
/// X19-X30 callee-saved GPRs (per AAPCS64, X19-X28 are callee-saved,
/// plus FP=X29 and LR=X30) = 12 registers, SP, PC (ELR_EL1 on restore),
/// SPSR_EL1, TTBR0_EL1 (per-thread address space root, ARM64's
/// equivalent of x86_64's CR3) = 4 more = 16 × 8 = 128 bytes, rounded
/// to 160 for headroom (TPIDR_EL0 for thread-local storage, plus
/// reserved slots), matching X86_64_CONTEXT_BYTES's own sizing
/// rationale in hal-x86_64/src/lib.rs.
pub const ARM64_CONTEXT_BYTES: usize = 160;

/// # Safety
/// Same contract as hal-x86_64's `hal_x86_64_rust_entry`: only sound
/// when called from this crate's own `boot.S` `_start`, after the EL2
/// -> EL1 drop (if needed), stack setup, and `.bss` zeroing have
/// already completed.
#[no_mangle]
pub extern "C" fn hal_arm64_rust_entry(uefi_memory_map: *const u8) -> ! {
    let cpu = cpu::Cpu::new();

    // SAFETY: `uefi_memory_map` was validated by this function's own
    // safety contract above — same reasoning as hal-x86_64's
    // equivalent call.
    let memory = unsafe { memory::Memory::from_uefi_memory_map(uefi_memory_map) };

    let timer = timer::Timer::new();
    let interrupt = interrupt::InterruptCtrl::new();
    let compute = compute::ComputeDiscovery::new();
    let power = power::PowerThermalImpl::new(&compute);

    let hal = Arm64Hal {
        cpu,
        memory,
        timer,
        interrupt,
        compute,
        power,
    };

    let kernel_image_phys_range = (
        unsafe { linker_symbol_addr(&__kernel_image_phys_start) },
        unsafe { linker_symbol_addr(&__kernel_image_phys_end) },
    );
    let boot_reserved_phys_range = (
        unsafe { linker_symbol_addr(&__boot_stack_bottom) },
        unsafe { linker_symbol_addr(&__boot_stack_top) },
    );

    let boot_info = hal_core::BootInfo::new(
        hal_core::BootProtocol::Uefi,
        memory::built_hardware_manifest(
            &hal.memory,
            &hal.compute,
            &hal.power,
            &hal.cpu,
            &hal.interrupt,
            &hal.timer,
        ),
        memory::current_page_table_phys(&hal.memory),
        kernel_image_phys_range,
        boot_reserved_phys_range,
        0,
    );

    debug_assert!(
        boot_info.validate().is_ok(),
        "hal-arm64 constructed an internally inconsistent BootInfo"
    );

    let hal_interface = hal_core::build_interface(&hal.cpu, &hal.timer);

    extern "Rust" {
        fn kernel_main(hal: hal_core::HalInterface, boot_info: hal_core::BootInfo) -> !;
    }

    // SAFETY: same reasoning as hal-x86_64's equivalent call — see
    // hal-core/src/interface.rs's build_interface doc comment.
    unsafe { kernel_main(hal_interface, boot_info) }
}
