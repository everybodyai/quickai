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

/// A file of a voice in a Hugging Face repository, pinned: where it is there, the name it is kept under, its size and
/// its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackFile {
    pub path: &'static str,
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// Where a voice offered for download comes from, pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackSource {
    /// One .tar.bz2 of sherpa-onnx's release `tts-models`, holding the pack's folder.
    Archive {
        archive: &'static str,
        bytes: u64,
        sha256: &'static str,
    },
    /// Files of a Hugging Face repository at one commit, kept in the pack's folder.
    HuggingFace {
        repo: &'static str,
        commit: &'static str,
        files: &'static [PackFile],
    },
}

/// A voice offered for download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePack {
    pub id: &'static str,
    pub name: &'static str,
    /// What it is, in a few words.
    pub about: &'static str,
    pub source: PackSource,
    /// The folder it is kept in (the one its archive holds).
    pub folder: &'static str,
    /// The model's file in that folder.
    pub model: &'static str,
    pub kind: ModelKind,
    pub license: &'static str,
    /// The names of its voices, in the order of their numbers.
    pub speakers: &'static [&'static str],
    /// A sentence in its language, for Settings › Voice to read with it (`sample`).
    pub sample: &'static str,
    /// A voice in Piper's own form (its model and the model's .json): made into one sherpa-onnx reads once it is here
    /// (`piper_to_sherpa`), and read with the espeak-ng data such voices share (`ESPEAK_DATA`).
    pub piper: bool,
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

/// Where the packs are: sherpa-onnx's release of voices on GitHub; Meta's MMS voices in sherpa-onnx's form, and Piper's
/// voices sherpa-onnx has not made its own, on Hugging Face.
const PACKS_AT: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/";
const MMS_REPO: &str = "willwade/mms-tts-multilingual-models-onnx";
const MMS_COMMIT: &str = "4f49fc254f11ddb0e35fbf25b6e48b04463a6779";
const PIPER_REPO: &str = "rhasspy/piper-voices";
const PIPER_COMMIT: &str = "cf910fb59f1d79ab3b625a63993a108751c6c25d";

