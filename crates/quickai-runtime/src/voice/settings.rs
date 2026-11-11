//! What the person chose for talking to QuickAI (`app.voice` in the settings), the speech models (whisper.cpp's, and
//! Omnilingual ASR), and the languages whisper hears.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The pause in talking that ends what was said, by default and at the least and most it may be set to.
pub const DEFAULT_PAUSE_MS: u32 = 1200;
pub const MIN_PAUSE_MS: u32 = 300;
pub const MAX_PAUSE_MS: u32 = 3000;

/// The person's choices. The defaults: voice off (nothing of it shows, and the microphone is never opened, until the
/// person turns it on in Settings › Voice), the system's microphone, the language heard, the base model, the words put
/// in the box to check, answers not read aloud, the system's voice at its usual speed.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSettings {
    /// Voice is on: the microphone, Read aloud, hands-free and their keys. Off until the person turns it on.
    pub enabled: bool,
    /// The microphone by the id `capture::microphones` gives it; `None` is the system's own choice.
    pub microphone: Option<String>,
    /// The language spoken, as whisper names it (`en`, `ml`); `None` lets it tell.
    pub language: Option<String>,
    /// The id of the speech model (`SPEECH_MODELS`).
    pub model: String,
    /// What was said is sent when the person stops talking, not put in the box first.
    pub send_when_stopped: bool,
    /// How long a pause in talking ends what was said, in milliseconds (send when stopped, and hands-free).
    pub pause_ms: u32,
    /// Every answer is read aloud as it comes.
    pub read_answers: bool,
    /// The voice to read in, by the name the speech program knows; `None` is its usual one.
    pub voice: Option<String>,
    /// 0.5 to 2, 1 is the usual speed.
    pub speed: f64,
    /// Hands-free shows the answers without reading them.
    pub hands_free_muted: bool,
    /// Symbols are read too (`#`, `/`, arrows, emoji…), by their names; off, they are left out.
    pub read_symbols: bool,
    /// What reads the answers aloud: nothing until the person chooses (QuickAI ships no voice).
    pub reader: Reader,
}

/// What reads the answers aloud. `voice` (in `VoiceSettings`) is the voice in it: a speaker's name of a pack, the
/// number of a speaker of a folder's voice, or the voice a speech server is asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Reader {
    #[default]
    None,
    /// A pack downloaded from `voices::VOICE_PACKS`, read by quickai-voice.
    Pack(String),
    /// A folder of a voice sherpa-onnx can read, the person's own, read by quickai-voice.
    Folder(String),
    /// A speech server that speaks OpenAI's `/v1/audio/speech` (for voices quickai-voice cannot run): its address,
    /// the model to ask for, and its key when it wants one. What it sends back is played by quickai-voice.
    Server {
        url: String,
        model: String,
        key: String,
    },
    /// A program of the person's own that reads its input aloud (a shell line).
    Program(String),
}

impl Reader {
    fn to_value(&self) -> Option<Value> {
        match self {
            Reader::None => None,
            Reader::Pack(id) => Some(json!({ "kind": "pack", "id": id })),
            Reader::Folder(path) => Some(json!({ "kind": "folder", "path": path })),
            Reader::Server { url, model, key } => {
                Some(json!({ "kind": "server", "url": url, "model": model, "key": key }))
            }
            Reader::Program(command) => Some(json!({ "kind": "program", "command": command })),
        }
    }

    fn from_value(value: Option<&Value>) -> Reader {
        let Some(v) = value.and_then(Value::as_object) else {
            return Reader::None;
        };
        let text = |key: &str| {
            v.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("")
                .to_string()
        };
        match v.get("kind").and_then(Value::as_str) {
            Some("pack") if !text("id").is_empty() => Reader::Pack(text("id")),
            Some("folder") if !text("path").is_empty() => Reader::Folder(text("path")),
            Some("server") => Reader::Server {
                url: text("url"),
                model: text("model"),
                key: text("key"),
            },
            Some("program") if !text("command").is_empty() => Reader::Program(text("command")),
            _ => Reader::None,
        }
    }
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: false,
            microphone: None,
            language: None,
            model: SPEECH_MODELS[0].id.to_string(),
            send_when_stopped: false,
            pause_ms: DEFAULT_PAUSE_MS,
            read_answers: false,
            voice: None,
            speed: 1.0,
            hands_free_muted: false,
            read_symbols: false,
            reader: Reader::None,
        }
    }
}

