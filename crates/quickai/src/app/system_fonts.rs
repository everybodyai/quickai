//! Letters of every script. The fonts QuickAI embeds (Plex, and egui's own behind it) have Latin, Greek and Cyrillic,
//! and a reply in Malayalam, Hindi, Arabic or Chinese was a row of boxes. QuickAI ships no font for the other scripts:
//! it finds, among the fonts this computer has, one for each script, by the letters each font has, and puts them
//! behind its own (egui shapes them with HarfRust, so the letters of a word join as that script joins them).
//!
//! Finding them reads the start of every font file, which takes a part of a second on a computer with thousands of
//! fonts, so the window does it on a thread of its own once it is open (`theme::apply`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{MetadataProvider as _, Tag};

/// A script, and letters a font must have to be the one for it.
struct Script {
    name: &'static str,
    letters: &'static [char],
}

/// The scripts looked for.
const SCRIPTS: &[Script] = &[
    Script {
        name: "Arabic",
        letters: &['ب', 'ع', 'ی'],
    },
    Script {
        name: "Hebrew",
        letters: &['א', 'ש'],
    },
    Script {
        name: "Devanagari",
        letters: &['क', 'ि', 'ं'],
    },
    Script {
        name: "Bengali",
        letters: &['ক', 'া'],
    },
    Script {
        name: "Gurmukhi",
        letters: &['ਕ', 'ਾ'],
    },
    Script {
        name: "Gujarati",
        letters: &['ક', 'ા'],
    },
    Script {
        name: "Oriya",
        letters: &['କ', 'ା'],
    },
    Script {
        name: "Tamil",
        letters: &['க', 'ா'],
    },
    Script {
        name: "Telugu",
        letters: &['క', 'ా'],
    },
    Script {
        name: "Kannada",
        letters: &['ಕ', 'ಾ'],
    },
    Script {
        name: "Malayalam",
        letters: &['ക', 'ാ', 'ൽ'],
    },
    Script {
        name: "Sinhala",
        letters: &['ක', 'ා'],
    },
    Script {
        name: "Thai",
        letters: &['ก', 'า'],
    },
    Script {
        name: "Lao",
        letters: &['ກ', 'າ'],
    },
    Script {
        name: "Khmer",
        letters: &['ក', 'ា'],
    },
    Script {
        name: "Myanmar",
        letters: &['က', 'ာ'],
    },
    Script {
        name: "Tibetan",
        letters: &['ཀ', 'ི'],
    },
    Script {
        name: "Georgian",
        letters: &['ა', 'ბ'],
    },
    Script {
        name: "Armenian",
        letters: &['Ա', 'ա'],
    },
    Script {
        name: "Ethiopic",
        letters: &['ሀ', 'ለ'],
    },
    Script {
        name: "Han",
        letters: &['中', '的', '国'],
    },
    Script {
        name: "Kana",
        letters: &['あ', 'ア'],
    },
    Script {
        name: "Hangul",
        letters: &['한', '글'],
    },
];

/// The fonts the systems themselves draw these scripts with (Windows, macOS, and the free ones Linux has),
/// lower-case: one of them is preferred to a font that merely has the letters.
const SYSTEM_FAMILIES: &[&str] = &[
    // Windows
    "nirmala ui",
    "segoe ui",
    "leelawadee ui",
    "ebrima",
    "gadugi",
    "myanmar text",
    "khmer ui",
    "lao ui",
    "microsoft himalaya",
    "microsoft yahei ui",
    "microsoft yahei",
    "microsoft jhenghei ui",
    "microsoft jhenghei",
    "yu gothic ui",
    "yu gothic",
    "meiryo ui",
    "meiryo",
    "malgun gothic",
    // macOS
    "pingfang sc",
    "pingfang tc",
    "pingfang hk",
    "hiragino sans",
    "hiragino sans gb",
    "apple sd gothic neo",
    "kohinoor devanagari",
    "kohinoor bangla",
    "kohinoor telugu",
    "kohinoor gujarati",
    "mukta mahee",
    "malayalam sangam mn",
    "tamil sangam mn",
    "kannada sangam mn",
    "sinhala sangam mn",
    "oriya sangam mn",
    "geeza pro",
    "arial hebrew",
    "thonburi",
    "lao sangam mn",
    "khmer sangam mn",
    "myanmar sangam mn",
    "kailasa",
    "kefa",
    // Linux, besides Noto
    "manjari",
    "lohit devanagari",
    "lohit bengali",
    "lohit gurmukhi",
    "lohit gujarati",
    "lohit odia",
    "lohit tamil",
    "lohit telugu",
    "lohit kannada",
    "lohit malayalam",
    "source han sans sc",
    "source han sans",
    "wenquanyi micro hei",
    "wenquanyi zen hei",
    "droid sans fallback",
];

