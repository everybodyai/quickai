//! The languages QuickAI hears and reads, each with its own name and the parts of the world it is spoken in: Settings ›
//! Voice lists them by region, with the most widely spoken first, so that a language is found in two clicks or by
//! typing a few letters of its name in English or in itself.
//!
//! Whisper hears 100 of them (`Language::whisper`); Omnilingual hears them all.

/// A part of the world: a group of the lists of languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Region {
    Africa,
    Americas,
    CentralAsia,
    EastAsia,
    Europe,
    MiddleEast,
    Pacific,
    SouthAsia,
    SoutheastAsia,
}

impl Region {
    /// In the order the lists show them.
    pub const ALL: [Region; 9] = [
        Region::Africa,
        Region::Americas,
        Region::CentralAsia,
        Region::EastAsia,
        Region::Europe,
        Region::MiddleEast,
        Region::Pacific,
        Region::SouthAsia,
        Region::SoutheastAsia,
    ];

    /// Its name in a list (short: the lists are narrow).
    pub fn name(self) -> &'static str {
        match self {
            Region::Africa => "Africa",
            Region::Americas => "Americas",
            Region::CentralAsia => "Central Asia",
            Region::EastAsia => "East Asia",
            Region::Europe => "Europe",
            Region::MiddleEast => "Middle East",
            Region::Pacific => "Pacific",
            Region::SouthAsia => "South Asia",
            Region::SoutheastAsia => "Southeast Asia",
        }
    }

    /// What it covers, in full.
    pub fn long_name(self) -> &'static str {
        match self {
            Region::CentralAsia => "Central Asia and the Caucasus",
            Region::MiddleEast => "The Middle East and North Africa",
            Region::Americas => "North, Central and South America, and the Caribbean",
            Region::Pacific => "Australia, New Zealand and the Pacific islands",
            other => other.name(),
        }
    }
}

/// A language: its code (whisper's, where whisper hears it), its name in English and in itself, the parts of the
/// world it is spoken in, and whether whisper hears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub native: &'static str,
    pub regions: &'static [Region],
    pub whisper: bool,
}

const fn lang(
    code: &'static str,
    name: &'static str,
    native: &'static str,
    regions: &'static [Region],
    whisper: bool,
) -> Language {
    Language {
        code,
        name,
        native,
        regions,
        whisper,
    }
}

use Region::*;

const AF: &[Region] = &[Africa];
const AM: &[Region] = &[Americas];
const CA: &[Region] = &[CentralAsia];
const EA: &[Region] = &[EastAsia];
const EU: &[Region] = &[Europe];
const ME: &[Region] = &[MiddleEast];
const PA: &[Region] = &[Pacific];
const SA: &[Region] = &[SouthAsia];
const SE: &[Region] = &[SoutheastAsia];

