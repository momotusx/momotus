//! Subtitle languages as people read them (plans/m13-subtitle-languages.md):
//! one tag, one English name and the language's own name for whatever code
//! an addon sends.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// OpenSubtitles' upload languages (opensubtitles.org/addons/export_languages.php,
/// read 2026-10-19): its code, the tag the core uses, the English name, and
/// its own name (plans/player-polish.md 2.4: "Turkish", "Türkçe").
const OPENSUBTITLES: &[(&str, &str, &str, &str)] = &[
    ("abk", "ab", "Abkhazian", "Аԥсшәа"),
    ("afr", "af", "Afrikaans", "Afrikaans"),
    ("alb", "sq", "Albanian", "Shqip"),
    ("amh", "am", "Amharic", "አማርኛ"),
    ("ara", "ar", "Arabic", "العربية"),
    ("arg", "an", "Aragonese", "Aragonés"),
    ("arm", "hy", "Armenian", "Հայերեն"),
    ("asm", "as", "Assamese", "অসমীয়া"),
    ("ast", "ast", "Asturian", "Asturianu"),
    ("aze", "az", "Azerbaijani", "Azərbaycan"),
    ("baq", "eu", "Basque", "Euskara"),
    ("bel", "be", "Belarusian", "Беларуская"),
    ("ben", "bn", "Bengali", "বাংলা"),
    ("bos", "bs", "Bosnian", "Bosanski"),
    ("bre", "br", "Breton", "Brezhoneg"),
    ("bul", "bg", "Bulgarian", "Български"),
    ("bur", "my", "Burmese", "မြန်မာ"),
    ("cat", "ca", "Catalan", "Català"),
    ("chi", "zh-Hans", "Chinese (Simplified)", "简体中文"),
    ("chv", "cv", "Chuvash", "Чӑвашла"),
    ("cze", "cs", "Czech", "Čeština"),
    ("dan", "da", "Danish", "Dansk"),
    ("dut", "nl", "Dutch", "Nederlands"),
    ("eng", "en", "English", "English"),
    ("epo", "eo", "Esperanto", "Esperanto"),
    ("est", "et", "Estonian", "Eesti"),
    ("fin", "fi", "Finnish", "Suomi"),
    ("fre", "fr", "French", "Français"),
    ("geo", "ka", "Georgian", "ქართული"),
    ("ger", "de", "German", "Deutsch"),
    ("gla", "gd", "Scottish Gaelic", "Gàidhlig"),
    ("gle", "ga", "Irish", "Gaeilge"),
    ("glg", "gl", "Galician", "Galego"),
    ("ell", "el", "Greek", "Ελληνικά"),
    ("heb", "he", "Hebrew", "עברית"),
    ("hin", "hi", "Hindi", "हिन्दी"),
    ("hrv", "hr", "Croatian", "Hrvatski"),
    ("hun", "hu", "Hungarian", "Magyar"),
    ("ibo", "ig", "Igbo", "Igbo"),
    ("ice", "is", "Icelandic", "Íslenska"),
    ("ina", "ia", "Interlingua", "Interlingua"),
    ("ind", "id", "Indonesian", "Bahasa Indonesia"),
    ("ita", "it", "Italian", "Italiano"),
    ("jpn", "ja", "Japanese", "日本語"),
    ("kan", "kn", "Kannada", "ಕನ್ನಡ"),
    ("kaz", "kk", "Kazakh", "Қазақ тілі"),
    ("khm", "km", "Khmer", "ខ្មែរ"),
    ("kir", "ky", "Kyrgyz", "Кыргызча"),
    ("kor", "ko", "Korean", "한국어"),
    ("kur", "ku", "Kurdish", "Kurdî"),
    ("lao", "lo", "Lao", "ລາວ"),
    ("lav", "lv", "Latvian", "Latviešu"),
    ("lit", "lt", "Lithuanian", "Lietuvių"),
    ("ltz", "lb", "Luxembourgish", "Lëtzebuergesch"),
    ("mac", "mk", "Macedonian", "Македонски"),
    ("mal", "ml", "Malayalam", "മലയാളം"),
    ("mar", "mr", "Marathi", "मराठी"),
    ("may", "ms", "Malay", "Bahasa Melayu"),
    ("mni", "mni", "Manipuri", "মৈতৈলোন্"),
    ("mon", "mn", "Mongolian", "Монгол"),
    ("nav", "nv", "Navajo", "Diné bizaad"),
    ("nep", "ne", "Nepali", "नेपाली"),
    ("nor", "no", "Norwegian", "Norsk"),
    ("oci", "oc", "Occitan", "Occitan"),
    ("ori", "or", "Odia", "ଓଡ଼ିଆ"),
    ("per", "fa", "Persian", "فارسی"),
    ("pol", "pl", "Polish", "Polski"),
    ("por", "pt", "Portuguese", "Português"),
    ("pus", "ps", "Pashto", "پښتو"),
    ("rus", "ru", "Russian", "Русский"),
    ("sat", "sat", "Santali", "ᱥᱟᱱᱛᱟᱲᱤ"),
    ("scc", "sr", "Serbian", "Српски"),
    ("sin", "si", "Sinhala", "සිංහල"),
    ("slo", "sk", "Slovak", "Slovenčina"),
    ("slv", "sl", "Slovenian", "Slovenščina"),
    ("sme", "se", "Northern Sami", "Davvisámegiella"),
    ("snd", "sd", "Sindhi", "سنڌي"),
    ("som", "so", "Somali", "Soomaali"),
    ("spa", "es", "Spanish", "Español"),
    ("swa", "sw", "Swahili", "Kiswahili"),
    ("swe", "sv", "Swedish", "Svenska"),
    ("syr", "syr", "Syriac", "ܣܘܪܝܝܐ"),
    ("tam", "ta", "Tamil", "தமிழ்"),
    ("tat", "tt", "Tatar", "Татар"),
    ("tel", "te", "Telugu", "తెలుగు"),
    ("tet", "tet", "Tetum", "Tetun"),
    ("tgl", "tl", "Tagalog", "Tagalog"),
    ("tha", "th", "Thai", "ไทย"),
    ("tuk", "tk", "Turkmen", "Türkmen dili"),
    ("tur", "tr", "Turkish", "Türkçe"),
    ("ukr", "uk", "Ukrainian", "Українська"),
    ("urd", "ur", "Urdu", "اردو"),
    ("uzb", "uz", "Uzbek", "Oʻzbek"),
    ("vie", "vi", "Vietnamese", "Tiếng Việt"),
    ("wel", "cy", "Welsh", "Cymraeg"),
    ("wen", "wen", "Sorbian", "Serbšćina"),
    ("zul", "zu", "Zulu", "isiZulu"),
    ("rum", "ro", "Romanian", "Română"),
    ("pob", "pt-BR", "Portuguese (Brazil)", "Português (Brasil)"),
    ("mne", "cnr", "Montenegrin", "Crnogorski"),
    ("zht", "zh-Hant", "Chinese (Traditional)", "繁體中文"),
    (
        "zhe",
        "zh-x-bilingual",
        "Chinese (Bilingual)",
        "中文（双语）",
    ),
    (
        "pom",
        "pt-MZ",
        "Portuguese (Mozambique)",
        "Português (Moçambique)",
    ),
    ("ext", "ext", "Extremaduran", "Estremeñu"),
    (
        "spl",
        "es-419",
        "Spanish (Latin America)",
        "Español (Latinoamérica)",
    ),
    ("spn", "es-ES", "Spanish (Spain)", "Español (España)"),
    ("prs", "prs", "Dari", "دری"),
    ("tok", "tok", "Toki Pona", "Toki pona"),
    ("zhc", "yue", "Chinese (Cantonese)", "粵語"),
    ("azb", "azb", "South Azerbaijani", "تۆرکجه"),
    ("zgh", "zgh", "Standard Moroccan Amazigh", "ⵜⴰⵎⴰⵣⵉⵖⵜ"),
    ("rki", "rki", "Rakhine", "ရခိုင်"),
    (
        "jpe",
        "ja-x-bilingual",
        "Japanese (Bilingual)",
        "日本語（バイリンガル）",
    ),
];

