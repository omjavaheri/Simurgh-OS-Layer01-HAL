//! ============================================================================
//! hal-x86_64
//!
//! The x86_64 implementation of every hal-core trait, per
//! 01-HAL-Layer.md sections 3 and 6. This file:
//!
//!   1. Declares the per-responsibility submodules (cpu, memory, timer,
//!      interrupt, compute, power) — one per hal-core trait, mirroring
//!      hal-core's own module layout so the mapping between "what a
//!      trait requires" and "how x86_64 provides it" stays obvious.
//!   2. Defines `hal_x86_64_rust_entry`, the first Rust function ever
//!      executed (called from `boot.S`'s `_start`, per that file's
//!      module docs).
//!   3. Assembles the top-level `X86_64Hal` type that implements every
//!      hal-core trait (and therefore, via hal-core's blanket impl,
//!      `PlatformHal`) by delegating to the submodules.
//!   4. Provides the `#[panic_handler]` this `no_std` binary needs
//!      (per 01-HAL-Layer.md section 0 / 02-Microkernel-Layer.md
//!      section 1.1: no unwinding, `panic = "abort"` — this handler is
//!      the terminal point every panic reaches).
//!
//! Per 01-HAL-Layer.md section 0, this crate and the microkernel are
//! compiled into the SAME final Privileged binary; `hal_x86_64_rust_entry`
//! is therefore the boundary where control eventually passes to the
//! microkernel's Root Task via a direct Rust function call — NOT a
//! syscall, NOT IPC (that boundary only starts existing one layer up,
//! per 02-Microkernel-Layer.md section 0).
//! ============================================================================

#![no_std]
#![no_main]
#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]

use core::panic::PanicInfo;

// ----------------------------------------------------------------------------
// Submodules — one per hal-core responsibility area (01-HAL-Layer.md
// section 3), each implementing the matching hal-core trait for real
// x86_64 hardware.
// ----------------------------------------------------------------------------

/// CPU Abstraction (hal_core::cpu::CpuAbstraction) for x86_64: CPUID
/// feature detection, GDT/IDT setup, context switch via manual register
/// save/restore.
pub mod cpu;

/// Memory Bootstrap (hal_core::memory::MemoryBootstrap) for x86_64:
/// UEFI Memory Map parsing (section 3.2), minimal page table setup.
pub mod memory;

/// Timer & Clock (hal_core::timer::TimerAbstraction) for x86_64:
/// TSC/HPET (section 3.3).
pub mod timer;

/// Interrupt Controller (hal_core::interrupt::InterruptController) for
/// x86_64: APIC/x2APIC (section 3.4).
pub mod interrupt;

/// Heterogeneous Compute Discovery (hal_core::compute::ComputeDeviceDiscovery)
/// for x86_64: PCI config space scan for GPU/NPU/TPU/FPGA (section 3.6).
pub mod compute;

/// Power & Thermal (hal_core::power::PowerThermal) for x86_64: RAPL /
/// MSR-based DVFS and thermal reporting (section 3.7).
pub mod power;

/// Optional direct hardware access (hal_direct::HalDirectAccess) for
/// x86_64, only compiled when this crate's "hal-direct-support"
/// feature is enabled (see Cargo.toml) — per section 1's requirement
/// that hal-core and hal-direct stay separable in the final binary.
#[cfg(feature = "hal-direct-support")]
pub mod direct;

// ----------------------------------------------------------------------------
// Linker-provided symbols (from linker.ld)
//
// These are addresses, not values — hence `extern "C"` statics of type
// `u8` accessed only via `&raw const` / address-of, never dereferenced
// as actual byte data. This is the standard idiom for consuming linker
// script symbols from Rust.
// ----------------------------------------------------------------------------

unsafe extern "C" {
    /// Physical start of the loaded kernel image (linker.ld:
    /// __kernel_image_phys_start), used to populate
    /// `BootInfo::kernel_image_phys_start` (hal-core/src/boot.rs).
    static __kernel_image_phys_start: u8;
    /// Physical end of the loaded kernel image (linker.ld:
    /// __kernel_image_phys_end).
    static __kernel_image_phys_end: u8;
    /// Bounds of the boot-time stack (linker.ld: __boot_stack_bottom /
    /// __boot_stack_top), used to compute the `boot_reserved_phys_*`
    /// range in `BootInfo` — this stack is only needed until the
    /// microkernel's Root Task establishes its own, at which point the
    /// range it occupies is safe to reclaim (hal-core/src/boot.rs:
    /// `BootInfo::overlaps_boot_reserved`).
    static __boot_stack_bottom: u8;
    static __boot_stack_top: u8;
}

