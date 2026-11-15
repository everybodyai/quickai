//! What the person gives the next message besides its words (C226): pictures and text files, from the paper clip,
//! pasted (Ctrl+V) or dropped on the window, and a picture on the clipboard (a screenshot, Copy image in a browser).
//!
//! They are read away from the window's thread, and only so much is taken, so that nothing given can swamp the chat or
//! hold the window: at most `MOST_PICTURES` pictures go with a message, a picture file is 12 MB at most and a pasted
//! picture is made smaller to `LONGEST_SIDE`, text files put in the box take 256 KB of it in all, and a folder is never
//! looked into. What is past that is not read at all; the chat says what was left and why.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use super::chat::{base64, picture_kind, Picture, Preview, MOST_PICTURE, MOST_TEXT};

/// The most pictures that go with a message.
pub(super) const MOST_PICTURES: usize = 10;
/// A picture pasted from the clipboard larger than this on its longest side is made this size (a model scales it down
/// itself; a screenshot of a large screen would otherwise be several MB of the request).
pub(super) const LONGEST_SIDE: u32 = 2048;
/// The size of a preview, in pixels (a tile of 60 points, drawn sharp on a screen of twice the pixels).
const PREVIEW_SIDE: u32 = 128;
/// How long a paste of text waits to learn whether the clipboard holds files, before it is pasted as the text it is.
const FILES_WAIT: Duration = Duration::from_millis(150);

/// What one thing given became.
#[derive(Debug)]
pub(super) enum Taken {
    /// A picture to go with the message, with its preview when it could be drawn; a pasted one has no name yet.
    Picture(Picture, Option<egui::ColorImage>),
    /// Text to put in the message: the file's block, under its name.
    Text(String),
    /// Why a thing given cannot go with the message, in a sentence.
    Refused(String),
    /// How many were not looked at, as the message takes no more of their kind.
    Left(usize),
}

/// How much more the message takes when the reading starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Room {
    pub pictures: usize,
    pub text: usize,
}

/// Where what is given comes from.
#[derive(Debug)]
pub(super) enum Source {
    Files(Vec<PathBuf>),
    /// The clipboard, when it held no text: copied files, else a picture.
    Clipboard,
}

/// What is being read: what was taken comes as it is ready.
pub(super) struct Reading {
    pub rx: Receiver<Taken>,
}

/// Starts reading what was given, on a thread of its own; `ctx` is asked to draw again as each thing is ready.
pub(super) fn start(source: Source, room: Room, ctx: &egui::Context) -> Reading {
    let (tx, rx) = crossbeam_channel::unbounded();
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("given".into())
        .spawn(move || {
            let said = |taken: Taken| {
                let _ = tx.send(taken);
                ctx.request_repaint();
            };
            match source {
                Source::Files(paths) => take_files(&paths, room, &said),
                Source::Clipboard => take_clipboard(room, &said),
            }
            ctx.request_repaint();
        })
        .expect("start the thread that reads what was given");
    Reading { rx }
}

