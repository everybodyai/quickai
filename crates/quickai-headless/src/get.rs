//! `get` (C228): the window of another computer has this one download a model itself, from where its files come from,
//! into QuickAI's models folder here. What to fetch is a file the window copied here (`Download`, as JSON); the files are
//! fetched as the window fetches them (`downloader::spawn_files_with`), checked against their sizes and checksums, and a
//! file that stopped half-way goes on from where it was.
//!
//! One download at a time, apart from the run: its folder is `get` in the data folder (`run_file::get_dir`), held,
//! said and stopped as the run's is (`run.json` with a step for each file and how far it has got, `last.json` for how
//! it ended, `stop` to cancel it). With `--detach` it goes on in the background, when the connection that asked for
//! it has closed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use quickai_core::lookup::present::Download;
use quickai_runtime::config::ConfigDirs;
use quickai_runtime::downloader::{self, FetchWith};
use quickai_runtime::provision::{Event, FileInfo, Secret, StepInfo};
use quickai_runtime::run_file::{self, NotHeld, RunFile};

use crate::args::GetInvocation;
use crate::background::{self, Kind};
use crate::{render, signals};

/// The hub the window's addresses name, which `HF_ENDPOINT` stands in for.
const HUB: &str = "https://huggingface.co";

/// How often what it has got is written to its run file and said.
const SAY_EVERY: Duration = Duration::from_secs(1);

/// What is said when another download goes on.
pub fn busy(run: Option<&RunFile>) -> String {
    match run {
        Some(run) if !run.configuration.is_empty() => format!(
            "the download of {} goes on on this computer; wait for it, or stop it with `quickai-headless get --cancel`",
            run.configuration
        ),
        _ => "another download goes on on this computer; wait for it, or stop it with `quickai-headless get --cancel`"
            .into(),
    }
}

/// `get FILE`: reads what to fetch, and fetches it here (or starts that in the background with `--detach`). `args` is
/// the whole command line, which the background starts again.
pub fn run(get: &GetInvocation, args: &[String]) -> i32 {
    let download = match read(&get.file) {
        Ok(download) => download,
        Err(message) => {
            eprintln!("quickai-headless: {message}");
            return 2;
        }
    };
    let dirs = ConfigDirs::from_environment();
    let dir = run_file::get_dir(&dirs);
    if get.detach {
        return background::begin(args, &dir, Kind::Download);
    }
    signals::install(get.background);
    let label = label_of(&download);
    let held = match run_file::hold(&dir, &label, get.background) {
        Ok(held) => held,
        Err(NotHeld::Busy(run)) => {
            eprintln!("quickai-headless: {}", busy(run.as_deref()));
            return 7;
        }
        Err(NotHeld::Io(e)) => {
            eprintln!(
                "quickai-headless: could not write in {}: {e}",
                dir.display()
            );
            return 1;
        }
    };
    signals::watch_stop_file(dir.clone());
    let models_dir = get
        .models_dir
        .clone()
        .unwrap_or_else(|| quickai_runtime::resolve::models_dirs(&dirs).1);
    let (download, with) = mirrored(download, &|name| std::env::var(name).ok());
    fetch(download, &models_dir, with, held, get.json)
}

/// Reads what to fetch from `file`: a `Download` with at least one file, each with an address and a plain name.
fn read(file: &Path) -> Result<Download, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let download: Download = serde_json::from_str(&text).map_err(|e| {
        format!(
            "{} does not say what to download as QuickAI writes it: {e}",
            file.display()
        )
    })?;
    if download.files.is_empty() {
        return Err(format!("{} names no file to download", file.display()));
    }
    for piece in &download.files {
        if piece.url.trim().is_empty() {
            return Err(format!(
                "{} has no address to fetch it from",
                piece.store_as
            ));
        }
        if quickai_core::lookup::present::safe_file_name(&piece.store_as) != piece.store_as {
            return Err(format!(
                "`{}` is not a plain file name: nothing is written outside the models folder",
                piece.store_as
            ));
        }
    }
    Ok(download)
}

