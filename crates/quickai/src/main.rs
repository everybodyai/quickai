// Suppress the console window that would otherwise appear behind the GUI on
// a release Windows build; debug builds keep it so `eprintln!`/panics are
// visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod downloader;
mod hardware;
mod models;
mod paths;
mod remote;
mod server_manager;
mod settings;

/// Must match the AppImage's `.desktop` file (`StartupWMClass=quickai`,
/// see `build/appimage.sh`) so window managers associate the running window
/// with the launcher icon — otherwise "pin to taskbar" after launch doesn't
/// stick.
const APP_ID: &str = "quickai";

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png"))
        .expect("bundled icon should be a valid PNG");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([520.0, 380.0])
            .with_title("QuickAI")
            .with_app_id(APP_ID)
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "quickai",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
