//! The voices QuickAI can read aloud with. None of them comes with QuickAI: the person downloads one (a pack of
//! sherpa-onnx's, read by the program quickai-voice, which is downloaded with the first), or uses one of their own (a
//! folder of a voice sherpa-onnx can read, a speech server, or a program).
//!
//! - `VOICE_PACKS`: the packs offered for download, pinned: where they are, their size, their hash, their licence and
//!   their voices.
//! - `ModelSpec`: what quickai-voice is told to load, for a pack or for a folder of the person's own (`describe_folder`).
//! - `VOICE_PROGRAM`: the release of quickai-voice QuickAI downloads, and `program`, where it is looked for.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::prims::install::{unpack_archive, ArchiveKind};

/// The kinds of voice sherpa-onnx reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Kokoro,
    Kitten,
    /// Piper's voices among them.
    Vits,
    Matcha,
}

impl ModelKind {
    pub fn word(self) -> &'static str {
        match self {
            ModelKind::Kokoro => "kokoro",
            ModelKind::Kitten => "kitten",
            ModelKind::Vits => "vits",
            ModelKind::Matcha => "matcha",
        }
    }
}

/// A voice for quickai-voice to load: its kind and its files, by the names its `load` request takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    pub kind: ModelKind,
    pub files: BTreeMap<&'static str, String>,
}

impl ModelSpec {
    /// The JSON of the `load` request: a flat object of strings.
    pub fn to_json(&self) -> String {
        let mut object = serde_json::Map::new();
        object.insert("kind".into(), self.kind.word().into());
        for (key, value) in &self.files {
            object.insert((*key).into(), value.clone().into());
        }
        serde_json::Value::Object(object).to_string()
    }
}

/// A voice offered for download: one archive of sherpa-onnx's release `tts-models`, pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePack {
    pub id: &'static str,
    pub name: &'static str,
    /// What it is, in a few words.
    pub about: &'static str,
    pub archive: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// The folder the archive holds.
    pub folder: &'static str,
    /// The model's file in that folder.
    pub model: &'static str,
    pub kind: ModelKind,
    pub license: &'static str,
    /// The names of its voices, in the order of their numbers.
    pub speakers: &'static [&'static str],
}

/// Kokoro v1.0's voices, as the model names them: the first letter is the language, the second w(oman) or m(an).
const KOKORO_V1: &[&str] = &[
    "af_alloy",
    "af_aoede",
    "af_bella",
    "af_heart",
    "af_jessica",
    "af_kore",
    "af_nicole",
    "af_nova",
    "af_river",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_echo",
    "am_eric",
    "am_fenrir",
    "am_liam",
    "am_michael",
    "am_onyx",
    "am_puck",
    "am_santa",
    "bf_alice",
    "bf_emma",
    "bf_isabella",
    "bf_lily",
    "bm_daniel",
    "bm_fable",
    "bm_george",
    "bm_lewis",
    "ef_dora",
    "em_alex",
    "ff_siwis",
    "hf_alpha",
    "hf_beta",
    "hm_omega",
    "hm_psi",
    "if_sara",
    "im_nicola",
    "jf_alpha",
    "jf_gongitsune",
    "jf_nezumi",
    "jf_tebukuro",
    "jm_kumo",
    "pf_dora",
    "pm_alex",
    "pm_santa",
    "zf_xiaobei",
    "zf_xiaoni",
    "zf_xiaoxiao",
    "zf_xiaoyi",
    "zm_yunjian",
    "zm_yunxi",
    "zm_yunxia",
    "zm_yunyang",
    "em_santa",
];

/// Where the packs are: sherpa-onnx's release of voices on GitHub.
const PACKS_AT: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/";

/// The packs offered, the recommended first: the whole Kokoro, which is also the quicker on a desktop's processor (a
/// sentence in a fifth of its own length on 8 threads), then the smaller one, for a slow connection.
pub const VOICE_PACKS: [VoicePack; 2] = [
    VoicePack {
        id: "kokoro",
        name: "Kokoro",
        about: "Natural voices for English and eight other languages",
        archive: "kokoro-multi-lang-v1_0.tar.bz2",
        bytes: 349_886_476,
        sha256: "60703707bc05ccd48d60e1eb723f38b07d6e474795a0b6a588d7220400e0f7a4",
        folder: "kokoro-multi-lang-v1_0",
        model: "model.onnx",
        kind: ModelKind::Kokoro,
        license: "Apache-2.0",
        speakers: KOKORO_V1,
    },
    VoicePack {
        id: "kokoro-small",
        name: "Kokoro, smaller",
        about: "The same voices in a third of the size; slower on most processors",
        archive: "kokoro-int8-multi-lang-v1_0.tar.bz2",
        bytes: 132_353_410,
        sha256: "72c347230107ef8a31098ec4d468a605244beef5f1ed00db97efa13518800653",
        folder: "kokoro-int8-multi-lang-v1_0",
        model: "model.int8.onnx",
        kind: ModelKind::Kokoro,
        license: "Apache-2.0",
        speakers: KOKORO_V1,
    },
];