/// The packs offered, the recommended first: the whole Kokoro, which is also the quicker on a desktop's processor (a
/// sentence in a fifth of its own length on 8 threads), then the smaller one, for a slow connection; then a voice for
/// each of the languages Kokoro lacks that QuickAI's people asked for, the clearest there is (2026-11: each read
/// sentences that speech recognition wrote back, Omnilingual large, with this many characters wrong in a hundred):
/// - Arabic: Nabra 10 (Piper's Kareem, 8, has no licence); Persian: Amir 6; Urdu: Fasih 10.
/// - Malayalam: Piper's Meera 3 and Arjun 4 (MMS 11); Telugu: Piper's Maya 2 and Venkatesh 8 (Padmavathi 31, MMS 42);
///   Nepali: Piper's Chitwan 8.
/// - MMS, the only voice there is: Kannada 2, Tamil 4, Assamese 10, Gujarati 11, Bengali and Marathi 13 (Piper's
///   Google voices of the two do not load in sherpa-onnx), Punjabi 17, Odia 18.
///
/// Each keeps its own licence, which Settings shows: MMS's is for non-commercial use, and IIT Madras's IndicTTS
/// licence (Meera, Arjun, Maya) asks a business to agree terms with it.
pub const VOICE_PACKS: [VoicePack; 18] = [
    VoicePack {
        id: "kokoro",
        name: "Kokoro",
        about: "Natural voices for English, Hindi and seven other languages",
        source: PackSource::Archive {
            archive: "kokoro-multi-lang-v1_0.tar.bz2",
            bytes: 349_886_476,
            sha256: "60703707bc05ccd48d60e1eb723f38b07d6e474795a0b6a588d7220400e0f7a4",
        },
        folder: "kokoro-multi-lang-v1_0",
        model: "model.onnx",
        kind: ModelKind::Kokoro,
        license: "Apache-2.0",
        speakers: KOKORO_V1,
        sample: "Hello. This is how QuickAI reads its answers.",
        piper: false,
    },
    VoicePack {
        id: "kokoro-small",
        name: "Kokoro, smaller",
        about: "The same voices in a third of the size; slower on most processors",
        source: PackSource::Archive {
            archive: "kokoro-int8-multi-lang-v1_0.tar.bz2",
            bytes: 132_353_410,
            sha256: "72c347230107ef8a31098ec4d468a605244beef5f1ed00db97efa13518800653",
        },
        folder: "kokoro-int8-multi-lang-v1_0",
        model: "model.int8.onnx",
        kind: ModelKind::Kokoro,
        license: "Apache-2.0",
        speakers: KOKORO_V1,
        sample: "Hello. This is how QuickAI reads its answers.",
        piper: false,
    },
    // Nabra-82M (oddadmix/Nabra-82M-v0.1): Kokoro fine-tuned for Modern Standard Arabic, in sherpa-onnx's packing
    VoicePack {
        id: "arabic-nabra",
        name: "Arabic · Nabra",
        about: "A natural voice for Modern Standard Arabic (Kokoro, fine-tuned)",
        source: PackSource::Archive {
            archive: "nabra-82m-arabic-int8.tar.bz2",
            bytes: 82_810_819,
            sha256: "a8608a504b1db4dd538b821409fe50b2d5cf37650c0a8753fa215bd6445e9964",
        },
        folder: "nabra-82m-arabic-int8",
        model: "model.int8.onnx",
        kind: ModelKind::Kokoro,
        license: "Apache-2.0",
        speakers: &["Nabra"],
        sample: "مرحبًا. هكذا تُقرأ الإجابات بصوت عالٍ.",
        piper: false,
    },
    VoicePack {
        id: "persian-amir",
        name: "Persian · Amir",
        about: "A man's voice for Persian (Farsi), by Piper",
        source: PackSource::Archive {
            archive: "vits-piper-fa_IR-amir-medium.tar.bz2",
            bytes: 67_180_373,
            sha256: "46af65c1cdd86ec300db3998f2bf31a68d9e38da930c0655c1ff6f610e84f864",
        },
        folder: "vits-piper-fa_IR-amir-medium",
        model: "fa_IR-amir-medium.onnx",
        kind: ModelKind::Vits,
        license: "CC0-1.0",
        speakers: &["Amir"],
        sample: "سلام. پاسخ‌ها این‌گونه با صدای بلند خوانده می‌شوند.",
        piper: false,
    },
    VoicePack {
        id: "urdu-fasih",
        name: "Urdu · Fasih",
        about: "A man's voice for Urdu, by Piper",
        source: PackSource::Archive {
            archive: "vits-piper-ur_PK-fasih-medium.tar.bz2",
            bytes: 67_227_718,
            sha256: "de64e03109f3c896b7cf910db6d9ffd2703e3d665580d9a19bedbf3e0760d0e4",
        },
        folder: "vits-piper-ur_PK-fasih-medium",
        model: "ur_PK-fasih-medium.onnx",
        kind: ModelKind::Vits,
        license: "MIT",
        speakers: &["Fasih"],
        sample: "السلام علیکم۔ جوابات اس طرح بلند آواز سے پڑھے جاتے ہیں۔",
        piper: false,
    },
    VoicePack {
        id: "assamese-mms",
        name: "Assamese · MMS",
        about: "Meta's MMS voice for Assamese",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "asm/model.onnx",
                    name: "model.onnx",
                    bytes: 114_038_532,
                    sha256: "85860bce4cf4cc1b985e48b5916eadf27a684e2e35dbc9ccef71c98f2a43e285",
                },
                PackFile {
                    path: "asm/tokens.txt",
                    name: "tokens.txt",
                    bytes: 440,
                    sha256: "2f61bedac3482dffdd0c32c0346188726d6bdf0f99be24bd5b10bfecacd0b973",
                },
            ],
        },
        folder: "mms-tts-asm",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "নমস্কাৰ, আপুনি কেনে আছে?",
        piper: false,
    },
    VoicePack {
        id: "bengali-mms",
        name: "Bengali · MMS",
        about: "Meta's MMS voice for Bengali",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "ben/model.onnx",
                    name: "model.onnx",
                    bytes: 114_044_676,
                    sha256: "c67081369061e87ba302bfbc0445543c4c23c04133514b7d6c7be2710363bc42",
                },
                PackFile {
                    path: "ben/tokens.txt",
                    name: "tokens.txt",
                    bytes: 480,
                    sha256: "a74190ec42b8f0afb349276430c716f1b585090d8fe6d3c1d6d5fe510798a387",
                },
            ],
        },
        folder: "mms-tts-ben",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "নমস্কার, আপনি কেমন আছেন?",
        piper: false,
    },
    VoicePack {
        id: "gujarati-mms",
        name: "Gujarati · MMS",
        about: "Meta's MMS voice for Gujarati",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "guj/model.onnx",
                    name: "model.onnx",
                    bytes: 114_033_924,
                    sha256: "6d9b4f268765e19b6620983bd698c5e4fa943309560b6c7b199b50a4aae30d47",
                },
                PackFile {
                    path: "guj/tokens.txt",
                    name: "tokens.txt",
                    bytes: 402,
                    sha256: "2d855f2affb7586cc6be095a4382eb0bee2a22242deda32c86aab6b1a810d8c4",
                },
            ],
        },
        folder: "mms-tts-guj",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "નમસ્તે, તમે કેમ છો?",
        piper: false,
    },
    VoicePack {
        id: "kannada-mms",
        name: "Kannada · MMS",
        about: "Meta's MMS voice for Kannada",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "kan/model.onnx",
                    name: "model.onnx",
                    bytes: 114_045_444,
                    sha256: "9644cfa46c5802c4c2c8196f91eedc0cd54d46527bbc32589ebaf967ed07591f",
                },
                PackFile {
                    path: "kan/tokens.txt",
                    name: "tokens.txt",
                    bytes: 487,
                    sha256: "a4a44037f7492c9e5b69a4483250e54af9a2d528bd4e19de95a21ff0c5e82a8a",
                },
            ],
        },
        folder: "mms-tts-kan",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "ನಮಸ್ಕಾರ, ನೀವು ಹೇಗಿದ್ದೀರಿ?",
        piper: false,
    },
    VoicePack {
        id: "malayalam-meera",
        name: "Malayalam · Meera",
        about: "A voice for Malayalam, by Piper",
        source: PackSource::Archive {
            archive: "vits-piper-ml_IN-meera-medium.tar.bz2",
            bytes: 67_195_010,
            sha256: "c8d870441e60870b7e8781c26eb22104aff51587b63bee94aba5e5f6c9fbb71f",
        },
        folder: "vits-piper-ml_IN-meera-medium",
        model: "ml_IN-meera-medium.onnx",
        kind: ModelKind::Vits,
        license: "IndicTTS (IIT Madras)",
        speakers: &["Meera"],
        sample: "നമസ്കാരം, സുഖമാണോ?",
        piper: false,
    },
    VoicePack {
        id: "malayalam-arjun",
        name: "Malayalam · Arjun",
        about: "A voice for Malayalam, by Piper",
        source: PackSource::Archive {
            archive: "vits-piper-ml_IN-arjun-medium.tar.bz2",
            bytes: 67_222_458,
            sha256: "3058d098e8b1ffcdd6069e96b1d492f319333235912a627c309c7c54cea59acf",
        },
        folder: "vits-piper-ml_IN-arjun-medium",
        model: "ml_IN-arjun-medium.onnx",
        kind: ModelKind::Vits,
        license: "IndicTTS (IIT Madras)",
        speakers: &["Arjun"],
        sample: "നമസ്കാരം, സുഖമാണോ?",
        piper: false,
    },
    VoicePack {
        id: "marathi-mms",
        name: "Marathi · MMS",
        about: "Meta's MMS voice for Marathi",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "mar/model.onnx",
                    name: "model.onnx",
                    bytes: 114_043_908,
                    sha256: "046cecf7cca77680c9ca319953e777c8d0233ce78e9b55a47aa909edf87d99e8",
                },
                PackFile {
                    path: "mar/tokens.txt",
                    name: "tokens.txt",
                    bytes: 490,
                    sha256: "4d968029d0754b41633cb0871cce6796a5ab3d3bc2b9b91c5721cfdf85156083",
                },
            ],
        },
        folder: "mms-tts-mar",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "नमस्कार, तुम्ही कसे आहात?",
        piper: false,
    },
    VoicePack {
        id: "nepali-chitwan",
        name: "Nepali · Chitwan",
        about: "A voice for Nepali, by Piper",
        source: PackSource::Archive {
            archive: "vits-piper-ne_NP-chitwan-medium.tar.bz2",
            bytes: 67_213_467,
            sha256: "65fa157c090844bbe5b38c1c3e4321535f569fdbb526c08aaa2ad22c09d51279",
        },
        folder: "vits-piper-ne_NP-chitwan-medium",
        model: "ne_NP-chitwan-medium.onnx",
        kind: ModelKind::Vits,
        license: "CC0-1.0",
        speakers: &["Chitwan"],
        sample: "नमस्ते, तपाईंलाई कस्तो छ?",
        piper: false,
    },
    VoicePack {
        id: "odia-mms",
        name: "Odia · MMS",
        about: "Meta's MMS voice for Odia",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "ory/model.onnx",
                    name: "model.onnx",
                    bytes: 114_046_212,
                    sha256: "ab867f39b00f1fcb39dc97f5079150959b50df50d7f10e75232c56b52f19c021",
                },
                PackFile {
                    path: "ory/tokens.txt",
                    name: "tokens.txt",
                    bytes: 495,
                    sha256: "bc90e5423446ca730aef6b56a4656cb07421c7538429a50b9c396fe0758fd587",
                },
            ],
        },
        folder: "mms-tts-ory",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "ନମସ୍କାର, ଆପଣ କେମିତି ଅଛନ୍ତି?",
        piper: false,
    },
    VoicePack {
        id: "punjabi-mms",
        name: "Punjabi · MMS",
        about: "Meta's MMS voice for Punjabi",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "pan/model.onnx",
                    name: "model.onnx",
                    bytes: 114_033_156,
                    sha256: "63e6eccea5b296ab8e9d01107bd5d7a090d5db19d3b61119ba5f920f1430fa87",
                },
                PackFile {
                    path: "pan/tokens.txt",
                    name: "tokens.txt",
                    bytes: 397,
                    sha256: "91344d6f3d6287e8adc71e66c7e9ca83f5f6e9a7d0c24f043a219773d9dec163",
                },
            ],
        },
        folder: "mms-tts-pan",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "ਸਤ ਸ੍ਰੀ ਅਕਾਲ, ਤੁਸੀਂ ਕਿਵੇਂ ਹੋ?",
        piper: false,
    },
    VoicePack {
        id: "tamil-mms",
        name: "Tamil · MMS",
        about: "Meta's MMS voice for Tamil",
        source: PackSource::HuggingFace {
            repo: MMS_REPO,
            commit: MMS_COMMIT,
            files: &[
                PackFile {
                    path: "tam/model.onnx",
                    name: "model.onnx",
                    bytes: 114_032_388,
                    sha256: "c44055b2e7c5719ca26708f6eb03680b0f1bbf6fdcb0df6480478f872f1f6e34",
                },
                PackFile {
                    path: "tam/tokens.txt",
                    name: "tokens.txt",
                    bytes: 375,
                    sha256: "0b3f692319bb5fae8658e2f84bf252bca92450d0207bbba7273caa1a182d81b8",
                },
            ],
        },
        folder: "mms-tts-tam",
        model: "model.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-NC-4.0",
        speakers: &["MMS"],
        sample: "வணக்கம், நீங்கள் எப்படி இருக்கிறீர்கள்?",
        piper: false,
    },
    VoicePack {
        id: "telugu-maya",
        name: "Telugu · Maya",
        about: "A voice for Telugu, by Piper",
        source: PackSource::HuggingFace {
            repo: PIPER_REPO,
            commit: PIPER_COMMIT,
            files: &[
                PackFile {
                    path: "te/te_IN/maya/medium/te_IN-maya-medium.onnx",
                    name: "te_IN-maya-medium.onnx",
                    bytes: 62_950_044,
                    sha256: "c3518ad4e3ca8ea6059c1e002f3772068f634960f58b237a96ff629db1c6200e",
                },
                PackFile {
                    path: "te/te_IN/maya/medium/te_IN-maya-medium.onnx.json",
                    name: "te_IN-maya-medium.onnx.json",
                    bytes: 5_040,
                    sha256: "c07074aadf0a33e230647611af9041e1fb6609b995d017ee95009586a491508f",
                },
            ],
        },
        folder: "piper-te-in-maya-medium",
        model: "te_IN-maya-medium.onnx",
        kind: ModelKind::Vits,
        license: "IndicTTS (IIT Madras)",
        speakers: &["Maya"],
        sample: "నమస్కారం, మీరు ఎలా ఉన్నారు?",
        piper: true,
    },
    VoicePack {
        id: "telugu-venkatesh",
        name: "Telugu · Venkatesh",
        about: "A voice for Telugu, by Piper",
        source: PackSource::HuggingFace {
            repo: PIPER_REPO,
            commit: PIPER_COMMIT,
            files: &[
                PackFile {
                    path: "te/te_IN/venkatesh/medium/te_IN-venkatesh-medium.onnx",
                    name: "te_IN-venkatesh-medium.onnx",
                    bytes: 63_516_050,
                    sha256: "dfaa5b7833cd48d946f3fe18c9c934aaa4e8590aac6922fddf34783a694c3c87",
                },
                PackFile {
                    path: "te/te_IN/venkatesh/medium/te_IN-venkatesh-medium.onnx.json",
                    name: "te_IN-venkatesh-medium.onnx.json",
                    bytes: 4_973,
                    sha256: "59bad556763d1f24b3434201d7bdee275bb1a70db3e1c65d38e6c3d39b224343",
                },
            ],
        },
        folder: "piper-te-in-venkatesh-medium",
        model: "te_IN-venkatesh-medium.onnx",
        kind: ModelKind::Vits,
        license: "CC-BY-4.0",
        speakers: &["Venkatesh"],
        sample: "నమస్కారం, మీరు ఎలా ఉన్నారు?",
        piper: true,
    },
];