/// What a download is called: its weights' name, and how many more files come with them.
pub fn label_of(download: &Download) -> String {
    let first = download
        .primary()
        .map(|p| p.store_as.clone())
        .unwrap_or_default();
    match download.files.len().saturating_sub(1) {
        0 => first,
        1 => format!("{first} and 1 more file"),
        n => format!("{first} and {n} more files"),
    }
}

/// The download with the hub's addresses on the mirror the variable `HF_ENDPOINT` names, and the login the
/// variables `HF_TOKEN` or `HUGGING_FACE_HUB_TOKEN` give for the hub (or its mirror) and no other site. `var` reads a
/// variable.
fn mirrored(mut download: Download, var: &dyn Fn(&str) -> Option<String>) -> (Download, FetchWith) {
    let set = |name: &str| var(name).filter(|v| !v.trim().is_empty());
    let endpoint = set("HF_ENDPOINT").map(|e| e.trim().trim_end_matches('/').to_string());
    let mut with = FetchWith::default();
    if let Some(token) = set("HF_TOKEN").or_else(|| set("HUGGING_FACE_HUB_TOKEN")) {
        let hub = endpoint.as_deref().unwrap_or(HUB);
        if let Some((_, host, _)) = quickai_core::profile::split_url_with(hub, true) {
            let host = host.split(':').next().unwrap_or(host);
            with.logins.push((host.to_string(), Secret::new(token)));
        }
    }
    if let Some(endpoint) = &endpoint {
        for piece in &mut download.files {
            if let Some(rest) = piece.url.strip_prefix(HUB) {
                piece.url = format!("{endpoint}{rest}");
            }
        }
    }
    (download, with)
}

/// Which file `downloaded` bytes of the whole have reached, and how many bytes of it: the files come one after the
/// other, each `sizes` long when that is known. One whose size is not known takes what is left. `at_least` is the
/// file it had reached before (it never goes back).
fn place(downloaded: u64, sizes: &[Option<u64>], at_least: usize) -> (usize, u64) {
    let last = sizes.len().saturating_sub(1);
    let mut before = 0u64;
    for (i, size) in sizes.iter().enumerate() {
        match size {
            Some(size) if i < last && downloaded >= before + size => before += size,
            _ => {
                let i = i.max(at_least).min(last);
                let before: u64 = sizes[..i].iter().map(|s| s.unwrap_or(0)).sum();
                return (i, downloaded.saturating_sub(before));
            }
        }
    }
    (at_least.min(last), 0)
}