pub fn voice_pack(id: &str) -> Option<&'static VoicePack> {
    VOICE_PACKS.iter().find(|p| p.id == id)
}

/// Where downloaded voices and the voice program are kept: `~/.quickai/voice`.
pub fn voice_dir() -> PathBuf {
    paths::user_quickai_dir().join("voice")
}

impl VoicePack {
    pub fn url(&self) -> String {
        format!("{PACKS_AT}{}", self.archive)
    }

    /// Where it is once unpacked.
    pub fn dir(&self) -> PathBuf {
        voice_dir().join("packs").join(self.folder)
    }

    /// It was downloaded and unpacked whole (its model is the last file an archive of sherpa-onnx's lists).
    pub fn installed(&self) -> bool {
        let dir = self.dir();
        dir.join(self.model).is_file() && dir.join("tokens.txt").is_file()
    }

    /// What quickai-voice loads for it.
    pub fn spec(&self) -> ModelSpec {
        describe_kokoro(&self.dir(), self.model)
    }

    /// The number of a voice by its name.
    pub fn speaker(&self, name: &str) -> Option<usize> {
        self.speakers.iter().position(|s| *s == name)
    }
}

fn describe_kokoro(dir: &Path, model: &str) -> ModelSpec {
    let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
    // the lexicons it has: English (American, or British for the voices that are) and Chinese
    let lexicons: Vec<String> = ["lexicon-us-en.txt", "lexicon-zh.txt"]
        .iter()
        .filter(|f| dir.join(f).is_file())
        .map(|f| at(f))
        .collect();
    let mut files = BTreeMap::from([
        ("model", at(model)),
        ("voices", at("voices.bin")),
        ("tokens", at("tokens.txt")),
        ("data_dir", at("espeak-ng-data")),
    ]);
    if dir.join("dict").is_dir() {
        files.insert("dict_dir", at("dict"));
    }
    if !lexicons.is_empty() {
        files.insert("lexicon", lexicons.join(","));
    }
    ModelSpec {
        kind: ModelKind::Kokoro,
        files,
    }
}

/// What quickai-voice loads for a folder of the person's own: a voice of sherpa-onnx's (Kokoro, Kitten, Piper or another
/// VITS, Matcha), told by the files it has.
pub fn describe_folder(dir: &Path) -> Result<ModelSpec, String> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let has = |n: &str| names.iter().any(|x| x == n);
    let onnx: Vec<&String> = names.iter().filter(|n| n.ends_with(".onnx")).collect();
    if !has("tokens.txt") || onnx.is_empty() {
        return Err(
            "this folder is not a voice of sherpa-onnx's: it needs a .onnx model and its tokens.txt".into(),
        );
    }
    let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // Matcha: an acoustic model and a vocoder beside it
    if let Some(vocoder) = onnx
        .iter()
        .find(|n| n.contains("vocos") || n.contains("hifigan") || n.contains("vocoder"))
    {
        let acoustic = onnx
            .iter()
            .find(|n| *n != vocoder)
            .ok_or("a Matcha voice needs its acoustic model beside the vocoder")?;
        let mut files = BTreeMap::from([
            ("acoustic_model", at(acoustic)),
            ("vocoder", at(vocoder)),
            ("tokens", at("tokens.txt")),
        ]);
        if has("espeak-ng-data") {
            files.insert("data_dir", at("espeak-ng-data"));
        }
        if has("lexicon.txt") {
            files.insert("lexicon", at("lexicon.txt"));
        }
        return Ok(ModelSpec {
            kind: ModelKind::Matcha,
            files,
        });
    }
    let model = onnx
        .iter()
        .find(|n| n.starts_with("model"))
        .unwrap_or(&onnx[0]);
    if has("voices.bin") {
        if folder.contains("kitten") {
            return Ok(ModelSpec {
                kind: ModelKind::Kitten,
                files: BTreeMap::from([
                    ("model", at(model)),
                    ("voices", at("voices.bin")),
                    ("tokens", at("tokens.txt")),
                    ("data_dir", at("espeak-ng-data")),
                ]),
            });
        }
        return Ok(describe_kokoro(dir, model));
    }
    // a VITS voice, as Piper's are
    let mut files = BTreeMap::from([("model", at(model)), ("tokens", at("tokens.txt"))]);
    if has("espeak-ng-data") {
        files.insert("data_dir", at("espeak-ng-data"));
    }
    if has("lexicon.txt") {
        files.insert("lexicon", at("lexicon.txt"));
    }
    Ok(ModelSpec {
        kind: ModelKind::Vits,
        files,
    })
}

