//! ============================================================================
//! hal-riscv64
//!
//! The RISC-V (RV64GC) implementation of every hal-core trait. Mirrors
//! hal-x86_64/hal-arm64's lib.rs structure — see those files' module
//! docs for the shared rationale. The key difference here is the entry
//! point signature (two parameters: hart_id + dtb_phys, per boot.S's
//! module docs on SBI's boot protocol) and the boot protocol reported
//! to BootInfo (no BootProtocol::Uefi variant applies here — see
//! hal-core/src/boot.rs's BootProtocol enum, which already anticipates
//! this via its SbiDeviceTree variant).
//! ============================================================================

#![no_std]
#![no_main]
#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]

use core::panic::PanicInfo;

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
    // TODO(layer 1 diagnostics): same as the other two architectures'
    // panic_handler — no serial/SBI-console output path exists yet in
    // this phase (SBI's Legacy Console extension or the newer DBCN
    // extension would be the natural path once implemented, mirroring
    // how kernel-stub's serial driver will eventually be superseded by
    // a real layer 3 driver).
    //
    // SAFETY: masking interrupts (via sstatus.SIE = 0) before halting
    // prevents a timer or IPI from waking this hart into a handler
    // that would run against already-inconsistent state, same
    // rationale as the other two architectures' panic_handler.
    unsafe {
        core::arch::asm!("csrci sstatus, 0x2"); // clear SIE bit
    }
    loop {
        // SAFETY: `wfi` with interrupts masked is the RISC-V
        // equivalent of x86_64's `hlt` / ARM64's `wfi` — an infinite,
        // side-effect-free halt.
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

/// The RISC-V realization of `hal_core::PlatformHal`. Mirrors the
/// other two architectures' top-level Hal struct exactly in shape.
pub struct Riscv64Hal {
    pub cpu: cpu::Cpu,
    pub memory: memory::Memory,
    pub timer: timer::Timer,
    pub interrupt: interrupt::InterruptCtrl,
    pub compute: compute::ComputeDiscovery,
    pub power: power::PowerThermalImpl,
}

/// Fixed size, in bytes, of one RISC-V saved hardware context. Covers:
/// callee-saved integer registers per the RISC-V ELF psABI (s0-s11,
/// i.e. x8-x9 and x18-x27 = 14 registers... actually s0/s1 = x8/x9,
/// s2-s11 = x18-x27, totaling 12 callee-saved "s" registers), plus ra
/// (x1, used as the resume PC on restore) and sp (x2) = 14 × 8 = 112
/// bytes, plus sepc/sstatus (S-mode equivalent of ARM64's
/// spsr_el1/elr) and satp (per-thread address space root, RISC-V's
/// equivalent of x86_64's CR3 / ARM64's TTBR0_EL1) = 3 more × 8 = 24,
/// totaling 136, rounded to 160 for headroom (tp / x4 for thread-local
/// storage per the RISC-V ELF psABI, plus reserved slots), matching
/// the other two architectures' context size rounding convention.
pub const RISCV64_CONTEXT_BYTES: usize = 160;

/// # Safety
/// Only sound when called from this crate's own `boot.S` `_start`,
/// after the secondary-hart park check, stack setup, and `.bss`
/// zeroing have already completed (per boot.S's module docs) — and
/// only ever for `hart_id == 0` (boot.S itself enforces this by
/// parking any other hart before reaching this call).
#[no_mangle]
pub extern "C" fn hal_riscv64_rust_entry(hart_id: usize, dtb_phys: *const u8) -> ! {
    let cpu = cpu::Cpu::new(hart_id);

    // SAFETY: `dtb_phys` was validated by this function's own safety
    // contract above — a valid Device Tree Blob pointer per the SBI
    // boot protocol's mandatory guarantee (01-HAL-Layer.md section
    // 3.2: "Device Tree (اجباری طبق مشخصات SBI)").
    let memory = unsafe { memory::Memory::from_device_tree(dtb_phys) };

    let timer = timer::Timer::new();
    let interrupt = interrupt::InterruptCtrl::new(memory.plic_base());
    let compute = compute::ComputeDiscovery::new();
    let power = power::PowerThermalImpl::new(&compute);

    let hal = Riscv64Hal {
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
        // Per section 3.5's RISC-V row: this architecture always uses
        // the SBI + Device Tree boot path, never UEFI — hal-core's
        // BootProtocol enum already anticipates this
        // (hal-core/src/boot.rs).
        hal_core::BootProtocol::SbiDeviceTree,
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
        hart_id as u32,
    );

    debug_assert!(
        boot_info.validate().is_ok(),
        "hal-riscv64 constructed an internally inconsistent BootInfo"
    );

    let hal_interface = hal_core::build_interface(&hal.cpu, &hal.timer);

    extern "Rust" {
        fn kernel_main(hal: hal_core::HalInterface, boot_info: hal_core::BootInfo) -> !;
    }

    // SAFETY: same reasoning as hal-x86_64's equivalent call — see
    // hal-core/src/interface.rs's build_interface doc comment.
    unsafe { kernel_main(hal_interface, boot_info) }
}