/// Reads a linker symbol's address as a `u64` physical address. Every
/// use site below immediately explains why taking the address (not the
/// value) is correct.
fn linker_symbol_addr(sym: &u8) -> u64 {
    sym as *const u8 as u64
}

// ----------------------------------------------------------------------------
// Panic handler
//
// Required by every no_std binary. Per 02-Microkernel-Layer.md section
// 1.1 and this workspace's `panic = "abort"` profile (os-project/
// Cargo.toml), there is no unwinding to perform here — this is
// genuinely the end of execution on whichever core panicked.
// ----------------------------------------------------------------------------

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // TODO(layer 1 diagnostics): once hal-x86_64's serial/UART driver
    // exists (a small, dedicated early-debug output path — NOT the
    // full Device Manager, which is layer 3 and does not exist yet at
    // the point a boot-time panic could occur), write `_info` there
    // before halting. Left as a halt-only handler for now since no
    // such output path has been built yet in this phase of the
    // implementation.
    //
    // SAFETY: disabling interrupts before halting prevents a timer or
    // IPI (hal_core::interrupt::InterruptController) from waking this
    // core out of `hlt` into a handler that would run against
    // already-inconsistent state — there is no recovery path from a
    // Rust panic in this no_std, no-unwind configuration, so this core
    // must never resume normal execution again.
    unsafe {
        core::arch::asm!("cli");
    }
    loop {
        // SAFETY: `hlt` with interrupts disabled (via `cli` above) is
        // an infinite, side-effect-free halt on x86_64 — the standard
        // terminal state for an unrecoverable no_std panic.
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

// ----------------------------------------------------------------------------
// Top-level platform type
// ----------------------------------------------------------------------------

/// The x86_64 realization of `hal_core::PlatformHal`, aggregating this
/// crate's six per-responsibility submodules behind hal-core's trait
/// contracts. A single value of this type is constructed once, in
/// `hal_x86_64_rust_entry` below, and its address is effectively what
/// the microkernel's `kernel-arch-glue`
/// (02-Microkernel-Layer.md section 7) is generic over on this
/// architecture.
///
/// `Cpu`/`Memory`/`Timer`/`InterruptCtrl`/`ComputeDiscovery`/
/// `PowerThermalImpl` types and their `CpuAbstraction`/
/// `MemoryBootstrap`/etc. trait implementations live in the
/// correspondingly-named submodules above; this struct just wires them
/// together as fields.
pub struct X86_64Hal {
    pub cpu: cpu::Cpu,
    pub memory: memory::Memory,
    pub timer: timer::Timer,
    pub interrupt: interrupt::InterruptCtrl,
    pub compute: compute::ComputeDiscovery,
    pub power: power::PowerThermalImpl,
}

/// The fixed size, in bytes, of one x86_64 saved hardware context
/// (general-purpose registers + control registers relevant to a
/// context switch). Concrete layout is defined in `cpu.rs`; this
/// constant is what `hal_core::cpu::CpuContext<N>` and
/// `hal_core::cpu::CpuAbstraction<N>` are instantiated with for this
/// architecture, per hal-core/src/cpu.rs's doc comment on
/// `ARCH_CONTEXT_BYTES`.
///
/// Value covers: 16 general-purpose registers (RAX, RBX, RCX, RDX,
/// RSI, RDI, RBP, RSP, R8-R15 = 16 × 8 bytes) + RIP + RFLAGS + CR3
/// (for address-space-switch-capable contexts) = 19 × 8 = 152 bytes,
/// rounded up to a 16-byte-aligned 160 for headroom (segment selectors
/// FS/GS base, used for thread-local storage per the SysV x86_64 ABI).
pub const X86_64_CONTEXT_BYTES: usize = 160;

// ----------------------------------------------------------------------------
// Rust entry point — called from boot.S's `_start`
// ----------------------------------------------------------------------------

/// The first Rust code executed anywhere in the system, on this
/// architecture. Called directly from `boot.S` (see that file's step
/// 4) with `uefi_memory_map` pointing at the UEFI-provided memory map
/// blob the bootloader stub obtained via `GetMemoryMap()` before
/// `ExitBootServices()`.
///
/// # Safety
/// This function's entire premise relies on preconditions only
/// `boot.S` can guarantee: a valid, 16-byte-aligned stack is active
/// (boot.S step 1), `.bss` has been zeroed (boot.S step 2), and
/// `uefi_memory_map` is a valid pointer handed off by UEFI before
/// `ExitBootServices()` was called (i.e. firmware boot services were
/// still available when this pointer was obtained, per section 3.2's
/// requirement to read "UEFI Memory Map / e820"). Calling this from
/// anywhere other than `boot.S`'s `_start` is unsound.
#[no_mangle]
pub extern "C" fn hal_x86_64_rust_entry(uefi_memory_map: *const u8) -> ! {
    // ------------------------------------------------------------------
    // Step 1: bring up this core's CPU abstraction (feature detection,
    // GDT/IDT — per hal-core section 3.1's per-core bootstrap
    // responsibility) before anything that might fault or interrupt.
    // ------------------------------------------------------------------
    let cpu = cpu::Cpu::new();

    // ------------------------------------------------------------------
    // Step 2: parse the firmware memory map into
    // hal_manifest::raw::MemoryRegionRaw entries (section 3.2) and
    // build this core's minimal identity/kernel mapping.
    //
    // SAFETY: `uefi_memory_map` was validated by this function's own
    // safety contract above (a precondition only boot.S's caller can
    // establish, which is why hal_x86_64_rust_entry itself is not
    // marked unsafe — its only caller is boot.S, which is trusted by
    // construction as part of this same crate's boot path).
    let memory = unsafe { memory::Memory::from_uefi_memory_map(uefi_memory_map) };

    // ------------------------------------------------------------------
    // Step 3: bring up the timer (section 3.3) and interrupt controller
    // (section 3.4) so the rest of boot can rely on both being usable.
    // ------------------------------------------------------------------
    let timer = timer::Timer::new(timer::HpetPresence { present: true });
    let interrupt = interrupt::InterruptCtrl::new();

    // ------------------------------------------------------------------
    // Step 4: run heterogeneous compute discovery (section 3.6) and
    // power/thermal domain discovery (section 3.7). Per section 2's
    // Discovery + Policy model, this ALWAYS runs in full regardless of
    // install profile — profile policy is applied later, in layer 4.
    // ------------------------------------------------------------------
    let compute = compute::ComputeDiscovery::new();
    let power = power::PowerThermalImpl::new(&compute);

    let hal = X86_64Hal {
        cpu,
        memory,
        timer,
        interrupt,
        compute,
        power,
    };

    // ------------------------------------------------------------------
    // Step 5: assemble BootInfo (hal-core/src/boot.rs) from everything
    // discovered above, using the linker-provided image/stack bounds
    // for the kernel-image and boot-reserved ranges.
    // ------------------------------------------------------------------
    let kernel_image_phys_range = (
        // SAFETY: these are linker-defined symbol ADDRESSES (not data
        // to read through the pointer), taken via `&` on an `extern
        // "C" static` — the standard, sound idiom for consuming linker
        // script symbols; see `linker_symbol_addr`'s doc comment.
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
        /* boot_core_id: */ 0, // bootstrap processor is always core 0
    );

    debug_assert!(
        boot_info.validate().is_ok(),
        "hal-x86_64 constructed an internally inconsistent BootInfo"
    );

    // ------------------------------------------------------------------
    // Step 6: hand off to the microkernel.
    //
    // Per 01-HAL-Layer.md section 0, this is a direct Rust function
    // call, not IPC/syscall — HAL and the microkernel share this same
    // Privileged binary. `kernel_main` is the microkernel's entry
    // point (02-Microkernel-Layer.md); for the current phase of this
    // project (HAL-only implementation), it is provided by the
    // separate `kernel-stub` crate's linked-in symbol until the real
    // microkernel (layer 2) is implemented.
    // ------------------------------------------------------------------
    extern "Rust" {
        fn kernel_main(hal: X86_64Hal, boot_info: hal_core::BootInfo) -> !;
    }

    // SAFETY: `kernel_main` is provided by whichever crate this binary
    // is ultimately linked against (kernel-stub for the current MVP
    // phase, per 01-HAL-Layer.md section 8.3's acceptance criterion
    // "تحویل کنترل به یک stub میکروکرنل"; the real microkernel later).
    // Its signature is fixed by this same workspace, so both sides
    // agree on the ABI by construction.
    unsafe { kernel_main(hal, boot_info) }
}