/// Each file, in order, as far as the room goes: what is past it is counted, not read.
pub(super) fn take_files(paths: &[PathBuf], mut room: Room, said: &dyn Fn(Taken)) {
    let mut left = 0;
    for path in paths {
        let picture = picture_kind(&name_of(path)).is_some();
        if (picture && room.pictures == 0) || (!picture && room.text == 0) {
            left += 1;
            continue;
        }
        let taken = take_file(path, room);
        match &taken {
            Taken::Picture(..) => room.pictures -= 1,
            Taken::Text(block) => room.text = room.text.saturating_sub(block.len()),
            _ => {}
        }
        said(taken);
    }
    if left > 0 {
        said(Taken::Left(left));
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// One file: a picture that can be drawn goes with the message, text goes in it; anything else is refused, and a
/// folder is not looked into.
fn take_file(path: &Path, room: Room) -> Taken {
    let name = name_of(path);
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) => return Taken::Refused(format!("Could not read {name}: {e}.")),
    };
    if meta.is_dir() {
        return Taken::Refused(format!(
            "{name} is a folder: add the files in it, not the folder."
        ));
    }
    if let Some(mime) = picture_kind(&name) {
        if meta.len() > MOST_PICTURE {
            return Taken::Refused(format!("{name} is too big to send (12 MB at most)."));
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => return Taken::Refused(format!("Could not read {name}: {e}.")),
        };
        // a file that is not the picture its name says would fail at the engine: it is said here
        let Some(preview) = preview_of(&bytes) else {
            return Taken::Refused(format!(
                "{name} could not be read as a picture (PNG, JPEG, WebP, GIF)."
            ));
        };
        return Taken::Picture(
            Picture {
                name,
                mime: mime.to_string(),
                base64: base64(&bytes),
                preview: Preview::default(),
            },
            Some(preview),
        );
    }
    if meta.len() > MOST_TEXT {
        return Taken::Refused(format!(
            "{name} is too long to put in a message (256 KB of text at most)."
        ));
    }
    if meta.len() as usize > room.text {
        return Taken::Refused(format!(
            "{name} would make the message longer than 256 KB of text."
        ));
    }
    match std::fs::read(path).map(String::from_utf8) {
        Ok(Ok(text)) => Taken::Text(format!("{name}:\n```\n{}\n```", text.trim_end())),
        Ok(Err(_)) => Taken::Refused(format!(
            "{name} is not text or a picture (PNG, JPEG, WebP, GIF): it cannot go with a message."
        )),
        Err(e) => Taken::Refused(format!("Could not read {name}: {e}.")),
    }
}

/// A picture's preview: at most `PREVIEW_SIDE` on its longest side. `None` when it cannot be read as a picture (the
/// decoders keep to their limits, so a file that says it is enormous is not taken).
pub(super) fn preview_of(bytes: &[u8]) -> Option<egui::ColorImage> {
    let picture = image::load_from_memory(bytes).ok()?;
    Some(color_image(
        &picture.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE).to_rgba8(),
    ))
}

fn color_image(rgba: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    )
}

/// A picture from its pixels (a pasted one): made `LONGEST_SIDE` at most, as a PNG, with its preview.
pub(super) fn picture_of_pixels(rgba: image::RgbaImage) -> Taken {
    let (w, h) = rgba.dimensions();
    let longest = w.max(h);
    let rgba = if longest > LONGEST_SIDE {
        let scale = f64::from(LONGEST_SIDE) / f64::from(longest);
        let size = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
        image::imageops::resize(
            &rgba,
            size(w),
            size(h),
            image::imageops::FilterType::Triangle,
        )
    } else {
        rgba
    };
    let picture = image::DynamicImage::ImageRgba8(rgba);
    let mut png = std::io::Cursor::new(Vec::new());
    if picture.write_to(&mut png, image::ImageFormat::Png).is_err() {
        return Taken::Refused("The picture on the clipboard could not be read.".into());
    }
    let png = png.into_inner();
    if png.len() as u64 > MOST_PICTURE {
        return Taken::Refused(
            "The picture on the clipboard is too big to send (12 MB at most).".into(),
        );
    }
    Taken::Picture(
        Picture {
            name: String::new(),
            mime: "image/png".into(),
            base64: base64(&png),
            preview: Preview::default(),
        },
        Some(color_image(
            &picture.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE).to_rgba8(),
        )),
    )
}

// ---- the clipboard -------------------------------------------------------------------------------------------------

/// The clipboard, opened once: on Linux it is X11's (through XWayland on a Wayland desktop), whose reading answers in a
/// moment, or in 4 s at the most when the program that copied does not.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<T>(read: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
    let mut held = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if held.is_none() {
        *held = arboard::Clipboard::new().ok();
    }
    read(held.as_mut()?)
}

/// The files copied in a file manager, when the clipboard holds some.
fn copied_files() -> Option<Vec<PathBuf>> {
    with_clipboard(|c| c.get().file_list().ok())
        .map(tidied)
        .filter(|files| !files.is_empty())
}

/// The paths of a file list as arboard reads it on Linux: it splits `text/uri-list` at its line ends but keeps the `\r`
/// of each (the list's lines end in CR LF, as file managers write them), and that `\r` is no part of a path.
fn tidied(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| match path.to_str() {
            Some(text) => PathBuf::from(text.trim_end_matches(['\r', '\n'])),
            None => path,
        })
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