/// The terms of a voice's licence that limits commercial use: what Settings › Voice shows under its warning mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Terms {
    pub title: &'static str,
    pub summary: &'static str,
    /// The licence itself.
    pub url: &'static str,
}

const NON_COMMERCIAL: Terms = Terms {
    title: "Non-commercial use only",
    summary: "This voice is licensed under the Creative Commons Attribution-NonCommercial 4.0 International \
              License (CC BY-NC 4.0). You may use it, and the audio it produces, for personal, educational, research \
              and other non-commercial purposes, with credit to its authors (Meta's MMS project). Commercial use is \
              not permitted.",
    url: "https://creativecommons.org/licenses/by-nc/4.0/",
};

const INDIC_TTS: Terms = Terms {
    title: "Commercial use requires an agreement",
    summary: "This voice was trained on the Indic TTS database of IIT Madras and is subject to its license. IIT \
              Madras makes the database available free of charge for research, and asks businesses to sign an \
              agreement with it before commercial use. Read the license before using this voice, or the audio it \
              produces, for any commercial purpose.",
    url: "https://www.iitm.ac.in/donlab/indictts/downloads/license.pdf",
};

impl VoicePack {
    /// The terms of its licence, when that licence limits commercial use; `None` when anyone may use it for anything
    /// (with credit, where it asks for it).
    pub fn terms(&self) -> Option<Terms> {
        match self.license {
            "CC-BY-NC-4.0" => Some(NON_COMMERCIAL),
            "IndicTTS (IIT Madras)" => Some(INDIC_TTS),
            _ => None,
        }
    }
}

