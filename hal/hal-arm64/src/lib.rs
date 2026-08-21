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

pub mod cpu;
pub mod memory;
pub mod timer;
pub mod interrupt;
pub mod compute;
pub mod power;

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
        memory::built_hardware_manifest(&hal.memory, &hal.compute, &hal.power, &hal.cpu, &hal.interrupt, &hal.timer),
        memory::current_page_table_phys(&hal.memory),
        kernel_image_phys_range,
        boot_reserved_phys_range,
        0,
    );

    debug_assert!(
        boot_info.validate().is_ok(),
        "hal-arm64 constructed an internally inconsistent BootInfo"
    );

    extern "Rust" {
        fn kernel_main(hal: Arm64Hal, boot_info: hal_core::BootInfo) -> !;
    }

    // SAFETY: same reasoning as hal-x86_64's equivalent call —
    // `kernel_main`'s signature is fixed by this workspace, and both
    // sides agree on it by construction.
    unsafe { kernel_main(hal, boot_info) }
}