/// Codes addons send for the same languages: ISO 639-2/T and other
/// spellings of OpenSubtitles' (ISO 639-2/B) codes, and common region tags.
const ALIASES: &[(&str, &str)] = &[
    ("deu", "ger"),
    ("fra", "fre"),
    ("nld", "dut"),
    ("ces", "cze"),
    ("ron", "rum"),
    ("fas", "per"),
    ("slk", "slo"),
    ("sqi", "alb"),
    ("hye", "arm"),
    ("eus", "baq"),
    ("kat", "geo"),
    ("isl", "ice"),
    ("mkd", "mac"),
    ("msa", "may"),
    ("mya", "bur"),
    ("cym", "wel"),
    ("gre", "ell"),
    ("srp", "scc"),
    // Device locales: Norway says Bokmål or Nynorsk, the Philippines Filipino.
    ("nb", "nor"),
    ("nn", "nor"),
    ("fil", "tgl"),
    ("pt-pt", "por"),
    ("pt-br", "pob"),
    ("zh-cn", "chi"),
    ("zh-sg", "chi"),
    ("zh-tw", "zht"),
    ("zh-hk", "zht"),
    ("es-mx", "spl"),
    ("es-la", "spl"),
];

/// Generic Chinese: neither script named.
const CHINESE: Names = ("zh", "Chinese", "中文");

