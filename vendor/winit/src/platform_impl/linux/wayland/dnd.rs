// Added for QuickAI, 2026-11-15: files dropped on a window on Wayland. See vendor/README.md in QuickAI's source.
//! Files dragged onto a window and dropped there, on Wayland.
//!
//! A drag that offers `text/uri-list` (what a file manager offers for files) is taken as files: while it is over one of
//! winit's windows, the window gets `HoveredFile` with an empty path (the paths are only read once the files are
//! dropped), and `HoveredFileCancelled` when it leaves; when it is dropped, the list is read through the event loop,
//! never by waiting on it, and the window gets `DroppedFile` for each local file in it, or `HoveredFileCancelled` when
//! it holds none. Any other drag is refused, as winit did before.

use std::io::Read;
use std::path::PathBuf;

use sctk::data_device_manager::data_device::{DataDeviceData, DataDeviceHandler};
use sctk::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::DataSourceHandler;
use sctk::data_device_manager::WritePipe;
use sctk::reexports::calloop::PostAction;
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use crate::event::WindowEvent;
use crate::platform_impl::wayland::state::WinitState;

/// What a file manager offers for files.
const URI_LIST: &str = "text/uri-list";

/// The most of a list of dropped files that is read (a list of a hundred thousand files is about 10 MB): what is past
/// it is left, and the line it cuts with it.
const MOST_LIST: usize = 4 * 1024 * 1024;

fn drag_offer(device: &WlDataDevice) -> Option<DragOffer> {
    device
        .data::<DataDeviceData>()
        .and_then(|data| data.drag_offer())
}

impl DataDeviceHandler for WinitState {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        device: &WlDataDevice,
        _x: f64,
        _y: f64,
        surface: &WlSurface,
    ) {
        let Some(offer) = drag_offer(device) else {
            return;
        };
        let window_id = super::make_wid(surface);
        let files = offer.with_mime_types(|types| types.iter().any(|t| t == URI_LIST));
        if !files || !self.windows.get_mut().contains_key(&window_id) {
            offer.accept_mime_type(offer.serial, None);
            offer.set_actions(DndAction::empty(), DndAction::empty());
            return;
        }
        offer.accept_mime_type(offer.serial, Some(URI_LIST.to_string()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        self.drag_window = Some(window_id);
        self.dispatched_events = true;
        self.events_sink
            .push_window_event(WindowEvent::HoveredFile(PathBuf::new()), window_id);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        // after a drop the drag leaves too: `drop_performed` has taken the window by then
        if let Some(window_id) = self.drag_window.take() {
            self.dispatched_events = true;
            self.events_sink
                .push_window_event(WindowEvent::HoveredFileCancelled, window_id);
        }
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}

    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}

    fn drop_performed(&mut self, conn: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice) {
        let Some(window_id) = self.drag_window.take() else {
            return;
        };
        let pipe = drag_offer(device).and_then(|offer| match offer.receive(URI_LIST.to_string()) {
            Ok(pipe) => Some((offer, pipe)),
            Err(_) => {
                offer.destroy();
                None
            }
        });
        let Some((offer, pipe)) = pipe else {
            self.dispatched_events = true;
            self.events_sink
                .push_window_event(WindowEvent::HoveredFileCancelled, window_id);
            return;
        };
        // the request for the list goes out now; the source writes it while the loop goes on
        let _ = conn.flush();
        let mut list = Vec::new();
        let read = self.loop_handle.insert_source(pipe, move |_, file, state| {
            let mut chunk = [0u8; 64 * 1024];
            // one read each time the pipe is ready: it never waits
            let ended = match file.as_ref().read(&mut chunk) {
                Ok(0) | Err(_) => true,
                Ok(n) => {
                    list.extend_from_slice(&chunk[..n]);
                    list.len() >= MOST_LIST
                }
            };
            if !ended {
                return PostAction::Continue;
            }
            offer.finish();
            offer.destroy();
            let paths = paths_of(&list);
            state.dispatched_events = true;
            if paths.is_empty() {
                state
                    .events_sink
                    .push_window_event(WindowEvent::HoveredFileCancelled, window_id);
            }
            for path in paths {
                state
                    .events_sink
                    .push_window_event(WindowEvent::DroppedFile(path), window_id);
            }
            PostAction::Remove
        });
        if read.is_err() {
            self.dispatched_events = true;
            self.events_sink
                .push_window_event(WindowEvent::HoveredFileCancelled, window_id);
        }
    }
}

impl DataOfferHandler for WinitState {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

/// winit offers nothing to other programs: a source of its own never comes.
impl DataSourceHandler for WinitState {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }

    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: String,
        _: WritePipe,
    ) {
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}

sctk::delegate_data_device!(WinitState);

/// The local files of a `text/uri-list` (RFC 2483): one URI a line, `#` lines are comments; `file:///path` and
/// `file://host/path` give `/path`, with `%XX` decoded. Any other URI, and a line the read cut, is left out.
fn paths_of(list: &[u8]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut lines: Vec<&[u8]> = list.split(|b| *b == b'\n').collect();
    // a last line with no line end is whole when the list was read to its end, and cut when the most was read
    if list.len() >= MOST_LIST && !list.ends_with(b"\n") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with(b"#"))
        .filter_map(|line| line.strip_prefix(b"file://"))
        .filter_map(|rest| {
            let at = rest.iter().position(|b| *b == b'/')?;
            Some(PathBuf::from(std::ffi::OsString::from_vec(decoded(
                &rest[at..],
            ))))
        })
        .collect()
}

/// `%XX` as the byte it stands for.
fn decoded(text: &[u8]) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'%' && i + 2 < text.len() {
            if let (Some(high), Some(low)) = (hex(text[i + 1]), hex(text[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_of_files_gives_their_paths() {
        let list = b"# copied\r\nfile:///home/a/My%20Notes.txt\r\nfile://host/tmp/b.png\r\nhttps://x.org/c.png\r\n";
        assert_eq!(
            paths_of(list),
            [
                PathBuf::from("/home/a/My Notes.txt"),
                PathBuf::from("/tmp/b.png")
            ]
        );
        assert_eq!(
            paths_of(b"file:///tmp/last.png"),
            [PathBuf::from("/tmp/last.png")]
        );
        assert_eq!(decoded(b"100%"), b"100%");
        assert_eq!(decoded(b"%e2%82%ac"), "\u{20ac}".as_bytes());
    }
}
