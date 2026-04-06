// Hide console window on Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod models;
mod services;

fn main() -> eframe::Result<()> {
    // Install panic hook that writes crash info to a log file
    // so we can diagnose crashes even when the console window is hidden
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let msg = format!(
            "RBN VFD Display crashed at timestamp {}\n\n{}\n\nBacktrace:\n{}\n",
            timestamp,
            info,
            std::backtrace::Backtrace::force_capture()
        );
        // Write to log file next to executable
        if let Ok(exe) = std::env::current_exe() {
            let log_path = exe.with_file_name("rbn-vfd-crash.log");
            let _ = std::fs::write(&log_path, &msg);
        }
        default_hook(info);
    }));

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([500.0, 600.0])
            .with_min_inner_size([400.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "RBN VFD Display",
        options,
        Box::new(|cc| Ok(Box::new(app::RbnVfdApp::new(cc)))),
    )
}