/// A voice of Kokoro's, in words: `af_heart` is "Heart · American English, woman".
pub fn speaker_label(name: &str) -> String {
    let Some((tag, given)) = name.split_once('_') else {
        return name.to_string();
    };
    let mut chars = tag.chars();
    let language = match chars.next() {
        Some('a') => "American English",
        Some('b') => "British English",
        Some('e') => "Spanish",
        Some('f') => "French",
        Some('h') => "Hindi",
        Some('i') => "Italian",
        Some('j') => "Japanese",
        Some('p') => "Portuguese (Brazil)",
        Some('z') => "Chinese",
        _ => return name.to_string(),
    };
    let who = match chars.next() {
        Some('f') => "woman",
        Some('m') => "man",
        _ => "",
    };
    let mut given_chars = given.chars();
    let given = match given_chars.next() {
        Some(first) => first.to_uppercase().chain(given_chars).collect::<String>(),
        None => String::new(),
    };
    if who.is_empty() {
        format!("{given} · {language}")
    } else {
        format!("{given} · {language}, {who}")
    }
}

/// A release of quickai-voice for one system: the archive and its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramAsset {
    pub archive: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// The release of quickai-voice QuickAI downloads (everybodyai/quickai-voice, GPL-3.0: it is a program of its own,
/// started by QuickAI, as llama.cpp's server is).
pub struct VoiceProgram {
    pub version: &'static str,
    pub linux: Option<ProgramAsset>,
    pub windows: Option<ProgramAsset>,
    pub macos: Option<ProgramAsset>,
}

pub const VOICE_PROGRAM: VoiceProgram = VoiceProgram {
    version: "0.1.0",
    // pinned once its release is made
    linux: None,
    windows: None,
    macos: None,
};

impl VoiceProgram {
    /// This system's archive, when there is one to download.
    pub fn here(&self) -> Option<ProgramAsset> {
        if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            self.linux
        } else if cfg!(windows) && cfg!(target_arch = "x86_64") {
            self.windows
        } else if cfg!(target_os = "macos") {
            self.macos
        } else {
            None
        }
    }

    pub fn url(&self, asset: &ProgramAsset) -> String {
        format!(
            "https://github.com/everybodyai/quickai-voice/releases/download/v{}/{}",
            self.version, asset.archive
        )
    }

    /// Where a downloaded release is unpacked.
    pub fn dir(&self) -> PathBuf {
        voice_dir().join("bin").join(self.version)
    }
}

/// Unpacks a downloaded pack (`archive`, its .tar.bz2) where `VoicePack::dir` says, replacing what was there, and
/// removes the archive.
pub fn install_pack(pack: &VoicePack, archive: &Path) -> Result<(), String> {
    install_pack_at(pack, archive, &voice_dir())
}

/// `install_pack` into `root` (`~/.quickai/voice`).
fn install_pack_at(pack: &VoicePack, archive: &Path, root: &Path) -> Result<(), String> {
    let packs = root.join("packs");
    let target = packs.join(pack.folder);
    let staging = packs.join(format!(".unpacking-{}", pack.id));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    unpack_archive(ArchiveKind::TarBz2, archive, &staging, &[])?;
    let unpacked = staging.join(pack.folder);
    if !unpacked.join(pack.model).is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("{} did not hold the voice", pack.archive));
    }
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&unpacked, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(archive);
    Ok(())
}

/// Unpacks a downloaded release of quickai-voice (`archive`, a .tar.gz or .zip) into `VoiceProgram::dir`, replacing
/// what was there, and removes the archive; `program` finds it there.
pub fn install_program(archive: &Path) -> Result<(), String> {
    let kind = ArchiveKind::from_url(&archive.to_string_lossy())
        .ok_or_else(|| format!("{}: {}", archive.display(), ArchiveKind::EXPECTED))?;
    let dir = VOICE_PROGRAM.dir();
    let staging = dir.with_extension("unpacking");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    unpack_archive(kind, archive, &staging, &[])?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::remove_file(archive);
    if program().is_none() {
        return Err("the voice program's archive did not hold it".into());
    }
    Ok(())
}

