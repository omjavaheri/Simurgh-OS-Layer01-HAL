//! ============================================================================
//! kernel-stub
//!
//! The minimal microkernel stand-in, per 01-HAL-Layer.md section 8,
//! MVP acceptance criterion 3. See Cargo.toml's module-level docs for
//! the full rationale.
//!
//! This binary has exactly one job: receive the `X86_64Hal` value and
//! `BootInfo` that `hal_x86_64_rust_entry` (hal-x86_64/src/lib.rs)
//! constructs, print a confirmation string over the serial port, and
//! halt. This is the FULL scope of "hello from kernel" per section
//! 8.3 — no scheduling, no IPC, no Capability model: those belong to
//! the real microkernel (02-Microkernel-Layer.md), implemented in a
//! later phase of this project.
//! ============================================================================

#![no_std]
#![no_main]
#![forbid(unsafe_op_in_unsafe_fn)]

use core::fmt::Write;
use core::panic::PanicInfo;

use hal_core::BootInfo;

// ----------------------------------------------------------------------------
// Minimal serial (UART 16550) output
//
// A deliberately tiny, standalone serial driver — NOT part of
// hal-x86_64 itself, since a real serial/console driver belongs to the
// layer 3 Device Manager (03-Kernel-Subsystems-Layer.md section 2.1)
// once that layer exists. This is a boot-diagnostics-only shortcut,
// scoped entirely to this stub crate, exactly the kind of "print
// hello" capability section 8.3 asks for and nothing more.
// ----------------------------------------------------------------------------

const COM1_PORT: u16 = 0x3F8;

struct SerialWriter;

impl SerialWriter {
    /// Initializes the COM1 UART to a standard 38400 8N1
    /// configuration, matching the conventional QEMU `-serial stdio`
    /// setup already wired into .cargo/config.toml's runner commands.
    fn init() {
        // SAFETY: COM1's standard I/O port range (0x3F8-0x3FF) is a
        // fixed ISA legacy device address; writing this well-known
        // UART initialization sequence (Intel/generic 16550 datasheet)
        // is the standard, universally safe bring-up procedure for
        // this port on any x86_64 platform, including every QEMU
        // machine type this project's section 8 acceptance criteria
        // target (QEMU always provides an ISA-compatible COM1 by
        // default).
        unsafe {
            out_byte(COM1_PORT + 1, 0x00); // disable interrupts
            out_byte(COM1_PORT + 3, 0x80); // enable DLAB (set baud rate divisor)
            out_byte(COM1_PORT + 0, 0x03); // divisor low byte (38400 baud)
            out_byte(COM1_PORT + 1, 0x00); // divisor high byte
            out_byte(COM1_PORT + 3, 0x03); // 8 bits, no parity, one stop bit
            out_byte(COM1_PORT + 2, 0xC7); // enable FIFO, clear, 14-byte threshold
            out_byte(COM1_PORT + 4, 0x0B); // IRQs disabled, RTS/DSR set
        }
    }

    fn write_byte(byte: u8) {
        // SAFETY: polling the Line Status Register (offset +5, bit 5 =
        // transmitter holding register empty) before writing to the
        // data register (offset +0) is the standard, well-defined
        // 16550 UART transmit procedure — no preconditions beyond the
        // port having been initialized by `init()` above, which every
        // call site in this file guarantees by construction (see
        // `kernel_main`'s call order).
        unsafe {
            while in_byte(COM1_PORT + 5) & 0x20 == 0 {
                core::hint::spin_loop();
            }
            out_byte(COM1_PORT, byte);
        }
    }
}

impl Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            Self::write_byte(byte);
        }
        Ok(())
    }
}

/// # Safety
/// `port` must be a valid, mapped x86_64 I/O port address the caller
/// intends to write `value` to — satisfied by every call site in this
/// file, which only ever targets the fixed COM1 port range documented
/// above.
unsafe fn out_byte(port: u16, value: u8) {
    // SAFETY: forwarded from this function's own contract.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value);
    }
}

