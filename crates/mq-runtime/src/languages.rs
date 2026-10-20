//! Subtitle languages as people read them (plans/m13-subtitle-languages.md):
//! one tag and one English name for whatever code an addon sends.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// OpenSubtitles' upload languages (opensubtitles.org/addons/export_languages.php,
/// read 2026-10-19): its code, the tag the core uses, the English name.
const OPENSUBTITLES: &[(&str, &str, &str)] = &[
    ("abk", "ab", "Abkhazian"),
    ("afr", "af", "Afrikaans"),
    ("alb", "sq", "Albanian"),
    ("amh", "am", "Amharic"),
    ("ara", "ar", "Arabic"),
    ("arg", "an", "Aragonese"),
    ("arm", "hy", "Armenian"),
    ("asm", "as", "Assamese"),
    ("ast", "ast", "Asturian"),
    ("aze", "az", "Azerbaijani"),
    ("baq", "eu", "Basque"),
    ("bel", "be", "Belarusian"),
    ("ben", "bn", "Bengali"),
    ("bos", "bs", "Bosnian"),
    ("bre", "br", "Breton"),
    ("bul", "bg", "Bulgarian"),
    ("bur", "my", "Burmese"),
    ("cat", "ca", "Catalan"),
    ("chi", "zh-Hans", "Chinese (Simplified)"),
    ("chv", "cv", "Chuvash"),
    ("cze", "cs", "Czech"),
    ("dan", "da", "Danish"),
    ("dut", "nl", "Dutch"),
    ("eng", "en", "English"),
    ("epo", "eo", "Esperanto"),
    ("est", "et", "Estonian"),
    ("fin", "fi", "Finnish"),
    ("fre", "fr", "French"),
    ("geo", "ka", "Georgian"),
    ("ger", "de", "German"),
    ("gla", "gd", "Scottish Gaelic"),
    ("gle", "ga", "Irish"),
    ("glg", "gl", "Galician"),
    ("ell", "el", "Greek"),
    ("heb", "he", "Hebrew"),
    ("hin", "hi", "Hindi"),
    ("hrv", "hr", "Croatian"),
    ("hun", "hu", "Hungarian"),
    ("ibo", "ig", "Igbo"),
    ("ice", "is", "Icelandic"),
    ("ina", "ia", "Interlingua"),
    ("ind", "id", "Indonesian"),
    ("ita", "it", "Italian"),
    ("jpn", "ja", "Japanese"),
    ("kan", "kn", "Kannada"),
    ("kaz", "kk", "Kazakh"),
    ("khm", "km", "Khmer"),
    ("kir", "ky", "Kyrgyz"),
    ("kor", "ko", "Korean"),
    ("kur", "ku", "Kurdish"),
    ("lao", "lo", "Lao"),
    ("lav", "lv", "Latvian"),
    ("lit", "lt", "Lithuanian"),
    ("ltz", "lb", "Luxembourgish"),
    ("mac", "mk", "Macedonian"),
    ("mal", "ml", "Malayalam"),
    ("mar", "mr", "Marathi"),
    ("may", "ms", "Malay"),
    ("mni", "mni", "Manipuri"),
    ("mon", "mn", "Mongolian"),
    ("nav", "nv", "Navajo"),
    ("nep", "ne", "Nepali"),
    ("nor", "no", "Norwegian"),
    ("oci", "oc", "Occitan"),
    ("ori", "or", "Odia"),
    ("per", "fa", "Persian"),
    ("pol", "pl", "Polish"),
    ("por", "pt", "Portuguese"),
    ("pus", "ps", "Pashto"),
    ("rus", "ru", "Russian"),
    ("sat", "sat", "Santali"),
    ("scc", "sr", "Serbian"),
    ("sin", "si", "Sinhala"),
    ("slo", "sk", "Slovak"),
    ("slv", "sl", "Slovenian"),
    ("sme", "se", "Northern Sami"),
    ("snd", "sd", "Sindhi"),
    ("som", "so", "Somali"),
    ("spa", "es", "Spanish"),
    ("swa", "sw", "Swahili"),
    ("swe", "sv", "Swedish"),
    ("syr", "syr", "Syriac"),
    ("tam", "ta", "Tamil"),
    ("tat", "tt", "Tatar"),
    ("tel", "te", "Telugu"),
    ("tet", "tet", "Tetum"),
    ("tgl", "tl", "Tagalog"),
    ("tha", "th", "Thai"),
    ("tuk", "tk", "Turkmen"),
    ("tur", "tr", "Turkish"),
    ("ukr", "uk", "Ukrainian"),
    ("urd", "ur", "Urdu"),
    ("uzb", "uz", "Uzbek"),
    ("vie", "vi", "Vietnamese"),
    ("wel", "cy", "Welsh"),
    ("wen", "wen", "Sorbian"),
    ("zul", "zu", "Zulu"),
    ("rum", "ro", "Romanian"),
    ("pob", "pt-BR", "Portuguese (Brazil)"),
    ("mne", "cnr", "Montenegrin"),
    ("zht", "zh-Hant", "Chinese (Traditional)"),
    ("zhe", "zh-x-bilingual", "Chinese (Bilingual)"),
    ("pom", "pt-MZ", "Portuguese (Mozambique)"),
    ("ext", "ext", "Extremaduran"),
    ("spl", "es-419", "Spanish (Latin America)"),
    ("spn", "es-ES", "Spanish (Spain)"),
    ("prs", "prs", "Dari"),
    ("tok", "tok", "Toki Pona"),
    ("zhc", "yue", "Chinese (Cantonese)"),
    ("azb", "azb", "South Azerbaijani"),
    ("zgh", "zgh", "Standard Moroccan Amazigh"),
    ("rki", "rki", "Rakhine"),
    ("jpe", "ja-x-bilingual", "Japanese (Bilingual)"),
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
const CHINESE: (&str, &str) = ("zh", "Chinese");

/// A subtitle language: a tag (ISO 639-1 where there is one, with its
/// region or script, `pt-BR`, `zh-Hant`; ISO 639-3 or the given code
/// otherwise) and its English name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Language {
    pub tag: String,
    pub name: String,
}