/// A tag, its English name and its own name.
type Names = (&'static str, &'static str, &'static str);

/// A subtitle language: a tag (ISO 639-1 where there is one, with its
/// region or script, `pt-BR`, `zh-Hant`; ISO 639-3 or the given code
/// otherwise), its English name and its own name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Language {
    pub tag: String,
    pub name: String,
    pub native: String,
}

static INDEX: LazyLock<HashMap<String, Names>> = LazyLock::new(|| {
    let mut index = HashMap::new();
    for &(code, tag, name, native) in OPENSUBTITLES {
        for key in [code, tag, name] {
            index
                .entry(key.to_lowercase())
                .or_insert((tag, name, native));
        }
    }
    for &(alias, code) in ALIASES {
        let found = index[code];
        index.insert(alias.to_owned(), found);
    }
    for key in ["zh", "zho", "chinese"] {
        index.insert(key.to_owned(), CHINESE);
    }
    index
});

/// The language an addon's `code` names: an OpenSubtitles code (`pob`,
/// `spl`), ISO 639-2 in either form (`ger`, `deu`), ISO 639-1 (`de`), a tag
/// with a region or script (`pt-BR`, `zh_TW`, `fr-CA`), or an English name
/// (`German`). Anything else keeps its code as the tag and as the name.
pub(crate) fn language(code: &str) -> Language {
    let key = code.trim().to_lowercase().replace('_', "-");
    let known = |(tag, name, native): Names| Language {
        tag: tag.to_owned(),
        name: name.to_owned(),
        native: native.to_owned(),
    };
    if let Some(&found) = INDEX.get(&key) {
        return known(found);
    }
    // A region the table does not list: its language, the region named.
    if let Some((primary, region)) = key.split_once('-')
        && let Some(&(tag, name, native)) = INDEX.get(primary)
    {
        let region = region.to_uppercase();
        return Language {
            tag: format!("{tag}-{region}"),
            name: format!("{name} ({region})"),
            native: format!("{native} ({region})"),
        };
    }
    let unknown = |tag: String, name: &str| Language {
        tag,
        name: name.to_owned(),
        native: name.to_owned(),
    };
    match code.trim() {
        "" => unknown("und".into(), "Unknown language"),
        other => unknown(key, other),
    }
}

/// A language for the settings pickers (journey reviews 2026-10-20, web
/// P2-11, Android P2-4): `code` is the tag the core reads it by, `name`
/// its English name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct LanguageChoice {
    pub code: String,
    pub name: String,
    /// The language the device's own code reads as ("nb" is Norwegian,
    /// "no"): the one a picker offers and chooses as the device's.
    pub device: bool,
}

