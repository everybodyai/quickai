//! The world the tests of the headless program run in (docs/headless.md): a fake GitHub that lists two
//! releases of a llama.cpp, a file server that serves their archives and the files of a model of the
//! recommended list, a stand-in `llama-server` that really listens on the port it is given, and the
//! shipped documents patched, the way a person patches them, to meet all of it and to look for the program
//! nowhere but where it was installed (so that what a developer has built on this machine is never run).
//!
//! The runtime's own tests (`p3/headless.rs`) and the tests of the program (`quickai-run/tests`) use the
//! same world, so that what one proves of the library the other proves of the binary.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde_json::json;

use crate::config::ConfigDirs;
use crate::provision::{self, Event, Options, Outcome, Request};
use crate::testkit::archives::{sha256_hex, tar_gz, Entry};
use crate::testkit::fake_github::FakeGithub;
use crate::testkit::files::FileServer;
use crate::testkit::{free_port, scratch, Scratch};

pub const STANDIN: &str = include_str!("../../tests/fixtures/launchers/standin.py");
pub const REV: &str = "0123456789abcdef0123456789abcdef01234567";
pub const REPO: &str = "acme/Alpha-4B-GGUF";
pub const FILE: &str = "Alpha-4B-Q4_K_M.gguf";
pub const PROJECTOR: &str = "mmproj-Alpha-4B-F16.gguf";

/// What a release of the fake llama.cpp offers for this machine: the name llama.cpp gives a build of a
/// backend (`vulkan`, or none for the one that names none), with this system's word in it, as the real
/// releases have (`ubuntu`, `macos`, `win`).
pub fn asset_name(tag: &str, backend: Option<&str>) -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "win",
        _ => "ubuntu",
    };
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    match backend {
        Some(b) => format!("llama-{tag}-bin-{os}-{b}-{arch}.tar.gz"),
        None => format!("llama-{tag}-bin-{os}-{arch}.tar.gz"),
    }
}

/// The archive of a build: `llama-server` is a script that runs the stand-in beside it.
pub fn archive(tag: &str) -> Vec<u8> {
    tar_gz(&[
        Entry::exe(
            &format!("llama-{tag}/llama-server"),
            b"#!/bin/sh\nexec python3 \"$(dirname \"$0\")/standin.py\" \"$@\"\n",
        ),
        Entry::file(&format!("llama-{tag}/standin.py"), STANDIN.as_bytes()),
    ])
}

/// The bytes the fake hub serves for a file of the model.
pub fn model_bytes(file: &str) -> Vec<u8> {
    crate::testkit::fake_hub::content(REPO, file, 40_000)
}

pub struct World {
    pub scratch: Scratch,
    /// The data directory when a test gives the application one (`ConfigDirs::data`).
    pub data: PathBuf,
    /// The user's configuration directory: the documents that patch the shipped ones.
    pub config: PathBuf,
    pub files: FileServer,
    pub github: FakeGithub,
    /// The port the stand-in engine is told to listen on.
    pub port: u16,
}

impl World {
    /// A world with releases `b99` and `b100` (each with a plain build and a Vulkan one), a model of the
    /// recommended list with a projector, and the documents patched to meet them.
    pub fn new(name: &str) -> World {
        Self::build(name, "", &["b100", "b99"])
    }

    /// As `new`, with `extra` added to the patch of the engine's document (the stand-in's environment).
    pub fn with(name: &str, spawn_env: &str) -> World {
        Self::build(name, spawn_env, &["b100", "b99"])
    }

    /// As `new`, with releases of these tags, newest first.
    pub fn with_tags(name: &str, tags: &[&str]) -> World {
        Self::build(name, "", tags)
    }

    fn build(name: &str, spawn_env: &str, tags: &[&str]) -> World {
        let scratch = scratch(name);
        let root = scratch.to_path_buf();
        let (data, config) = (root.join("data"), root.join("config"));
        let files = FileServer::start();
        let mut releases = Vec::new();
        for tag in tags {
            let mut assets = Vec::new();
            for backend in [None, Some("vulkan")] {
                let name = asset_name(tag, backend);
                let bytes = archive(tag);
                files.add(&format!("/{name}"), bytes.clone());
                assets.push(json!({
                    "name": name,
                    "browser_download_url": files.url(&format!("/{name}")),
                    "size": bytes.len(),
                }));
            }
            releases.push(json!({
                "tag_name": tag, "draft": false, "prerelease": true, "assets": assets
            }));
        }
        let github = FakeGithub::with_releases(json!(releases));
        for file in [FILE, PROJECTOR] {
            files.add(&format!("/{REPO}/resolve/{REV}/{file}"), model_bytes(file));
        }
        files.add(
            &format!("/{REPO}/resolve/main/Hinted.gguf"),
            model_bytes("Hinted.gguf"),
        );
        let port = free_port();
        let world = World {
            scratch,
            data,
            config,
            files,
            github,
            port,
        };
        world.write_documents(spawn_env, 0);
        world
    }

