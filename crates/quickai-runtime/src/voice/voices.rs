//! The voices QuickAI can read aloud with. None of them comes with QuickAI: the person downloads one (a pack of
//! sherpa-onnx's, read by the program quickai-voice, which is downloaded with the first), or uses one of their own (a
//! folder of a voice sherpa-onnx can read, a speech server, or a program).
//!
//! - `VOICE_PACKS`: the packs offered for download, pinned: where they are, their size, their hash, their licence,
//!   the language they read and their voices. Settings › Voice offers them by language (`voices_for`).
//! - `ModelSpec`: what quickai-voice is told to load, for a pack or for a folder of the person's own (`describe_folder`).
//! - `VOICE_PROGRAM`: the release of quickai-voice QuickAI downloads, and `program`, where it is looked for.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::languages::{self, LANGUAGES};
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

/// Who a voice sounds like, as Settings › Voice says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Woman,
    Man,
    /// Not said: many people's recordings made into one voice, or not known.
    Unsaid,
}

/// A voice offered for download: one model, with one voice in it or several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePack {
    pub id: &'static str,
    /// What made it: "Kokoro", "Piper", "MMS"…
    pub name: &'static str,
    /// What it is, in a few words, where its name and language do not say enough; else empty.
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
    /// Who each of them sounds like, in the same order; none for Kokoro, whose names say it.
    pub who: &'static [Who],
    /// The language its voices read, by its code in `languages`; empty for Kokoro, whose voices each read their own.
    pub language: &'static str,
    /// Where its accent is from, for a language spoken with several ("Spain", "Mexico"); else empty.
    pub accent: &'static str,
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

/// The language a voice of Kokoro's reads (told by the first letter of its name), what sherpa-onnx is told to read it
/// as, and its accent. sherpa-onnx reads English and Chinese with Kokoro's lexicons, and the other languages through
/// espeak-ng, which it must be told the language for: untold, it read Spanish and French as English (2026-11: a
/// fifth to half of the letters wrong, none told). Japanese is not offered: espeak-ng reads kana but not kanji, and
/// Japanese is written with both (Kokoro's Japanese voices read 38 letters in a hundred wrong).
fn kokoro_language(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match name.as_bytes().first()? {
        b'a' => Some(("en", "", "American")),
        b'b' => Some(("en", "", "British")),
        b'e' => Some(("es", "es", "")),
        b'f' => Some(("fr", "fr", "")),
        b'h' => Some(("hi", "hi", "")),
        b'i' => Some(("it", "it", "")),
        b'p' => Some(("pt", "pt-br", "Brazil")),
        b'z' => Some(("zh", "", "")),
        _ => None,
    }
}

/// Where the packs are: sherpa-onnx's release of voices on GitHub; Meta's MMS voices in sherpa-onnx's form, and Piper's
/// voices sherpa-onnx has not made its own, on Hugging Face.
const PACKS_AT: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/";
const MMS_REPO: &str = "willwade/mms-tts-multilingual-models-onnx";
const MMS_COMMIT: &str = "4f49fc254f11ddb0e35fbf25b6e48b04463a6779";
const PIPER_REPO: &str = "rhasspy/piper-voices";
const PIPER_COMMIT: &str = "2f358b6a560e3a913ebf509b2f3344f20b367f94";

/// A voice of sherpa-onnx's release, its archive `$folder.tar.bz2` holding the folder `$folder`: a VITS voice (Piper's,
/// Coqui's, Mimic 3's), with its voices (each a name and who it sounds like) in the order of their numbers.
macro_rules! archive_pack {
    ($id:literal, $name:literal, $lang:literal, $accent:literal, $folder:literal, $model:literal, $bytes:literal,
     $sha:literal, $license:literal, [$($speaker:literal $who:ident),+ $(,)?]) => {
        VoicePack {
            id: $id,
            name: $name,
            about: "",
            source: PackSource::Archive { archive: concat!($folder, ".tar.bz2"), bytes: $bytes, sha256: $sha },
            folder: $folder,
            model: $model,
            kind: ModelKind::Vits,
            license: $license,
            speakers: &[$($speaker),+],
            who: &[$(Who::$who),+],
            language: $lang,
            accent: $accent,
            piper: false,
        }
    };
}

/// A voice of Piper's own form in rhasspy/piper-voices: its model and the model's .json in the folder `$path` there,
/// kept in `$folder` and made one sherpa-onnx reads once it is here.
macro_rules! piper_pack {
    ($id:literal, $lang:literal, $accent:literal, $folder:literal, $path:literal, $model:literal, $bytes:literal,
     $sha:literal, $json_bytes:literal, $json_sha:literal, $license:literal,
     [$($speaker:literal $who:ident),+ $(,)?]) => {
        VoicePack {
            id: $id,
            name: "Piper",
            about: "",
            source: PackSource::HuggingFace {
                repo: PIPER_REPO,
                commit: PIPER_COMMIT,
                files: &[
                    PackFile { path: concat!($path, $model), name: $model, bytes: $bytes, sha256: $sha },
                    PackFile {
                        path: concat!($path, $model, ".json"),
                        name: concat!($model, ".json"),
                        bytes: $json_bytes,
                        sha256: $json_sha,
                    },
                ],
            },
            folder: $folder,
            model: $model,
            kind: ModelKind::Vits,
            license: $license,
            speakers: &[$($speaker),+],
            who: &[$(Who::$who),+],
            language: $lang,
            accent: $accent,
            piper: true,
        }
    };
}

/// Meta's MMS voice for a language, in sherpa-onnx's form (willwade's): the folder `$dir` there, kept in
/// `mms-tts-$dir`. It reads letters, and needs no espeak-ng.
macro_rules! mms_pack {
    ($id:literal, $lang:literal, $dir:literal, $bytes:literal, $sha:literal, $tokens_bytes:literal,
     $tokens_sha:literal) => {
        VoicePack {
            id: $id,
            name: "MMS",
            about: "Meta's voices for over 1,100 languages",
            source: PackSource::HuggingFace {
                repo: MMS_REPO,
                commit: MMS_COMMIT,
                files: &[
                    PackFile {
                        path: concat!($dir, "/model.onnx"),
                        name: "model.onnx",
                        bytes: $bytes,
                        sha256: $sha,
                    },
                    PackFile {
                        path: concat!($dir, "/tokens.txt"),
                        name: "tokens.txt",
                        bytes: $tokens_bytes,
                        sha256: $tokens_sha,
                    },
                ],
            },
            folder: concat!("mms-tts-", $dir),
            model: "model.onnx",
            kind: ModelKind::Vits,
            license: "CC-BY-NC-4.0",
            speakers: &["MMS"],
            who: &[Who::Unsaid],
            language: $lang,
            accent: "",
            piper: false,
        }
    };
}