/// Every language, in the order of their names in English. A language spoken widely in several parts of the world is
/// in each of their groups.
pub const LANGUAGES: [Language; 135] = [
    lang("af", "Afrikaans", "Afrikaans", AF, true),
    lang("ak", "Akan (Twi)", "Akan", AF, false),
    lang("sq", "Albanian", "Shqip", EU, true),
    lang("am", "Amharic", "አማርኛ", AF, true),
    lang("ar", "Arabic", "العربية", ME, true),
    lang("hy", "Armenian", "Հայերեն", CA, true),
    lang("as", "Assamese", "অসমীয়া", SA, true),
    lang("ay", "Aymara", "Aymar aru", AM, false),
    lang("az", "Azerbaijani", "Azərbaycanca", CA, true),
    lang("ba", "Bashkir", "Башҡортса", EU, true),
    lang("eu", "Basque", "Euskara", EU, true),
    lang("be", "Belarusian", "Беларуская", EU, true),
    lang("bn", "Bengali", "বাংলা", SA, true),
    lang("bho", "Bhojpuri", "भोजपुरी", SA, false),
    lang("bs", "Bosnian", "Bosanski", EU, true),
    lang("br", "Breton", "Brezhoneg", EU, true),
    lang("bg", "Bulgarian", "Български", EU, true),
    lang("my", "Burmese", "မြန်မာ", SE, true),
    lang("yue", "Cantonese", "粵語", EA, true),
    lang("ca", "Catalan", "Català", EU, true),
    lang("ceb", "Cebuano", "Binisaya", SE, false),
    lang("ny", "Chichewa", "Chichewa", AF, false),
    lang("zh", "Chinese", "中文", EA, true),
    lang("hr", "Croatian", "Hrvatski", EU, true),
    lang("cs", "Czech", "Čeština", EU, true),
    lang("da", "Danish", "Dansk", EU, true),
    lang("dv", "Dhivehi", "ދިވެހި", SA, false),
    lang("nl", "Dutch", "Nederlands", EU, true),
    lang("dz", "Dzongkha", "རྫོང་ཁ", SA, false),
    lang(
        "en",
        "English",
        "English",
        &[Europe, Americas, Pacific],
        true,
    ),
    lang("et", "Estonian", "Eesti", EU, true),
    lang("ee", "Ewe", "Eʋegbe", AF, false),
    lang("fo", "Faroese", "Føroyskt", EU, true),
    lang("fj", "Fijian", "Vosa Vakaviti", PA, false),
    lang("tl", "Filipino (Tagalog)", "Filipino", SE, true),
    lang("fi", "Finnish", "Suomi", EU, true),
    lang(
        "fr",
        "French",
        "Français",
        &[Europe, Africa, Americas],
        true,
    ),
    lang("ff", "Fula", "Fulfulde", AF, false),
    lang("gl", "Galician", "Galego", EU, true),
    lang("ka", "Georgian", "ქართული", CA, true),
    lang("de", "German", "Deutsch", EU, true),
    lang("el", "Greek", "Ελληνικά", EU, true),
    lang("gn", "Guarani", "Avañeʼẽ", AM, false),
    lang("gu", "Gujarati", "ગુજરાતી", SA, true),
    lang("ht", "Haitian Creole", "Kreyòl ayisyen", AM, true),
    lang("ha", "Hausa", "Hausa", AF, true),
    lang("haw", "Hawaiian", "ʻŌlelo Hawaiʻi", PA, true),
    lang("he", "Hebrew", "עברית", ME, true),
    lang("hi", "Hindi", "हिन्दी", SA, true),
    lang("hu", "Hungarian", "Magyar", EU, true),
    lang("is", "Icelandic", "Íslenska", EU, true),
    lang("ig", "Igbo", "Asụsụ Igbo", AF, false),
    lang("ilo", "Ilocano", "Ilokano", SE, false),
    lang("id", "Indonesian", "Bahasa Indonesia", SE, true),
    lang("ga", "Irish", "Gaeilge", EU, false),
    lang("it", "Italian", "Italiano", EU, true),
    lang("ja", "Japanese", "日本語", EA, true),
    lang("jw", "Javanese", "Basa Jawa", SE, true),
    lang("kn", "Kannada", "ಕನ್ನಡ", SA, true),
    lang("kk", "Kazakh", "Қазақша", CA, true),
    lang("km", "Khmer", "ខ្មែរ", SE, true),
    lang("rw", "Kinyarwanda", "Ikinyarwanda", AF, false),
    lang("kok", "Konkani", "कोंकणी", SA, false),
    lang("ko", "Korean", "한국어", EA, true),
    lang("ku", "Kurdish (Kurmanji)", "Kurmancî", ME, false),
    lang("ckb", "Kurdish (Sorani)", "سۆرانی", ME, false),
    lang("ky", "Kyrgyz", "Кыргызча", CA, false),
    lang("lo", "Lao", "ລາວ", SE, true),
    lang("la", "Latin", "Latina", EU, true),
    lang("lv", "Latvian", "Latviešu", EU, true),
    lang("ln", "Lingala", "Lingála", AF, true),
    lang("lt", "Lithuanian", "Lietuvių", EU, true),
    lang("lg", "Luganda", "Luganda", AF, false),
    lang("lb", "Luxembourgish", "Lëtzebuergesch", EU, true),
    lang("mk", "Macedonian", "Македонски", EU, true),
    lang("mai", "Maithili", "मैथिली", SA, false),
    lang("mg", "Malagasy", "Malagasy", AF, true),
    lang("ms", "Malay", "Bahasa Melayu", SE, true),
    lang("ml", "Malayalam", "മലയാളം", SA, true),
    lang("mt", "Maltese", "Malti", EU, true),
    lang("mi", "Maori", "Te reo Māori", PA, true),
    lang("mr", "Marathi", "मराठी", SA, true),
    lang("mn", "Mongolian", "Монгол", EA, true),
    lang("ne", "Nepali", "नेपाली", SA, true),
    lang("no", "Norwegian", "Norsk bokmål", EU, true),
    lang("nn", "Norwegian Nynorsk", "Norsk nynorsk", EU, true),
    lang("oc", "Occitan", "Occitan", EU, true),
    lang("or", "Odia", "ଓଡ଼ିଆ", SA, false),
    lang("om", "Oromo", "Afaan Oromoo", AF, false),
    lang("ps", "Pashto", "پښتو", SA, true),
    lang("fa", "Persian", "فارسی", ME, true),
    lang("pl", "Polish", "Polski", EU, true),
    lang(
        "pt",
        "Portuguese",
        "Português",
        &[Europe, Americas, Africa],
        true,
    ),
    lang("pa", "Punjabi", "ਪੰਜਾਬੀ", SA, true),
    lang("qu", "Quechua", "Runa simi", AM, false),
    lang("ro", "Romanian", "Română", EU, true),
    lang("ru", "Russian", "Русский", &[Europe, CentralAsia], true),
    lang("sm", "Samoan", "Gagana Sāmoa", PA, false),
    lang("sa", "Sanskrit", "संस्कृतम्", SA, true),
    lang("sat", "Santali", "ᱥᱟᱱᱛᱟᱲᱤ", SA, false),
    lang("gd", "Scottish Gaelic", "Gàidhlig", EU, false),
    lang("sr", "Serbian", "Српски", EU, true),
    lang("st", "Sesotho", "Sesotho", AF, false),
    lang("tn", "Setswana", "Setswana", AF, false),
    lang("sn", "Shona", "chiShona", AF, true),
    lang("sd", "Sindhi", "سنڌي", SA, true),
    lang("si", "Sinhala", "සිංහල", SA, true),
    lang("sk", "Slovak", "Slovenčina", EU, true),
    lang("sl", "Slovenian", "Slovenščina", EU, true),
    lang("so", "Somali", "Soomaali", AF, true),
    lang("es", "Spanish", "Español", &[Europe, Americas], true),
    lang("su", "Sundanese", "Basa Sunda", SE, true),
    lang("sw", "Swahili", "Kiswahili", AF, true),
    lang("sv", "Swedish", "Svenska", EU, true),
    lang("tg", "Tajik", "Тоҷикӣ", CA, true),
    lang("ta", "Tamil", "தமிழ்", SA, true),
    lang("tt", "Tatar", "Татарча", EU, true),
    lang("te", "Telugu", "తెలుగు", SA, true),
    lang("th", "Thai", "ไทย", SE, true),
    lang("bo", "Tibetan", "བོད་སྐད", &[EastAsia, SouthAsia], true),
    lang("ti", "Tigrinya", "ትግርኛ", AF, false),
    lang("to", "Tongan", "Lea faka-Tonga", PA, false),
    lang("tr", "Turkish", "Türkçe", &[MiddleEast, Europe], true),
    lang("tk", "Turkmen", "Türkmençe", CA, true),
    lang("uk", "Ukrainian", "Українська", EU, true),
    lang("ur", "Urdu", "اردو", SA, true),
    lang("ug", "Uyghur", "ئۇيغۇرچە", &[CentralAsia, EastAsia], false),
    lang("uz", "Uzbek", "Oʻzbekcha", CA, true),
    lang("vi", "Vietnamese", "Tiếng Việt", SE, true),
    lang("cy", "Welsh", "Cymraeg", EU, true),
    lang("wo", "Wolof", "Wolof", AF, false),
    lang("xh", "Xhosa", "isiXhosa", AF, false),
    lang("yi", "Yiddish", "ייִדיש", EU, true),
    lang("yo", "Yoruba", "Yorùbá", AF, true),
    lang("zu", "Zulu", "isiZulu", AF, false),
];