/// The sentence Settings › Voice reads to let a voice be heard: in English, unless the voice is of another language
/// (a pack's own, or a Hindi voice of Kokoro's).
pub fn sample(pack: Option<&VoicePack>, speaker: Option<&str>) -> &'static str {
    const HINDI: &str = "नमस्ते। उत्तर इस तरह बोलकर पढ़े जाते हैं।";
    match (pack, speaker) {
        (Some(p), Some(name)) if p.kind == ModelKind::Kokoro && name.starts_with('h') => HINDI,
        (Some(p), _) => p.sample,
        _ => "Hello. This is how QuickAI reads its answers.",
    }
}

pub fn voice_pack(id: &str) -> Option<&'static VoicePack> {
    VOICE_PACKS.iter().find(|p| p.id == id)
}

/// Where downloaded voices and the voice program are kept: `~/.quickai/voice`.
pub fn voice_dir() -> PathBuf {
    paths::user_quickai_dir().join("voice")
}

/// The espeak-ng data Piper's voices are read with, as sherpa-onnx releases it (its own packs carry a copy each):
/// downloaded once, with the first voice that needs it, into `espeak_dir`.
pub const ESPEAK_DATA: (&str, u64, &str) = (
    "espeak-ng-data.tar.bz2",
    7_252_012,
    "4135ccf82e1f40613491c0874d4945ae9e9c7840933d8e25a6f9e003d9ebf533",
);