    /// The patch of `llamacpp` and the catalog document of the model. `size_factor` > 0 makes the model
    /// absurdly big (to find out what happens when there is no room).
    pub fn write_documents(&self, spawn_env: &str, size_factor: u64) {
        std::fs::create_dir_all(self.config.join("engines")).unwrap();
        std::fs::create_dir_all(self.config.join("catalog")).unwrap();
        let env = if spawn_env.is_empty() {
            String::new()
        } else {
            format!("\n[launch.spawn]\nenv = [ {spawn_env} ]\n")
        };
        std::fs::write(
            self.config.join("engines/llamacpp.toml"),
            format!(
                r#"schema = 1
kind = "engine"
id = "llamacpp"

# the program is where it was installed and nowhere else: not next to this test, not in a developer's tree
[locate]
strategies = [ {{ use = "setting", field = "bin_override" }} ]

[acquire.feed]
repo = "acme/llama"
api = "{api}"

[[models.lookup]]
id = "hub"
site = "{site}"
{env}"#,
                api = self.github.base(),
                site = self.files.base(),
            ),
        )
        .unwrap();
        let size = if size_factor > 0 {
            1_000_000_000_000_000u64 * size_factor
        } else {
            model_bytes(FILE).len() as u64
        };
        std::fs::write(
            self.config.join("catalog/t.toml"),
            format!(
                r#"schema = 1
kind = "catalog"
id = "t"
title = "Test"
verified = "2026-10-12"

[[model]]
id = "alpha-4b"
name = "Alpha 4B"
by = "Acme"
type = "chat"
note = "A small model that sees pictures."
license = "Apache-2.0"
params = "4B"
input = ["text", "image"]

[model.run.llamacpp]
repo = "{REPO}"
rev = "{REV}"
files = [ {{ file = "{FILE}", size = {size}, sha256 = "{sum}", best = true }} ]
[model.run.llamacpp.companions]
mmproj = {{ file = "{PROJECTOR}", size = {psize}, sha256 = "{psum}" }}
"#,
                sum = sha256_hex(&model_bytes(FILE)),
                psize = model_bytes(PROJECTOR).len(),
                psum = sha256_hex(&model_bytes(PROJECTOR)),
            ),
        )
        .unwrap();
    }

    pub fn dirs(&self) -> ConfigDirs {
        ConfigDirs {
            user: Some(self.config.clone()),
            data: Some(self.data.clone()),
            ..ConfigDirs::default()
        }
    }

    /// A configuration file of the person's, outside the application: `body` is what follows the header.
    pub fn file(&self, name: &str, body: &str) -> PathBuf {
        let path = self.scratch.join(name);
        std::fs::write(
            &path,
            format!(
                "schema = 1\nkind = \"profile\"\nid = \"{name}\"\nname = \"Test {name}\"\nengine = \"llamacpp\"\n\n{body}"
            ),
        )
        .unwrap();
        path
    }

    /// The usual configuration: the model of the list, on this world's port.
    pub fn usual(&self) -> PathBuf {
        self.file(
            "usual",
            &format!(
                "[[model]]\ncatalog = \"alpha-4b\"\nfile = \"{FILE}\"\n\n[values.llamacpp]\nport = {}\n",
                self.port
            ),
        )
    }

    /// Options that do not look at this machine's cards.
    pub fn options(&self) -> Options {
        Options {
            gpus: Some(Vec::new()),
            ..Options::default()
        }
    }

    pub fn request(&self, target: &Path, options: Options) -> Request {
        Request {
            dirs: self.dirs(),
            target: target.display().to_string(),
            options,
        }
    }