static INDEX: LazyLock<HashMap<String, (&'static str, &'static str)>> = LazyLock::new(|| {
    let mut index = HashMap::new();
    for &(code, tag, name) in OPENSUBTITLES {
        for key in [code, tag, name] {
            index.entry(key.to_lowercase()).or_insert((tag, name));
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
    let known = |(tag, name): (&str, &str)| Language {
        tag: tag.to_owned(),
        name: name.to_owned(),
    };
    if let Some(&found) = INDEX.get(&key) {
        return known(found);
    }
    // A region the table does not list: its language, the region named.
    if let Some((primary, region)) = key.split_once('-')
        && let Some(&(tag, name)) = INDEX.get(primary)
    {
        let region = region.to_uppercase();
        return Language {
            tag: format!("{tag}-{region}"),
            name: format!("{name} ({region})"),
        };
    }
    match code.trim() {
        "" => Language {
            tag: "und".into(),
            name: "Unknown language".into(),
        },
        other => Language {
            tag: key,
            name: other.to_owned(),
        },
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
}

/// Every language the core knows (OpenSubtitles' list), by English name.
pub fn all() -> Vec<LanguageChoice> {
    let mut all: Vec<LanguageChoice> = OPENSUBTITLES
        .iter()
        .map(|&(_, tag, name)| LanguageChoice {
            code: tag.to_owned(),
            name: name.to_owned(),
        })
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
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
        let all = all();
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

    #[test]
    fn the_primary_subtag() {
        assert_eq!(primary("pt-BR"), "pt");
        assert_eq!(primary("tr"), "tr");
    }
}