/// The packs offered, the recommended first: the whole Kokoro (also the quicker on a desktop's processor),
/// then its smaller download, Nabra for Arabic, then the other voices by language, in the order of the
/// languages' names, each language's best first. Every one read a sentence of its language that speech
/// recognition wrote back with few letters wrong (2026-11: Omnilingual 1B, and Whisper large turbo where it
/// knows the language better), and each keeps its own licence, which Settings shows: a licence that limits
/// commercial use has an (i) mark with its terms. A voice whose licence could not be found is not offered.
#[rustfmt::skip]
pub static VOICE_PACKS: [VoicePack; 142] = [
    VoicePack {
        id: "kokoro",
        name: "Kokoro",
        about: "natural voices for English, Chinese, Hindi, Spanish, French, Italian and Portuguese",
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
        who: &[],
        language: "",
        accent: "",
        piper: false,
    },
    VoicePack {
        id: "kokoro-small",
        name: "Kokoro, smaller",
        about: "the same voices in a third of the size; slower on most processors",
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
        who: &[],
        language: "",
        accent: "",
        piper: false,
    },
    // Nabra-82M (oddadmix/Nabra-82M-v0.1): Kokoro fine-tuned for Modern Standard Arabic, in sherpa-onnx's packing
    VoicePack {
        id: "arabic-nabra",
        name: "Nabra",
        about: "Kokoro, fine-tuned for Modern Standard Arabic",
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
        who: &[Who::Unsaid],
        language: "ar",
        accent: "",
        piper: false,
    },
    archive_pack!("afrikaans-nwu", "Mimic 3", "af", "", "vits-mimic3-af_ZA-google-nwu_low", "af_ZA-google-nwu_low.onnx", 79_962_606, "a4d2649d4b5e72e04d981c843e419b41d76845eec297a9c06f73bdd44e79ac1f", "CC-BY-SA-4.0", ["NWU" Unsaid]),
    mms_pack!("akan-mms", "ak", "aka", 114_010_884, "6667cf7efbc9deeef0ad37f1ff36bf06b9e167fe355cda1e90e09124068c6ebc", 258, "e45ba292dd337d7dd2a83bd81aed813c706412d6e6887c91329d943e02abd300"),
    archive_pack!("albanian-edon", "Piper", "sq", "", "vits-piper-sq_AL-edon-medium", "sq_AL-edon-medium.onnx", 67_191_820, "2d7a3b3647a499ad40a28662edd5ff50eb8594ad7ace94a5d3525e280212bf99", "CC0-1.0", ["Edon" Man]),
    piper_pack!("armenian-gor", "hy", "", "piper-hy-am-gor-medium", "hy/hy_AM/gor/medium/", "hy_AM-gor-medium.onnx", 63_511_038, "1c91fc33fecbbfc6b7921f9d11fd7487a859053a3a1987147f5c361155b89e7c", 4_967, "01a8458d4ff226fac1b7dc7b1f619851fdbc8a9bd09d6a604e65e304c9600c2e", "GPL-2.0", ["Gor" Man]),
    mms_pack!("assamese-mms", "as", "asm", 114_038_532, "85860bce4cf4cc1b985e48b5916eadf27a684e2e35dbc9ccef71c98f2a43e285", 440, "2f61bedac3482dffdd0c32c0346188726d6bdf0f99be24bd5b10bfecacd0b973"),
    mms_pack!("aymara-mms", "ay", "ayr", 114_019_332, "06f7525be81f95b4879569b0c7174ca5945df96791f60b444d8d87e51250a687", 395, "de3335300658158d6aea823953b8748a8d86382ed9dc480c08138bd0c070502d"),
    mms_pack!("azerbaijani-mms", "az", "azj-script_latin", 114_020_100, "d3146ccd5f9658f96cd8ce1201b7a730b6682e5de7c12224f86927cc4d52d9f0", 361, "829cdeea5fa08ced7c900a2a075281e8bbeda48a2dbeacda2918ba3c43d28c5b"),
    archive_pack!("basque-maider", "Piper", "eu", "", "vits-piper-eu_ES-maider-medium", "eu_ES-maider-medium.onnx", 67_247_291, "60493b81a7f117ea96b6efc841d7465c1d4107ea72ef553ff91f92f72fbd95c1", "CC-BY-4.0", ["Maider" Woman]),
    archive_pack!("basque-antton", "Piper", "eu", "", "vits-piper-eu_ES-antton-medium", "eu_ES-antton-medium.onnx", 67_216_651, "94ea0b459cc883913961f164f49a63d4d419394897ca6f9755f68eeeb1b06747", "CC-BY-4.0", ["Antton" Man]),
    mms_pack!("bengali-mms", "bn", "ben", 114_044_676, "c67081369061e87ba302bfbc0445543c4c23c04133514b7d6c7be2710363bc42", 480, "a74190ec42b8f0afb349276430c716f1b585090d8fe6d3c1d6d5fe510798a387"),
    piper_pack!("bulgarian-dimitar", "bg", "", "piper-bg-bg-dimitar-medium", "bg/bg_BG/dimitar/medium/", "bg_BG-dimitar-medium.onnx", 63_221_984, "2305bc55def71c799fbfc866d15415f57a3465097b7bf9be41a14a26574ad6f3", 5_474, "aa3b0cf8236c205ce953d449e112e184ebde325e7db82e769cfcf2926c375631", "CC0-1.0", ["Dimitar" Man]),
    archive_pack!("bulgarian-cv", "Coqui", "bg", "", "vits-coqui-bg-cv", "model.onnx", 67_042_012, "fa690d739e2f1e331261a1372d12c726d1f9ac58874bf3cbc4dfd8c95d8c4606", "BSD-3-Clause", ["Common Voice" Unsaid]),
    mms_pack!("burmese-mms", "my", "mya", 114_031_620, "2dd0874209259565df110160dbad655bbb09855d8210882b640f0fc82bd4a42a", 385, "cd0a6ecfb0c91dde4d8ced1a6f8d18a7bc48086a438a179ae00230b86d5e76e3"),
    archive_pack!("catalan-ona", "Piper", "ca", "", "vits-piper-ca_ES-upc_ona-medium", "ca_ES-upc_ona-medium.onnx", 67_223_188, "b6c50f22e13cfc72ce51c496d22b987adf6704047f6d342968197b4b0b3127a5", "CC-BY-SA-3.0", ["Ona" Woman]),
    archive_pack!("catalan-pau", "Piper", "ca", "", "vits-piper-ca_ES-upc_pau-x_low", "ca_ES-upc_pau-x_low.onnx", 26_520_095, "9c06bc1c2fa9f132df02f413d5b3a93b37682edcb6d41be97b725775bf5e28ba", "CC-BY-SA-3.0", ["Pau" Man]),
    mms_pack!("cebuano-mms", "ceb", "ceb", 114_019_332, "f050f64c06a7d3a09f462adb5be829ff3cb3f637e45df9ba82115aaf6e9a46ce", 320, "d49961b05b65dfba32ad5dcee11a86db92c4650522d9daa102a117fd97fed7f1"),
    mms_pack!("chichewa-mms", "ny", "nya", 114_017_796, "16035b60edbc0d2736191e6d6828856e9d8dafc8755e940732a3ac52ff88d944", 303, "e6868455fdd5ee56329d9a51512f7a0902911cc36ecdcd6cf5c05498563b7e2c"),
    archive_pack!("croatian-cv", "Coqui", "hr", "", "vits-coqui-hr-cv", "model.onnx", 67_071_025, "c6674d678545297484637a0eef3059cd1e16b091d2fb98bbc09331888f41dcab", "BSD-3-Clause", ["Common Voice" Unsaid]),
    archive_pack!("czech-jirka", "Piper", "cs", "", "vits-piper-cs_CZ-jirka-medium", "cs_CZ-jirka-medium.onnx", 67_212_775, "321aafd83dd89b3d5b2c36c5898e0710d4f472f3c191cb7f73901a89b7f8539f", "CC0-1.0", ["Jirka" Unsaid]),
    piper_pack!("czech-kasandra", "cs", "", "piper-cs-cz-kasandra-medium", "cs/cs_CZ/kasandra/medium/", "cs_CZ-kasandra-medium.onnx", 63_511_038, "09d08aa23691ed2d53fb3db242cfe82b189adf9c1812dd5fe928b5789ed76df7", 3_232, "5039f9e49def4f4f8438db031868800a13bf4c6246b62afdb0947f4c5478770e", "CC-BY-4.0", ["Kasandra" Woman]),
    archive_pack!("danish-talesyntese", "Piper", "da", "", "vits-piper-da_DK-talesyntese-medium", "da_DK-talesyntese-medium.onnx", 67_185_257, "f2e2cbcdb2b21e76f93635b35105d07b37997ace434d7289d04b689a97c19bd0", "CC0-1.0", ["Talesyntese" Unsaid]),
    archive_pack!("dutch-alex", "Piper", "nl", "Netherlands", "vits-piper-nl_NL-alex-medium", "nl_NL-alex-medium.onnx", 67_172_584, "52be645a8617eda0e592c604eb67f419fcb09376310c3ec7fc781beeb8e84bb6", "CC0-1.0", ["Alex" Unsaid]),
    archive_pack!("dutch-nathalie", "Piper", "nl", "Belgium", "vits-piper-nl_BE-nathalie-medium", "nl_BE-nathalie-medium.onnx", 67_226_472, "c7484313abea395243e872196bc75181c07aad8120b8a61e0f6f4b3863b6a7c8", "CC0-1.0", ["Nathalie" Unsaid]),
    archive_pack!("dutch-rdh", "Piper", "nl", "Belgium", "vits-piper-nl_BE-rdh-medium", "nl_BE-rdh-medium.onnx", 67_047_636, "3e99e86fcd1ec1bf04bb68fc9e1588e5b88c4a5fa2ee507bd46ca9a947420d84", "CC0-1.0", ["Rdh" Man]),
    archive_pack!("dutch-pim", "Piper", "nl", "Netherlands", "vits-piper-nl_NL-pim-medium", "nl_NL-pim-medium.onnx", 67_240_538, "e78ed301bf6fc66561a04c110754cd6928d772630a3b80b09a83f89410130171", "CC0-1.0", ["Pim" Man]),
    archive_pack!("dutch-ronnie", "Piper", "nl", "Netherlands", "vits-piper-nl_NL-ronnie-medium", "nl_NL-ronnie-medium.onnx", 67_194_164, "d2cc688abf46054eba1d47b3b1406c742ccf66732cd47998a3ee727c1b96c09f", "CC0-1.0", ["Ronnie" Man]),
    mms_pack!("dzongkha-mms", "dz", "dzo", 114_032_388, "3ec4468d456460ee802093db8adb20927fbbe9cca3f38a331588c3ac6593649c", 394, "8a7654094293bb4d6e96e86a31671323349d54308788b90ad664cf0c29f7e195"),
    piper_pack!("estonian-news", "et", "", "piper-et-ee-news-medium", "et/et_EE/news/medium/", "et_EE-news-medium.onnx", 76_757_937, "ddd679e874e0f6ec4e8f76c99cef77d490341b001aa6e839415028be5d04f9bf", 5_320, "2f56bbaf9cd2c44bf4b1e02fb36cfd31876f7ed3f78ca199f2aca6727e96441a", "CC-BY-4.0", ["Albert" Man, "Kalev" Man, "Mari" Woman, "Vesta" Woman]),
    mms_pack!("ewe-mms", "ee", "ewe", 114_027_012, "1b30a611858830eea8dd18271b786ecaef91611ec99c37f5d16c108959f1a6e4", 503, "4991afa36af2598579b09c25cad441eea04d78a0dea47fd1f560b9cdf2687cdf"),
    mms_pack!("faroese-mms", "fo", "fao", 114_016_260, "99816551c649c096960472bbfaa3a02024b9f8e7dddf8b3064a96d80a628e5a8", 334, "c8dc90a2b1a238822c600dabdd3a8dfc3ca87a0e9dfee3caa05152ce668773a0"),
    mms_pack!("fijian-mms", "fj", "fij", 114_015_492, "e961adb930756d708ad4aba80370b397c289595798c4bb865f383c404dd778bc", 280, "3a14414d6370fa58cf9c57cc595d4863aee5b07a7e9b7213c778cbc7bcf0e2bb"),
    mms_pack!("filipino-mms", "tl", "tgl", 114_020_868, "f0594f259f7016a0ff00d5e72886f946e6ed6ae8edf1d9e78649f24d59c83ffb", 337, "5f4883ec7ba2128b7467cda01aa875d02bee8d600dc5822843216d8a14224ab0"),
    archive_pack!("finnish-harri", "Piper", "fi", "", "vits-piper-fi_FI-harri-medium", "fi_FI-harri-medium.onnx", 67_205_198, "a7fd608e73983c366a5e0bcd13382e6529a6749f67efc406b5a7f9021e3a8b1d", "CC0-1.0", ["Harri" Man]),
    archive_pack!("french-tom", "Piper", "fr", "France", "vits-piper-fr_FR-tom-medium", "fr_FR-tom-medium.onnx", 67_202_955, "3d8258ef8466b5d5f2cc46c06e1415d3d01c9cbaeed8b6d5b3d152073672472c", "AGPL-3.0", ["Tom" Unsaid]),
    archive_pack!("french-siwis", "Piper", "fr", "France", "vits-piper-fr_FR-siwis-medium", "fr_FR-siwis-medium.onnx", 67_207_459, "375909aa30842b3a4efa10b1beb1d761af792960ae6873b4d53889f96c66195b", "CC-BY-4.0", ["Siwis" Woman]),
    archive_pack!("french-upmc", "Piper", "fr", "France", "vits-piper-fr_FR-upmc-medium", "fr_FR-upmc-medium.onnx", 80_422_639, "e9830a331a16f6cc5ef3116a287065e015d3495c3f56b974889a266da7f89a7f", "CC-BY-SA-4.0", ["Jessica" Woman, "Pierre" Man]),
    archive_pack!("french-gilles", "Piper", "fr", "France", "vits-piper-fr_FR-gilles-low", "fr_FR-gilles-low.onnx", 67_094_350, "9f4fdd2650f52638e5d890d330b4ee3a30fc6bb91d6ae6f37e639d0f474bc521", "CC0-1.0", ["Gilles" Man]),
    mms_pack!("fula-mms", "ff", "ful", 114_019_332, "ea4d68f2c6e4abca462eb258c939997b8b43f4d4e1be17d4109a0da546ecf578", 328, "b34fca4aa77eee1b64b709eed4b83252dc25ea9132fea1b1d6e938d6bf1d753e"),
    piper_pack!("german-kerstin", "de", "", "piper-de-de-kerstin-medium", "de/de_DE/kerstin/medium/", "de_DE-kerstin-medium.onnx", 63_221_984, "c56d70475fb99248a20e60dafbcd107475e60d7e01deec18ba65201d288d015d", 5_459, "8e86846ab0c5c5403c25125e3a505841eec4a52eed7d2babf904990883f7e83d", "CC0-1.0", ["Kerstin" Woman]),
    archive_pack!("german-thorsten", "Piper", "de", "", "vits-piper-de_DE-thorsten-medium", "de_DE-thorsten-medium.onnx", 67_214_254, "50487d9c95fdf2191f31d2588569381063ba1591dcd4c7d4bdd30f12b2191714", "CC0-1.0", ["Thorsten" Man]),
    piper_pack!("greek-rapunzelina", "el", "", "piper-el-gr-rapunzelina-medium", "el/el_GR/rapunzelina/medium/", "el_GR-rapunzelina-medium.onnx", 62_950_044, "3ca9fb3092215ee92edfc019b43feb0115ff4dfe638eb34474833ab1de840952", 4_973, "3a6182ec7c7550e14ef15e5d9badbb18f973a434086ac9658a1b10991fd192f8", "CC0-1.0", ["Rapunzelina" Woman]),
    piper_pack!("greek-joy", "el", "", "piper-el-gr-joy-medium", "el/el_GR/joy/medium/", "el_GR-joy-medium.onnx", 63_516_050, "31b1d51dc72dd43a5429b67798e27599f8cdcfce8a80265bc6f50d932bf144bc", 7_367, "451a8885a0ea72e4432fffe22e65a75bbd2e65f6757273d1a5a9ed8a3419852e", "CC-BY-NC-4.0", ["Joy" Woman]),
    mms_pack!("guarani-mms", "gn", "grn", 114_028_548, "8bc86015c1855e435c8f96b4d1455009f065b884dca8d5ce5ddb5c399dffbeb6", 470, "3a82f4a962da76b81e91428205d8b98a423b96a21960006c5794cef8d22cff97"),
    mms_pack!("gujarati-mms", "gu", "guj", 114_033_924, "6d9b4f268765e19b6620983bd698c5e4fa943309560b6c7b199b50a4aae30d47", 402, "2d855f2affb7586cc6be095a4382eb0bee2a22242deda32c86aab6b1a810d8c4"),
    mms_pack!("haitian-mms", "ht", "hat", 114_012_420, "1f6e6164dd9ad518b149853a866f2fc646e111fa742d7a25b6ac9be807127654", 284, "79e3717b32acb413d43cd28aa801222409fb3701f9515ff54eb7332372b1194c"),
    mms_pack!("hausa-mms", "ha", "hau", 114_013_956, "5ce225af7c0177e7387ad549a41bf29a9134fc9f12b9220025b31e91a7b28f32", 304, "78b39313ddfc60be59cc12a957f11bf0f5dd4d1cdd3d5c16fe0bf3fae165d0c9"),
    mms_pack!("hebrew-mms", "he", "heb", 114_012_420, "0ca8214041a8af3fa7765054ca73a825509a8e640e8fda0d72d4347563e3b178", 179, "d376b756ea9f18037316c20fe16ed041e86153f31bb53dd4ac98c5fe89cf0b4c"),
    archive_pack!("hindi-pratham", "Piper", "hi", "", "vits-piper-hi_IN-pratham-medium", "hi_IN-pratham-medium.onnx", 67_238_438, "2084d321e1d2752f2b64ed3012ba27751df01a80da46f52920098cdcb7e35648", "CC-BY-NC-SA-4.0", ["Pratham" Man]),
    archive_pack!("hindi-rohan", "Piper", "hi", "", "vits-piper-hi_IN-rohan-medium", "hi_IN-rohan-medium.onnx", 67_221_139, "f0bb7781724951e9c90c49f68abfe821cffcbc198a9ae0031366d045a6ad5488", "IndicTTS (IIT Madras)", ["Rohan" Man]),
    archive_pack!("hindi-priyamvada", "Piper", "hi", "", "vits-piper-hi_IN-priyamvada-medium", "hi_IN-priyamvada-medium.onnx", 67_240_610, "399d91cc97eb288725633261f26b715f9a971e3bf7ec4fa1d7910cd0080d37eb", "CC-BY-NC-SA-4.0", ["Priyamvada" Woman]),
    archive_pack!("hungarian-anna", "Piper", "hu", "", "vits-piper-hu_HU-anna-medium", "hu_HU-anna-medium.onnx", 67_167_701, "c67b13e57b71eca19098fe499afc35ebce8f63a0b524017c786af2025821216a", "CC0-1.0", ["Anna" Woman]),
    archive_pack!("hungarian-imre", "Piper", "hu", "", "vits-piper-hu_HU-imre-medium", "hu_HU-imre-medium.onnx", 67_218_203, "7be44b928bb26b03e403660894cd8210828dd8a0e9dccc4bade4a1632fc4f8c4", "CC0-1.0", ["Imre" Man]),
    archive_pack!("hungarian-berta", "Piper", "hu", "", "vits-piper-hu_HU-berta-medium", "hu_HU-berta-medium.onnx", 67_178_197, "a00ba073fa7f8e1b2be58f881b24892e2dca16c3c5979ee4ea4bc6ccdd89dbc0", "CC0-1.0", ["Berta" Woman]),
    archive_pack!("icelandic-ugla", "Piper", "is", "", "vits-piper-is_IS-ugla-medium", "is_IS-ugla-medium.onnx", 67_376_655, "2d2295b13746758620ef02ee35294ff1096d94fd01cb055c53f70de17a222519", "CC-BY-4.0", ["Ugla" Woman]),
    archive_pack!("icelandic-salka", "Piper", "is", "", "vits-piper-is_IS-salka-medium", "is_IS-salka-medium.onnx", 67_359_734, "493ad56f2d8a03ba9071c30b6d5aa59b96aba13955f7ae4387d426ee65fd60bd", "CC-BY-4.0", ["Salka" Woman]),
    archive_pack!("icelandic-steinn", "Piper", "is", "", "vits-piper-is_IS-steinn-medium", "is_IS-steinn-medium.onnx", 67_377_681, "22acffbaf4b919881ec463126eda7bf14f7dc8d4beb0cc84e161760702ada328", "CC-BY-4.0", ["Steinn" Man]),
    archive_pack!("icelandic-bui", "Piper", "is", "", "vits-piper-is_IS-bui-medium", "is_IS-bui-medium.onnx", 67_271_654, "404a4de4b2a49dafb5a9f67ccf5366ab19d97e146aab602d348d8ba39dfb89d9", "CC-BY-4.0", ["Búi" Man]),
    mms_pack!("ilocano-mms", "ilo", "ilo", 114_019_332, "3bbbe62e11cd10e501a6a7a38167accc4540624b2901f752a154c7c14b8bc8ec", 318, "f0740eab1439adc920e22e90fccdff93d80fe423c4943fd1d871d8eb9c604bad"),
    mms_pack!("indonesian-mms", "id", "ind", 114_014_724, "9c212f118b6c31e35949f16d826a5cf67c8146c05939cbd747a461e428bfe7cf", 281, "773bcb490befda1aa9227571f765cc23702a4aa611c05a2c84835eaa41859d29"),
    archive_pack!("irish-cv", "Coqui", "ga", "", "vits-coqui-ga-cv", "model.onnx", 67_043_776, "947a130bb347d8ffd775514642f8c8948d96411ab6e7aa1a4a41c3147c2b6253", "BSD-3-Clause", ["Common Voice" Unsaid]),
    archive_pack!("italian-paola", "Piper", "it", "", "vits-piper-it_IT-paola-medium", "it_IT-paola-medium.onnx", 67_221_173, "7541f75778afa164e44e34baaef63befad7698595df26a95ca944b63ef1a16b4", "CC0-1.0", ["Paola" Woman]),
    piper_pack!("italian-serena", "it", "", "piper-it-it-serena-medium", "it/it_IT/serena/medium/", "it_IT-serena-medium.onnx", 63_516_051, "fe4e26b2c1236e2a44d2e295cad564d94a0d8b0a9fca1ccdfd70dd4af3116eb5", 5_254, "14cb4995874138b2792ae07753b5540942124761af9ae6609f9b520f28837881", "CC-BY-4.0", ["Serena" Woman]),
    mms_pack!("javanese-mms", "jw", "jav", 114_007_812, "b54cdac4114c39564cd94987e62ed9506dae9cc440d6aab6e82c23addbc9f0b3", 226, "bb92676f9376fbb68d9b686e77cacfd060199608b6f0e056fac964353c743184"),
    mms_pack!("kannada-mms", "kn", "kan", 114_045_444, "9644cfa46c5802c4c2c8196f91eedc0cd54d46527bbc32589ebaf967ed07591f", 487, "a4a44037f7492c9e5b69a4483250e54af9a2d528bd4e19de95a21ff0c5e82a8a"),
    archive_pack!("kazakh-issai", "Piper", "kk", "", "vits-piper-kk_KZ-issai-high", "kk_KZ-issai-high.onnx", 129_278_805, "6c59955f7f5e3fc50b130ced59a430eb4ef62d69155af4ce836db0dde4a42682", "CC-BY-4.0", ["Kazakh 1" Man, "Iseke" Man, "Kazakh 2" Woman, "Raya" Woman, "Kazakh 3" Woman, "Kazakh 4" Woman]),
    mms_pack!("khmer-mms", "km", "khm", 114_044_676, "70acf4f024df6d6e742b6056d6e22e7632ebba8e11eab4e35a0d6f33bbf47189", 503, "b15954d9cc8eb2aad3eb564dce826c1361152ef4b83ac5e784f5ee86929ec317"),
    mms_pack!("kinyarwanda-mms", "rw", "kin", 114_021_636, "531563be608dc805fce34d2a979d42a67a757735631544d09ed841faf3ecc055", 356, "2ab58191eec47a2cf944503c11ce307a05b13bc9e908a3c29c1e06d2881b4ee4"),
    piper_pack!("korean-kss", "ko", "", "piper-ko-kr-kss-medium", "ko/ko_KR/kss/medium/", "ko_KR-kss-medium.onnx", 63_221_984, "624fd774e26895f24bebae1bd9a3379e3394baeade4b584924f83e414096e2c9", 5_232, "153b5619d0580f824a59108d83ca19434de410eb8e4fe80325b58684fdc8a1df", "CC-BY-NC-SA-4.0", ["KSS" Woman]),
    archive_pack!("kurdish-berfin-renas", "Piper", "ku", "", "vits-piper-ku_TR-berfin_renas-medium", "ku_TR-berfin_renas-medium.onnx", 80_263_336, "b72695bc6f1ac30eba67bc096c5cc5be0ddafac948efc3231f217d1c49b7cfaa", "CC-BY-NC-4.0", ["Berfin" Woman, "Renas" Man]),
    mms_pack!("kyrgyz-mms", "ky", "kir", 114_017_028, "a2f06e9d3d878a2b73adad4d560edadf0d626d4e1e29b7c0e721ac5e2e207eeb", 411, "7ae294618335285bcef6a10078053aa315a2403bfd763a196eba954e578bd53f"),
    mms_pack!("lao-mms", "lo", "lao", 114_027_012, "4fe7b25f40804781e1f91340ce1183bba637f47d20a77a27aeff8eb3efbe77e5", 339, "2efa143e564d865e23290e645adccecd3fc68cf880441f7276c34083da74891b"),
    archive_pack!("latvian-aivars", "Piper", "lv", "", "vits-piper-lv_LV-aivars-medium", "lv_LV-aivars-medium.onnx", 67_196_173, "a0df58c9924761e201ef75d82473b90bde32c5372267e9b4c628fdd267cf2508", "CC0-1.0", ["Aivars" Man]),
    archive_pack!("lithuanian-cv", "Coqui", "lt", "", "vits-coqui-lt-cv", "model.onnx", 67_056_512, "51d82dbcdf4bf24cb74c28b62c72ca1108a0663728516f72e915e42c079ba504", "BSD-3-Clause", ["Common Voice" Unsaid]),
    piper_pack!("lithuanian-reginute", "lt", "", "piper-lt-lt-reginute1-medium", "lt/lt_LT/reginute1/medium/", "lt_LT-reginute1-medium.onnx", 63_516_051, "0417c4ef351686ec0b0e7bf63426aefc72faea4a7693cfcb2f01ea8f6ee986ee", 5_274, "a0b3634fd41c4d2805a65605980471785b0894e7a8f8ee7597a90ce9dbc7a5a8", "CC-BY-4.0", ["Reginutė" Woman]),
    mms_pack!("luganda-mms", "lg", "lug", 114_010_884, "80998534c5b9f98615e212726e7ef5eb996b837e4b16fe1267d4a98ce543e333", 258, "5a6a6c8aa49c1c44811a8aa5041cb8728b0d219024fee3901cb982b025538adf"),
    archive_pack!("luxembourgish-marylux", "Piper", "lb", "", "vits-piper-lb_LU-marylux-medium", "lb_LU-marylux-medium.onnx", 67_184_319, "f11117fc121db0d5c4dc46e07eac7b1d8ea719b04f0b381836a23050310ea5a4", "CC-BY-NC-SA-4.0", ["Marylux" Woman]),
    mms_pack!("maithili-mms", "mai", "mai", 114_038_532, "a4b628a1ecf177effd82de4f46af51eb0e711a05b17643e2afb2522354e8b29a", 444, "1a2a8966d9c889496118dce0b1f80f83eee2c1533fc027a9d16cddba43cdb6fa"),
    mms_pack!("malagasy-mms", "mg", "mlg", 114_010_884, "14b608ff71fd2c2203f024782c1b65a977a75c8744f8a209807513cbb89a9f6d", 273, "3820c57711dd3efd606524206de79451726b39e7c643179960e13d926c873321"),
    mms_pack!("malay-mms", "ms", "zlm", 114_013_956, "4bcb70903e8ec84a94cd0e64c35633f365d4f44bd016d399b1bb8e7b4eb5894b", 275, "b15202c9758d0b77291cce82c06a9a3e5c110402e1e9074da2de1e9100098b49"),
    archive_pack!("malayalam-meera", "Piper", "ml", "", "vits-piper-ml_IN-meera-medium", "ml_IN-meera-medium.onnx", 67_195_010, "c8d870441e60870b7e8781c26eb22104aff51587b63bee94aba5e5f6c9fbb71f", "IndicTTS (IIT Madras)", ["Meera" Woman]),
    archive_pack!("malayalam-arjun", "Piper", "ml", "", "vits-piper-ml_IN-arjun-medium", "ml_IN-arjun-medium.onnx", 67_222_458, "3058d098e8b1ffcdd6069e96b1d492f319333235912a627c309c7c54cea59acf", "IndicTTS (IIT Madras)", ["Arjun" Man]),
    archive_pack!("maltese-cv", "Coqui", "mt", "", "vits-coqui-mt-cv", "model.onnx", 67_069_724, "c7090a9200828bbb6644ecfda75f3182b819ad667a7f6c8082b2bd72de115b1f", "BSD-3-Clause", ["Common Voice" Unsaid]),
    mms_pack!("marathi-mms", "mr", "mar", 114_043_908, "046cecf7cca77680c9ca319953e777c8d0233ce78e9b55a47aa909edf87d99e8", 490, "4d968029d0754b41633cb0871cce6796a5ab3d3bc2b9b91c5721cfdf85156083"),
    mms_pack!("mongolian-mms", "mn", "mon", 114_036_996, "53b6eff2ac8edec7d3b78bc4d58ca1c6cf122cf8315985201e7137886bb20d71", 671, "db75e80d45eff194330b411cffe020e592a97ad786e6025568c5d8f34621261c"),
    archive_pack!("nepali-chitwan", "Piper", "ne", "", "vits-piper-ne_NP-chitwan-medium", "ne_NP-chitwan-medium.onnx", 67_213_467, "65fa157c090844bbe5b38c1c3e4321535f569fdbb526c08aaa2ad22c09d51279", "CC0-1.0", ["Chitwan" Unsaid]),
    archive_pack!("norwegian-talesyntese", "Piper", "no", "", "vits-piper-no_NO-talesyntese-medium", "no_NO-talesyntese-medium.onnx", 67_194_829, "dcb7c36b7222e290cca1fd4cdecb474e7fd651dc5b0d3d42c33d416a8e62018b", "CC0-1.0", ["Talesyntese" Unsaid]),
    mms_pack!("odia-mms", "or", "ory", 114_046_212, "ab867f39b00f1fcb39dc97f5079150959b50df50d7f10e75232c56b52f19c021", 495, "bc90e5423446ca730aef6b56a4656cb07421c7538429a50b9c396fe0758fd587"),
    archive_pack!("persian-amir", "Piper", "fa", "", "vits-piper-fa_IR-amir-medium", "fa_IR-amir-medium.onnx", 67_180_373, "46af65c1cdd86ec300db3998f2bf31a68d9e38da930c0655c1ff6f610e84f864", "CC0-1.0", ["Amir" Man]),
    archive_pack!("persian-reza", "Piper", "fa", "", "vits-piper-fa_IR-reza_ibrahim-medium", "fa_IR-reza_ibrahim-medium.onnx", 67_187_249, "c5a6c1da647f93e284c7956013dd2262210365b80a4870bc28f1ba7bc1f00bfe", "CC0-1.0", ["Reza" Man]),
    archive_pack!("persian-ganji", "Piper", "fa", "", "vits-piper-fa_IR-ganji-medium", "fa_IR-ganji-medium.onnx", 67_186_157, "6eae2acccd1b4460159fa5acdad4bb5d2df6d64d8da3e4029dbdff4db14e7a7a", "CC0-1.0", ["Ganji" Man]),
    archive_pack!("polish-darkman", "Piper", "pl", "", "vits-piper-pl_PL-darkman-medium", "pl_PL-darkman-medium.onnx", 67_215_635, "444727aa46eb6db645a2bc88fe73868e4bd7596b7f56ca39fad5ef53c41210d4", "CC0-1.0", ["Darkman" Man]),
    archive_pack!("polish-gosia", "Piper", "pl", "", "vits-piper-pl_PL-gosia-medium", "pl_PL-gosia-medium.onnx", 67_211_182, "75bd34dcbdc4dd98d763954756b4b34b4208100497c836381542e4d73dcefa9c", "CC0-1.0", ["Gosia" Woman]),
    archive_pack!("polish-mc-speech", "Piper", "pl", "", "vits-piper-pl_PL-mc_speech-medium", "pl_PL-mc_speech-medium.onnx", 67_234_285, "6252744660fb9d2e4c93589b88d8d738df5003b0c651e52ed1202539aed77649", "CC0-1.0", ["MC Speech" Unsaid]),
    archive_pack!("polish-bass", "Piper", "pl", "", "vits-piper-pl_PL-bass-high", "pl_PL-bass-high.onnx", 115_576_949, "6640efabebff64f6ad066fa2b048b091961d63c2f5abf75eaebe81ddeeaea1e6", "Apache-2.0", ["Bass" Man]),
    archive_pack!("portuguese-faber", "Piper", "pt", "Brazil", "vits-piper-pt_BR-faber-medium", "pt_BR-faber-medium.onnx", 67_183_065, "7add3f923ad6bc25ca8a192805fd1a64d1b3893e4611c4a9719545a825039a83", "CC0-1.0", ["Faber" Unsaid]),
    archive_pack!("portuguese-cadu", "Piper", "pt", "Brazil", "vits-piper-pt_BR-cadu-medium", "pt_BR-cadu-medium.onnx", 67_207_562, "aba78157d4b89acc17ddef15a70f4b2474f4c189d4de6035002ac7fec9d5d303", "CC0-1.0", ["Cadu" Man]),
    archive_pack!("portuguese-jeff", "Piper", "pt", "Brazil", "vits-piper-pt_BR-jeff-medium", "pt_BR-jeff-medium.onnx", 67_207_052, "da4c870fc7b20600c74261b3e1dd1c816da2ce063e18c46e1f2cd86dea88f3f0", "CC0-1.0", ["Jeff" Man]),
    archive_pack!("portuguese-tugao", "Piper", "pt", "Portugal", "vits-piper-pt_PT-tugao-medium", "pt_PT-tugao-medium.onnx", 67_195_014, "6b4f0a7bfc957bef26d6ac0703a8fa1ba659c8ab2df8b294f20206551859070c", "CC0-1.0", ["Tugão" Unsaid]),
    mms_pack!("punjabi-mms", "pa", "pan", 114_033_156, "63e6eccea5b296ab8e9d01107bd5d7a090d5db19d3b61119ba5f920f1430fa87", 397, "91344d6f3d6287e8adc71e66c7e9ca83f5f6e9a7d0c24f043a219773d9dec163"),
    mms_pack!("quechua-mms", "qu", "quz", 114_016_260, "fa783294c797fb071060219c18344951711e5ec077274f3b3a8a6a0c11a3c878", 340, "282d9f760ca388baa9cab340d1efa9fb8362300dc353c4080e062d0d74d74649"),
    archive_pack!("romanian-mihai", "Piper", "ro", "", "vits-piper-ro_RO-mihai-medium", "ro_RO-mihai-medium.onnx", 67_182_137, "cfbee3cdc25cf063339f69e189f4d93cc3be6610d267567537323ced3cf87f4d", "CC0-1.0", ["Mihai" Man]),
    archive_pack!("russian-denis", "Piper", "ru", "", "vits-piper-ru_RU-denis-medium", "ru_RU-denis-medium.onnx", 67_190_991, "efa4c18e0b5e32b81d1b6df36b9d312831e5d545200e27848ef926a4cd930300", "CC0-1.0", ["Denis" Unsaid]),
    archive_pack!("russian-dmitri", "Piper", "ru", "", "vits-piper-ru_RU-dmitri-medium", "ru_RU-dmitri-medium.onnx", 67_188_551, "c86d0803737de13d441923ff3b3f309482fab8d7af3ec85949942809eb9a3660", "CC0-1.0", ["Dmitri" Unsaid]),
    archive_pack!("russian-ruslan", "Piper", "ru", "", "vits-piper-ru_RU-ruslan-medium", "ru_RU-ruslan-medium.onnx", 67_210_684, "0690b1cad01f86e8db9ba988af24898bdc1af774e23cb2e46b9c730269b6fd83", "CC-BY-NC-SA-4.0", ["Ruslan" Man]),
    mms_pack!("samoan-mms", "sm", "smo", 114_007_812, "c9fb1cf6d65d0c544048aa8f34c1b2603c93ba544550945f63aeee960f9c97d2", 231, "d20e8db7cfac406165243b0c90e405d1b6db57d8454240d9a9a51bd01212b0a1"),
    archive_pack!("setswana-nwu", "Mimic 3", "tn", "", "vits-mimic3-tn_ZA-google-nwu_low", "tn_ZA-google-nwu_low.onnx", 79_920_863, "7f43753eb4d3c4b17ff43c8764d2fb90204ba5e8247ee4023cfe9e0ac40816d3", "CC-BY-SA-4.0", ["NWU" Unsaid]),
    mms_pack!("shona-mms", "sn", "sna", 114_013_188, "4d419c2c53b674f8d7cb72c0f52ed571db4043d465797ec1a1031c53483094de", 272, "98391f18dc3ba8db4876b75abe253a0053d49c02b85c4295f1db8c6af646f443"),
    archive_pack!("slovak-lili", "Piper", "sk", "", "vits-piper-sk_SK-lili-medium", "sk_SK-lili-medium.onnx", 67_183_092, "a00dfbca5d34733862a59d9e309f4e94a92fccc9337c2b124de9a241b604fc0d", "CC0-1.0", ["Lili" Woman]),
    archive_pack!("slovenian-artur", "Piper", "sl", "", "vits-piper-sl_SI-artur-medium", "sl_SI-artur-medium.onnx", 67_025_668, "df805f31c4513864edebf7e287cf0b54d72788fc661ed73f88dc4e5aabc5efd5", "CC-BY-4.0", ["Artur" Man]),
    mms_pack!("somali-mms", "so", "som", 114_008_580, "0199dd00b919d345110a04a7d3c96f621834afcce4bff396afd493c3e10de0c1", 232, "7735b35bace882ce0116589b5243f4072d7ff26f76903353bf879e3501c9b088"),
    archive_pack!("spanish-claude", "Piper", "es", "Mexico", "vits-piper-es_MX-claude-high", "es_MX-claude-high.onnx", 67_207_890, "ec33fb689c248fe64810aab564cba97babf0f506672cfd404928d46e751a4721", "Apache-2.0", ["Claude" Unsaid]),
    archive_pack!("spanish-davefx", "Piper", "es", "Spain", "vits-piper-es_ES-davefx-medium", "es_ES-davefx-medium.onnx", 67_184_952, "a3f6beb54a9cb893279f72978a22f807a4d9fc9c7848157b524d5cc7b7f58b22", "CC0-1.0", ["Davefx" Man]),
    archive_pack!("spanish-sharvard", "Piper", "es", "Spain", "vits-piper-es_ES-sharvard-medium", "es_ES-sharvard-medium.onnx", 80_318_184, "b30a7a83df0518f0ee1c7039506648cade99f1f9b498fc49ed2ced2e2536bb5a", "CC-BY-3.0", ["Sharvard 1" Man, "Sharvard 2" Woman]),
    archive_pack!("spanish-ald", "Piper", "es", "Mexico", "vits-piper-es_MX-ald-medium", "es_MX-ald-medium.onnx", 67_196_497, "c60cf7bc853eb01a875fba0756523fecf9d9f46c763b19785d223fc981fb4fb6", "Unlicense", ["Ald" Man]),
    archive_pack!("spanish-daniela", "Piper", "es", "Argentina", "vits-piper-es_AR-daniela-high", "es_AR-daniela-high.onnx", 115_562_134, "71cbf6b7f646ab74f3c51336151abef41e6c54467ac929ffdb19ed706a07dd7b", "CC-BY-SA-4.0", ["Daniela" Woman]),
    mms_pack!("sundanese-mms", "su", "sun", 114_016_260, "be1f351b1b9b58c89f864b997a2b441e4963affacc0dffe3d7db1f50cbe31ebd", 281, "b0803ad9a381188f7ecb9fcb6b677596b42550ef084d8533ae5540f546e8dde4"),
    mms_pack!("swahili-mms", "sw", "swh", 114_017_796, "af4f2e2174960af06a7a7d07810ea7eb2d78fba827be7690abb230159717d250", 304, "783097de87c5fbd965298ffd8733986d6f4d2e7d14749965a2836f96f198a293"),
    archive_pack!("swedish-alma", "Piper", "sv", "", "vits-piper-sv_SE-alma-medium", "sv_SE-alma-medium.onnx", 67_154_700, "4ab8212595575e52595bc95c37d0b4f79450c0f00b63496346cdc5941d913edc", "CC-BY-4.0", ["Alma" Woman]),
    archive_pack!("swedish-nst", "Piper", "sv", "", "vits-piper-sv_SE-nst-medium", "sv_SE-nst-medium.onnx", 67_144_884, "5aed0f5ab75135fc3ba6a4f3567e171d72df753305b08a01ca42f79411587f67", "CC0-1.0", ["NST" Unsaid]),
    mms_pack!("tajik-mms", "tg", "tgk", 114_019_332, "45cc7aecb4b7dae96a8ace8a72e09b00eaf311e2f1bb69f44da7013eb275cfeb", 449, "16f3918b85b7ba3c9346dd0934acc415697ed22dcc9d7793dc80034b2f8abf0d"),
    mms_pack!("tamil-mms", "ta", "tam", 114_032_388, "c44055b2e7c5719ca26708f6eb03680b0f1bbf6fdcb0df6480478f872f1f6e34", 375, "0b3f692319bb5fae8658e2f84bf252bca92450d0207bbba7273caa1a182d81b8"),
    mms_pack!("tatar-mms", "tt", "tat", 114_020_868, "80b5ba3b03f60ba0a23c6c5b05a32a4ecc9e300ed846d2a9ba5b735c48ad9c63", 464, "ff132903b330ff091179534f3225a59a8384103597546b96c1ef26267fa081f1"),
    piper_pack!("telugu-maya", "te", "", "piper-te-in-maya-medium", "te/te_IN/maya/medium/", "te_IN-maya-medium.onnx", 62_950_044, "c3518ad4e3ca8ea6059c1e002f3772068f634960f58b237a96ff629db1c6200e", 5_040, "c07074aadf0a33e230647611af9041e1fb6609b995d017ee95009586a491508f", "IndicTTS (IIT Madras)", ["Maya" Woman]),
    piper_pack!("telugu-venkatesh", "te", "", "piper-te-in-venkatesh-medium", "te/te_IN/venkatesh/medium/", "te_IN-venkatesh-medium.onnx", 63_516_050, "dfaa5b7833cd48d946f3fe18c9c934aaa4e8590aac6922fddf34783a694c3c87", 4_973, "59bad556763d1f24b3434201d7bdee275bb1a70db3e1c65d38e6c3d39b224343", "CC-BY-4.0", ["Venkatesh" Man]),
    mms_pack!("thai-mms", "th", "tha", 114_042_372, "c740d5cafb6d1f64a38b56d581d7384dd9d8488940ea911b23048558be7cf91e", 473, "7e1f455df08c2bed0583f91b91921d6fc4d149bc232511a0ae11041cf489d798"),
    archive_pack!("turkish-fahrettin", "Piper", "tr", "", "vits-piper-tr_TR-fahrettin-medium", "tr_TR-fahrettin-medium.onnx", 67_212_827, "2ee3c18b5dc0a5faf913f995b027163befa6e7a6ad2f5c6b8a2e99fe1aad8b6d", "CC0-1.0", ["Fahrettin" Man]),
    archive_pack!("turkish-fettah", "Piper", "tr", "", "vits-piper-tr_TR-fettah-medium", "tr_TR-fettah-medium.onnx", 67_174_342, "314a9910616fb17be882c77f0bcf76796a1914d4d606f3c984f9094cb9abf1e5", "CC0-1.0", ["Fettah" Unsaid]),
    archive_pack!("turkish-dfki", "Piper", "tr", "", "vits-piper-tr_TR-dfki-medium", "tr_TR-dfki-medium.onnx", 67_201_221, "b63babaefcdd63202c7079aba25b72a2013825ad5895e2aeff03d1ae5a5b3bbf", "CC-BY-NC-SA-4.0", ["DFKI" Unsaid]),
    mms_pack!("turkish-mms", "tr", "tur", 114_020_868, "df761c9725d13be9751de46c80612edc2b76fd32d1d40b23d1d819d4061cc60b", 365, "26287a3bbc21e557ed30ac16747cd1e0a2fa58859051666ad0e1b26befbf2648"),
    mms_pack!("turkmen-mms", "tk", "tuk-script_latin", 114_015_492, "c6428ac26f29ca22d9d9019d1641692020343653643d0c940609184e7e14e53f", 330, "2109c3e04a4765ec2757e618a6da75b32f4e692a06e13183c2eecd2806791895"),
    piper_pack!("ukrainian-mykyta", "uk", "", "piper-uk-ua-mykyta-high", "uk/uk_UA/mykyta/high/", "uk_UA-mykyta-high.onnx", 114_204_024, "081d253cd246d7d4d698c6dd147b74cad498dafd213527887a3a490519138243", 4_904, "b5db6b4efb02d4a0e8f128253fd64e82dfa72d1105e9060b0c6fd9610eec6a4f", "Apache-2.0", ["Mykyta" Man]),
    piper_pack!("ukrainian-tetiana", "uk", "", "piper-uk-ua-tetiana-high", "uk/uk_UA/tetiana/high/", "uk_UA-tetiana-high.onnx", 114_204_024, "1206d8447b99632badeb63d6132241f76cf551874e5254eb8836db36aed7c85c", 4_905, "c96a051028976afc74269f73470651384deb6872431f890cad3b6424d09e812b", "Apache-2.0", ["Tetiana" Woman]),
    archive_pack!("urdu-fasih", "Piper", "ur", "", "vits-piper-ur_PK-fasih-medium", "ur_PK-fasih-medium.onnx", 67_227_718, "de64e03109f3c896b7cf910db6d9ffd2703e3d665580d9a19bedbf3e0760d0e4", "MIT", ["Fasih" Man]),
    piper_pack!("urdu-aegis", "ur", "", "piper-ur-pk-aegis-female-medium", "ur/ur_PK/aegis_female/medium/", "ur_PK-aegis_female-medium.onnx", 63_515_589, "3ec306035cfd0bbc22342fee5890450708a16dd231809522731678aa583c961f", 5_236, "a8385c217a7e276acf5234bf01dbd0adb007a57c32ffbcc57d2db7eac3979bb6", "MIT", ["Aegis" Woman]),
    mms_pack!("uyghur-mms", "ug", "uig-script_arabic", 114_019_332, "291be670067d93b0e69d285e8b72fed97428789eefbdcf113d2a77a2e538e069", 227, "0cebcea7931fbe1533c9581070d4bb67a30774650d3cbefe6c39c864c55c371f"),
    mms_pack!("uzbek-mms", "uz", "uzb-script_cyrillic", 114_022_404, "98a3c603d3721e3224166ec6cc6b91c2a4e470e7e0a6fee193834d320437639d", 448, "7a0f66003c6a99a5cd5a783dfede839b74897ec594a642f304c11af1a5d1c8ee"),
    archive_pack!("vietnamese-vais1000", "Piper", "vi", "", "vits-piper-vi_VN-vais1000-medium", "vi_VN-vais1000-medium.onnx", 67_154_040, "fa1367710767d36ed5cf13b4a449e20c35ffd12791c2e47c2e64142bfa55551a", "CC-BY-4.0", ["VAIS" Woman]),
    archive_pack!("welsh-bu-tts", "Piper", "cy", "", "vits-piper-cy_GB-bu_tts-medium", "cy_GB-bu_tts-medium.onnx", 80_313_215, "09e5fd87e68ff28ffa9fbfd58a0e307acb6a45c572db1d4c06ed98f01ff5fe6a", "CC-BY-4.0", ["South 1" Woman, "North 1" Woman, "North 2" Man, "North 3" Man, "South 2" Man, "North 4" Man, "North 5" Woman]),
    mms_pack!("yoruba-mms", "yo", "yor", 114_020_868, "1daf1b837dfb032995a9afdfd1b974d07fd69fd42c91765fc551969ba6d4c183", 416, "3fca3219266d24915a05e358f16fc57e7f748150cf25d1ffe5b8b6e530f915f0"),
];