impl VoiceSettings {
    /// Read from what the settings keep; what is missing or wrong is the default.
    pub fn from_value(value: Option<&Value>) -> VoiceSettings {
        let d = VoiceSettings::default();
        let Some(v) = value.and_then(Value::as_object) else {
            return d;
        };
        let text = |key: &str| {
            v.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        };
        let flag =
            |key: &str, default: bool| v.get(key).and_then(Value::as_bool).unwrap_or(default);
        VoiceSettings {
            enabled: flag("enabled", d.enabled),
            microphone: text("microphone"),
            language: text("language"),
            model: text("model")
                .filter(|m| speech_model(m).is_some())
                .unwrap_or(d.model),
            send_when_stopped: flag("send_when_stopped", d.send_when_stopped),
            pause_ms: v
                .get("pause_ms")
                .and_then(Value::as_u64)
                .map(|ms| (ms as u32).clamp(MIN_PAUSE_MS, MAX_PAUSE_MS))
                .unwrap_or(d.pause_ms),
            read_answers: flag("read_answers", d.read_answers),
            voice: text("voice"),
            speed: v
                .get("speed")
                .and_then(Value::as_f64)
                .map(|s| s.clamp(0.5, 2.0))
                .unwrap_or(d.speed),
            hands_free_muted: flag("hands_free_muted", d.hands_free_muted),
            read_symbols: flag("read_symbols", d.read_symbols),
            reader: Reader::from_value(v.get("reader")),
        }
    }

    /// What the settings keep: only what differs from the default, and nothing when nothing does.
    pub fn to_value(&self) -> Option<Value> {
        let d = VoiceSettings::default();
        let mut map = serde_json::Map::new();
        if self.enabled != d.enabled {
            map.insert("enabled".into(), json!(self.enabled));
        }
        if let Some(m) = &self.microphone {
            map.insert("microphone".into(), json!(m));
        }
        if let Some(l) = &self.language {
            map.insert("language".into(), json!(l));
        }
        if self.model != d.model {
            map.insert("model".into(), json!(self.model));
        }
        if self.send_when_stopped != d.send_when_stopped {
            map.insert("send_when_stopped".into(), json!(self.send_when_stopped));
        }
        if self.pause_ms != d.pause_ms {
            map.insert("pause_ms".into(), json!(self.pause_ms));
        }
        if self.read_answers != d.read_answers {
            map.insert("read_answers".into(), json!(self.read_answers));
        }
        if let Some(v) = &self.voice {
            map.insert("voice".into(), json!(v));
        }
        if (self.speed - d.speed).abs() > 0.001 {
            map.insert("speed".into(), json!((self.speed * 100.0).round() / 100.0));
        }
        if self.hands_free_muted != d.hands_free_muted {
            map.insert("hands_free_muted".into(), json!(self.hands_free_muted));
        }
        if self.read_symbols != d.read_symbols {
            map.insert("read_symbols".into(), json!(self.read_symbols));
        }
        if let Some(reader) = self.reader.to_value() {
            map.insert("reader".into(), reader);
        }
        (!map.is_empty()).then_some(Value::Object(map))
    }
}

/// What writes down with a speech model: quickai-whisper (whisper.cpp), or quickai-voice `--hear` (sherpa-onnx).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechEngine {
    Whisper,
    /// Meta's Omnilingual ASR: 1600 languages, the ones whisper writes poorly among them; it tells the language itself,
    /// and writes without capitals or punctuation.
    Omnilingual,
}

/// A file of a speech model, pinned: its name in the repository (and on this computer), its size and its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// A speech model, pinned: files of a Hugging Face repository at one commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechModel {
    pub id: &'static str,
    pub name: &'static str,
    pub engine: SpeechEngine,
    /// The Hugging Face repository its files are in, and the commit they are taken from.
    pub repo: &'static str,
    pub commit: &'static str,
    /// The folder of the speech models folder its files are kept in; empty for the folder itself.
    pub folder: &'static str,
    /// Its files, the model first.
    pub files: &'static [SpeechFile],
    /// What it is good for, in a few words.
    pub about: &'static str,
}