/// Where the shared espeak-ng data is: `~/.quickai/voice/espeak-ng-data`.
pub fn espeak_dir() -> PathBuf {
    voice_dir().join("espeak-ng-data")
}

fn espeak_here(root: &Path) -> bool {
    root.join("espeak-ng-data").join("phontab").is_file()
}

impl VoicePack {
    /// What is downloaded for it: (address, name it is kept under, bytes, sha256) for its archive or each of its
    /// files.
    pub fn downloads(&self) -> Vec<(String, &'static str, u64, &'static str)> {
        match self.source {
            PackSource::Archive {
                archive,
                bytes,
                sha256,
            } => vec![(format!("{PACKS_AT}{archive}"), archive, bytes, sha256)],
            PackSource::HuggingFace {
                repo,
                commit,
                files,
            } => files
                .iter()
                .map(|f| {
                    (
                        format!("https://huggingface.co/{repo}/resolve/{commit}/{}", f.path),
                        f.name,
                        f.bytes,
                        f.sha256,
                    )
                })
                .collect(),
        }
    }

    /// The bytes of what is downloaded for it.
    pub fn bytes(&self) -> u64 {
        self.downloads().iter().map(|d| d.2).sum()
    }

    /// Where it is once unpacked.
    pub fn dir(&self) -> PathBuf {
        voice_dir().join("packs").join(self.folder)
    }

    /// It was downloaded and unpacked whole (its model is the last file an archive of sherpa-onnx's lists; a voice of
    /// Piper's form has tokens.txt once it was made one sherpa-onnx reads), with the espeak-ng data it needs.
    pub fn installed(&self) -> bool {
        let dir = self.dir();
        dir.join(self.model).is_file()
            && dir.join("tokens.txt").is_file()
            && (!self.piper || espeak_here(&voice_dir()))
    }