/// What a paste with no text finds: copied files, else a picture.
fn take_clipboard(room: Room, said: &dyn Fn(Taken)) {
    if let Some(files) = copied_files() {
        return take_files(&files, room, said);
    }
    let Some(image) = with_clipboard(|c| c.get_image().ok()) else {
        return;
    };
    if room.pictures == 0 {
        return said(Taken::Left(1));
    }
    let rgba = image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    );
    match rgba {
        Some(rgba) => said(picture_of_pixels(rgba)),
        None => said(Taken::Refused(
            "The picture on the clipboard could not be read.".into(),
        )),
    }
}

/// Text that is pasted may be what a file manager offers with copied files: their paths, or their `file://` addresses,
/// one a line. Only then is the clipboard asked whether it holds files, and only for `FILES_WAIT`: the files, when it
/// does. Text that is not such a list is pasted as it is, without asking.
pub(super) fn files_behind(text: &str) -> Option<Vec<PathBuf>> {
    if !looks_like_files(text) {
        return None;
    }
    let (tx, rx): (Sender<Option<Vec<PathBuf>>>, _) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("clipboard files".into())
        .spawn(move || {
            let _ = tx.send(copied_files());
        })
        .ok()?;
    rx.recv_timeout(FILES_WAIT).ok().flatten()
}

