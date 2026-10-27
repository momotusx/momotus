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
    // Before the bilingual row: a search for "chinese" ties by this order.
    ("zhc", "yue", "Chinese (Cantonese)", "粵語"),
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
/// its English name, `native` its own name, `sort` its place in the A to Z
/// directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct LanguageChoice {
    pub code: String,
    pub name: String,
    /// The language's own name ("Türkçe").
    pub native: String,
    /// The directory key: the English name with a qualifier moved behind
    /// ("Azerbaijani, South"), so its letter is where a person looks.
    pub sort: String,
    /// The language the device's own code reads as ("nb" is Norwegian,
    /// "no"): the one a picker offers and chooses as the device's.
    pub device: bool,
}

/// Every language the core knows (OpenSubtitles' list), by English name;
/// `device` marks the one the device's code reads as, if the list has it.
pub fn all(device: Option<&str>) -> Vec<LanguageChoice> {
    let mine = device.and_then(|d| device_tag(d, &ENTRIES));
    let mut all: Vec<LanguageChoice> = ENTRIES.iter().map(|e| e.choice(mine)).collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// The common languages, offered before any other (plans/m13-subtitle-languages.md 7).
const QUICK: [&str; 8] = ["en", "tr", "es", "fr", "de", "it", "pt", "ja"];

/// Suggested holds this many languages, or every chosen one when there are more.
const SUGGESTED: usize = 6;

/// A Latin spelling of each own name in another script, so a keyboard with
/// a to z reaches every language by its own name.
const LATIN: &[(&str, &str)] = &[
    ("ab", "apsua"),
    ("am", "amarinya"),
    ("ar", "arabiya"),
    ("hy", "hayeren"),
    ("as", "asamiya"),
    ("be", "belaruskaya"),
    ("bn", "bangla"),
    ("bg", "balgarski"),
    ("my", "myanma"),
    ("zh-Hans", "jianti zhongwen hanyu"),
    ("zh-Hant", "fanti zhongwen hanyu"),
    ("yue", "yueyu gwongdungwaa"),
    ("zh-x-bilingual", "zhongwen shuangyu"),
    ("cv", "chavashla"),
    ("ka", "kartuli"),
    ("el", "ellinika"),
    ("he", "ivrit"),
    ("hi", "hindi"),
    ("ja", "nihongo"),
    ("ja-x-bilingual", "nihongo bairingaru"),
    ("kn", "kannada"),
    ("kk", "qazaq tili"),
    ("km", "khmer"),
    ("ky", "kyrgyzcha"),
    ("ko", "hangugeo"),
    ("lo", "lao"),
    ("mk", "makedonski"),
    ("ml", "malayalam"),
    ("mr", "marathi"),
    ("mni", "meiteilon"),
    ("mn", "mongol"),
    ("ne", "nepali"),
    ("or", "odia"),
    ("fa", "farsi"),
    ("ps", "pashto"),
    ("ru", "russkiy"),
    ("sat", "santali"),
    ("sr", "srpski"),
    ("si", "sinhala"),
    ("sd", "sindhi"),
    ("syr", "suryaya"),
    ("ta", "tamil"),
    ("tt", "tatar"),
    ("te", "telugu"),
    ("th", "phasa thai"),
    ("uk", "ukrainska"),
    ("ur", "urdu"),
    ("prs", "dari"),
    ("azb", "torkce"),
    ("zgh", "tamazight"),
    ("rki", "rakhine"),
];

/// Other names people search by.
const ALTERNATE: &[(&str, &[&str])] = &[
    ("fa", &["farsi", "parsi"]),
    ("zh-Hans", &["mandarin", "putonghua", "中文", "汉语"]),
    ("zh-Hant", &["mandarin", "中文", "漢語"]),
    ("zh-x-bilingual", &["mandarin"]),
    ("yue", &["cantonese", "中文"]),
    ("tl", &["filipino", "pilipino"]),
    ("no", &["bokmål", "nynorsk"]),
    ("nl", &["flemish", "vlaams"]),
    ("es", &["castilian", "castellano"]),
    ("es-ES", &["castilian", "castellano"]),
    ("es-419", &["latino", "latinoamericano", "mexican"]),
    ("pt-BR", &["brazilian"]),
    ("my", &["myanmar"]),
    ("ga", &["gaelic", "irish gaelic"]),
    ("gd", &["gaelic"]),
    ("lb", &["letzeburgesch"]),
    ("he", &["ivrit"]),
    ("be", &["byelorussian", "belarusan"]),
    ("az", &["azeri"]),
    ("azb", &["azeri"]),
    ("zgh", &["berber", "tamazight"]),
    ("wen", &["lusatian", "wendish"]),
    ("or", &["oriya"]),
    ("sl", &["slovene"]),
    ("si", &["sinhalese"]),
    ("ky", &["kirghiz"]),
    ("km", &["cambodian"]),
    ("lo", &["laotian"]),
    ("ro", &["moldovan"]),
    ("sr", &["serbo-croatian"]),
    ("hr", &["serbo-croatian"]),
    ("bs", &["serbo-croatian"]),
    ("cnr", &["serbo-croatian"]),
    ("el", &["hellenic"]),
    ("ms", &["melayu"]),
    ("id", &["bahasa"]),
    ("ku", &["kurmanji", "sorani"]),
    ("prs", &["afghan persian"]),
];

/// Directory keys where the English name starts with a qualifier.
const SORT: &[(&str, &str)] = &[
    ("azb", "Azerbaijani, South"),
    ("se", "Sami, Northern"),
    ("zgh", "Amazigh, Standard Moroccan"),
];

/// Text as the picker's search reads it: NFKD, lower case, no combining
/// marks in any script, the letters that do not decompose mapped, Arabic
/// script's look-alike letters made one, apostrophes gone, and every other
/// character that is not a letter or a digit a word break. The same fold
/// reads the table and the query, so both sides always agree.
pub(crate) fn fold(text: &str) -> Vec<String> {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    let mut out = String::with_capacity(text.len());
    for c in text.nfkd().flat_map(char::to_lowercase) {
        if is_combining_mark(c) {
            continue;
        }
        match c {
            'ı' => out.push('i'),
            'ł' => out.push('l'),
            'ø' => out.push('o'),
            'đ' | 'ð' => out.push('d'),
            'þ' => out.push_str("th"),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            'ə' => out.push('e'),
            'ς' => out.push('σ'),
            'ي' | 'ى' => out.push('ی'),
            'ك' => out.push('ک'),
            'ة' => out.push('ه'),
            // أ إ آ ؤ need no arm: NFKD gives their letter and a mark, dropped above.
            'ٱ' => out.push('ا'),
            // Tatweel and apostrophes ("Oʻzbek", "O‘zbek" are "ozbek").
            '\u{0640}' | '\'' | '`' | '\u{2018}' | '\u{2019}' | '\u{02BB}' | '\u{02BC}' => {}
            c if c.is_alphabetic() || c.is_numeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().map(str::to_owned).collect()
}

/// One language as the picker searches it. `terms` hold each name's folded
/// words: the English name first, the own name second, its Latin spelling
/// third (empty when it has none), then the codes and other names; `codes`
/// the tag, codes and aliases with their words joined ("ptbr").
#[derive(Debug, Clone)]
struct Entry {
    tag: String,
    name: String,
    native: String,
    sort: String,
    terms: Vec<Vec<String>>,
    codes: Vec<String>,
}

impl Entry {
    fn choice(&self, device: Option<&str>) -> LanguageChoice {
        LanguageChoice {
            code: self.tag.clone(),
            name: self.name.clone(),
            native: self.native.clone(),
            sort: self.sort.clone(),
            device: device == Some(self.tag.as_str()),
        }
    }

    fn words(&self) -> impl Iterator<Item = &String> {
        self.terms.iter().flatten()
    }
}

/// The table as the picker reads it, in the table's own order.
static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    OPENSUBTITLES
        .iter()
        .map(|&(os, tag, name, native)| {
            let aliases = ALIASES.iter().filter(|&&(_, to)| to == os).map(|&(a, _)| a);
            let codes: Vec<&str> = [tag, os].into_iter().chain(aliases).collect();
            let latin = LATIN
                .iter()
                .find(|&&(t, _)| t == tag)
                .map_or("", |&(_, l)| l);
            let other = ALTERNATE
                .iter()
                .filter(|&&(t, _)| t == tag)
                .flat_map(|&(_, n)| n.iter().copied());
            let mut terms = vec![fold(name), fold(native), fold(latin)];
            terms.extend(
                codes
                    .iter()
                    .chain(other.collect::<Vec<_>>().iter())
                    .map(|t| fold(t)),
            );
            Entry {
                tag: tag.to_owned(),
                name: name.to_owned(),
                native: native.to_owned(),
                sort: SORT
                    .iter()
                    .find(|&&(t, _)| t == tag)
                    .map_or(name, |&(_, s)| s)
                    .to_owned(),
                codes: codes.iter().map(|c| fold(c).concat()).collect(),
                terms,
            }
        })
        .collect()
});

/// Audio's rows: one per primary language ("es" for Spanish, Latin
/// American and Spain's), named as the table names the primary (Chinese,
/// which the table has only by script, as the core's generic Chinese),
/// searched by the union of the group's terms.
static AUDIO: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    let mut rows: Vec<Entry> = Vec::new();
    for e in ENTRIES.iter() {
        let code = primary(&e.tag);
        if let Some(row) = rows.iter_mut().find(|r| r.tag == code) {
            row.terms.extend(e.terms.iter().cloned());
            row.codes.extend(e.codes.iter().cloned());
            continue;
        }
        let mut row = e.clone();
        if row.tag != code {
            let (_, name, native) = INDEX.get(code).copied().unwrap_or(CHINESE);
            row = Entry {
                tag: code.to_owned(),
                name: name.to_owned(),
                native: native.to_owned(),
                sort: name.to_owned(),
                terms: vec![fold(name), fold(native), e.terms[2].clone()],
                codes: vec![fold(code).concat()],
            };
            row.terms.extend(e.terms.iter().cloned());
            row.codes.extend(e.codes.iter().cloned());
        }
        rows.push(row);
    }
    rows
});