/// A font of this computer found for a script: the file mapped into memory for as long as QuickAI runs, and which
/// face of the file it is.
pub(super) struct Face {
    pub key: String,
    pub data: &'static [u8],
    pub index: u32,
}

/// A face that has the letters of one script or more, before the best for each is chosen.
#[derive(Clone, Debug)]
struct Candidate {
    path: PathBuf,
    index: u32,
    family: String,
    weight: f32,
    italic: bool,
    /// Which of `SCRIPTS` it has the letters of.
    scripts: Vec<usize>,
}

/// The fonts of this computer for the scripts QuickAI's own do not have.
pub(super) fn find_here() -> Vec<Face> {
    find(&font_dirs(), &language())
}

/// The fonts in `dirs` for the scripts, the ideographs in the style of `language` (a locale such as `ja_JP.UTF-8`).
fn find(dirs: &[PathBuf], language: &str) -> Vec<Face> {
    let candidates: Vec<Candidate> = font_files(dirs).iter().flat_map(|p| read(p)).collect();
    choose(&candidates, language)
        .into_iter()
        .filter_map(|c| {
            let data = map(&c.path)?;
            Some(Face {
                key: format!("system:{}:{}", c.family, c.index),
                data,
                index: c.index,
            })
        })
        .collect()
}

/// The faces to use: for each script, the candidate of the best score, each once, the best first. A face is tried
/// for a letter only when the faces before it lack it (egui walks them in order), and the best face for a script has
/// the best score of all that have its letters, so it comes before every other chosen face that has them.
fn choose(candidates: &[Candidate], language: &str) -> Vec<Candidate> {
    let better = |a: &&Candidate, b: &&Candidate| {
        score(a, language)
            .cmp(&score(b, language))
            .then_with(|| b.family.len().cmp(&a.family.len()))
            .then_with(|| b.path.cmp(&a.path))
            .then_with(|| b.index.cmp(&a.index))
    };
    let mut chosen: Vec<&Candidate> = Vec::new();
    for s in 0..SCRIPTS.len() {
        let best = candidates
            .iter()
            .filter(|c| c.scripts.contains(&s))
            .max_by(better);
        if let Some(best) = best {
            if !chosen.iter().any(|c| std::ptr::eq(*c, best)) {
                chosen.push(best);
            }
        }
    }
    chosen.sort_by(|a, b| better(b, a));
    chosen.into_iter().cloned().collect()
}

/// How fit a face is to draw its scripts in a window: a font the system draws them with, or a Noto Sans, before one
/// that merely has the letters; a sans before a serif, the regular weight and upright; never a font of pixels if there
/// is another; and for the ideographs, the style of the person's language.
fn score(c: &Candidate, language: &str) -> i32 {
    let family = c.family.to_lowercase();
    let mut score = 0;
    if family.starts_with("noto sans") {
        score += 40;
        if family.ends_with(" ui") {
            // the UI faces keep to the height of Latin text
            score += 5;
        }
    } else if SYSTEM_FAMILIES.contains(&family.as_str()) {
        score += 40;
    }
    if ["sans", "gothic", "hei", " ui"]
        .iter()
        .any(|w| family.contains(w))
    {
        score += 10;
    }
    if ["serif", "mincho", "song", "ming"]
        .iter()
        .any(|w| family.contains(w))
    {
        score -= 10;
    }
    if family.contains("mono") {
        score -= 15;
    }
    if family.contains("unifont") {
        score -= 60;
    }
    if c.italic {
        score -= 30;
    }
    score -= ((c.weight - 400.0).abs() / 25.0) as i32;
    let ideographs = c
        .scripts
        .iter()
        .any(|&s| matches!(SCRIPTS[s].name, "Han" | "Kana" | "Hangul"));
    if ideographs && in_style_of(&family, language) {
        score += 15;
    }
    score
}