/// The commit of `ggerganov/whisper.cpp` on Hugging Face whisper's files are taken from (2024-10-29).
const WHISPER_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// The models, whisper's smallest first, then Omnilingual: the first is the default, and the one the bundled packages
/// carry.
pub const SPEECH_MODELS: [SpeechModel; 4] = [
    SpeechModel {
        id: "base",
        name: "Whisper base",
        engine: SpeechEngine::Whisper,
        repo: "ggerganov/whisper.cpp",
        commit: WHISPER_COMMIT,
        folder: "",
        files: &[SpeechFile {
            name: "ggml-base-q8_0.bin",
            bytes: 81_768_585,
            sha256: "c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9",
        }],
        about: "Quick; clear speech in a quiet room",
    },
    SpeechModel {
        id: "small",
        name: "Whisper small",
        engine: SpeechEngine::Whisper,
        repo: "ggerganov/whisper.cpp",
        commit: WHISPER_COMMIT,
        folder: "",
        files: &[SpeechFile {
            name: "ggml-small-q8_0.bin",
            bytes: 264_464_607,
            sha256: "49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f",
        }],
        about: "Hears accents, noise and other languages better; slower",
    },
    SpeechModel {
        id: "turbo",
        name: "Whisper large turbo",
        engine: SpeechEngine::Whisper,
        repo: "ggerganov/whisper.cpp",
        commit: WHISPER_COMMIT,
        folder: "",
        files: &[SpeechFile {
            name: "ggml-large-v3-turbo-q5_0.bin",
            bytes: 574_041_195,
            sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        }],
        about: "Hears best; slow on a computer without a strong processor",
    },
    SpeechModel {
        id: "omnilingual",
        name: "Omnilingual",
        engine: SpeechEngine::Omnilingual,
        repo: "csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-int8-2025-11-12",
        commit: "6abf1ece20cd2308bdb7d13cd78ec1c44fa4c094",
        folder: "omnilingual-asr-300m",
        files: &[
            SpeechFile {
                name: "model.int8.onnx",
                bytes: 365_352_120,
                sha256: "e7c4e54ee4c4c47829cc6667d5d00ed8ea7bef1dcfeef0fce766f77752a2726c",
            },
            SpeechFile {
                name: "tokens.txt",
                bytes: 86_423,
                sha256: "a7a044c52cb29cbe8b0dc1953e92cefd4ca16b0ed968177b6beab21f9a7d0b31",
            },
        ],
        about: "1600 languages, Malayalam, Telugu and Kannada among them; quick; no capitals or punctuation",
    },
];

pub fn speech_model(id: &str) -> Option<&'static SpeechModel> {
    SPEECH_MODELS.iter().find(|m| m.id == id)
}

/// The model Settings suggests for a language whisper writes poorly.
pub fn omnilingual() -> &'static SpeechModel {
    &SPEECH_MODELS[3]
}

impl SpeechModel {
    /// Where `file` is downloaded from.
    pub fn url(&self, file: &SpeechFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.commit, file.name
        )
    }

    /// The bytes of all its files.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }

    /// `82 MB`.
    pub fn size_words(&self) -> String {
        format!("{} MB", (self.bytes() as f64 / 1_000_000.0).round())
    }

    /// The folder its files are kept in, in the speech models folder `dir`.
    pub fn dir_in(&self, dir: &Path) -> PathBuf {
        if self.folder.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(self.folder)
        }
    }

    /// Where it is on this computer: the package's own (`bundled`), then the one downloaded (`user`), as the path of its
    /// model file. Every file is there, whole: a file of the wrong size (cut short) is not it.
    pub fn found_in(&self, bundled: &Path, user: &Path) -> Option<PathBuf> {
        [bundled, user].into_iter().find_map(|dir| {
            let dir = self.dir_in(dir);
            self.files
                .iter()
                .all(|f| std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.bytes))
                .then(|| dir.join(self.files[0].name))
        })
    }
}

/// The languages whisper writes poorly, by whisper's codes: Settings suggests Omnilingual for them. Measured on read
/// sentences (2026-11), Whisper small wrote Malayalam as sounds in Latin letters, Punjabi and Gujarati in Devanagari,
/// and Telugu, Kannada, Marathi and Bengali as made-up or repeated words; Omnilingual wrote all but Gujarati with a few
/// characters in a hundred wrong. Tamil, which whisper writes fairly, is not among them.
pub const WHISPER_WRITES_POORLY: [&str; 7] = ["bn", "gu", "kn", "ml", "mr", "pa", "te"];