/// The picker's whole answer (plans/onboarding-language-picker.md): the
/// suggested languages, where No subtitles goes, and the results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct LanguageList {
    /// The chosen languages, then the device's, then the common ones: six
    /// in all, or every chosen one when there are more; empty while a
    /// query is typed.
    pub suggested: Vec<LanguageChoice>,
    /// Subtitles: No subtitles goes first and is the pre-chosen state (an
    /// English device, or one whose language the table lacks, with nothing
    /// chosen); else it goes last. Audio has no No subtitles.
    pub none_first: bool,
    /// With no query, every language in directory order; with one, the
    /// matches, best first; empty when nothing matches.
    pub results: Vec<LanguageChoice>,
}

/// The row the device's code reads as: its tag ("no" for "nb"), else its
/// language ("pt" for an unlisted "pt-AO"), else the first row of that
/// language (a Chinese device sends "zh": Simplified, or audio's "zh").
fn device_tag<'a>(code: &str, rows: &'a [Entry]) -> Option<&'a str> {
    let tag = language(code).tag;
    let base = primary(&tag);
    rows.iter()
        .find(|e| e.tag == tag)
        .or_else(|| rows.iter().find(|e| e.tag == base))
        .or_else(|| rows.iter().find(|e| primary(&e.tag) == base))
        .map(|e| e.tag.as_str())
}

