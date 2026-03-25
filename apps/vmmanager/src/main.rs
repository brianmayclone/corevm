use eframe::egui;

mod app;
mod config;
mod engine;
mod ui;

/// Show a fatal error message on stderr.
fn show_fatal_error(title: &str, message: &str) {
    eprintln!("{}: {}", title, message);
}

fn check_hardware_support() -> Result<(), String> {
    use libcorevm::ffi::corevm_has_hw_support;

    if corevm_has_hw_support() == 1 {
        return Ok(());
    }

    let mut diag = String::from("Hardware virtualization is not available.\n\n");

    #[cfg(target_os = "linux")]
    {
        diag.push_str("Diagnostics:\n");
        // Check if /dev/kvm exists
        if std::path::Path::new("/dev/kvm").exists() {
            diag.push_str("  - /dev/kvm exists but could not be opened.\n");
            diag.push_str("  - Check permissions: is your user in the 'kvm' group?\n");
            diag.push_str("    Run: sudo usermod -aG kvm $USER\n");
        } else {
            diag.push_str("  - /dev/kvm not found.\n");
            diag.push_str("  - KVM kernel module may not be loaded.\n");
            diag.push_str("    Run: sudo modprobe kvm_intel  (or kvm_amd for AMD CPUs)\n");
            diag.push_str("  - If running in a VM, enable nested virtualization.\n");
        }
    }

    Err(diag)
}

fn main() -> eframe::Result {
    // Install signal handlers (Linux) to print backtraces on SIGSEGV/SIGABRT.
    // This catches glibc "double free or corruption" (SIGABRT) and segfaults
    // (SIGSEGV) so we can see the actual call stack in the crash.
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        unsafe {
            extern "C" fn crash_handler(sig: libc::c_int) {
                let name = match sig {
                    libc::SIGSEGV => "SIGSEGV",
                    libc::SIGABRT => "SIGABRT",
                    libc::SIGBUS => "SIGBUS",
                    _ => "UNKNOWN",
                };
                let _ = writeln!(std::io::stderr(), "\n=== CRASH: {} (signal {}) ===", name, sig);
                // Use backtrace crate or std::backtrace
                let bt = std::backtrace::Backtrace::force_capture();
                let _ = writeln!(std::io::stderr(), "{}", bt);
                let _ = std::io::stderr().flush();
                // Re-raise with default handler to get core dump
                unsafe {
                    libc::signal(sig, libc::SIG_DFL);
                    libc::raise(sig);
                }
            }

            libc::signal(libc::SIGSEGV, crash_handler as libc::sighandler_t);
            libc::signal(libc::SIGABRT, crash_handler as libc::sighandler_t);
            libc::signal(libc::SIGBUS, crash_handler as libc::sighandler_t);
        }
    }

    // Under WSL2, Mesa's Zink (OpenGL-over-Vulkan) may fail if the virtual
    // GPU doesn't expose a usable Vulkan physical device.  Let WSLg handle
    // GPU passthrough natively — users can still set LIBGL_ALWAYS_SOFTWARE=1
    // manually if hardware GL causes issues.

    // Check hardware support before launching UI
    if let Err(diag) = check_hardware_support() {
        show_fatal_error("CoreVM Manager - Hardware Support", &diag);
        std::process::exit(1);
    }

    // Force glow (OpenGL) renderer — wgpu/Vulkan fails under WSLg
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("CoreVM Manager"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "CoreVM Manager",
        options,
        Box::new(|_cc| Ok(Box::new(app::CoreVmApp::new()))),
    )
}