/// The terms of a voice's licence that limits commercial use: what Settings › Voice shows under its (i) mark.
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
              and other non-commercial purposes, with credit to its authors. Commercial use is not permitted.",
    url: "https://creativecommons.org/licenses/by-nc/4.0/",
};

const NON_COMMERCIAL_SHARE_ALIKE: Terms = Terms {
    title: "Non-commercial use only",
    summary: "This voice is licensed under the Creative Commons Attribution-NonCommercial-ShareAlike 4.0 \
              International License (CC BY-NC-SA 4.0). You may use it, and the audio it produces, for personal, \
              educational, research and other non-commercial purposes, with credit to its authors; what you make from \
              the voice itself is shared under the same license. Commercial use is not permitted.",
    url: "https://creativecommons.org/licenses/by-nc-sa/4.0/",
};

const NON_COMMERCIAL_SHARE_ALIKE_3: Terms = Terms {
    title: "Non-commercial use only",
    summary: "This voice is licensed under the Creative Commons Attribution-NonCommercial-ShareAlike 3.0 \
              License (CC BY-NC-SA 3.0). You may use it, and the audio it produces, for personal, educational, \
              research and other non-commercial purposes, with credit to its authors; what you make from the voice \
              itself is shared under the same license. Commercial use is not permitted.",
    url: "https://creativecommons.org/licenses/by-nc-sa/3.0/",
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
            "CC-BY-NC-SA-4.0" => Some(NON_COMMERCIAL_SHARE_ALIKE),
            "CC-BY-NC-SA-3.0" => Some(NON_COMMERCIAL_SHARE_ALIKE_3),
            "IndicTTS (IIT Madras)" => Some(INDIC_TTS),
            _ => None,
        }
    }

    /// Its name in a sentence: "Kokoro", or the language and its voice ("Spanish · Davefx", "Swahili · MMS").
    pub fn title(&self) -> String {
        if self.language.is_empty() {
            return self.name.to_string();
        }
        let voice = match self.speakers {
            [one] => one,
            _ => self.name,
        };
        format!("{} · {voice}", languages::name(self.language))
    }

    /// The voice read when none was chosen: Kokoro's af_heart, else the first.
    pub fn first_speaker(&self) -> usize {
        self.speaker("af_heart").unwrap_or(0)
    }
}

