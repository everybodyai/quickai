// Suppress the console window that would otherwise appear behind the GUI on
// a release Windows build; debug builds keep it so `eprintln!`/panics are
// visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod bundles;
mod cli;
mod config;
mod configuration_cli;
mod configurations;
mod downloader;
mod events;
#[cfg(test)]
mod golden;
mod gpu;
mod hardware;
mod jobs;
mod launch;
mod models;
#[cfg(test)]
mod p3;
mod paths;
mod prims;
mod proxy;

mod runtime;
mod settings;
mod sources;
#[cfg(test)]
mod testkit;

/// Must match the AppImage's `.desktop` file (`StartupWMClass=quickai`, made
/// by `build/appimage.sh` in the `quickai_base` repository) so window managers
/// associate the running window with the launcher icon — otherwise "pin to
/// taskbar" after launch doesn't stick.
const APP_ID: &str = "quickai";

/// The application's icon, which every window it opens carries (decoded once).
fn app_icon() -> std::sync::Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<std::sync::Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        std::sync::Arc::new(
            eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png"))
                .expect("bundled icon should be a valid PNG"),
        )
    })
    .clone()
}

fn main() -> eframe::Result {
    // `quickai config check` and `quickai plan` are headless: they run and exit before any window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([520.0, 380.0])
            .with_title("QuickAI")
            .with_app_id(APP_ID)
            .with_icon(app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "quickai",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