/// Fetches `download` into `models_dir`, saying each file as a step, how far it has got, and how it ended; `held` is
/// the download's folder, which says it to `get --status`. The exit code.
fn fetch(
    download: Download,
    models_dir: &Path,
    with: FetchWith,
    mut held: run_file::Held,
    json: bool,
) -> i32 {
    let mut human = render::Human::new(false);
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut say = |held: &mut run_file::Held, event: Event| {
        held.note(&event);
        if json {
            let _ = writeln!(stdout.lock(), "{}", render::json_line(&event));
        } else {
            human.show(&event, &mut stdout.lock(), &mut stderr.lock());
        }
    };
    let sizes: Vec<Option<u64>> = download.files.iter().map(|p| p.size).collect();
    let texts: Vec<String> = download
        .files
        .iter()
        .map(|p| format!("Download {}", p.store_as))
        .collect();
    let steps = download
        .files
        .iter()
        .zip(&texts)
        .map(|(piece, text)| StepInfo {
            kind: "fetch".into(),
            text: text.clone(),
            bytes: piece.size,
            files: vec![FileInfo {
                name: piece.store_as.clone(),
                how: "fetch".into(),
                from: piece.url.clone(),
                to: folder_of(models_dir, piece.place.as_deref())
                    .join(&piece.store_as)
                    .display()
                    .to_string(),
                bytes: piece.size,
                sha256: piece.sha256.clone(),
            }],
        })
        .collect();
    let count = texts.len();
    say(&mut held, Event::Plan { steps });
    say(
        &mut held,
        Event::Step {
            index: 1,
            of: count,
            text: texts[0].clone(),
        },
    );
    let mut job = downloader::spawn_files_with(download, models_dir.to_path_buf(), with, || {});
    let mut current = 0usize;
    let mut said_at: Option<Instant> = None;
    let outcome = loop {
        if signals::STOP.load(Ordering::SeqCst) {
            job.cancel();
        }
        let ended = job.poll();
        for line in job.take_statuses() {
            say(&mut held, Event::Say { text: line });
        }
        let (index, within) = place(job.downloaded, &sizes, current);
        while current < index {
            say(&mut held, Event::StepDone { index: current + 1 });
            current += 1;
            say(
                &mut held,
                Event::Step {
                    index: current + 1,
                    of: count,
                    text: texts[current].clone(),
                },
            );
        }
        if ended.is_some() || said_at.is_none_or(|at| at.elapsed() >= SAY_EVERY) {
            said_at = Some(Instant::now());
            say(
                &mut held,
                Event::Progress {
                    index: current + 1,
                    done: within,
                    total: sizes[current].or(job.total.filter(|_| count == 1)),
                    detail: None,
                },
            );
        }
        if let Some(ended) = ended {
            break ended;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let (word, message, code) = match outcome {
        Ok(_) => {
            for index in current + 1..=count {
                say(&mut held, Event::StepDone { index });
            }
            ("done", None, 0)
        }
        Err(e) if e == quickai_runtime::jobs::CANCELLED || signals::STOP.load(Ordering::SeqCst) => {
            ("stopped", None, 0)
        }
        Err(e) => ("failed", Some(e), 1),
    };
    say(
        &mut held,
        Event::Finished {
            outcome: word.into(),
            message: message.clone(),
            lack: None,
        },
    );
    if word == "done" && !json {
        println!("Downloaded into {}.", models_dir.display());
    }
    held.record_end(word, message, code);
    // the download's file goes before the exit, which runs no destructor
    drop(held);
    code
}

/// The folder of the models folder a file is kept in: the models folder, or the folder of it `place` names.
fn folder_of(models_dir: &Path, place: Option<&str>) -> PathBuf {
    match place {
        Some(place) if quickai_core::lookup::present::is_place(place) => models_dir.join(place),
        _ => models_dir.to_path_buf(),
    }
}

/// `get --status`: the download that goes on here and how far each file has got, or how the last one ended; as
/// JSON, the get folder's `run_file::Status`.
pub fn status(dir: &Path, json: bool) -> i32 {
    let said = run_file::status(dir, 20);
    if json {
        return match serde_json::to_string(&said) {
            Ok(text) => {
                println!("{text}");
                0
            }
            Err(e) => {
                eprintln!("quickai-headless: {e}");
                1
            }
        };
    }
    match (&said.running, &said.last) {
        (Some(run), _) => {
            println!("Downloading {} (process {}).", run.configuration, run.pid);
            for step in &run.steps {
                let far = match step.total.filter(|t| *t > 0) {
                    Some(total) => format!(
                        "{} of {}",
                        quickai_runtime::hardware::format_bytes(step.done),
                        quickai_runtime::hardware::format_bytes(total)
                    ),
                    None => quickai_runtime::hardware::format_bytes(step.done),
                };
                println!("  {}: {} ({far})", step.text, step.state);
            }
        }
        (None, Some(last)) => {
            println!("No download goes on.");
            let why = last
                .message
                .as_deref()
                .map(|m| format!(": {m}"))
                .unwrap_or_default();
            println!(
                "The last, of {}, ended {} ago ({}{why}).",
                last.configuration,
                background::how_long(run_file::now().saturating_sub(last.ended)),
                last.outcome
            );
        }
        (None, None) => println!("No download goes on."),
    }
    0
}

/// `get --cancel`: asks the download to stop, and waits until it has (`wait`).
pub fn cancel(dir: &Path, wait: bool) -> i32 {
    let Some(run) = run_file::running(dir) else {
        println!("No download goes on.");
        return 0;
    };
    if let Err(e) = run_file::ask_stop(dir) {
        eprintln!("quickai-headless: could not ask the download to stop: {e}");
        return 1;
    }
    if !wait {
        println!("The download of {} was asked to stop.", run.configuration);
        return 0;
    }
    let asked = Instant::now();
    while run_file::running(dir).is_some() {
        if asked.elapsed() > Duration::from_secs(60) {
            eprintln!(
                "quickai-headless: the download of {} has not stopped in a minute; it goes on stopping.",
                run.configuration
            );
            return 1;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    println!("The download of {} stopped.", run.configuration);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickai_core::lookup::present::Piece;

    fn piece(store_as: &str, url: &str, size: Option<u64>) -> Piece {
        Piece {
            kind: None,
            label: "model".into(),
            url: url.into(),
            store_as: store_as.into(),
            name: store_as.into(),
            size,
            sha256: None,
            place: None,
        }
    }

    #[test]
    fn the_bytes_of_the_whole_are_said_of_the_file_they_have_reached() {
        let sizes = [Some(100), Some(50)];
        assert_eq!(place(0, &sizes, 0), (0, 0));
        assert_eq!(place(99, &sizes, 0), (0, 99));
        assert_eq!(place(100, &sizes, 0), (1, 0));
        assert_eq!(place(140, &sizes, 0), (1, 40));
        // the last takes what is more than the sizes said
        assert_eq!(place(160, &sizes, 0), (1, 60));
        // a file whose size is not known takes what is left
        assert_eq!(place(500, &[None, Some(50)], 0), (0, 500));
        // it never goes back
        assert_eq!(place(10, &sizes, 1), (1, 0));
    }

    #[test]
    fn the_hub_is_put_on_the_mirror_and_its_login_goes_to_it_alone() {
        let download = Download {
            files: vec![
                piece(
                    "m.gguf",
                    "https://huggingface.co/o/r/resolve/main/m.gguf",
                    Some(1),
                ),
                piece("p.gguf", "https://example.org/p.gguf", Some(1)),
            ],
            bundle: None,
            source: "huggingface:o/r".into(),
            mode: None,
        };
        let vars = |name: &str| match name {
            "HF_ENDPOINT" => Some("https://mirror.example/".to_string()),
            "HUGGING_FACE_HUB_TOKEN" => Some("hf_x".to_string()),
            _ => None,
        };
        let (download, with) = mirrored(download, &vars);
        assert_eq!(
            download.files[0].url,
            "https://mirror.example/o/r/resolve/main/m.gguf"
        );
        assert_eq!(download.files[1].url, "https://example.org/p.gguf");
        assert_eq!(with.logins.len(), 1);
        assert_eq!(with.logins[0].0, "mirror.example");
        let (_, none) = mirrored(
            Download {
                files: vec![piece("m.gguf", "https://huggingface.co/m.gguf", None)],
                bundle: None,
                source: String::new(),
                mode: None,
            },
            &|_| None,
        );
        assert!(none.logins.is_empty());
    }

    #[test]
    fn what_to_fetch_is_read_and_a_name_that_climbs_out_is_refused() {
        let dir = std::env::temp_dir().join(format!("qa-get-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = Download {
            files: vec![piece("m.gguf", "https://huggingface.co/m.gguf", Some(3))],
            bundle: Some("vision".into()),
            source: "huggingface:o/r".into(),
            mode: None,
        };
        let file = dir.join("get.json");
        std::fs::write(&file, serde_json::to_string(&good).unwrap()).unwrap();
        assert_eq!(read(&file).unwrap(), good);
        assert_eq!(label_of(&good), "m.gguf");
        let bad = Download {
            files: vec![piece("../m.gguf", "https://huggingface.co/m.gguf", None)],
            ..good.clone()
        };
        std::fs::write(&file, serde_json::to_string(&bad).unwrap()).unwrap();
        assert!(read(&file).unwrap_err().contains("plain file name"));
        std::fs::write(&file, "{}").unwrap();
        assert!(read(&file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