/// The smaller download of a pack's voices: the same voices, in a third of the size.
const SMALLER: [(&str, &str); 1] = [("kokoro", "kokoro-small")];

/// The smaller download of `pack`'s voices, when there is one.
pub fn smaller(pack: &VoicePack) -> Option<&'static VoicePack> {
    SMALLER
        .iter()
        .find(|(of, _)| *of == pack.id)
        .and_then(|(_, small)| voice_pack(small))
}

/// The pack `pack` is the smaller download of, when it is one.
pub fn larger(pack: &VoicePack) -> Option<&'static VoicePack> {
    SMALLER
        .iter()
        .find(|(_, small)| *small == pack.id)
        .and_then(|(of, _)| voice_pack(of))
}

/// A voice offered for a language: speaker `speaker` of `pack`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voice {
    pub pack: &'static VoicePack,
    pub speaker: usize,
}

impl Voice {
    /// Its name, as the settings keep it (`voice`).
    pub fn name(&self) -> &'static str {
        self.pack.speakers[self.speaker]
    }

    /// The language it reads, by its code; `None` for a voice not offered (Kokoro's Japanese).
    pub fn language(&self) -> Option<&'static str> {
        if self.pack.language.is_empty() {
            kokoro_language(self.name()).map(|(language, _, _)| language)
        } else {
            Some(self.pack.language)
        }
    }

    pub fn who(&self) -> Who {
        if self.pack.who.is_empty() {
            match self.name().as_bytes().get(1) {
                Some(b'f') => Who::Woman,
                Some(b'm') => Who::Man,
                _ => Who::Unsaid,
            }
        } else {
            self.pack
                .who
                .get(self.speaker)
                .copied()
                .unwrap_or(Who::Unsaid)
        }
    }

    pub fn accent(&self) -> &'static str {
        if self.pack.language.is_empty() {
            kokoro_language(self.name()).map_or("", |(_, _, accent)| accent)
        } else {
            self.pack.accent
        }
    }

    /// In words, for a list of one language's voices: "Heart · woman (American)", "Davefx · man (Spain)", "MMS".
    pub fn label(&self) -> String {
        let name = self.name();
        let given = match name.split_once('_') {
            Some((_, given)) if self.pack.language.is_empty() => {
                let mut chars = given.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect::<String>()
                })
            }
            _ => name.to_string(),
        };
        let who = match self.who() {
            Who::Woman => "woman",
            Who::Man => "man",
            Who::Unsaid => "",
        };
        match (who, self.accent()) {
            ("", "") => given,
            (who, "") => format!("{given} · {who}"),
            ("", accent) => format!("{given} ({accent})"),
            (who, accent) => format!("{given} · {who} ({accent})"),
        }
    }
}

