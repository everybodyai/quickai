// Suppress the console window that would otherwise appear behind the GUI on
// a release Windows build; debug builds keep it so `eprintln!`/panics are
// visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

// The window is a client of the runtime crate. These names keep the paths its modules were written
// with (`crate::config`, `crate::runtime::Session`, ...), so that what the window does to the computer
// is plainly the runtime's and the window's own code did not have to change to say so.
use quickai_runtime::{
    bundles, chats, cli, config, configuration_cli, configurations, downloader, events, gpu,
    hardware, jobs, launch, models, paths, ports, prims, provision, remote, runtime, settings,
    sources,
};
// what is said of a run (on a computer QuickAI sets up)
#[cfg(feature = "remote-setup")]
use quickai_runtime::run_file;
#[cfg(test)]
use quickai_runtime::{golden, testkit};

/// Must match the AppImage's `.desktop` file (`StartupWMClass=quickai`, made
/// by `build/appimage.sh` in the `quickai_base` repository) so window managers
/// associate the running window with the launcher icon — otherwise "pin to
/// taskbar" after launch doesn't stick.
const APP_ID: &str = "quickai";

/// The application's icon (decoded once): the windows' (`with_app_icon`) and the logo's.
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

/// `builder` with the application's icon, which every window it opens carries. Not on macOS: there the
/// Dock shows the app's own icon (`QuickAI.app`, drawn in the shape and glass macOS gives icons), and a
/// window's icon would replace it with the flat picture.
fn with_app_icon(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") {
        builder
    } else {
        builder.with_icon(app_icon())
    }
}

fn main() -> eframe::Result {
    // `quickai config check` and `quickai plan` are headless: they run and exit before any window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }

    let native_options = eframe::NativeOptions {
        viewport: with_app_icon(
            egui::ViewportBuilder::default()
                .with_inner_size([980.0, 680.0])
                .with_min_inner_size([640.0, 440.0])
                .with_title("QuickAI")
                .with_app_id(APP_ID),
        ),
        ..Default::default()
    };

    eframe::run_native(
        "quickai",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