/// Each line an absolute path that is there, or a `file://` address: what a file manager pastes as text.
pub(super) fn looks_like_files(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    !lines.is_empty()
        && lines.len() <= 1000
        && lines.iter().all(|line| {
            line.starts_with("file://")
                || (Path::new(line).is_absolute() && Path::new(line).exists())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quickai-given-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            w,
            h,
            image::Rgba([200, 30, 30, 255]),
        ))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
        out.into_inner()
    }

    fn taken(paths: &[PathBuf], room: Room) -> Vec<Taken> {
        let all = std::cell::RefCell::new(Vec::new());
        take_files(paths, room, &|t| all.borrow_mut().push(t));
        all.into_inner()
    }

    const ROOMY: Room = Room {
        pictures: MOST_PICTURES,
        text: MOST_TEXT as usize,
    };

    #[test]
    fn p3_i56_files_given_are_taken_as_the_paper_clip_takes_them_and_a_folder_is_not_looked_into() {
        let dir = scratch("kinds");
        std::fs::write(dir.join("dot.png"), png(300, 200)).unwrap();
        std::fs::write(dir.join("notes.md"), "# Notes\nline\n").unwrap();
        std::fs::write(dir.join("fake.png"), "not a picture").unwrap();
        std::fs::write(dir.join("blob.bin"), [0xff, 0xfe, 0x00]).unwrap();
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        std::fs::write(dir.join("photos/inside.png"), png(2, 2)).unwrap();
        let all = taken(
            &[
                "dot.png", "notes.md", "fake.png", "blob.bin", "photos", "gone.txt",
            ]
            .map(|n| dir.join(n)),
            ROOMY,
        );
        let Taken::Picture(picture, Some(preview)) = &all[0] else {
            panic!("{:?}", all[0])
        };
        assert_eq!(
            (picture.name.as_str(), picture.mime.as_str()),
            ("dot.png", "image/png")
        );
        assert_eq!(
            preview.size,
            [128, 85],
            "the preview keeps the picture's shape"
        );
        assert!(
            matches!(&all[1], Taken::Text(t) if t == "notes.md:\n```\n# Notes\nline\n```"),
            "{:?}",
            all[1]
        );
        let refused: Vec<&str> = all[2..]
            .iter()
            .map(|t| match t {
                Taken::Refused(why) => why.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(
            refused[0].contains("could not be read as a picture"),
            "{refused:?}"
        );
        assert!(refused[1].contains("not text or a picture"), "{refused:?}");
        assert_eq!(
            refused[2],
            "photos is a folder: add the files in it, not the folder."
        );
        assert!(
            refused[3].starts_with("Could not read gone.txt"),
            "{refused:?}"
        );
        assert_eq!(all.len(), 6, "nothing in the folder was taken");
    }

    #[test]
    fn p3_i57_what_is_past_the_room_of_a_message_is_counted_and_not_read() {
        let dir = scratch("room");
        let mut paths = Vec::new();
        for i in 0..14 {
            let path = dir.join(format!("p{i}.png"));
            std::fs::write(&path, png(4, 4)).unwrap();
            paths.push(path);
        }
        // three pictures already wait: seven more go, and the other seven are only counted
        let all = taken(
            &paths,
            Room {
                pictures: 7,
                text: 0,
            },
        );
        assert_eq!(
            all.iter()
                .filter(|t| matches!(t, Taken::Picture(..)))
                .count(),
            7
        );
        assert!(
            matches!(all.last(), Some(Taken::Left(7))),
            "{:?}",
            all.last()
        );
        // text: what is in the box counts; a file that would make it longer than the most is refused unread
        std::fs::write(dir.join("a.txt"), "a".repeat(1000)).unwrap();
        std::fs::write(dir.join("b.txt"), "b".repeat(1000)).unwrap();
        let all = taken(
            &[dir.join("a.txt"), dir.join("b.txt")],
            Room {
                pictures: 0,
                text: 1500,
            },
        );
        assert!(matches!(&all[0], Taken::Text(_)), "{:?}", all[0]);
        assert!(
            matches!(&all[1], Taken::Refused(why) if why == "b.txt would make the message longer than 256 KB of text."),
            "{:?}",
            all[1]
        );
        // a file over the most is refused by its size, before it is read
        std::fs::write(dir.join("big.txt"), vec![b'x'; MOST_TEXT as usize + 1]).unwrap();
        let all = taken(&[dir.join("big.txt")], ROOMY);
        assert!(
            matches!(&all[0], Taken::Refused(why) if why.contains("256 KB of text at most")),
            "{:?}",
            all[0]
        );
    }

    #[test]
    fn p3_i58_a_large_pasted_picture_is_made_smaller_and_sent_as_a_png() {
        let Taken::Picture(picture, Some(preview)) = picture_of_pixels(
            image::RgbaImage::from_pixel(4096, 1024, image::Rgba([1, 2, 3, 255])),
        ) else {
            panic!("no picture")
        };
        assert_eq!(picture.mime, "image/png");
        assert!(picture.name.is_empty(), "the chat names it");
        let bytes = super::super::chat::base64_decoded(&picture.base64).unwrap();
        let sent = image::load_from_memory(&bytes).unwrap();
        assert_eq!((sent.width(), sent.height()), (LONGEST_SIDE, 512));
        assert_eq!(preview.size, [128, 32]);
        // one within the size is sent at its size
        let Taken::Picture(small, _) = picture_of_pixels(image::RgbaImage::new(640, 480)) else {
            panic!()
        };
        let sent =
            image::load_from_memory(&super::super::chat::base64_decoded(&small.base64).unwrap())
                .unwrap();
        assert_eq!((sent.width(), sent.height()), (640, 480));
    }

    #[test]
    fn the_line_ends_of_a_copied_file_list_are_no_part_of_its_paths() {
        assert_eq!(
            tidied(vec![
                PathBuf::from("/home/a/seeds.txt\r"),
                PathBuf::from("/home/a/b.png"),
                PathBuf::from("\r"),
            ]),
            [
                PathBuf::from("/home/a/seeds.txt"),
                PathBuf::from("/home/a/b.png")
            ]
        );
    }

    #[test]
    fn only_text_that_is_a_list_of_files_makes_the_clipboard_be_asked_for_files() {
        let dir = scratch("looks");
        let file = dir.join("a.png");
        std::fs::write(&file, png(1, 1)).unwrap();
        assert!(looks_like_files(&file.display().to_string()));
        assert!(looks_like_files("file:///tmp/x.png\r\nfile:///tmp/y.png\n"));
        assert!(!looks_like_files("What does /usr/bin mean?"));
        assert!(!looks_like_files(
            &dir.join("not-there.png").display().to_string()
        ));
        assert!(!looks_like_files("relative/path.png"));
        assert!(!looks_like_files("  \n"));
    }
}