/// The voices offered for `language`, in the order of the packs (the most natural first). The smaller Kokoro has the
/// same voices as Kokoro: it is offered as their smaller download, not here.
pub fn voices_for(language: &str) -> Vec<Voice> {
    VOICE_PACKS
        .iter()
        .filter(|pack| larger(pack).is_none())
        .flat_map(|pack| {
            // the pack's own first voice before the rest (Kokoro's Heart, the voice English is read with until another
            // is chosen), the rest in the order of their numbers
            let first = pack.first_speaker();
            std::iter::once(first)
                .chain((0..pack.speakers.len()).filter(move |s| *s != first))
                .map(move |speaker| Voice { pack, speaker })
        })
        .filter(|voice| voice.language() == Some(language))
        .collect()
}

/// The pack a language QuickAI has no voice for is read with, until the person uses a voice of their own.
pub const DEFAULT_PACK: &str = "kokoro";

/// The languages there is no voice for yet, by their codes, in the order of their names.
pub fn unoffered_languages() -> &'static [&'static str] {
    static UNOFFERED: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    UNOFFERED.get_or_init(|| {
        LANGUAGES
            .iter()
            .filter(|l| voices_for(l.code).is_empty())
            .map(|l| l.code)
            .collect()
    })
}

/// The voice of a pack chosen (`id`), by its name in the settings (`name`), or the pack's first.
pub fn voice_in(id: &str, name: Option<&str>) -> Option<Voice> {
    let pack = voice_pack(id)?;
    let speaker = name
        .and_then(|n| pack.speaker(n))
        .unwrap_or(pack.first_speaker());
    Some(Voice { pack, speaker })
}