/// The most widely spoken languages (counting all who speak them, as a first language or not), the first group of the
/// lists: from English (1.5 billion) to Japanese (126 million).
pub const WIDELY_SPOKEN: [&str; 13] = [
    "en", "zh", "hi", "es", "ar", "fr", "bn", "pt", "ru", "id", "ur", "de", "ja",
];

/// The language of `code`.
pub fn language(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.code == code)
}

/// The name of the language of `code`, or the code.
pub fn name(code: &str) -> &str {
    language(code).map_or(code, |l| l.name)
}

/// whisper hears the language of `code` (told it, it writes that language; Omnilingual hears every language here).
pub fn whisper_hears(code: &str) -> bool {
    language(code).is_some_and(|l| l.whisper)
}

/// A group of a list of languages: the most widely spoken, or a part of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    WidelySpoken,
    Region(Region),
}

impl Group {
    /// In the order the lists show them.
    pub fn all() -> impl Iterator<Item = Group> {
        std::iter::once(Group::WidelySpoken).chain(Region::ALL.into_iter().map(Group::Region))
    }

    pub fn name(self) -> &'static str {
        match self {
            Group::WidelySpoken => "Widely spoken",
            Group::Region(r) => r.name(),
        }
    }

    /// Whether `language` is in it.
    pub fn has(self, language: &Language) -> bool {
        match self {
            Group::WidelySpoken => WIDELY_SPOKEN.contains(&language.code),
            Group::Region(r) => language.regions.contains(&r),
        }
    }

    /// The group a list opens on for `code`: the most widely spoken, or the first region it is spoken in.
    pub fn of(code: Option<&str>) -> Group {
        match code.and_then(language) {
            Some(l) if !WIDELY_SPOKEN.contains(&l.code) => l
                .regions
                .first()
                .map_or(Group::WidelySpoken, |r| Group::Region(*r)),
            _ => Group::WidelySpoken,
        }
    }
}

