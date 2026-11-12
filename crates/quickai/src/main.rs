// Suppress the console window that would otherwise appear behind the GUI on
// a release Windows build; debug builds keep it so `eprintln!`/panics are
// visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

// The window is a client of the runtime crate. These names keep the paths its modules were written
// with (`crate::config`, `crate::runtime::Session`, ...), so that what the window does to the computer
// is plainly the runtime's and the window's own code did not have to change to say so.
use quickai_runtime::{
    bundles, chats, cli, config, configuration_cli, configurations, devices, downloader, events,
    gpu, hardware, jobs, launch, model_store, models, paths, ports, prims, provision, remote,
    runtime, settings, sources, voice,
};
// what is said of a run (on a computer QuickAI sets up)
#[cfg(feature = "remote-setup")]
use quickai_runtime::run_file;
#[cfg(test)]
use quickai_runtime::{golden, testkit};

/// Must match the AppImage's `.desktop` file (`StartupWMClass=quickai`) so window managers
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

/// How long, on Wayland, a window waits for the compositor to ask for its next frame before it is drawn anyway. A
/// compositor asks only a window it shows: one that is covered, minimized or on another workspace is never asked, and
/// the other windows of QuickAI (Add a model, Settings, a problem report) are drawn in the frames of the main one. They
/// stopped with it, and the window, drawing nothing while a download ran, was taken for one that stopped answering.
const FRAME_PATIENCE: std::time::Duration = std::time::Duration::from_millis(100);

/// Whether the windows are Wayland's: winit chooses Wayland when the session says where it is (`WAYLAND_DISPLAY`, or
/// `WAYLAND_SOCKET`), and X11 otherwise.
fn on_wayland(var: impl Fn(&str) -> Option<String>) -> bool {
    cfg!(target_os = "linux")
        && ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
            .iter()
            .any(|name| var(name).is_some_and(|value| !value.is_empty()))
}

/// How a frame is handed to the screen. On Wayland a frame that waits for the compositor (vsync, FIFO) waits for ever
/// when the window it is for is not shown, and every window waits with it, since all are drawn by one thread: a frame
/// there replaces the one that waits instead (mailbox, or immediate where the compositor allows it), and the frames of
/// a shown window are paced by the compositor's requests as before. Elsewhere, vsync.
fn present_mode(wayland: bool) -> eframe::wgpu::PresentMode {
    if wayland {
        eframe::wgpu::PresentMode::AutoNoVsync
    } else {
        eframe::wgpu::PresentMode::AutoVsync
    }
}

fn main() -> eframe::Result {
    // `quickai config check` and `quickai plan` are headless: they run and exit before any window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }

    let wayland = on_wayland(|name| std::env::var(name).ok());
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    wgpu_options.surface.present_mode = present_mode(wayland);
    // a window the compositor does not ask for frames is drawn anyway, at most once a FRAME_PATIENCE while something is
    // to be drawn (a download's progress): its frames no longer wait for the compositor (above)
    #[cfg(target_os = "linux")]
    if wayland {
        winit::platform::wayland::set_frame_callback_patience(Some(FRAME_PATIENCE));
    }

    let native_options = eframe::NativeOptions {
        viewport: with_app_icon(
            egui::ViewportBuilder::default()
                .with_inner_size([980.0, 680.0])
                .with_min_inner_size([640.0, 440.0])
                .with_title("QuickAI")
                .with_app_id(APP_ID),
        ),
        wgpu_options,
        ..Default::default()
    };

    eframe::run_native(
        "quickai",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::{on_wayland, present_mode};
    use eframe::wgpu::PresentMode;

    fn session(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn the_windows_are_wayland_ones_where_winit_makes_them_so() {
        let wayland = cfg!(target_os = "linux");
        assert_eq!(
            on_wayland(session(&[
                ("WAYLAND_DISPLAY", "wayland-0"),
                ("DISPLAY", ":0")
            ])),
            wayland
        );
        assert_eq!(on_wayland(session(&[("WAYLAND_SOCKET", "5")])), wayland);
        // an X11 session, or a Wayland variable left empty (winit takes it for not set)
        assert!(!on_wayland(session(&[("DISPLAY", ":0")])));
        assert!(!on_wayland(session(&[
            ("WAYLAND_DISPLAY", ""),
            ("DISPLAY", ":0")
        ])));
        assert!(!on_wayland(session(&[])));
    }

    #[test]
    fn a_frame_waits_for_the_compositor_only_where_a_hidden_window_does_not_keep_it_waiting() {
        // Wayland: no frame waits for a window the compositor does not show
        assert_eq!(present_mode(true), PresentMode::AutoNoVsync);
        // X11, Windows, macOS: vsync, as eframe has it
        assert_eq!(present_mode(false), PresentMode::AutoVsync);
        assert_eq!(
            present_mode(false),
            eframe::egui_wgpu::WgpuConfiguration::default()
                .surface
                .present_mode
        );
    }
}