/// A sentence in each language there are voices for, for Settings › Voice to read: "Hello. This is how answers are
/// read aloud.", or a greeting where that is the surer sentence.
const SAMPLES: &[(&str, &str)] = &[
    ("af", "Hallo. So word die antwoorde hardop gelees."),
    ("ak", "Maakye. Wo ho te sɛn?"),
    ("ar", "مرحبًا. هكذا تُقرأ الإجابات بصوت عالٍ."),
    ("as", "নমস্কাৰ, আপুনি কেনে আছে?"),
    ("ay", "Aski urukipan. Kamisaraki?"),
    ("az", "Salam. Cavablar belə ucadan oxunur."),
    ("bg", "Здравейте. Така отговорите се четат на глас."),
    ("bn", "নমস্কার, আপনি কেমন আছেন?"),
    ("bo", "བཀྲ་ཤིས་བདེ་ལེགས།"),
    (
        "ca",
        "Hola. Així és com es llegeixen les respostes en veu alta.",
    ),
    ("ceb", "Maayong buntag. Kumusta ka?"),
    ("cs", "Dobrý den. Takto se odpovědi čtou nahlas."),
    (
        "cy",
        "Helo. Dyma sut mae'r atebion yn cael eu darllen yn uchel.",
    ),
    ("da", "Hej. Sådan bliver svarene læst højt."),
    ("de", "Hallo. So werden die Antworten vorgelesen."),
    ("dv", "އައްސަލާމު ޢަލައިކުމް. ކިހިނެއް ހާލު؟"),
    ("dz", "ཀུ་ཟུ་ཟང་པོ་ལ།"),
    ("ee", "Ŋdi na wò. Èfɔ̀a?"),
    ("el", "Γεια σας. Έτσι διαβάζονται φωναχτά οι απαντήσεις."),
    ("en", "Hello. This is how QuickAI reads its answers."),
    (
        "es",
        "Hola. Así es como se leen las respuestas en voz alta.",
    ),
    ("et", "Tere. Nii loetakse vastuseid ette."),
    ("eu", "Kaixo. Horrela irakurtzen dira erantzunak ozen."),
    ("fa", "سلام. پاسخ‌ها این‌گونه با صدای بلند خوانده می‌شوند."),
    ("ff", "Jam waali. No mbaɗ-ɗaa?"),
    ("fi", "Hei. Näin vastaukset luetaan ääneen."),
    ("fj", "Bula. O vakacava tiko?"),
    ("fo", "Góðan dag. Hvussu hevur tú tað?"),
    (
        "fr",
        "Bonjour. Voici comment les réponses sont lues à voix haute.",
    ),
    ("ga", "Dia duit. Seo mar a léitear na freagraí os ard."),
    ("gn", "Mba'éichapa reiko?"),
    ("gu", "નમસ્તે, તમે કેમ છો?"),
    ("ha", "Sannu. Yaya kake?"),
    ("he", "שלום. כך התשובות מוקראות בקול."),
    ("hi", "नमस्ते। उत्तर इस तरह बोलकर पढ़े जाते हैं।"),
    ("hr", "Bok. Ovako se odgovori čitaju naglas."),
    ("ht", "Bonjou. Kòman ou ye?"),
    ("hu", "Helló! Így olvasom fel a válaszokat."),
    ("hy", "Բարև։ Այսպես են պատասխանները կարդացվում բարձրաձայն։"),
    (
        "id",
        "Halo. Beginilah jawaban dibacakan dengan suara keras.",
    ),
    ("ilo", "Naimbag nga aldaw. Kumusta ka?"),
    ("is", "Halló. Svona eru svörin lesin upp."),
    (
        "it",
        "Ciao. Ecco come vengono lette ad alta voce le risposte.",
    ),
    ("ja", "こんにちは。回答はこのように読み上げられます。"),
    ("jw", "Sugeng enjing, piye kabare?"),
    ("ka", "გამარჯობა. ასე იკითხება პასუხები ხმამაღლა."),
    ("kk", "Сәлеметсіз бе. Жауаптар осылай дауыстап оқылады."),
    ("km", "សួស្តី។ សុខសប្បាយជាទេ?"),
    ("kn", "ನಮಸ್ಕಾರ, ನೀವು ಹೇಗಿದ್ದೀರಿ?"),
    ("ko", "안녕하세요. 이렇게 답변을 소리 내어 읽습니다."),
    ("ku", "Silav. Bersiv bi vî awayî bi deng têne xwendin."),
    ("ky", "Салам. Жооптор ушундай үн чыгарып окулат."),
    ("lb", "Moien. Esou ginn d'Äntwerte virgelies."),
    ("lg", "Gyebale ko. Oli otya?"),
    ("lo", "ສະບາຍດີ. ເຈົ້າສະບາຍດີບໍ່?"),
    ("lt", "Labas. Štai kaip atsakymai skaitomi garsiai."),
    ("lv", "Sveiki. Šādi atbildes tiek nolasītas skaļi."),
    ("mai", "प्रणाम। अहाँ केहन छी?"),
    ("mg", "Manao ahoana. Salama ve ianao?"),
    ("ml", "നമസ്കാരം, സുഖമാണോ?"),
    ("mn", "Сайн байна уу. Хариултыг ингэж чангаар уншина."),
    ("mr", "नमस्कार, तुम्ही कसे आहात?"),
    ("ms", "Helo. Beginilah jawapan dibacakan dengan kuat."),
    ("mt", "Bonġu. Hekk jinqraw it-tweġibiet b'leħen għoli."),
    ("my", "မင်္ဂလာပါ။ နေကောင်းလား။"),
    ("ne", "नमस्ते, तपाईंलाई कस्तो छ?"),
    ("nl", "Hallo. Zo worden de antwoorden voorgelezen."),
    ("no", "Hei. Slik blir svarene lest opp."),
    ("ny", "Moni. Muli bwanji?"),
    ("om", "Akkam. Nagaa jirtaa?"),
    ("or", "ନମସ୍କାର, ଆପଣ କେମିତି ଅଛନ୍ତି?"),
    ("pa", "ਸਤ ਸ੍ਰੀ ਅਕਾਲ, ਤੁਸੀਂ ਕਿਵੇਂ ਹੋ?"),
    ("pl", "Cześć. Tak odpowiedzi są czytane na głos."),
    ("pt", "Olá. É assim que as respostas são lidas em voz alta."),
    ("qu", "Allin p'unchay. Imaynallan kashanki?"),
    (
        "ro",
        "Bună ziua. Așa sunt citite cu voce tare răspunsurile.",
    ),
    ("ru", "Здравствуйте. Так ответы читаются вслух."),
    ("rw", "Muraho. Amakuru yawe?"),
    ("sk", "Dobrý deň. Takto sa odpovede čítajú nahlas."),
    ("sl", "Pozdravljeni. Tako se odgovori berejo na glas."),
    ("sm", "Talofa. O a mai oe?"),
    ("sn", "Mhoro. Makadii?"),
    ("so", "Iska warran. Sidee tahay?"),
    ("sq", "Përshëndetje. Kështu lexohen përgjigjet me zë."),
    ("su", "Wilujeng enjing. Kumaha damang?"),
    ("sv", "Hej. Så här läses svaren upp."),
    ("sw", "Habari. Hivi ndivyo majibu yanavyosomwa kwa sauti."),
    ("ta", "வணக்கம், நீங்கள் எப்படி இருக்கிறீர்கள்?"),
    ("te", "నమస్కారం, మీరు ఎలా ఉన్నారు?"),
    (
        "tg",
        "Салом. Ҷавобҳо ҳамин тавр бо овози баланд хонда мешаванд.",
    ),
    ("th", "สวัสดี นี่คือวิธีอ่านคำตอบออกเสียง"),
    ("tk", "Salam. Jogaplar şeýle sesli okalýar."),
    ("tl", "Kumusta. Ganito binabasa nang malakas ang mga sagot."),
    ("tn", "Dumela. O tsogile jang?"),
    ("tr", "Merhaba. Yanıtlar bu şekilde sesli okunur."),
    ("tt", "Сәлам. Җаваплар менә шулай кычкырып укыла."),
    ("ug", "ئەسسالامۇ ئەلەيكۇم. ياخشىمۇسىز؟"),
    ("uk", "Привіт. Ось так відповіді читаються вголос."),
    (
        "ur",
        "السلام علیکم۔ جوابات اس طرح بلند آواز سے پڑھے جاتے ہیں۔",
    ),
    ("uz", "Салом. Жавоблар шундай овоз чиқариб ўқилади."),
    ("vi", "Xin chào. Đây là cách các câu trả lời được đọc to."),
    ("yo", "Ẹ káàárọ̀. Báwo ni?"),
    ("yue", "你好。答案就係咁樣讀出嚟。"),
    ("zh", "你好。回答就是这样朗读出来的。"),
];