/// Every language the core knows (OpenSubtitles' list), by English name;
/// `device` marks the one the device's code reads as, if the list has it.
pub fn all(device: Option<&str>) -> Vec<LanguageChoice> {
    let mine = device.map(|code| language(code).tag);
    let mut all: Vec<LanguageChoice> = OPENSUBTITLES
        .iter()
        .map(|&(_, tag, name, _)| LanguageChoice {
            code: tag.to_owned(),
            name: name.to_owned(),
            device: mine.as_deref() == Some(tag),
        })
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// The tag the core reads `code` by ("no" for a Norwegian device's "nb").
pub fn tag(code: &str) -> String {
    language(code).tag
}

/// Whether `word` names a language the core knows, as a code ("EN", "ger")
/// or an English name ("English").
pub(crate) fn is_known(word: &str) -> bool {
    INDEX.contains_key(&word.trim().to_lowercase().replace('_', "-"))
}

/// The language without its region or script: `pt` for `pt-BR`.
pub(crate) fn primary(tag: &str) -> &str {
    tag.split('-').next().unwrap_or(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(code: &str) -> (String, String) {
        let l = language(code);
        (l.tag, l.name)
    }

    #[test]
    fn opensubtitles_specials_read_as_people_say_them() {
        for (code, tag, name) in [
            ("pob", "pt-BR", "Portuguese (Brazil)"),
            ("spl", "es-419", "Spanish (Latin America)"),
            ("spn", "es-ES", "Spanish (Spain)"),
            ("zht", "zh-Hant", "Chinese (Traditional)"),
            ("chi", "zh-Hans", "Chinese (Simplified)"),
            ("zhc", "yue", "Chinese (Cantonese)"),
            ("zhe", "zh-x-bilingual", "Chinese (Bilingual)"),
            ("mne", "cnr", "Montenegrin"),
            ("scc", "sr", "Serbian"),
            ("ell", "el", "Greek"),
            ("mac", "mk", "Macedonian"),
        ] {
            assert_eq!(read(code), (tag.into(), name.into()), "{code}");
        }
    }

    #[test]
    fn every_spelling_of_one_language_is_one_language() {
        for code in ["ger", "deu", "de", "DE", "German", " ger "] {
            assert_eq!(read(code), ("de".into(), "German".into()), "{code}");
        }
        for code in ["tur", "tr", "Turkish"] {
            assert_eq!(read(code), ("tr".into(), "Turkish".into()), "{code}");
        }
        assert_eq!(read("pt_BR").0, "pt-BR");
        assert_eq!(read("zh-TW").0, "zh-Hant");
        assert_eq!(read("zho"), ("zh".into(), "Chinese".into()));
        assert_eq!(read("gre").0, "el");
    }

    /// A device's own language code reads as the language the core lists,
    /// so a profile set from it gets subtitles: Norway's Bokmål and Nynorsk
    /// are Norwegian, Filipino is Tagalog.
    #[test]
    fn a_device_language_reads_as_a_listed_one() {
        for code in ["nb", "nn", "NB"] {
            assert_eq!(read(code), ("no".into(), "Norwegian".into()), "{code}");
        }
        for code in ["fil", "FIL"] {
            assert_eq!(read(code), ("tl".into(), "Tagalog".into()), "{code}");
        }
        assert!(is_known("nb") && is_known("fil"));
        // The pickers mark the listed language a device's code reads as.
        let mine = |code: &str| {
            let all = all(Some(code));
            let marked: Vec<String> = all
                .into_iter()
                .filter(|l| l.device)
                .map(|l| l.code)
                .collect();
            marked
        };
        assert_eq!(mine("nb"), ["no"]);
        assert_eq!(mine("fil"), ["tl"]);
        assert_eq!(mine("tr"), ["tr"]);
        assert!(mine("xx").is_empty());
        assert!(all(None).iter().all(|l| !l.device));
    }

    #[test]
    fn a_region_the_table_lacks_and_an_unknown_code() {
        assert_eq!(read("fr-CA"), ("fr-CA".into(), "French (CA)".into()));
        assert_eq!(read("xx"), ("xx".into(), "xx".into()));
        assert_eq!(read(""), ("und".into(), "Unknown language".into()));
    }

    /// The pickers list every language the core reads, once each, by name,
    /// and each code reads back as itself.
    #[test]
    fn the_pickers_list_every_known_language() {
        let all = all(None);
        assert_eq!(all.len(), OPENSUBTITLES.len());
        assert!(all.windows(2).all(|w| w[0].name < w[1].name));
        for choice in &all {
            assert_eq!(
                read(&choice.code),
                (choice.code.clone(), choice.name.clone())
            );
        }
        let names: Vec<&str> = all.iter().map(|l| l.name.as_str()).collect();
        for name in ["Arabic", "Dutch", "Korean", "Polish", "Russian"] {
            assert!(names.contains(&name), "{name}");
        }
    }

    /// Each language's own name beside its English one, a region the table
    /// lacks kept (plans/player-polish.md 2.4).
    #[test]
    fn each_language_has_its_own_name() {
        let native = |code: &str| language(code).native;
        assert_eq!(native("tur"), "Türkçe");
        assert_eq!(native("pob"), "Português (Brasil)");
        assert_eq!(native("deu"), "Deutsch");
        assert_eq!(native("eng"), "English");
        assert_eq!(native("zho"), "中文");
        assert_eq!(native("fr-CA"), "Français (CA)");
        assert_eq!(native("xx"), "xx");
        assert!(OPENSUBTITLES.iter().all(|row| !row.3.trim().is_empty()));
    }

    #[test]
    fn the_primary_subtag() {
        assert_eq!(primary("pt-BR"), "pt");
        assert_eq!(primary("tr"), "tr");
    }
}