/// The languages whisper hears, as it names them, with their names: the common ones first, then the rest in the
/// order of their names.
pub const LANGUAGES: [(&str, &str); 44] = [
    ("en", "English"),
    ("ar", "Arabic"),
    ("bn", "Bengali"),
    ("zh", "Chinese"),
    ("cs", "Czech"),
    ("da", "Danish"),
    ("nl", "Dutch"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("de", "German"),
    ("el", "Greek"),
    ("gu", "Gujarati"),
    ("he", "Hebrew"),
    ("hi", "Hindi"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("kn", "Kannada"),
    ("ko", "Korean"),
    ("ms", "Malay"),
    ("ml", "Malayalam"),
    ("mr", "Marathi"),
    ("no", "Norwegian"),
    ("fa", "Persian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("pa", "Punjabi"),
    ("ro", "Romanian"),
    ("ru", "Russian"),
    ("sr", "Serbian"),
    ("es", "Spanish"),
    ("sw", "Swahili"),
    ("sv", "Swedish"),
    ("tl", "Tagalog"),
    ("ta", "Tamil"),
    ("te", "Telugu"),
    ("th", "Thai"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("ur", "Urdu"),
    ("vi", "Vietnamese"),
    ("cy", "Welsh"),
    ("ne", "Nepali"),
];

/// The name of a language whisper names `code`, or the code.
pub fn language_name(code: &str) -> &str {
    LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(code, |(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_kept_as_nothing_and_a_choice_comes_back() {
        assert_eq!(VoiceSettings::default().to_value(), None);
        assert_eq!(VoiceSettings::from_value(None), VoiceSettings::default());
        let chosen = VoiceSettings {
            enabled: true,
            microphone: Some("hw:CARD=USB,DEV=0".into()),
            language: Some("ml".into()),
            model: "small".into(),
            send_when_stopped: true,
            pause_ms: 800,
            read_answers: true,
            voice: Some("en-gb".into()),
            speed: 1.25,
            hands_free_muted: true,
            read_symbols: true,
            reader: Reader::Server {
                url: "http://127.0.0.1:8880".into(),
                model: "kokoro".into(),
                key: String::new(),
            },
        };
        let kept = chosen.to_value().unwrap();
        assert_eq!(VoiceSettings::from_value(Some(&kept)), chosen);
    }

    #[test]
    fn voice_is_off_until_it_is_turned_on() {
        assert!(!VoiceSettings::default().enabled);
        assert!(!VoiceSettings::from_value(Some(&json!({ "read_answers": true }))).enabled);
        let on = VoiceSettings {
            enabled: true,
            ..VoiceSettings::default()
        };
        assert_eq!(on.to_value(), Some(json!({ "enabled": true })));
        assert!(VoiceSettings::from_value(on.to_value().as_ref()).enabled);
    }

    #[test]
    fn what_is_wrong_in_the_settings_is_the_default() {
        let odd = json!({ "model": "huge", "speed": 9, "language": " ", "read_answers": "yes" });
        let read = VoiceSettings::from_value(Some(&odd));
        assert_eq!(read.model, "base");
        assert_eq!(read.speed, 2.0);
        assert_eq!(read.language, None);
        assert!(!read.read_answers);
    }

    #[test]
    fn the_languages_omnilingual_is_offered_for_are_whispers() {
        for code in WHISPER_WRITES_POORLY {
            assert_ne!(language_name(code), code, "{code} is not in LANGUAGES");
        }
        assert_eq!(omnilingual().engine, SpeechEngine::Omnilingual);
        assert_eq!(speech_model("omnilingual"), Some(omnilingual()));
    }

    #[test]
    fn a_model_is_found_where_it_is_whole() {
        let dir = std::env::temp_dir().join(format!("quickai-speech-{}", std::process::id()));
        let (bundled, user) = (dir.join("bundled"), dir.join("user"));
        std::fs::create_dir_all(&user).unwrap();
        let base = &SPEECH_MODELS[0];
        assert_eq!(base.found_in(&bundled, &user), None);
        let file = std::fs::File::create(user.join(base.files[0].name)).unwrap();
        file.set_len(base.bytes() - 1).unwrap();
        assert_eq!(
            base.found_in(&bundled, &user),
            None,
            "a file cut short is not it"
        );
        file.set_len(base.bytes()).unwrap();
        assert_eq!(
            base.found_in(&bundled, &user),
            Some(user.join("ggml-base-q8_0.bin"))
        );
        assert!(base.url(&base.files[0]).ends_with(
            "/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base-q8_0.bin"
        ));
        assert_eq!(base.size_words(), "82 MB");

        // Omnilingual: its files in a folder of their own, every one of them there
        let omni = omnilingual();
        assert_eq!(omni.id, "omnilingual");
        let folder = user.join("omnilingual-asr-300m");
        std::fs::create_dir_all(&folder).unwrap();
        let model = std::fs::File::create(folder.join("model.int8.onnx")).unwrap();
        model.set_len(omni.files[0].bytes).unwrap();
        assert_eq!(
            omni.found_in(&bundled, &user),
            None,
            "its tokens are missing"
        );
        let tokens = std::fs::File::create(folder.join("tokens.txt")).unwrap();
        tokens.set_len(omni.files[1].bytes).unwrap();
        assert_eq!(
            omni.found_in(&bundled, &user),
            Some(folder.join("model.int8.onnx"))
        );
        assert!(omni
            .url(&omni.files[1])
            .ends_with("/resolve/6abf1ece20cd2308bdb7d13cd78ec1c44fa4c094/tokens.txt"));
        assert_eq!(omni.size_words(), "365 MB");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