/// # Safety
/// Same contract as `out_byte`.
unsafe fn in_byte(port: u16) -> u8 {
    let value: u8;
    // SAFETY: forwarded from this function's own contract.
    unsafe {
        core::arch::asm!("in al, dx", in("dx") port, out("al") value);
    }
    value
}

// ----------------------------------------------------------------------------
// kernel_main — the symbol hal_x86_64_rust_entry calls
// ----------------------------------------------------------------------------

/// The stub microkernel entry point. Signature MUST exactly match the
/// `extern "Rust" { fn kernel_main(...) -> !; }` declaration in
/// hal-x86_64/src/lib.rs's `hal_x86_64_rust_entry` — both sides agree
/// on this ABI by construction, since they are compiled from the same
/// workspace.
#[no_mangle]
pub extern "Rust" fn kernel_main(hal: hal_x86_64::X86_64Hal, boot_info: BootInfo) -> ! {
    SerialWriter::init();
    let mut serial = SerialWriter;

    let _ = writeln!(serial, "hello from kernel");
    let _ = writeln!(serial, "---------------------------------------------");

    // Print a handful of BootInfo/manifest fields, primarily to give
    // this stub SOME observable confirmation (beyond the literal
    // "hello from kernel" string) that HAL's discovery actually ran
    // and produced sane values — useful for manually sanity-checking
    // a QEMU boot run during this phase's development, without this
    // stub trying to be a real diagnostics console (that belongs to a
    // much later layer 3/4 concern).
    let _ = writeln!(serial, "boot protocol: {:?}", boot_info.protocol);
    let _ = writeln!(
        serial,
        "cpu cores: {}",
        boot_info.hardware_manifest.cpu_core_count
    );
    let _ = writeln!(
        serial,
        "memory regions: {}",
        boot_info.hardware_manifest.memory_region_count
    );
    let _ = writeln!(
        serial,
        "compute devices: {}",
        boot_info.hardware_manifest.compute_device_count
    );
    let _ = writeln!(
        serial,
        "power domains: {}",
        boot_info.hardware_manifest.power_domain_count
    );

    match boot_info.validate() {
        Ok(()) => {
            let _ = writeln!(serial, "BootInfo validation: OK");
        }
        Err(err) => {
            let _ = writeln!(serial, "BootInfo validation FAILED: {err}");
        }
    }

    let _ = writeln!(serial, "---------------------------------------------");
    let _ = writeln!(serial, "kernel-stub halting (Phase 1 HAL boot test complete)");

    // `hal` is intentionally unused beyond this point in the stub —
    // the real microkernel (a later project phase) will use it to
    // drive the scheduler/IPC/Capability subsystems
    // (02-Microkernel-Layer.md); this stub's only job is confirming
    // HAL handoff succeeded, per section 8.3's minimal scope.
    let _ = &hal;

    halt_forever();
}

/// Halts this core permanently. Per section 8.3's minimal scope,
/// kernel-stub has nothing further to do once it has printed its
/// confirmation output — there is no scheduler to hand off to yet.
fn halt_forever() -> ! {
    loop {
        // SAFETY: `cli` + `hlt` in a loop is the standard, side-effect-
        // free terminal halt state on x86_64 — same justification as
        // hal-x86_64's own panic_handler (lib.rs).
        unsafe {
            core::arch::asm!("cli");
            core::arch::asm!("hlt");
        }
    }
}

// ----------------------------------------------------------------------------
// Panic handler
//
// kernel-stub is its own top-level binary crate (has its own
// #![no_main]), so it needs its own panic handler — hal-x86_64's
// panic_handler (lib.rs) only applies when hal-x86_64 itself is built
// as the top-level crate, which it is not here (kernel-stub is).
// ----------------------------------------------------------------------------

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    SerialWriter::init();
    let mut serial = SerialWriter;
    let _ = writeln!(serial, "kernel-stub PANIC: {info}");

    // SAFETY: same terminal-halt justification as halt_forever above —
    // there is no recovery path from a panic in this no_std, no-unwind
    // configuration (panic = "abort", per the workspace root
    // Cargo.toml profile).
    unsafe {
        core::arch::asm!("cli");
    }
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}