    /// What quickai-voice loads for it.
    pub fn spec(&self) -> ModelSpec {
        let dir = self.dir();
        match self.kind {
            ModelKind::Vits => {
                let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
                let mut files =
                    BTreeMap::from([("model", at(self.model)), ("tokens", at("tokens.txt"))]);
                // espeak-ng's data: the shared one for a voice of Piper's form, its own for a pack of sherpa-onnx's;
                // MMS's voices (files of Hugging Face) read letters, and need none
                match (self.piper, self.source) {
                    (true, _) => {
                        files.insert("data_dir", espeak_dir().to_string_lossy().into_owned());
                    }
                    (false, PackSource::Archive { .. }) => {
                        files.insert("data_dir", at("espeak-ng-data"));
                    }
                    (false, PackSource::HuggingFace { .. }) => {}
                }
                ModelSpec {
                    kind: ModelKind::Vits,
                    files,
                }
            }
            _ => describe_kokoro(&dir, self.model),
        }
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
/// started by QuickAI, as llama.cpp's server is). 0.2.0 or later: Omnilingual needs its `--hear`.
pub struct VoiceProgram {
    pub version: &'static str,
    pub linux: Option<ProgramAsset>,
    pub windows: Option<ProgramAsset>,
    pub macos: Option<ProgramAsset>,
}

pub const VOICE_PROGRAM: VoiceProgram = VoiceProgram {
    version: "0.2.0",
    linux: Some(ProgramAsset {
        archive: "quickai-voice-0.2.0-linux-x86_64.tar.gz",
        bytes: 10_966_146,
        sha256: "fa7ee52cd508251745e5caf0b9be4ddc7bb1ec17faa37de1b52a07b0c300b3a8",
    }),
    windows: Some(ProgramAsset {
        archive: "quickai-voice-0.2.0-windows-x86_64.zip",
        bytes: 8_533_969,
        sha256: "24f4d0ec1d9eec27d9f255c4d8362e1669599f3f98831f9cfd6a4ad0e0952862",
    }),
    macos: Some(ProgramAsset {
        archive: "quickai-voice-0.2.0-macos-universal.tar.gz",
        bytes: 20_824_408,
        sha256: "6b338151e0188bd2e4bc6ce23f38b7d912b59a64418e51a73eb3e6cd967340d7",
    }),
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

/// Puts a downloaded pack where `VoicePack::dir` says, replacing what was there, from `downloads` (where its archive
/// is, or its files in a folder of its name): an archive is unpacked and removed, a voice of Piper's form made one
/// sherpa-onnx reads, and the shared espeak-ng data unpacked when it came too.
pub fn install_pack(pack: &VoicePack, downloads: &Path) -> Result<(), String> {
    install_pack_at(pack, downloads, &voice_dir())
}

/// `install_pack` into `root` (`~/.quickai/voice`).
fn install_pack_at(pack: &VoicePack, downloads: &Path, root: &Path) -> Result<(), String> {
    let packs = root.join("packs");
    let target = packs.join(pack.folder);
    std::fs::create_dir_all(&packs).map_err(|e| format!("{}: {e}", packs.display()))?;
    match pack.source {
        PackSource::Archive { archive, .. } => {
            let archive = downloads.join(archive);
            let staging = packs.join(format!(".unpacking-{}", pack.id));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
            unpack_archive(ArchiveKind::TarBz2, &archive, &staging, &[])?;
            let unpacked = staging.join(pack.folder);
            if !unpacked.join(pack.model).is_file() {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(format!("{} did not hold the voice", archive.display()));
            }
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&unpacked, &target)
                .map_err(|e| format!("{}: {e}", target.display()))?;
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_file(&archive);
        }
        PackSource::HuggingFace { .. } => {
            let came = downloads.join(pack.folder);
            if !came.join(pack.model).is_file() {
                return Err(format!("the voice's files are not in {}", came.display()));
            }
            let _ = std::fs::remove_dir_all(&target);
            std::fs::rename(&came, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    if pack.piper {
        piper_to_sherpa(&target, pack.model)?;
        let espeak = downloads.join(ESPEAK_DATA.0);
        if espeak.is_file() {
            let staging = root.join(".unpacking-espeak-ng-data");
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
            unpack_archive(ArchiveKind::TarBz2, &espeak, &staging, &[])?;
            let _ = std::fs::remove_dir_all(root.join("espeak-ng-data"));
            std::fs::rename(staging.join("espeak-ng-data"), root.join("espeak-ng-data"))
                .map_err(|e| format!("espeak-ng-data: {e}"))?;
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_file(&espeak);
        }
        if !espeak_here(root) {
            return Err("the espeak-ng data this voice needs is not here".into());
        }
    }
    Ok(())
}

/// Makes a voice in Piper's own form (`model` and `model.json` in `dir`) one sherpa-onnx reads, as sherpa-onnx's
/// scripts/piper/add_meta_data.py does: tokens.txt from the .json's phonemes, and the model's metadata (what it is,
/// its language, its espeak-ng voice, its speakers and rate), appended to the model as ONNX's `metadata_props` (field
/// 14 of the protobuf ModelProto, which a field appended at the end adds to). tokens.txt is written last: it says the
/// voice is ready.
pub fn piper_to_sherpa(dir: &Path, model: &str) -> Result<(), String> {
    let config_path = dir.join(format!("{model}.json"));
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    let config: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", config_path.display()))?;
    let ids = config
        .get("phoneme_id_map")
        .and_then(serde_json::Value::as_object)
        .ok_or("the voice's .json has no phoneme_id_map")?;
    let mut tokens = String::new();
    for (symbol, id) in ids {
        if symbol == "\n" {
            continue;
        }
        let id = id
            .as_array()
            .and_then(|a| a.first())
            .or(Some(id))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("the phoneme {symbol:?} has no number"))?;
        tokens.push_str(&format!("{symbol} {id}\n"));
    }
    let text_of = |path: &[&str]| {
        path.iter()
            .try_fold(&config, |v, key| v.get(*key))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let voice = text_of(&["lang_code"])
        .or_else(|| text_of(&["espeak", "voice"]))
        .ok_or("the voice's .json names no espeak-ng voice")?;
    let language = text_of(&["language", "name_english"]).unwrap_or_else(|| voice.clone());
    let rate = match config
        .pointer("/audio/sample_rate")
        .and_then(serde_json::Value::as_u64)
    {
        Some(22_500) => 22_050,
        Some(rate) => rate,
        None => return Err("the voice's .json has no sample rate".into()),
    };
    let speakers = config
        .get("num_speakers")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let metadata = [
        ("model_type", "vits".to_string()),
        ("comment", "piper".to_string()),
        ("language", language),
        ("voice", voice),
        ("version", "1".to_string()),
        ("has_espeak", "1".to_string()),
        ("has_g2pw", "0".to_string()),
        ("n_speakers", speakers.to_string()),
        ("sample_rate", rate.to_string()),
    ];
    let mut appended = Vec::new();
    for (key, value) in &metadata {
        let mut entry = protobuf_field(1, key.as_bytes());
        entry.extend(protobuf_field(2, value.as_bytes()));
        appended.extend(protobuf_field(14, &entry));
    }
    use std::io::Write;
    let model_path = dir.join(model);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&model_path)
        .map_err(|e| format!("{}: {e}", model_path.display()))?;
    file.write_all(&appended)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", model_path.display()))?;
    std::fs::write(dir.join("tokens.txt"), tokens).map_err(|e| format!("tokens.txt: {e}"))
}

/// A length-delimited protobuf field: its tag (`number`, wire type 2), its length and `data`.
fn protobuf_field(number: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 12);
    for mut n in [(number << 3) | 2, data.len() as u64] {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }
    out.extend_from_slice(data);
    out
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
    // the releases downloaded before this one are no longer used
    if let (Some(bin), Some(this)) = (dir.parent(), dir.file_name()) {
        for entry in std::fs::read_dir(bin).into_iter().flatten().flatten() {
            if entry.file_name() != this && entry.path().is_dir() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
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
        assert!(kokoro.downloads()[0]
            .0
            .ends_with("/tts-models/kokoro-multi-lang-v1_0.tar.bz2"));
    }

    /// A Piper voice is loaded as VITS, with its espeak-ng data; Nabra, Kokoro's Arabic, as Kokoro.
    #[test]
    fn each_pack_is_loaded_as_its_kind() {
        let amir = voice_pack("persian-amir").unwrap();
        let spec = amir.spec();
        assert_eq!(spec.kind, ModelKind::Vits);
        assert!(
            spec.files["model"].ends_with("vits-piper-fa_IR-amir-medium/fa_IR-amir-medium.onnx")
        );
        assert!(spec.files["data_dir"].ends_with("espeak-ng-data"));
        assert_eq!(amir.speaker("Amir"), Some(0));
        let nabra = voice_pack("arabic-nabra").unwrap().spec();
        assert_eq!(nabra.kind, ModelKind::Kokoro);
        assert!(nabra.files["voices"].ends_with("nabra-82m-arabic-int8/voices.bin"));
        for pack in &VOICE_PACKS {
            let downloads = pack.downloads();
            assert!(
                !pack.speakers.is_empty() && !downloads.is_empty(),
                "{}",
                pack.id
            );
            assert!(downloads.iter().all(|d| d.3.len() == 64), "{}", pack.id);
            // a voice of Piper's form brings its model and its .json
            if pack.piper {
                assert!(downloads
                    .iter()
                    .any(|d| d.1 == format!("{}.json", pack.model)));
            }
        }
        // a licence that limits commercial use has its terms; the others none
        assert_eq!(
            voice_pack("tamil-mms").unwrap().terms().map(|t| t.title),
            Some("Non-commercial use only")
        );
        assert!(voice_pack("malayalam-meera").unwrap().terms().is_some());
        assert!(voice_pack("telugu-maya").unwrap().terms().is_some());
        for free in [
            "kokoro",
            "arabic-nabra",
            "persian-amir",
            "urdu-fasih",
            "telugu-venkatesh",
            "nepali-chitwan",
        ] {
            assert_eq!(voice_pack(free).unwrap().terms(), None, "{free}");
        }
        // files of Hugging Face are kept in a folder the downloader takes
        for pack in &VOICE_PACKS {
            if let PackSource::HuggingFace { .. } = pack.source {
                assert!(
                    quickai_core::lookup::present::is_place(pack.folder),
                    "{}",
                    pack.folder
                );
            }
        }
        // MMS reads letters: no espeak-ng data
        let tamil = voice_pack("tamil-mms").unwrap().spec();
        assert_eq!(tamil.kind, ModelKind::Vits);
        assert!(!tamil.files.contains_key("data_dir"));
        assert!(tamil.files["model"].ends_with("mms-tts-tam/model.onnx"));
        // a voice of Piper's form reads with the shared espeak-ng data
        let telugu = voice_pack("telugu-maya").unwrap().spec();
        assert_eq!(telugu.files["data_dir"], espeak_dir().to_string_lossy());
    }

    #[test]
    fn a_downloaded_pack_is_unpacked_into_its_folder_and_one_without_its_model_is_refused() {
        use crate::testkit::archives::{tar_bz2, Entry};
        let root = std::env::temp_dir().join(format!("quickai-packs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let downloads = root.join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let pack = voice_pack("kokoro").unwrap();
        let archive = downloads.join(pack.downloads()[0].1);
        std::fs::write(
            &archive,
            tar_bz2(&[
                Entry::file("kokoro-multi-lang-v1_0/model.onnx", b"weights"),
                Entry::file("kokoro-multi-lang-v1_0/tokens.txt", b"a 0"),
                Entry::file("kokoro-multi-lang-v1_0/voices.bin", b"v"),
            ]),
        )
        .unwrap();
        install_pack_at(pack, &downloads, &root).unwrap();
        let dir = root.join("packs").join(pack.folder);
        assert_eq!(std::fs::read(dir.join("model.onnx")).unwrap(), b"weights");
        assert!(!archive.exists(), "the archive is removed once unpacked");
        // an archive without the model leaves what was there
        std::fs::write(&archive, tar_bz2(&[Entry::file("other/readme.txt", b"x")])).unwrap();
        assert!(install_pack_at(pack, &downloads, &root).is_err());
        assert!(dir.join("model.onnx").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A voice of Piper's form comes as its files, is made one sherpa-onnx reads (tokens.txt, and its metadata
    /// appended to the model), and gets the shared espeak-ng data that came with it.
    #[test]
    fn a_piper_voice_is_made_one_sherpa_onnx_reads() {
        use crate::testkit::archives::{tar_bz2, Entry};
        let root = std::env::temp_dir().join(format!("quickai-piper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let downloads = root.join("downloads");
        let pack = voice_pack("telugu-venkatesh").unwrap();
        assert!(pack.piper);
        let came = downloads.join(pack.folder);
        std::fs::create_dir_all(&came).unwrap();
        std::fs::write(came.join(pack.model), b"MODEL").unwrap();
        std::fs::write(
            came.join(format!("{}.json", pack.model)),
            r#"{"audio":{"sample_rate":22050},"espeak":{"voice":"te"},
                "language":{"name_english":"Telugu"},"num_speakers":1,
                "phoneme_id_map":{"_":[0],"a":[14],"\n":[99]}}"#,
        )
        .unwrap();
        std::fs::write(
            downloads.join(ESPEAK_DATA.0),
            tar_bz2(&[Entry::file("espeak-ng-data/phontab", b"p")]),
        )
        .unwrap();
        install_pack_at(pack, &downloads, &root).unwrap();
        let dir = root.join("packs").join(pack.folder);
        let tokens = std::fs::read_to_string(dir.join("tokens.txt")).unwrap();
        assert_eq!(tokens, "_ 0\na 14\n");
        let model = std::fs::read(dir.join(pack.model)).unwrap();
        assert!(model.starts_with(b"MODEL"));
        // field 14, then its entry: the key (field 1) and the value (field 2)
        let entry = [&[0x0a, 10][..], b"model_type", &[0x12, 4], b"vits"].concat();
        let field = [&[0x72, entry.len() as u8][..], &entry].concat();
        assert_eq!(&model[5..5 + field.len()], &field[..]);
        let all = String::from_utf8_lossy(&model);
        for wanted in [
            "comment",
            "piper",
            "voice",
            "Telugu",
            "n_speakers",
            "sample_rate",
            "22050",
        ] {
            assert!(all.contains(wanted), "{wanted}");
        }
        assert!(root.join("espeak-ng-data/phontab").is_file());
        assert!(!downloads.join(ESPEAK_DATA.0).exists());
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