/// The sentence Settings › Voice reads to let a voice be heard, in the language it reads (English for a voice of the
/// person's own).
pub fn sample(language: Option<&str>) -> &'static str {
    language
        .and_then(|code| SAMPLES.iter().find(|(c, _)| *c == code))
        .map_or(SAMPLES_ENGLISH, |(_, sentence)| sentence)
}

const SAMPLES_ENGLISH: &str = "Hello. This is how QuickAI reads its answers.";

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

    /// What quickai-voice loads for it, to read with speaker `speaker`.
    pub fn spec(&self, speaker: usize) -> ModelSpec {
        self.spec_in(&self.dir(), &espeak_dir(), speaker)
    }

    /// `spec`, for the pack unpacked in `dir` (and the shared espeak-ng data in `espeak`): a VITS voice with the
    /// espeak-ng data and lexicon it has; a voice of Kokoro's with the lexicon of its English, and the language it reads
    /// where sherpa-onnx must be told it.
    fn spec_in(&self, dir: &Path, espeak: &Path, speaker: usize) -> ModelSpec {
        let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
        match self.kind {
            ModelKind::Vits => {
                let mut files =
                    BTreeMap::from([("model", at(self.model)), ("tokens", at("tokens.txt"))]);
                // espeak-ng's data: the shared one for a voice of Piper's form, its own where a pack carries it; MMS's
                // and Coqui's voices read letters, and need none
                if self.piper {
                    files.insert("data_dir", espeak.to_string_lossy().into_owned());
                } else if dir.join("espeak-ng-data").is_dir() {
                    files.insert("data_dir", at("espeak-ng-data"));
                }
                if dir.join("lexicon.txt").is_file() {
                    files.insert("lexicon", at("lexicon.txt"));
                }
                ModelSpec {
                    kind: ModelKind::Vits,
                    files,
                }
            }
            _ => {
                let name = self.speakers.get(speaker).copied().unwrap_or("");
                let mut spec = describe_kokoro(dir, self.model, name.starts_with('b'));
                if self.language.is_empty() {
                    if let Some((_, lang, _)) = kokoro_language(name) {
                        if !lang.is_empty() {
                            spec.files.insert("lang", lang.to_string());
                        }
                    }
                }
                spec
            }
        }
    }

    /// The number of a voice by its name.
    pub fn speaker(&self, name: &str) -> Option<usize> {
        self.speakers.iter().position(|s| *s == name)
    }
}