/// The program's file name on this system.
fn program_name() -> String {
    format!("quickai-voice{}", std::env::consts::EXE_SUFFIX)
}

/// Where quickai-voice is: `QUICKAI_VOICE_BIN`, else the release downloaded into `~/.quickai/voice/bin/<version>`
/// (the archive's folder, or the folder itself), else next to the app.
pub fn program() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("QUICKAI_VOICE_BIN") {
        return Some(PathBuf::from(named)).filter(|p| p.is_file());
    }
    let dir = VOICE_PROGRAM.dir();
    let name = program_name();
    let mut places = vec![dir.join(&name)];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            places.push(entry.path().join(&name));
        }
    }
    if let Some(exe) = paths::exe_dir() {
        places.push(exe.join(&name));
    }
    places.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kokoros_voices_are_said_in_words() {
        assert_eq!(speaker_label("af_heart"), "Heart · American English, woman");
        assert_eq!(speaker_label("bm_george"), "George · British English, man");
        assert_eq!(speaker_label("zf_xiaoxiao"), "Xiaoxiao · Chinese, woman");
        assert_eq!(speaker_label("speaker 3"), "speaker 3");
        let kokoro = voice_pack("kokoro").unwrap();
        assert_eq!(kokoro.speakers.len(), 54);
        assert_eq!(kokoro.speaker("af_heart"), Some(3));
        assert!(kokoro
            .url()
            .ends_with("/tts-models/kokoro-multi-lang-v1_0.tar.bz2"));
    }

    #[test]
    fn a_downloaded_pack_is_unpacked_into_its_folder_and_one_without_its_model_is_refused() {
        use crate::testkit::archives::{tar_bz2, Entry};
        let root = std::env::temp_dir().join(format!("quickai-packs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pack = voice_pack("kokoro").unwrap();
        let archive = root.join(pack.archive);
        std::fs::write(
            &archive,
            tar_bz2(&[
                Entry::file("kokoro-multi-lang-v1_0/model.onnx", b"weights"),
                Entry::file("kokoro-multi-lang-v1_0/tokens.txt", b"a 0"),
                Entry::file("kokoro-multi-lang-v1_0/voices.bin", b"v"),
            ]),
        )
        .unwrap();
        install_pack_at(pack, &archive, &root).unwrap();
        let dir = root.join("packs").join(pack.folder);
        assert_eq!(std::fs::read(dir.join("model.onnx")).unwrap(), b"weights");
        assert!(!archive.exists(), "the archive is removed once unpacked");
        // an archive without the model leaves what was there
        std::fs::write(&archive, tar_bz2(&[Entry::file("other/readme.txt", b"x")])).unwrap();
        assert!(install_pack_at(pack, &archive, &root).is_err());
        assert!(dir.join("model.onnx").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_of_ones_own_is_told_by_its_files() {
        let root = std::env::temp_dir().join(format!("quickai-voices-{}", std::process::id()));
        let kokoro = root.join("kokoro-custom");
        let piper = root.join("vits-piper-en_US-amy");
        let matcha = root.join("matcha-en");
        let empty = root.join("empty");
        for (dir, files) in [
            (&kokoro, &["model.onnx", "voices.bin", "tokens.txt"][..]),
            (&piper, &["en_US-amy-low.onnx", "tokens.txt"][..]),
            (
                &matcha,
                &[
                    "model-steps-3.onnx",
                    "vocos-22khz-univ.onnx",
                    "tokens.txt",
                    "lexicon.txt",
                ][..],
            ),
            (&empty, &["README.md"][..]),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            for f in files {
                std::fs::write(dir.join(f), b"x").unwrap();
            }
        }
        std::fs::create_dir_all(kokoro.join("espeak-ng-data")).unwrap();
        std::fs::create_dir_all(piper.join("espeak-ng-data")).unwrap();
        let k = describe_folder(&kokoro).unwrap();
        assert_eq!(k.kind, ModelKind::Kokoro);
        assert!(k.to_json().contains("\"voices\""), "{}", k.to_json());
        let p = describe_folder(&piper).unwrap();
        assert_eq!(p.kind, ModelKind::Vits);
        assert!(p.files["model"].ends_with("en_US-amy-low.onnx"));
        assert!(p.files.contains_key("data_dir"));
        let m = describe_folder(&matcha).unwrap();
        assert_eq!(m.kind, ModelKind::Matcha);
        assert!(m.files["vocoder"].ends_with("vocos-22khz-univ.onnx"));
        assert!(m.files["acoustic_model"].ends_with("model-steps-3.onnx"));
        assert!(describe_folder(&empty).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