/// `text` made small and without the marks over its Latin letters, to match what is typed with: "espanol" finds
/// "Español", "francais" "Français".
pub fn folded(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'č' | 'ć' => 'c',
            'ď' | 'đ' => 'd',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ě' | 'ę' | 'ə' => 'e',
            'ğ' => 'g',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ı' => 'i',
            'ł' | 'ľ' => 'l',
            'ñ' | 'ň' | 'ń' => 'n',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => 'o',
            'ř' => 'r',
            'š' | 'ś' | 'ș' | 'ş' => 's',
            'ț' | 'ţ' | 'ť' => 't',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ž' | 'ź' | 'ż' => 'z',
            other => other,
        })
        .collect()
}

impl Language {
    /// Whether what was typed (`query`, folded) is in its name, its own name or its code.
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || folded(self.name).contains(query)
            || folded(self.native).contains(query)
            || self.code == query
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// whisper.cpp's languages (its `g_lang` table, 1.8.3): each is here, and marked as whisper's.
    const WHISPER_CODES: [&str; 100] = [
        "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv",
        "it", "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no",
        "th", "ur", "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr",
        "az", "sl", "kn", "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw",
        "gl", "mr", "pa", "si", "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu",
        "am", "yi", "lo", "uz", "fo", "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl",
        "mg", "as", "tt", "haw", "ln", "ha", "ba", "jw", "su", "yue",
    ];

    #[test]
    fn every_language_whisper_hears_is_here_and_marked() {
        for code in WHISPER_CODES {
            assert!(whisper_hears(code), "{code}");
        }
        for l in &LANGUAGES {
            assert_eq!(l.whisper, WHISPER_CODES.contains(&l.code), "{}", l.code);
        }
    }

    #[test]
    fn the_table_is_in_the_order_of_the_names_with_each_code_once_and_a_region_each() {
        for pair in LANGUAGES.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "{} before {}",
                pair[0].name,
                pair[1].name
            );
        }
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len(), "a code is twice");
        for l in &LANGUAGES {
            assert!(!l.regions.is_empty() && !l.native.is_empty(), "{}", l.code);
        }
        for code in WIDELY_SPOKEN {
            assert!(language(code).is_some(), "{code}");
        }
        // every region has languages
        for r in Region::ALL {
            assert!(LANGUAGES.iter().any(|l| l.regions.contains(&r)), "{r:?}");
        }
    }

    #[test]
    fn a_language_is_found_by_its_name_its_own_name_or_its_code() {
        let spanish = language("es").unwrap();
        assert!(spanish.matches(&folded("Espanol")));
        assert!(spanish.matches("spa"));
        assert!(spanish.matches("es"));
        assert!(!spanish.matches("ger"));
        assert!(language("hi").unwrap().matches("हिन्दी"));
        assert!(language("de").unwrap().matches(&folded("Deutsch")));
        assert!(language("fr").unwrap().matches(&folded("francais")));
        assert_eq!(name("ml"), "Malayalam");
        assert_eq!(name("xx"), "xx");
    }

    #[test]
    fn a_list_opens_on_the_group_of_what_was_chosen() {
        assert_eq!(Group::of(None), Group::WidelySpoken);
        assert_eq!(Group::of(Some("en")), Group::WidelySpoken);
        assert_eq!(Group::of(Some("ml")), Group::Region(Region::SouthAsia));
        assert_eq!(Group::of(Some("sw")), Group::Region(Region::Africa));
        assert_eq!(Group::of(Some("xx")), Group::WidelySpoken);
        assert!(Group::Region(Region::Americas).has(language("es").unwrap()));
        assert!(Group::WidelySpoken.has(language("ja").unwrap()));
        assert_eq!(Group::all().count(), 10);
    }
}