/// Whether a CJK family draws the ideographs as the language's readers write them: Japanese, Korean, traditional
/// Chinese, or else simplified Chinese.
fn in_style_of(family: &str, language: &str) -> bool {
    let language = language.to_lowercase().replace('-', "_");
    let has = |marks: &[&str]| marks.iter().any(|m| family.contains(m));
    if language.starts_with("ja") {
        has(&[" jp", "japanese", "yu gothic", "meiryo"])
            || (family.starts_with("hiragino sans") && !family.ends_with(" gb"))
    } else if language.starts_with("ko") {
        has(&[" kr", "korean", "malgun", "apple sd gothic", "nanum"])
    } else if language.starts_with("zh_tw") || language.starts_with("zh_hk") {
        has(&[" tc", " hk", "jhenghei", "pingfang tc", "pingfang hk"])
    } else {
        has(&[
            " sc",
            "yahei",
            "pingfang sc",
            "hiragino sans gb",
            "wenquanyi",
            "heiti sc",
        ])
    }
}

/// The person's language as the system says it, for the style of the ideographs.
fn language() -> String {
    ["LC_ALL", "LC_CTYPE", "LANG", "LANGUAGE"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .unwrap_or_default()
}

/// Where this system keeps fonts: its own folders and the person's.
fn font_dirs() -> Vec<PathBuf> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(windows) {
        let windows =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        dirs.push(windows.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push("/System/Library/Fonts".into());
        dirs.push("/Library/Fonts".into());
        if let Some(home) = &home {
            dirs.push(home.join("Library/Fonts"));
        }
    } else {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        if let Some(data_home) = data_home {
            dirs.push(data_home.join("fonts"));
        }
        if let Some(home) = &home {
            dirs.push(home.join(".fonts"));
        }
        let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
            dirs.push(Path::new(dir).join("fonts"));
        }
        dirs.push("/usr/local/share/fonts".into());
        dirs.push("/usr/share/fonts".into());
    }
    dirs
}

/// The font files in `dirs` and the folders in them, each once.
fn font_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    const DEEPEST: usize = 8;
    const MOST: usize = 20_000;
    let mut seen_dirs = HashSet::new();
    let mut seen_files = HashSet::new();
    let mut files = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    stack.reverse();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(real) = dir.canonicalize() else {
            continue;
        };
        if !seen_dirs.insert(real) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                if depth < DEEPEST {
                    stack.push((path, depth + 1));
                }
            } else if is_font(&path) && files.len() < MOST {
                if let Ok(real) = path.canonicalize() {
                    if seen_files.insert(real.clone()) {
                        files.push(real);
                    }
                }
            }
        }
    }
    files
}

fn is_font(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["ttf", "otf", "ttc", "otc"].contains(&e.to_ascii_lowercase().as_str()))
}

/// The faces of a font file that have the letters of a script, with their family, weight and slant. A face with no
/// outlines (a font of coloured pictures) and the Last Resort font (a sign for every letter) are not candidates.
fn read(path: &Path) -> Vec<Candidate> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    // Safety: a font file is read while it is mapped; a font replaced by an update is a new file, and the map keeps
    // the old one
    let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else {
        return Vec::new();
    };
    let Ok(file_ref) = FileRef::new(&map) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (index, font) in file_ref.fonts().enumerate() {
        let Ok(font) = font else { continue };
        let outlined = [b"glyf", b"CFF ", b"CFF2"]
            .iter()
            .any(|t| font.table_data(Tag::new(t)).is_some());
        if !outlined {
            continue;
        }
        let charmap = font.charmap();
        let scripts: Vec<usize> = SCRIPTS
            .iter()
            .enumerate()
            .filter(|(_, s)| s.letters.iter().all(|&l| charmap.map(l).is_some()))
            .map(|(i, _)| i)
            .collect();
        if scripts.is_empty() {
            continue;
        }
        let name = |id| {
            font.localized_strings(id)
                .english_or_first()
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
        };
        let Some(family) =
            name(StringId::TYPOGRAPHIC_FAMILY_NAME).or_else(|| name(StringId::FAMILY_NAME))
        else {
            continue;
        };
        let lower = family.to_lowercase();
        if lower.contains("last resort") || lower.contains("lastresort") {
            continue;
        }
        let attributes = font.attributes();
        found.push(Candidate {
            path: path.to_path_buf(),
            index: index as u32,
            family,
            weight: attributes.weight.value(),
            italic: attributes.style != skrifa::attribute::Style::Normal,
            scripts,
        });
    }
    found
}

