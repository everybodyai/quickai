//! What the person chose for talking to QuickAI (`app.voice` in the settings), the speech models whisper.cpp can use,
//! and the languages it hears.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

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
    /// Every answer is read aloud as it comes.
    pub read_answers: bool,
    /// The voice to read in, by the name the speech program knows; `None` is its usual one.
    pub voice: Option<String>,
    /// 0.5 to 2, 1 is the usual speed.
    pub speed: f64,
    /// Hands-free shows the answers without reading them.
    pub hands_free_muted: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: false,
            microphone: None,
            language: None,
            model: SPEECH_MODELS[0].id.to_string(),
            send_when_stopped: false,
            read_answers: false,
            voice: None,
            speed: 1.0,
            hands_free_muted: false,
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
            read_answers: flag("read_answers", d.read_answers),
            voice: text("voice"),
            speed: v
                .get("speed")
                .and_then(Value::as_f64)
                .map(|s| s.clamp(0.5, 2.0))
                .unwrap_or(d.speed),
            hands_free_muted: flag("hands_free_muted", d.hands_free_muted),
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
        (!map.is_empty()).then_some(Value::Object(map))
    }
}

/// A speech model of whisper.cpp, pinned: a file of `ggerganov/whisper.cpp` at one commit, its size and its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechModel {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// What it is good for, in a few words.
    pub about: &'static str,
}

/// The commit of `ggerganov/whisper.cpp` on Hugging Face the files are taken from (2024-10-29).
const REPO_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// The models, smallest first: the first is the default, and the one the bundled packages carry.
pub const SPEECH_MODELS: [SpeechModel; 3] = [
    SpeechModel {
        id: "base",
        name: "Whisper base",
        file: "ggml-base-q8_0.bin",
        bytes: 81_768_585,
        sha256: "c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9",
        about: "Quick; clear speech in a quiet room",
    },
    SpeechModel {
        id: "small",
        name: "Whisper small",
        file: "ggml-small-q8_0.bin",
        bytes: 264_464_607,
        sha256: "49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f",
        about: "Hears accents, noise and other languages better; slower",
    },
    SpeechModel {
        id: "turbo",
        name: "Whisper large turbo",
        file: "ggml-large-v3-turbo-q5_0.bin",
        bytes: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        about: "Hears best; slow on a computer without a strong processor",
    },
];

pub fn speech_model(id: &str) -> Option<&'static SpeechModel> {
    SPEECH_MODELS.iter().find(|m| m.id == id)
}

impl SpeechModel {
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/{REPO_COMMIT}/{}",
            self.file
        )
    }

    /// `82 MB`.
    pub fn size_words(&self) -> String {
        format!("{} MB", (self.bytes as f64 / 1_000_000.0).round())
    }

    /// Where it is on this computer: the package's own (`bundled`), then the one downloaded (`user`). A file of the
    /// wrong size (cut short) is not it.
    pub fn found_in(&self, bundled: &Path, user: &Path) -> Option<PathBuf> {
        [bundled, user]
            .into_iter()
            .map(|dir| dir.join(self.file))
            .find(|p| std::fs::metadata(p).is_ok_and(|m| m.len() == self.bytes))
    }
}

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
            read_answers: true,
            voice: Some("en-gb".into()),
            speed: 1.25,
            hands_free_muted: true,
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
    fn a_model_is_found_where_it_is_whole() {
        let dir = std::env::temp_dir().join(format!("quickai-speech-{}", std::process::id()));
        let (bundled, user) = (dir.join("bundled"), dir.join("user"));
        std::fs::create_dir_all(&user).unwrap();
        let base = &SPEECH_MODELS[0];
        assert_eq!(base.found_in(&bundled, &user), None);
        let file = std::fs::File::create(user.join(base.file)).unwrap();
        file.set_len(base.bytes - 1).unwrap();
        assert_eq!(
            base.found_in(&bundled, &user),
            None,
            "a file cut short is not it"
        );
        file.set_len(base.bytes).unwrap();
        assert_eq!(base.found_in(&bundled, &user), Some(user.join(base.file)));
        assert!(base.url().ends_with("/ggml-base-q8_0.bin"));
        assert_eq!(base.size_words(), "82 MB");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