    /// Runs to the end on this thread and returns what happened.
    pub fn up(&self, target: &Path, options: Options) -> (Outcome, Vec<Event>) {
        let mut events = Vec::new();
        let stop = AtomicBool::new(false);
        let outcome = provision::up(
            &self.request(target, options),
            &mut |e| events.push(e),
            &stop,
        );
        (outcome, events)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// The `argv.txt` the stand-in wrote beside itself in the installed build, split into lines.
    pub fn argv(&self) -> Vec<String> {
        let mut found = Vec::new();
        fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, found);
                    } else if path.file_name().is_some_and(|n| n == "argv.txt") {
                        found.push(path);
                    }
                }
            }
        }
        walk(&self.data.join("llama-bin"), &mut found);
        assert_eq!(found.len(), 1, "one build ran: {found:?}");
        std::fs::read_to_string(&found[0])
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

// ---- a hub that remembers who asked ------------------------------------------------------------------

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::mini_http::MiniServer;

/// What was asked of a hub, and whether a login came with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    pub path: String,
    pub authorization: Option<String>,
}

#[derive(Default)]
struct HubState {
    files: BTreeMap<String, Vec<u8>>,
    redirects: BTreeMap<String, String>,
    statuses: BTreeMap<String, u16>,
    /// `(path prefix, token)`: what is under the prefix answers 401 without a login and 403 with another.
    login: Option<(String, String)>,
    asked: Vec<Asked>,
}

/// A file server that remembers every request with its `Authorization` header, answers 401 or 403 where a
/// login is needed, and can send a file on to another address (a hub sends a file on to a download server).
pub struct Hub {
    server: MiniServer,
    state: Arc<Mutex<HubState>>,
}

impl Hub {
    pub fn start() -> Hub {
        let state: Arc<Mutex<HubState>> = Arc::default();
        let shared = state.clone();
        let server = MiniServer::start(None, move |req, resp| {
            let route = req.route().to_string();
            let authorization = req.header("authorization").map(str::to_string);
            let (statuses, login, redirect, file) = {
                let mut state = shared.lock().unwrap();
                state.asked.push(Asked {
                    path: route.clone(),
                    authorization: authorization.clone(),
                });
                (
                    state.statuses.get(&route).copied(),
                    state.login.clone(),
                    state.redirects.get(&route).cloned(),
                    state.files.get(&route).cloned(),
                )
            };
            if let Some(code) = statuses {
                resp.status(code);
                return resp.send(b"no");
            }
            if let Some((prefix, token)) = login {
                if route.starts_with(&prefix) {
                    match authorization {
                        Some(a) if a == format!("Bearer {token}") => {}
                        Some(_) => {
                            resp.status(403);
                            return resp.send(b"not allowed");
                        }
                        None => {
                            resp.status(401);
                            return resp.send(b"login");
                        }
                    }
                }
            }
            if let Some(to) = redirect {
                resp.status(302).header("Location", &to);
                return resp.send(b"");
            }
            match file {
                Some(bytes) => {
                    resp.status(200)
                        .header("Content-Type", "application/octet-stream");
                    resp.send(&bytes);
                }
                None => {
                    resp.status(404);
                    resp.send(b"not found");
                }
            }
        })
        .expect("a free port");
        Hub { server, state }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.port())
    }

    /// The same server by a name that is another host to a client (`localhost`).
    pub fn base_as_localhost(&self) -> String {
        format!("http://localhost:{}", self.server.port())
    }

    pub fn add(&self, path: &str, bytes: Vec<u8>) -> &Self {
        self.state
            .lock()
            .unwrap()
            .files
            .insert(path.to_string(), bytes);
        self
    }

    pub fn redirect(&self, path: &str, to: &str) -> &Self {
        self.state
            .lock()
            .unwrap()
            .redirects
            .insert(path.to_string(), to.to_string());
        self
    }

    /// Answers this status to this path whoever asks.
    pub fn status(&self, path: &str, code: u16) -> &Self {
        self.state
            .lock()
            .unwrap()
            .statuses
            .insert(path.to_string(), code);
        self
    }

    /// What is under `prefix` is for whoever brings `token` as a bearer login.
    pub fn require_login(&self, prefix: &str, token: &str) -> &Self {
        self.state.lock().unwrap().login = Some((prefix.to_string(), token.to_string()));
        self
    }

    pub fn asked(&self) -> Vec<Asked> {
        self.state.lock().unwrap().asked.clone()
    }

    /// The paths asked for, in order.
    pub fn paths(&self) -> Vec<String> {
        self.asked().into_iter().map(|a| a.path).collect()
    }
}