/// The picker's answer for `query` (empty: none), the profile's `chosen`
/// languages in rank order, and the device's code. `audio` gives one row per
/// primary language and no No subtitles.
pub fn picker(device: Option<&str>, query: &str, chosen: &[String], audio: bool) -> LanguageList {
    let rows: &[Entry] = if audio { &AUDIO } else { &ENTRIES };
    let code_of = |tag: &str| -> String {
        let tag = language(tag).tag;
        if audio { primary(&tag).to_owned() } else { tag }
    };
    let device = device.and_then(|d| device_tag(d, rows)).map(code_of);
    let mut chosen: Vec<String> = chosen.iter().map(|c| code_of(c)).collect();
    // Audio's "es" and "es-419" are one row; the first keeps its rank.
    let mut seen = std::collections::HashSet::new();
    chosen.retain(|c| seen.insert(c.clone()));
    let choice = |code: &str| -> LanguageChoice {
        rows.iter().find(|e| e.tag == code).map_or_else(
            || {
                // A chosen language the table lacks ("fr-CA"): named as the core reads it.
                let l = language(code);
                LanguageChoice {
                    code: l.tag,
                    sort: l.name.clone(),
                    name: l.name,
                    native: l.native,
                    device: false,
                }
            },
            |e| e.choice(device.as_deref()),
        )
    };
    let none_first = !audio && chosen.is_empty() && device.as_deref().is_none_or(|d| d == "en");
    let words = fold(query);
    if !words.is_empty() {
        let results = search(rows, &words, device.as_deref(), &chosen, audio)
            .into_iter()
            .map(|e| e.choice(device.as_deref()))
            .collect();
        return LanguageList {
            suggested: Vec::new(),
            none_first,
            results,
        };
    }
    let mut codes: Vec<String> = chosen.clone();
    let fill = device
        .iter()
        .cloned()
        .chain(QUICK.iter().map(|q| code_of(q)));
    for code in fill {
        if codes.len() >= chosen.len().max(SUGGESTED) {
            break;
        }
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    let mut results: Vec<&Entry> = rows.iter().collect();
    results.sort_by(|a, b| a.sort.cmp(&b.sort));
    LanguageList {
        suggested: codes.iter().map(|c| choice(c)).collect(),
        none_first,
        results: results
            .into_iter()
            .map(|e| e.choice(device.as_deref()))
            .collect(),
    }
}

/// The rows `words` match, best first: 0 an exact code, 1 every word whole,
/// 2 the English name's first word, 3 the own name's or its Latin
/// spelling's first word, 4 every word a prefix of some word; only when
/// none of those matched, and for three letters or more, 5 a substring.
/// Ties: the device's language, then the chosen ones in order, then the
/// common ones, then the table's order.
fn search<'a>(
    rows: &'a [Entry],
    words: &[String],
    device: Option<&str>,
    chosen: &[String],
    audio: bool,
) -> Vec<&'a Entry> {
    let joined = words.concat();
    let first = &words[0];
    let starts = |term: &[String]| term.first().is_some_and(|w| w.starts_with(first.as_str()));
    let tier = |e: &Entry| -> Option<u8> {
        // A code typed whole matches with its words joined ("ptbr", "es419").
        if e.codes.contains(&joined) {
            return Some(0);
        }
        if !words
            .iter()
            .all(|q| e.words().any(|w| w.starts_with(q.as_str())))
        {
            return None;
        }
        Some(if words.iter().all(|q| e.words().any(|w| w == q)) {
            1
        } else if starts(&e.terms[0]) {
            2
        } else if starts(&e.terms[1]) || starts(&e.terms[2]) {
            3
        } else {
            4
        })
    };
    let mut hits: Vec<(u8, usize, &Entry)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, e)| tier(e).map(|t| (t, i, e)))
        .collect();
    if hits.is_empty() && joined.chars().count() >= 3 {
        hits = rows
            .iter()
            .enumerate()
            .filter(|(_, e)| e.terms.iter().any(|t| t.concat().contains(&joined)))
            .map(|(i, e)| (5, i, e))
            .collect();
    }
    let quick: Vec<String> = QUICK
        .iter()
        .map(|q| {
            if audio {
                primary(q).to_owned()
            } else {
                (*q).to_owned()
            }
        })
        .collect();
    let rank = |e: &Entry, i: usize| -> (usize, usize) {
        if device == Some(e.tag.as_str()) {
            (0, 0)
        } else if let Some(n) = chosen.iter().position(|c| *c == e.tag) {
            (1, n)
        } else if let Some(n) = quick.iter().position(|q| *q == e.tag) {
            (2, n)
        } else {
            (3, i)
        }
    };
    hits.sort_by_key(|&(t, i, e)| (t, rank(e, i)));
    hits.into_iter().map(|(_, _, e)| e).collect()
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

    fn codes(list: &[LanguageChoice]) -> Vec<&str> {
        list.iter().map(|l| l.code.as_str()).collect()
    }

    /// The fold reads every script the same way on both sides
    /// (plans/onboarding-language-picker.md, the fold).
    #[test]
    fn the_fold() {
        let f = |text: &str| fold(text).join(" ");
        for (text, folded) in [
            ("Türkçe", "turkce"),
            ("Čeština", "cestina"),
            ("Łódź", "lodz"),
            ("Azərbaycan", "azerbaycan"),
            ("Oʻzbek", "ozbek"),
            ("O‘zbek", "ozbek"),
            ("ΕΛΛΗΝΙΚΑ", "ελληνικα"),
            ("λόγος", "λογοσ"),
            ("pt-BR", "pt br"),
            ("ｔｕｒｋ", "turk"),
            ("İstanbul", "istanbul"),
            ("ISTANBUL", "istanbul"),
            ("Íslenska", "islenska"),
            ("Þórður", "thordur"),
            ("Straße", "strasse"),
            ("فارسي", "فارسی"),
            ("كتاب", "کتاب"),
            ("عـربـي", "عربی"),
            ("हिन्दी", "हनद"),
            // Syriac vowel points go as Hebrew and Arabic ones do.
            ("ܣܘ\u{0730}ܪܝܝܐ", "ܣܘܪܝܝܐ"),
            ("Chinese (Traditional)", "chinese traditional"),
            ("  - ", ""),
        ] {
            assert_eq!(f(text), folded, "{text}");
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Vector {
        query: String,
        #[serde(default)]
        device: Option<String>,
        #[serde(default)]
        audio: bool,
        #[serde(default)]
        first: Option<Vec<String>>,
        #[serde(default)]
        all: Option<Vec<String>>,
    }

    /// The golden vectors every platform's tests also run.
    #[test]
    fn the_search_vectors() {
        let vectors: Vec<Vector> =
            serde_json::from_str(include_str!("../tests/data/language-search.json")).unwrap();
        for v in vectors {
            let got = picker(v.device.as_deref(), &v.query, &[], v.audio).results;
            let got = codes(&got);
            let at = format!("{:?} device {:?} audio {}", v.query, v.device, v.audio);
            assert!(v.first.is_some() || v.all.is_some(), "{at}: no expectation");
            if let Some(first) = v.first {
                assert!(
                    got.len() >= first.len() && got[..first.len()] == first[..],
                    "{at}: {got:?}"
                );
            }
            if let Some(all) = v.all {
                assert_eq!(got, all, "{at}");
            }
        }
    }

    /// A code typed whole wins even when another language has a word that
    /// starts with it, which the table never has today (so the vectors
    /// cannot show it): without tier 0 "ptbr" would reach its language only
    /// through the substring tier, and lose to any prefix match.
    #[test]
    fn a_whole_code_wins_over_a_prefix() {
        let entry = |tag: &str, words: &[&str], codes: &[&str]| Entry {
            tag: tag.to_owned(),
            name: tag.to_owned(),
            native: tag.to_owned(),
            sort: tag.to_owned(),
            terms: vec![
                words.iter().map(|w| (*w).to_owned()).collect(),
                Vec::new(),
                Vec::new(),
            ],
            codes: codes.iter().map(|c| (*c).to_owned()).collect(),
        };
        let rows = [
            entry("x", &["ptbrx"], &["x"]),
            entry("pt-BR", &["portuguese"], &["ptbr"]),
        ];
        let found: Vec<&str> = search(&rows, &["ptbr".to_owned()], None, &[], false)
            .iter()
            .map(|e| e.tag.as_str())
            .collect();
        assert_eq!(found, ["pt-BR", "x"]);
    }

    /// A Latin route to every own name in another script, so a to z reaches it.
    #[test]
    fn every_other_script_has_a_latin_spelling() {
        for e in ENTRIES.iter() {
            let latin = |w: &String| w.chars().all(|c| (c as u32) < 0x250);
            if !e.terms[1].iter().all(latin) {
                assert!(
                    !e.terms[2].is_empty() && e.terms[2].iter().all(latin),
                    "{}",
                    e.tag
                );
            }
        }
        for &(tag, _) in LATIN.iter().chain(SORT) {
            assert!(ENTRIES.iter().any(|e| e.tag == tag), "{tag}");
        }
        for &(tag, _) in ALTERNATE {
            assert!(ENTRIES.iter().any(|e| e.tag == tag), "{tag}");
        }
    }

    /// Every alias finds its language, typed as a code.
    #[test]
    fn every_alias_finds_its_language() {
        for &(alias, os) in ALIASES {
            let tag = OPENSUBTITLES.iter().find(|r| r.0 == os).unwrap().1;
            let got = picker(None, alias, &[], false).results;
            assert_eq!(got.first().map(|l| l.code.as_str()), Some(tag), "{alias}");
        }
    }

    #[test]
    fn suggested_on_a_first_run() {
        let first = |device: Option<&str>| {
            let list = picker(device, "", &[], false);
            (
                codes(&list.suggested)
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
                list.none_first,
            )
        };
        // An English device starts on No subtitles, English first after it.
        assert_eq!(
            first(Some("en")),
            (
                vec!["en", "tr", "es", "fr", "de", "it"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                true
            )
        );
        assert!(first(Some("en-GB")).1);
        // Another device: its language first and chosen, No subtitles last.
        assert_eq!(
            first(Some("tr")),
            (
                vec!["tr", "en", "es", "fr", "de", "it"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                false
            )
        );
        assert_eq!(first(Some("nb")).0[0], "no");
        assert_eq!(first(Some("ko")).0, ["ko", "en", "tr", "es", "fr", "de"]);
        // A device language the table lacks, or none: nothing to pre-choose.
        assert_eq!(
            first(Some("xx")),
            (
                vec!["en", "tr", "es", "fr", "de", "it"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                true
            )
        );
        assert!(first(None).1);
        // A Chinese device sends "zh": Simplified is its language, audio's "zh" its row.
        assert_eq!(first(Some("zh")).0[0], "zh-Hans");
        assert_eq!(picker(Some("zh"), "", &[], true).suggested[0].code, "zh");
        // No subtitles keeps its place while the viewer types.
        assert!(picker(Some("en"), "turk", &[], false).none_first);
        assert!(!picker(Some("tr"), "turk", &[], false).none_first);
        // The device's row is marked in the results too.
        let list = picker(Some("tr"), "", &[], false);
        assert_eq!(
            list.results
                .iter()
                .filter(|l| l.device)
                .map(|l| l.code.as_str())
                .collect::<Vec<_>>(),
            ["tr"]
        );
        assert!(list.suggested.iter().all(|l| !l.native.is_empty()));
    }

    #[test]
    fn suggested_in_settings_puts_the_chosen_first() {
        let chosen: Vec<String> = ["fr", "tr", "fr-CA"].map(str::to_owned).to_vec();
        let list = picker(Some("de"), "", &chosen, false);
        assert_eq!(
            codes(&list.suggested),
            ["fr", "tr", "fr-CA", "de", "en", "es"]
        );
        assert!(!list.none_first);
        assert_eq!(list.suggested[2].name, "French (CA)");
        // Every chosen language shows, even past six.
        let many: Vec<String> = ["en", "tr", "es", "fr", "de", "it", "ja", "ko"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(picker(None, "", &many, false).suggested.len(), 8);
        // The chosen ones break a tie in the results.
        assert_eq!(
            codes(&picker(None, "turk", &["tk".to_owned()], false).results)[..2],
            ["tk", "tr"]
        );
    }

    #[test]
    fn audio_has_one_row_per_primary_language() {
        let list = picker(
            Some("pt-BR"),
            "",
            &["es-419".to_owned(), "spa".to_owned()],
            true,
        );
        assert!(!list.none_first);
        assert_eq!(codes(&list.suggested), ["es", "pt", "en", "tr", "fr", "de"]);
        let all = codes(&list.results);
        for gone in [
            "es-419",
            "es-ES",
            "pt-BR",
            "pt-MZ",
            "zh-Hans",
            "zh-Hant",
            "zh-x-bilingual",
            "ja-x-bilingual",
        ] {
            assert!(!all.contains(&gone), "{gone}");
        }
        for one in ["es", "pt", "zh", "ja", "yue"] {
            assert_eq!(all.iter().filter(|c| **c == one).count(), 1, "{one}");
        }
        let zh = list.results.iter().find(|l| l.code == "zh").unwrap();
        assert_eq!((zh.name.as_str(), zh.native.as_str()), ("Chinese", "中文"));
        assert!(list.results.iter().any(|l| l.code == "pt" && l.device));
    }

    #[test]
    fn every_language_has_its_own_name_and_a_sort_key() {
        for e in ENTRIES.iter().chain(AUDIO.iter()) {
            assert!(
                !e.native.trim().is_empty() && !e.sort.trim().is_empty(),
                "{}",
                e.tag
            );
        }
    }

    #[test]
    fn the_directory_is_in_its_own_order() {
        let list = picker(None, "", &[], false);
        assert_eq!(list.results.len(), OPENSUBTITLES.len());
        assert!(list.results.windows(2).all(|w| w[0].sort <= w[1].sort));
        let at = |code: &str| list.results.iter().position(|l| l.code == code).unwrap();
        assert!(at("azb") < at("eu"), "South Azerbaijani files under A");
        assert!(
            at("se") > at("ru") && at("se") < at("sr"),
            "Northern Sami under S, as Sami"
        );
        assert!(at("zgh") < at("am"), "Standard Moroccan Amazigh under A");
    }
}