/// A chosen font file, mapped for as long as QuickAI runs (the window's text is drawn from it until it closes).
fn map(path: &Path) -> Option<&'static [u8]> {
    let file = std::fs::File::open(path).ok()?;
    // Safety: as in `read`
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    let map: &'static memmap2::Mmap = Box::leak(Box::new(map));
    Some(&map[..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(family: &str, weight: f32, scripts: &[&str]) -> Candidate {
        Candidate {
            path: PathBuf::from(format!("/fonts/{family}.ttf")),
            index: 0,
            family: family.into(),
            weight,
            italic: false,
            scripts: scripts
                .iter()
                .map(|n| SCRIPTS.iter().position(|s| s.name == *n).unwrap())
                .collect(),
        }
    }

    fn families(chosen: &[Candidate]) -> Vec<&str> {
        chosen.iter().map(|c| c.family.as_str()).collect()
    }

    #[test]
    fn the_systems_own_font_for_a_script_beats_one_that_merely_has_its_letters() {
        let candidates = [
            candidate("Unifont", 400.0, &["Malayalam", "Devanagari", "Han"]),
            candidate("FreeSerif", 400.0, &["Malayalam", "Devanagari"]),
            candidate("Noto Sans Malayalam", 700.0, &["Malayalam"]),
            candidate("Noto Sans Malayalam UI", 400.0, &["Malayalam"]),
            candidate("Noto Serif Malayalam", 400.0, &["Malayalam"]),
        ];
        let chosen = choose(&candidates, "en_IN.UTF-8");
        // the regular UI face of Noto draws Malayalam, FreeSerif Devanagari, and Unifont only the ideographs, last
        assert_eq!(
            families(&chosen),
            ["Noto Sans Malayalam UI", "FreeSerif", "Unifont"]
        );
    }

    #[test]
    fn one_font_that_has_many_scripts_serves_them_all() {
        let candidates = [
            candidate(
                "Nirmala UI",
                400.0,
                &["Devanagari", "Bengali", "Tamil", "Malayalam"],
            ),
            candidate("Lohit Tamil", 400.0, &["Tamil"]),
        ];
        assert_eq!(families(&choose(&candidates, "")), ["Nirmala UI"]);
    }

    #[test]
    fn the_ideographs_are_drawn_as_the_persons_language_writes_them() {
        let cjk = |style: &str| {
            candidate(
                &format!("Noto Sans CJK {style}"),
                400.0,
                &["Han", "Kana", "Hangul"],
            )
        };
        let candidates = [
            cjk("JP"),
            cjk("KR"),
            cjk("SC"),
            cjk("TC"),
            candidate("Noto Sans Mono CJK SC", 400.0, &["Han", "Kana", "Hangul"]),
        ];
        assert_eq!(
            families(&choose(&candidates, "ja_JP.UTF-8")),
            ["Noto Sans CJK JP"]
        );
        assert_eq!(
            families(&choose(&candidates, "ko_KR.UTF-8")),
            ["Noto Sans CJK KR"]
        );
        assert_eq!(
            families(&choose(&candidates, "zh_TW.UTF-8")),
            ["Noto Sans CJK TC"]
        );
        assert_eq!(
            families(&choose(&candidates, "en_US.UTF-8")),
            ["Noto Sans CJK SC"]
        );
    }

    #[test]
    fn italic_and_bold_faces_give_way_to_the_regular_one() {
        let mut italic = candidate("Noto Sans Arabic", 400.0, &["Arabic"]);
        italic.italic = true;
        italic.path = PathBuf::from("/fonts/a-italic.ttf");
        let candidates = [
            italic,
            candidate("Noto Sans Arabic", 700.0, &["Arabic"]),
            candidate("Noto Sans Arabic", 400.0, &["Arabic"]),
        ];
        let chosen = choose(&candidates, "");
        assert_eq!(
            (chosen.len(), chosen[0].weight, chosen[0].italic),
            (1, 400.0, false)
        );
    }

    #[test]
    fn the_fonts_quickai_embeds_are_no_candidates_and_a_broken_file_is_passed_over() {
        let dir = crate::testkit::scratch("system-fonts");
        std::fs::write(dir.join("broken.ttf"), b"not a font at all").unwrap();
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        assert!(find(&[dir.clone(), assets, dir.join("missing")], "").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