/// A voice of Kokoro's in `dir`: its lexicons are its English (British for `british` voices, where it has that one)
/// and Chinese.
fn describe_kokoro(dir: &Path, model: &str, british: bool) -> ModelSpec {
    let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
    let english = if british && dir.join("lexicon-gb-en.txt").is_file() {
        "lexicon-gb-en.txt"
    } else {
        "lexicon-us-en.txt"
    };
    let lexicons: Vec<String> = [english, "lexicon-zh.txt"]
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
        return Ok(describe_kokoro(dir, model, false));
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
        // a phoneme of several letters (voices made since 2025 add the diphthongs, "aɪ") is one sherpa-onnx never
        // makes, for it reads espeak-ng's phonemes a letter at a time; and it refuses a tokens.txt that has one
        if symbol == "\n" || symbol.chars().count() != 1 {
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

    /// A folder of `files` (a name ending in / is a folder), made afresh in the temporary folder.
    fn folder_of(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("quickai-voice-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in files {
            match f.strip_suffix('/') {
                Some(sub) => std::fs::create_dir_all(dir.join(sub)).unwrap(),
                None => {
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join(f), b"x").unwrap();
                }
            }
        }
        dir
    }

    #[test]
    fn a_voice_is_said_by_its_name_who_it_sounds_like_and_its_accent() {
        let kokoro = voice_pack("kokoro").unwrap();
        let label = |name| {
            Voice {
                pack: kokoro,
                speaker: kokoro.speaker(name).unwrap(),
            }
            .label()
        };
        assert_eq!(label("af_heart"), "Heart · woman (American)");
        assert_eq!(label("bm_george"), "George · man (British)");
        assert_eq!(label("zf_xiaoxiao"), "Xiaoxiao · woman");
        assert_eq!(label("pm_alex"), "Alex · man (Brazil)");
        assert_eq!(kokoro.speakers.len(), 54);
        assert_eq!(kokoro.first_speaker(), 3);
        assert!(kokoro.downloads()[0]
            .0
            .ends_with("/tts-models/kokoro-multi-lang-v1_0.tar.bz2"));
        assert_eq!(
            voice_in("persian-amir", None).unwrap().label(),
            "Amir · man"
        );
        assert_eq!(voice_in("tamil-mms", None).unwrap().label(), "MMS");
        // a voice not known by that name is the pack's first
        assert_eq!(
            voice_in("kokoro", Some("nobody")).unwrap().name(),
            "af_heart"
        );
        assert_eq!(voice_in("nothing", None), None);
        assert_eq!(
            voice_pack("persian-amir").unwrap().title(),
            "Persian · Amir"
        );
        assert_eq!(kokoro.title(), "Kokoro");
    }

    /// Kokoro reads seven languages: sherpa-onnx is told the ones it reads through espeak-ng, and reads English (with
    /// the lexicon of the voice's English) and Chinese with its lexicons. Its Japanese voices are not offered.
    #[test]
    fn kokoro_is_told_the_language_of_each_voice_it_reads_through_espeak() {
        let kokoro = voice_pack("kokoro").unwrap();
        let dir = folder_of(
            "kokoro",
            &[
                "model.onnx",
                "voices.bin",
                "tokens.txt",
                "lexicon-us-en.txt",
                "lexicon-gb-en.txt",
                "lexicon-zh.txt",
                "espeak-ng-data/",
            ],
        );
        let spec = |name| kokoro.spec_in(&dir, &espeak_dir(), kokoro.speaker(name).unwrap());
        for (name, lang) in [
            ("ef_dora", "es"),
            ("ff_siwis", "fr"),
            ("hf_alpha", "hi"),
            ("im_nicola", "it"),
            ("pm_alex", "pt-br"),
        ] {
            assert_eq!(
                spec(name).files.get("lang").map(String::as_str),
                Some(lang),
                "{name}"
            );
        }
        for name in ["af_heart", "bm_george", "zf_xiaoxiao"] {
            assert!(!spec(name).files.contains_key("lang"), "{name}");
        }
        assert!(spec("bf_emma").files["lexicon"].contains("lexicon-gb-en.txt"));
        assert!(spec("af_heart").files["lexicon"].contains("lexicon-us-en.txt"));
        assert!(spec("af_heart").files["lexicon"].contains("lexicon-zh.txt"));
        // offered for its seven languages, first; not for Japanese, and the smaller download not as voices of its own
        for code in ["en", "zh", "hi", "es", "fr", "it", "pt"] {
            assert_eq!(voices_for(code)[0].pack.id, "kokoro", "{code}");
        }
        assert!(voices_for("ja")
            .iter()
            .all(|v| v.pack.kind != ModelKind::Kokoro));
        assert!(voices_for("en").iter().all(|v| v.pack.id == "kokoro"));
        assert_eq!(voices_for("en").len(), 28);
        // English chosen again is read with Heart, the voice it starts with, and not the first of Kokoro's table (Alloy)
        assert_eq!(voices_for("en")[0].name(), "af_heart");
        assert_eq!(voices_for("es")[0].name(), "ef_dora");
        assert_eq!(smaller(kokoro).map(|p| p.id), Some("kokoro-small"));
        assert_eq!(
            larger(voice_pack("kokoro-small").unwrap()).map(|p| p.id),
            Some("kokoro")
        );
        assert_eq!(smaller(voice_pack("persian-amir").unwrap()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every pack is pinned, reads a language QuickAI lists and has a sentence for, and says who each of its voices
    /// sounds like; ids and folders are each one pack's.
    #[test]
    fn every_pack_is_pinned_and_reads_a_language_of_the_list() {
        let mut ids = std::collections::BTreeSet::new();
        let mut folders = std::collections::BTreeSet::new();
        for pack in &VOICE_PACKS {
            assert!(ids.insert(pack.id), "{} twice", pack.id);
            assert!(folders.insert(pack.folder), "{} twice", pack.folder);
            let downloads = pack.downloads();
            assert!(
                !pack.speakers.is_empty() && !downloads.is_empty(),
                "{}",
                pack.id
            );
            assert!(
                downloads
                    .iter()
                    .all(|d| d.3.len() == 64 && d.3.chars().all(|c| c.is_ascii_hexdigit())),
                "{}",
                pack.id
            );
            if pack.language.is_empty() {
                assert_eq!(pack.kind, ModelKind::Kokoro, "{}", pack.id);
                assert!(pack.who.is_empty(), "{}", pack.id);
            } else {
                assert!(languages::language(pack.language).is_some(), "{}", pack.id);
                assert_eq!(pack.who.len(), pack.speakers.len(), "{}", pack.id);
                assert_ne!(sample(Some(pack.language)), SAMPLES_ENGLISH, "{}", pack.id);
            }
            // a voice of Piper's form brings its model and its .json
            if pack.piper {
                assert!(downloads
                    .iter()
                    .any(|d| d.1 == format!("{}.json", pack.model)));
            }
            // files of Hugging Face are kept in a folder the downloader takes
            if let PackSource::HuggingFace { .. } = pack.source {
                assert!(
                    quickai_core::lookup::present::is_place(pack.folder),
                    "{}",
                    pack.folder
                );
            }
            // the names of the voices are each one voice's
            let mut names: Vec<&str> = pack.speakers.to_vec();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), pack.speakers.len(), "{}", pack.id);
        }
        // every language with a voice is offered, and the others are said to be without one
        for l in &LANGUAGES {
            assert_eq!(
                voices_for(l.code).is_empty(),
                unoffered_languages().contains(&l.code),
                "{}",
                l.code
            );
        }
        assert!(voice_pack(DEFAULT_PACK).is_some());
    }

    #[test]
    fn a_licence_that_limits_commercial_use_has_its_terms() {
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
        for pack in &VOICE_PACKS {
            let limited = pack.license.contains("-NC") || pack.license.starts_with("IndicTTS");
            assert_eq!(
                pack.terms().is_some(),
                limited,
                "{}: {}",
                pack.id,
                pack.license
            );
        }
    }

    /// What quickai-voice loads for a VITS voice is the files its folder has: a voice of Piper's form reads with the
    /// shared espeak-ng data, a pack of sherpa-onnx's with its own, MMS's and Coqui's with none; a lexicon where it
    /// has one.
    #[test]
    fn a_vits_voice_is_loaded_with_what_its_folder_has() {
        let amir = voice_pack("persian-amir").unwrap();
        let dir = folder_of(
            "amir",
            &["fa_IR-amir-medium.onnx", "tokens.txt", "espeak-ng-data/"],
        );
        let spec = amir.spec_in(&dir, &espeak_dir(), 0);
        assert_eq!(spec.kind, ModelKind::Vits);
        assert!(Path::new(&spec.files["model"]).ends_with("fa_IR-amir-medium.onnx"));
        assert!(Path::new(&spec.files["data_dir"]).ends_with("espeak-ng-data"));
        assert_eq!(amir.speaker("Amir"), Some(0));
        // a pack without espeak-ng's data (characters, as Coqui's) is not told of any
        let bare = folder_of(
            "bare",
            &["fa_IR-amir-medium.onnx", "tokens.txt", "lexicon.txt"],
        );
        let spec = amir.spec_in(&bare, &espeak_dir(), 0);
        assert!(!spec.files.contains_key("data_dir"));
        assert!(Path::new(&spec.files["lexicon"]).ends_with("lexicon.txt"));
        // MMS reads letters: no espeak-ng data
        let tamil = voice_pack("tamil-mms").unwrap();
        let spec = tamil.spec(0);
        assert_eq!(spec.kind, ModelKind::Vits);
        assert!(!spec.files.contains_key("data_dir"));
        assert!(Path::new(&spec.files["model"]).ends_with("mms-tts-tam/model.onnx"));
        // a voice of Piper's form reads with the shared espeak-ng data
        let telugu = voice_pack("telugu-maya").unwrap().spec(0);
        assert_eq!(telugu.files["data_dir"], espeak_dir().to_string_lossy());
        // Nabra, Kokoro's Arabic, as Kokoro, and told no language: its model says it
        let nabra = voice_pack("arabic-nabra").unwrap().spec(0);
        assert_eq!(nabra.kind, ModelKind::Kokoro);
        assert!(Path::new(&nabra.files["voices"]).ends_with("nabra-82m-arabic-int8/voices.bin"));
        assert!(!nabra.files.contains_key("lang"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&bare);
    }

    /// With the real quickai-voice and the packs' downloads (`QUICKAI_TEST_VOICE_BIN`; `QUICKAI_TEST_PACKS`, a folder
    /// of what is downloaded for them, as the downloader leaves it), every pack (or those named in
    /// `QUICKAI_TEST_PACK_IDS`, separated by commas) is installed as QuickAI installs it, and each voice it offers reads
    /// its language's sentence through quickai-voice, loaded as QuickAI loads it (one voice of each of Kokoro's
    /// languages). Left out when they are not there.
    #[test]
    fn every_pack_installs_and_each_of_its_voices_reads() {
        use super::super::speak::{Backend, Speaker, Spoken};
        use std::time::Duration;
        let (Some(program), Some(downloads)) = (
            std::env::var_os("QUICKAI_TEST_VOICE_BIN"),
            std::env::var_os("QUICKAI_TEST_PACKS"),
        ) else {
            return;
        };
        let downloads = PathBuf::from(downloads);
        let only: Vec<String> = std::env::var("QUICKAI_TEST_PACK_IDS")
            .map(|ids| ids.split(',').map(str::to_string).collect())
            .unwrap_or_default();
        let root = std::env::temp_dir().join(format!("quickai-every-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // installing takes what was downloaded: it is given links to it
        let link = |from: &Path, to: &Path| -> Result<(), String> {
            std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::hard_link(from, to)
                .or_else(|_| std::fs::copy(from, to).map(|_| ()))
                .map_err(|e| format!("{}: {e}", from.display()))
        };
        let mut failed = Vec::new();
        let mut read = 0;
        for pack in VOICE_PACKS
            .iter()
            .filter(|p| only.is_empty() || only.iter().any(|id| id == p.id))
        {
            let staging = root.join("downloads");
            let _ = std::fs::remove_dir_all(&staging);
            let linked: Result<(), String> =
                pack.downloads()
                    .iter()
                    .try_for_each(|(_, name, _, _)| match pack.source {
                        PackSource::Archive { .. } => {
                            link(&downloads.join(name), &staging.join(name))
                        }
                        PackSource::HuggingFace { .. } => link(
                            &downloads.join(pack.folder).join(name),
                            &staging.join(pack.folder).join(name),
                        ),
                    });
            let linked = linked.and_then(|()| {
                if pack.piper && !espeak_here(&root) {
                    link(&downloads.join(ESPEAK_DATA.0), &staging.join(ESPEAK_DATA.0))
                } else {
                    Ok(())
                }
            });
            if let Err(why) = linked.and_then(|()| install_pack_at(pack, &staging, &root)) {
                failed.push(format!("{}: {why}", pack.id));
                continue;
            }
            let dir = root.join("packs").join(pack.folder);
            let mut speakers: Vec<usize> = (0..pack.speakers.len())
                .filter(|&s| Voice { pack, speaker: s }.language().is_some())
                .collect();
            if pack.language.is_empty() {
                let mut seen = Vec::new();
                speakers.retain(|&s| {
                    let language = Voice { pack, speaker: s }.language();
                    let new = !seen.contains(&language);
                    seen.push(language);
                    new
                });
            }
            for speaker in speakers {
                let voice = Voice { pack, speaker };
                let spec = pack.spec_in(&dir, &root.join("espeak-ng-data"), speaker);
                let (wake_tx, wake) = std::sync::mpsc::channel::<()>();
                let reader = Speaker::new(
                    Ok(Backend::Voice {
                        program: program.clone().into(),
                        spec,
                        speaker,
                    }),
                    move || {
                        let _ = wake_tx.send(());
                    },
                );
                reader.say(1, vec![sample(voice.language()).into()], false, None, 1.0);
                let mut heard = Vec::new();
                let outcome = loop {
                    if wake.recv_timeout(Duration::from_secs(90)).is_err() {
                        break Err("no answer in 90 s".to_string());
                    }
                    heard.extend(reader.take());
                    if let Some(Spoken::Failed { why, .. }) =
                        heard.iter().find(|s| matches!(s, Spoken::Failed { .. }))
                    {
                        break Err(why.clone());
                    }
                    if heard.contains(&Spoken::Ended { utterance: 1 }) {
                        break if heard.iter().any(|s| matches!(s, Spoken::Began { .. })) {
                            Ok(())
                        } else {
                            Err("it ended without being heard".to_string())
                        };
                    }
                };
                match outcome {
                    Ok(()) => read += 1,
                    Err(why) => failed.push(format!("{} {}: {why}", pack.id, voice.name())),
                }
            }
            // a voice read is not kept: the next has room
            let _ = std::fs::remove_dir_all(&dir);
        }
        let _ = std::fs::remove_dir_all(&root);
        eprintln!("{read} voices read");
        assert!(failed.is_empty(), "{failed:#?}");
        assert!(read > 0);
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
                "phoneme_id_map":{"_":[0],"a":[14],"\n":[99],"aɪ":[161]}}"#,
        )
        .unwrap();
        std::fs::write(
            downloads.join(ESPEAK_DATA.0),
            tar_bz2(&[Entry::file("espeak-ng-data/phontab", b"p")]),
        )
        .unwrap();
        install_pack_at(pack, &downloads, &root).unwrap();
        let dir = root.join("packs").join(pack.folder);
        // a phoneme of several letters is left out: sherpa-onnx never makes one, and refuses a line of one
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
        assert!(Path::new(&p.files["model"]).ends_with("en_US-amy-low.onnx"));
        assert!(p.files.contains_key("data_dir"));
        let m = describe_folder(&matcha).unwrap();
        assert_eq!(m.kind, ModelKind::Matcha);
        assert!(Path::new(&m.files["vocoder"]).ends_with("vocos-22khz-univ.onnx"));
        assert!(Path::new(&m.files["acoustic_model"]).ends_with("model-steps-3.onnx"));
        assert!(describe_folder(&empty).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
