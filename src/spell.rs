//! Spell checking with Hunspell dictionaries, through the `spellbook` crate.
//!
//! English (US) and French are built in; other Hunspell dictionaries installed on the
//! system are offered too. Dictionaries load on a background thread, so the window never
//! waits for them, and squiggles appear once one is ready.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use eframe::egui;

use crate::i18n::t;
use crate::model::StyledText;

const EN_US_AFF: &str = include_str!("../assets/dictionaries/en_US.aff");
const EN_US_DIC: &str = include_str!("../assets/dictionaries/en_US.dic");
const FR_AFF: &str = include_str!("../assets/dictionaries/fr.aff");
const FR_DIC: &str = include_str!("../assets/dictionaries/fr.dic");

/// Used when the system language has no dictionary.
const FALLBACK: &str = "en_US";

#[derive(Debug, Clone)]
pub struct DictionaryInfo {
    /// The Hunspell code, e.g. `en_US` or `fr`.
    pub code: String,
    /// A readable name for menus, e.g. "Français".
    pub name: String,
    source: Source,
}

#[derive(Debug, Clone)]
enum Source {
    Builtin {
        aff: &'static str,
        dic: &'static str,
    },
    Files {
        aff: PathBuf,
        dic: PathBuf,
        encoding: Encoding,
    },
}

/// The character sets Folio reads Hunspell files in (their `SET` line).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoding {
    Utf8,
    /// ISO8859-1, Hunspell's default when there is no `SET` line.
    Latin1,
    /// ISO8859-15: Latin-1 with €, Š, š, Ž, ž, Œ, œ and Ÿ.
    Latin9,
}

/// The built-in dictionaries, then system ones for other languages.
pub fn available() -> Vec<DictionaryInfo> {
    let mut list = vec![
        DictionaryInfo {
            code: "en_US".into(),
            name: language_name("en_US"),
            source: Source::Builtin {
                aff: EN_US_AFF,
                dic: EN_US_DIC,
            },
        },
        DictionaryInfo {
            code: "fr".into(),
            name: language_name("fr"),
            source: Source::Builtin {
                aff: FR_AFF,
                dic: FR_DIC,
            },
        },
    ];
    let mut files: Vec<PathBuf> = system_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "dic"))
        .collect();
    files.sort();
    let mut seen = HashSet::new();
    // One entry per name: a system `fr_FR` that is a copy of the French Folio ships (or
    // `de` next to `de_DE`) would otherwise be listed twice.
    let mut names: HashSet<String> = list.iter().map(|known| known.name.clone()).collect();
    let mut found = Vec::new();
    for dic in files {
        let aff = dic.with_extension("aff");
        let Some(code) = dic.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !aff.is_file() || code.starts_with("hyph") || code.starts_with("th_") {
            continue;
        }
        // Symlinked variants (fr_FR → fr) are one dictionary: list it once, and not at
        // all when it is one Folio ships.
        let real = std::fs::canonicalize(&dic).unwrap_or_else(|_| dic.clone());
        let real_code = real
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(code);
        let name = language_name(code);
        if list
            .iter()
            .any(|known| known.code == code || known.code == real_code)
            || names.contains(&name)
            || !seen.insert(real.clone())
        {
            continue;
        }
        // A character set Folio can't decode would flag every word.
        let Some(encoding) = declared_encoding(&aff) else {
            continue;
        };
        names.insert(name.clone());
        found.push(DictionaryInfo {
            code: code.to_string(),
            name,
            source: Source::Files { aff, dic, encoding },
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    list.extend(found);
    list
}

fn system_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/Library/Spelling"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(Path::new(&home).join("Library/Spelling"));
        }
    } else if cfg!(unix) {
        for dir in [
            "/usr/share/hunspell",
            "/usr/share/myspell",
            "/usr/share/myspell/dicts",
            "/usr/local/share/hunspell",
        ] {
            dirs.push(PathBuf::from(dir));
        }
    }
    dirs
}

/// Dictionary names as a list: "A", "A and B", "A, B and C" (in the UI language).
fn join_names(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => {
            t!("spell-names-two", first = rest.join(", "), second = *last).into_owned()
        }
    }
}

/// "Français" for `fr`, "English (UK)" for `en_GB`; unknown codes stay as they are.
pub fn language_name(code: &str) -> String {
    let name = match code.to_ascii_lowercase().replace('-', "_").as_str() {
        "en" | "en_us" => "English (US)",
        "en_gb" => "English (UK)",
        "en_au" => "English (Australia)",
        "en_ca" => "English (Canada)",
        "en_za" => "English (South Africa)",
        "fr" | "fr_fr" => "Français",
        "fr_be" => "Français (Belgique)",
        "fr_ca" => "Français (Canada)",
        "fr_ch" => "Français (Suisse)",
        "de" | "de_de" => "Deutsch",
        "de_at" => "Deutsch (Österreich)",
        "de_ch" => "Deutsch (Schweiz)",
        "es" | "es_es" => "Español",
        "es_mx" => "Español (México)",
        "it" | "it_it" => "Italiano",
        "pt" | "pt_pt" => "Português",
        "pt_br" => "Português (Brasil)",
        "nl" | "nl_nl" => "Nederlands",
        "ca" | "ca_es" => "Català",
        "pl" | "pl_pl" => "Polski",
        "sv" | "sv_se" => "Svenska",
        "da" | "da_dk" => "Dansk",
        "nb" | "nb_no" => "Norsk bokmål",
        "ru" | "ru_ru" => "Русский",
        _ => return code.to_string(),
    };
    name.to_string()
}

/// The system language as a Hunspell-style code, e.g. `fr_FR` (empty when unknown).
fn system_locale() -> String {
    let locale = sys_locale::get_locale()
        .unwrap_or_default()
        .replace('-', "_");
    locale.split('.').next().unwrap_or_default().to_string()
}

/// The dictionaries for a `spell_language` setting, the main one first: none when
/// spelling is off. A code whose dictionary is gone falls back to the automatic choice.
fn pick(setting: &str, locale: &str, dictionaries: &[DictionaryInfo]) -> Vec<usize> {
    match setting {
        "off" => Vec::new(),
        "" | "auto" => automatic(locale, dictionaries),
        code => match dictionaries
            .iter()
            .position(|dictionary| dictionary.code == code)
        {
            Some(index) => vec![index],
            None => automatic(locale, dictionaries),
        },
    }
}

/// Automatic: the system language, plus English when that is another language — Folio's
/// own pages are English and so is much of what people paste, so neither gets flagged
/// word by word (Office detects the language of each paragraph to the same end).
fn automatic(locale: &str, dictionaries: &[DictionaryInfo]) -> Vec<usize> {
    let main = pick_for_locale(locale, dictionaries);
    let mut list = vec![main];
    let english = dictionaries.iter().position(|d| d.code == FALLBACK);
    if let Some(english) = english {
        if language_of(&dictionaries[main].code) != language_of(FALLBACK) {
            list.push(english);
        }
    }
    list
}

fn language_of(code: &str) -> &str {
    code.split(['_', '-']).next().unwrap_or_default()
}

/// The exact locale (`en_GB`), else the same language (`fr_FR` → `fr`), else English.
fn pick_for_locale(locale: &str, dictionaries: &[DictionaryInfo]) -> usize {
    let language = locale.split('_').next().unwrap_or_default();
    let same_language = |dictionary: &DictionaryInfo| {
        !language.is_empty() && dictionary.code.split('_').next() == Some(language)
    };
    dictionaries
        .iter()
        .position(|dictionary| dictionary.code.eq_ignore_ascii_case(locale))
        .or_else(|| dictionaries.iter().position(same_language))
        .or_else(|| dictionaries.iter().position(|d| d.code == FALLBACK))
        .unwrap_or(0)
}

/// Longer runs without a space are data (encoded blobs, minified code), not prose.
const MAX_CHUNK: usize = 256;

/// Words to check in `text`, as char ranges. Apostrophes and hyphens inside a word keep
/// it whole ("l'école", "well-known"). Numbers, one-letter words, ACRONYMS and web or
/// e-mail addresses are skipped, as in the desktop notebook. One pass, however long the text.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let chars: Vec<char> = text.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        // Words never hold a space, so each run of non-space text is tokenized on its
        // own, and an address is recognised once for the whole run.
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let chunk = &chars[start..i];
        if chunk.len() <= MAX_CHUNK && !is_address(chunk) {
            words_in(chunk, start, &mut result);
        }
    }
    result
}

/// The words of a run of non-space text that starts at char `offset`.
fn words_in(chunk: &[char], offset: usize, result: &mut Vec<Range<usize>>) {
    let mut i = 0;
    while i < chunk.len() {
        if !chunk[i].is_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < chunk.len() {
            if chunk[end].is_alphanumeric() {
                end += 1;
            } else if is_joiner(chunk[end])
                && chunk
                    .get(end + 1)
                    .is_some_and(|next| next.is_alphanumeric())
            {
                end += 2;
            } else {
                break;
            }
        }
        i = end;
        let word = &chunk[start..end];
        if word.len() < 2
            || word.iter().any(|c| c.is_numeric())
            || word
                .iter()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_uppercase())
        {
            continue;
        }
        result.push(offset + start..offset + end);
    }
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\'' | '’' | '-' | '\u{2011}')
}

/// Whether a run of non-space text is a web or e-mail address.
fn is_address(chunk: &[char]) -> bool {
    chunk.contains(&'@')
        || chunk.windows(3).any(|three| three == [':', '/', '/'])
        || (chunk.len() >= 4
            && chunk[..4]
                .iter()
                .map(char::to_ascii_lowercase)
                .eq("www.".chars()))
}

/// French elisions checked on their own when the dictionary lacks the whole form.
const ELISIONS: [&str; 14] = [
    "c", "d", "j", "l", "m", "n", "qu", "s", "t", "jusqu", "lorsqu", "puisqu", "quoiqu", "presqu",
];

/// Whether `dictionary` knows `word`, trying both apostrophes, a French elision on its
/// own and each part of a hyphenated word. `accept` holds the user's own words, which
/// also count as parts ("Folioz-based" once "Folioz" is added).
fn check_with(
    dictionary: &spellbook::Dictionary,
    word: &str,
    accept: &dyn Fn(&str) -> bool,
) -> bool {
    if accept(word) || dictionary.check(word) {
        return true;
    }
    // Dictionaries list words with one of the two apostrophes.
    if word.contains('’') && dictionary.check(&word.replace('’', "'")) {
        return true;
    }
    if word.contains('\'') && dictionary.check(&word.replace('\'', "’")) {
        return true;
    }
    if let Some(split) = word.find(['\'', '’']) {
        let (prefix, rest) = word.split_at(split);
        let rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
        if ELISIONS.contains(&prefix.to_lowercase().as_str())
            && rest.chars().count() > 1
            && check_with(dictionary, rest, accept)
        {
            return true;
        }
    }
    // The parts hold no hyphen, so this recursion stops after one level.
    word.contains('-')
        && word
            .split('-')
            .all(|part| part.chars().count() < 2 || check_with(dictionary, part, accept))
}

/// A dictionary and the letters its language is written with.
struct Loaded {
    dictionary: spellbook::Dictionary,
    /// Lower-case letters.
    alphabet: HashSet<char>,
}

impl Loaded {
    fn new(aff: &str, dic: &str) -> Result<Self, String> {
        let dictionary = spellbook::Dictionary::new(aff, dic).map_err(|error| error.to_string())?;
        Ok(Self {
            dictionary,
            alphabet: alphabet(aff, dic),
        })
    }

    /// Whether the word is written only with this language's letters. Words with
    /// others (Cyrillic or Chinese next to French, "Dvořák" in English) are left alone
    /// rather than flagged.
    fn writes(&self, word: &str) -> bool {
        word.chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.to_lowercase().all(|c| self.alphabet.contains(&c)))
    }
}

/// The letters a language uses: its `.aff` `TRY` line (the letters Hunspell tries when
/// suggesting), else every letter of its words. `.dic` files also list symbols such as
/// "α" or "Ω", so `TRY` is the better guide.
fn alphabet(aff: &str, dic: &str) -> HashSet<char> {
    let tried = aff.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some("TRY")).then(|| fields.next().unwrap_or_default())
    });
    let letters: Box<dyn Iterator<Item = char>> = match tried {
        Some(tried) if tried.chars().any(char::is_alphabetic) => Box::new(tried.chars()),
        _ => Box::new(dic.lines().skip(1).flat_map(|line| {
            line.split(|c: char| c == '/' || c.is_whitespace())
                .next()
                .unwrap_or_default()
                .chars()
        })),
    };
    letters
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The user's own words: exact, or in lower case.
fn user_accepts(personal: &HashSet<String>, ignored: &HashSet<String>, word: &str) -> bool {
    if personal.contains(word) || ignored.contains(word) {
        return true;
    }
    let lower = word.to_lowercase();
    personal.contains(&lower) || ignored.contains(&lower)
}

/// Suggestions from several dictionaries: the one the text is in first, or alternating
/// when that is unclear. At most six, without repeats.
fn merge_suggestions(lists: Vec<Vec<String>>, alternate: bool) -> Vec<String> {
    let mut ordered = Vec::new();
    if alternate {
        let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
        for i in 0..longest {
            ordered.extend(lists.iter().filter_map(|list| list.get(i)).cloned());
        }
    } else {
        ordered = lists.into_iter().flatten().collect();
    }
    let mut seen = HashSet::new();
    ordered.retain(|word| seen.insert(word.clone()));
    ordered.truncate(6);
    ordered
}

enum Load {
    Off,
    Loading(mpsc::Receiver<Result<Vec<Loaded>, String>>),
    /// The main dictionary first.
    Ready(Vec<Arc<Loaded>>),
    Failed(LoadError),
}

/// Why the dictionaries did not load. Folio's own reason is kept as a reason, not as text,
/// so `Speller::status` words it in the language shown when it is read.
enum LoadError {
    /// The loader stopped without an answer.
    Unreadable,
    /// The system's or the dictionary reader's own words, shown as they are.
    Other(String),
}

/// Misspelled char ranges per block, keyed by a hash of the text, code runs and generation.
type BlockCache = HashMap<egui::Id, (u64, Arc<Vec<Range<usize>>>)>;

/// Suggestions are looked up per word and per [`Speller::language_near`] answer.
type SuggestionKey = (String, Option<usize>);

/// Spell-checking state for the editor: the dictionary, the user's words and caches.
pub struct Speller {
    /// The `spell_language` setting: "auto", "off" or a dictionary code.
    setting: String,
    dictionaries: Vec<DictionaryInfo>,
    /// Indices into `dictionaries` of the ones in use, the main one first.
    active: Vec<usize>,
    load: Load,
    /// Bumped for each dictionary load, so late suggestions for an old one are dropped.
    load_id: u64,
    personal: HashSet<String>,
    personal_changed: bool,
    ignored: HashSet<String>,
    /// Bumped whenever answers may change (new dictionary, added or ignored words).
    generation: u64,
    known: HashMap<String, bool>,
    blocks: BlockCache,
    suggestions: HashMap<SuggestionKey, Option<Vec<String>>>,
    suggestion_tx: mpsc::Sender<(u64, SuggestionKey, Vec<String>)>,
    suggestion_rx: mpsc::Receiver<(u64, SuggestionKey, Vec<String>)>,
}

impl Default for Speller {
    fn default() -> Self {
        let (suggestion_tx, suggestion_rx) = mpsc::channel();
        Self {
            setting: "auto".into(),
            dictionaries: Vec::new(),
            active: Vec::new(),
            load: Load::Off,
            load_id: 0,
            personal: HashSet::new(),
            personal_changed: false,
            ignored: HashSet::new(),
            generation: 0,
            known: HashMap::new(),
            blocks: HashMap::new(),
            suggestions: HashMap::new(),
            suggestion_tx,
            suggestion_rx,
        }
    }
}

impl Source {
    fn load(&self) -> Result<Loaded, String> {
        match self {
            Source::Builtin { aff, dic } => Loaded::new(aff, dic),
            Source::Files { aff, dic, encoding } => {
                Loaded::new(&read_text(aff, *encoding)?, &read_text(dic, *encoding)?)
            }
        }
    }
}

/// The character set an `.aff` file declares on its `SET` line, when Folio can read it.
fn declared_encoding(aff: &Path) -> Option<Encoding> {
    use std::io::BufRead;
    let file = std::fs::File::open(aff).ok()?;
    for line in std::io::BufReader::new(file).split(b'\n').take(500) {
        let line = line.ok()?;
        let line = String::from_utf8_lossy(&line);
        let mut fields = line.trim_start_matches('\u{feff}').split_whitespace();
        if fields.next() == Some("SET") {
            return encoding_named(fields.next().unwrap_or_default());
        }
    }
    Some(Encoding::Latin1)
}

fn encoding_named(name: &str) -> Option<Encoding> {
    match name.to_ascii_uppercase().replace(['-', '_'], "").as_str() {
        "UTF8" => Some(Encoding::Utf8),
        "ISO88591" => Some(Encoding::Latin1),
        "ISO885915" => Some(Encoding::Latin9),
        _ => None,
    }
}

/// Reads a dictionary file in the character set its `.aff` declares.
fn read_text(path: &Path, encoding: Encoding) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(decode(bytes, encoding))
}

fn decode(bytes: Vec<u8>, encoding: Encoding) -> String {
    match encoding {
        Encoding::Utf8 => String::from_utf8(bytes)
            .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()),
        Encoding::Latin1 => bytes.into_iter().map(char::from).collect(),
        Encoding::Latin9 => bytes
            .into_iter()
            .map(|byte| match byte {
                0xA4 => '€',
                0xA6 => 'Š',
                0xA8 => 'š',
                0xB4 => 'Ž',
                0xB8 => 'ž',
                0xBC => 'Œ',
                0xBD => 'œ',
                0xBE => 'Ÿ',
                _ => char::from(byte),
            })
            .collect(),
    }
}

impl Speller {
    /// Applies the saved settings and starts loading the dictionary.
    pub fn configure(&mut self, setting: &str, personal: &str) {
        self.personal = personal
            .lines()
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect();
        self.personal_changed = false;
        self.setting.clear();
        self.set_language(setting);
    }

    /// Switches to "auto", "off" or a dictionary code; the dictionary loads in the background.
    pub fn set_language(&mut self, setting: &str) {
        self.set_language_in(setting, &system_locale());
    }

    /// [`Self::set_language`] on a computer set to `locale` (`fr_FR`).
    fn set_language_in(&mut self, setting: &str, locale: &str) {
        let setting = if setting.is_empty() { "auto" } else { setting };
        if self.setting == setting {
            return;
        }
        self.setting = setting.to_string();
        if self.dictionaries.is_empty() {
            self.dictionaries = available();
        }
        self.active = pick(setting, locale, &self.dictionaries);
        self.load_id += 1;
        self.suggestions.clear();
        self.load = if self.active.is_empty() {
            Load::Off
        } else {
            let sources: Vec<Source> = self
                .active
                .iter()
                .map(|&index| self.dictionaries[index].source.clone())
                .collect();
            let (tx, rx) = mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("folio-spelling".into())
                .spawn(move || {
                    let _ = tx.send(sources.iter().map(Source::load).collect());
                });
            match spawned {
                Ok(_) => Load::Loading(rx),
                Err(error) => Load::Failed(LoadError::Other(error.to_string())),
            }
        };
        self.forget();
    }

    /// Picks up a dictionary that finished loading and suggestions that are ready.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let received = match &self.load {
            Load::Loading(rx) => Some(rx.try_recv()),
            _ => None,
        };
        match received {
            Some(Ok(Ok(loaded))) => {
                self.load = Load::Ready(loaded.into_iter().map(Arc::new).collect());
                self.forget();
                ctx.request_repaint();
            }
            Some(Ok(Err(error))) => self.load = Load::Failed(LoadError::Other(error)),
            Some(Err(mpsc::TryRecvError::Empty)) => {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.load = Load::Failed(LoadError::Unreadable);
            }
            None => {}
        }
        while let Ok((load_id, key, list)) = self.suggestion_rx.try_recv() {
            if load_id == self.load_id {
                self.suggestions.insert(key, Some(list));
                ctx.request_repaint();
            }
        }
        if self.suggestions.values().any(Option::is_none) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.load, Load::Ready(_))
    }

    /// The `spell_language` setting in use: "auto", "off" or a code.
    pub fn setting(&self) -> &str {
        &self.setting
    }

    pub fn dictionaries(&self) -> &[DictionaryInfo] {
        &self.dictionaries
    }

    /// The languages "Automatic" means on this computer, e.g. "Français + English (US)".
    pub fn automatic_name(&self) -> String {
        self.names(&pick("auto", &system_locale(), &self.dictionaries), " + ")
    }

    fn names(&self, indices: &[usize], separator: &str) -> String {
        indices
            .iter()
            .filter_map(|&index| self.dictionaries.get(index))
            .map(|dictionary| dictionary.name.as_str())
            .collect::<Vec<_>>()
            .join(separator)
    }

    /// One line for the settings dialog, in the UI language.
    pub fn status(&self) -> String {
        let names: Vec<&str> = self
            .active
            .iter()
            .filter_map(|&index| self.dictionaries.get(index))
            .map(|dictionary| dictionary.name.as_str())
            .collect();
        let names = join_names(&names);
        let count = self.active.len() as i64;
        match &self.load {
            Load::Off => t!("spell-status-off").into_owned(),
            Load::Loading(_) => t!(
                "spell-status-loading",
                names = names.as_str(),
                count = count
            )
            .into_owned(),
            Load::Ready(_) => t!("spell-status-ready", names = names.as_str()).into_owned(),
            Load::Failed(error) => {
                let error = match error {
                    LoadError::Unreadable => t!("spell-error-unreadable"),
                    LoadError::Other(error) => std::borrow::Cow::Borrowed(error.as_str()),
                };
                t!(
                    "spell-status-failed",
                    names = names.as_str(),
                    count = count,
                    error = error.as_ref()
                )
                .into_owned()
            }
        }
    }

    /// Misspelled words in a block, as char ranges in order. Cached per block and text,
    /// so this is cheap to call every frame.
    pub fn misspelled(&mut self, block: egui::Id, text: &StyledText) -> Arc<Vec<Range<usize>>> {
        if !self.is_ready() {
            return Arc::default();
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.text.hash(&mut hasher);
        for run in &text.runs {
            (run.start, run.end, run.style.code).hash(&mut hasher);
        }
        self.generation.hash(&mut hasher);
        let key = hasher.finish();
        if let Some((cached, ranges)) = self.blocks.get(&block) {
            if *cached == key {
                return Arc::clone(ranges);
            }
        }
        let chars: Vec<char> = text.text.chars().collect();
        let bytes: Vec<usize> = text.text.char_indices().map(|(byte, _)| byte).collect();
        let mut found = Vec::new();
        for range in words(&text.text) {
            // Inline code is not prose.
            if text.style_at(bytes[range.start]).code {
                continue;
            }
            let word: String = chars[range.clone()].iter().collect();
            if !self.check(&word) {
                found.push(range);
            }
        }
        let found = Arc::new(found);
        if self.blocks.len() > 4096 {
            self.blocks.clear();
        }
        self.blocks.insert(block, (key, Arc::clone(&found)));
        found
    }

    /// Whether a word is spelled right in any of the languages in use, or the user chose
    /// to accept it. Words with letters none of them uses are not judged.
    pub fn check(&mut self, word: &str) -> bool {
        let Load::Ready(loaded) = &self.load else {
            return true;
        };
        let (personal, ignored) = (&self.personal, &self.ignored);
        if user_accepts(personal, ignored, word) {
            return true;
        }
        if let Some(&known) = self.known.get(word) {
            return known;
        }
        // A non-breaking hyphen joins words like "-" does.
        let normal = word.replace('\u{2011}', "-");
        let accept = |part: &str| user_accepts(personal, ignored, part);
        let known = !loaded.iter().any(|each| each.writes(&normal))
            || loaded
                .iter()
                .any(|each| check_with(&each.dictionary, &normal, &accept));
        if self.known.len() > 50_000 {
            self.known.clear();
        }
        self.known.insert(word.to_string(), known);
        known
    }

    /// Which language in use the words around `range` (chars of `text`) are written in,
    /// as an index into the loaded dictionaries: the one that knows the most of them,
    /// `None` on a tie. Suggestions come from that language first.
    pub fn language_near(&self, text: &str, range: Range<usize>) -> Option<usize> {
        let Load::Ready(loaded) = &self.load else {
            return Some(0);
        };
        if loaded.len() < 2 {
            return Some(0);
        }
        const AROUND: usize = 30;
        let all = words(text);
        let at = all.partition_point(|word| word.end <= range.start);
        let chars: Vec<char> = text.chars().collect();
        let accept = |_: &str| false;
        let mut counts = vec![0usize; loaded.len()];
        for word in all[at.saturating_sub(AROUND)..(at + AROUND + 1).min(all.len())]
            .iter()
            .filter(|word| **word != range)
        {
            let word: String = chars[word.clone()].iter().collect();
            let word = word.replace('\u{2011}', "-");
            for (count, each) in counts.iter_mut().zip(loaded) {
                if check_with(&each.dictionary, &word, &accept) {
                    *count += 1;
                }
            }
        }
        let best = counts.iter().copied().max().unwrap_or(0);
        let mut leaders = counts
            .iter()
            .enumerate()
            .filter(|(_, &count)| count == best);
        match (leaders.next(), leaders.next()) {
            (Some((index, _)), None) => Some(index),
            _ => None,
        }
    }

    /// Starts looking up suggestions on a background thread (they can take a few hundred
    /// milliseconds); see [`Self::suggestions`]. `language` is from [`Self::language_near`].
    pub fn request_suggestions(&mut self, word: &str, language: Option<usize>) {
        let Load::Ready(loaded) = &self.load else {
            return;
        };
        let key = (word.to_string(), language);
        if self.suggestions.contains_key(&key) {
            return;
        }
        // The text's language first; the others after it, or alternating with it when
        // the language is unclear.
        let mut order: Vec<Arc<Loaded>> = loaded.clone();
        if let Some(first) = language.filter(|&index| index < order.len()) {
            let main = order.remove(first);
            order.insert(0, main);
        }
        let tx = self.suggestion_tx.clone();
        let (load_id, owned) = (self.load_id, key.clone());
        let spawned = std::thread::Builder::new()
            .name("folio-suggestions".into())
            .spawn(move || {
                let mut lists = Vec::new();
                for each in &order {
                    if language.is_some() && lists.iter().map(Vec::len).sum::<usize>() >= 6 {
                        break;
                    }
                    // A panic in the dictionary must still answer, or the menu would
                    // wait (and repaint) forever.
                    let list = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut list = Vec::new();
                        each.dictionary.suggest(&owned.0, &mut list);
                        list
                    }))
                    .unwrap_or_default();
                    lists.push(list);
                }
                let _ = tx.send((load_id, owned, merge_suggestions(lists, language.is_none())));
            });
        let pending = if spawned.is_ok() {
            None
        } else {
            Some(Vec::new())
        };
        self.suggestions.insert(key, pending);
    }

    /// Suggestions for a word, best first; `None` while they are being looked up.
    pub fn suggestions(&self, word: &str, language: Option<usize>) -> Option<&[String]> {
        self.suggestions
            .get(&(word.to_string(), language))?
            .as_deref()
    }

    pub fn add_personal(&mut self, word: &str) {
        if self.personal.insert(word.to_string()) {
            self.personal_changed = true;
            self.forget();
        }
    }

    /// Accepts a word until Folio closes.
    pub fn ignore_all(&mut self, word: &str) {
        if self.ignored.insert(word.to_string()) {
            self.forget();
        }
    }

    /// The `code`s of the dictionaries in use, the main one first: `language_near`'s
    /// index picks one.
    pub fn active_codes(&self) -> Vec<&str> {
        self.active
            .iter()
            .map(|&index| self.dictionaries[index].code.as_str())
            .collect()
    }

    pub fn personal_count(&self) -> usize {
        self.personal.len()
    }

    pub fn clear_personal(&mut self) {
        if !self.personal.is_empty() {
            self.personal.clear();
            self.forget();
        }
        self.personal_changed = false;
    }

    /// The personal dictionary to save (one word per line), once after each change.
    pub fn take_personal_change(&mut self) -> Option<String> {
        if !std::mem::take(&mut self.personal_changed) {
            return None;
        }
        let mut words: Vec<&str> = self.personal.iter().map(String::as_str).collect();
        words.sort_unstable();
        Some(words.join("\n"))
    }

    fn forget(&mut self) {
        self.generation += 1;
        self.known.clear();
        self.blocks.clear();
    }
}

#[cfg(test)]
impl Speller {
    /// A speller with its dictionary loaded, for tests.
    pub fn ready(setting: &str) -> Self {
        Self::ready_in(setting, "C")
    }

    /// A speller with its dictionaries loaded, on a computer set to `locale`.
    pub fn ready_in(setting: &str, locale: &str) -> Self {
        let mut speller = Self::default();
        speller.setting.clear();
        speller.set_language_in(setting, locale);
        speller.wait_until_ready();
        speller
    }

    /// Blocks until the dictionary has loaded.
    pub fn wait_until_ready(&mut self) {
        let received = match &self.load {
            Load::Loading(rx) => Some(rx.recv_timeout(Duration::from_secs(30))),
            _ => None,
        };
        if let Some(result) = received {
            self.load = match result.expect("the dictionary loads") {
                Ok(loaded) => Load::Ready(loaded.into_iter().map(Arc::new).collect()),
                Err(error) => Load::Failed(LoadError::Other(error)),
            };
            self.forget();
        }
        assert!(self.is_ready(), "{}", self.status());
    }

    /// Blocks until suggestions for `word` are known: the ones the menu asked for, or
    /// else from the main language.
    pub fn wait_for_suggestions(&mut self, word: &str) -> Vec<String> {
        if !self.suggestions.keys().any(|(asked, _)| asked == word) {
            self.request_suggestions(word, Some(0));
        }
        while self
            .suggestions
            .iter()
            .any(|((asked, _), list)| asked == word && list.is_none())
        {
            let (load_id, key, list) = self
                .suggestion_rx
                .recv_timeout(Duration::from_secs(30))
                .expect("suggestions arrive");
            if load_id == self.load_id {
                self.suggestions.insert(key, Some(list));
            }
        }
        self.suggestions
            .iter()
            .find_map(|((asked, _), list)| (asked == word).then(|| list.clone()))
            .flatten()
            .unwrap_or_default()
    }

    /// Blocks until the suggestions for `word` in `language` are known.
    pub fn wait_for_suggestions_in(&mut self, word: &str, language: Option<usize>) -> Vec<String> {
        self.request_suggestions(word, language);
        while self.suggestions(word, language).is_none() {
            let (load_id, key, list) = self
                .suggestion_rx
                .recv_timeout(Duration::from_secs(30))
                .expect("suggestions arrive");
            if load_id == self.load_id {
                self.suggestions.insert(key, Some(list));
            }
        }
        self.suggestions(word, language)
            .unwrap_or_default()
            .to_vec()
    }

    pub fn has_personal(&self, word: &str) -> bool {
        self.personal.contains(word)
    }

    /// The misspelled words of a text, in order.
    pub fn misspelled_words(&mut self, text: &str) -> Vec<String> {
        let styled = StyledText {
            text: text.to_string(),
            runs: Vec::new(),
        };
        let chars: Vec<char> = text.chars().collect();
        self.misspelled(egui::Id::unique("spell-test"), &styled)
            .iter()
            .map(|range| chars[range.clone()].iter().collect())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        words(text)
            .into_iter()
            .map(|range| chars[range].iter().collect())
            .collect()
    }

    #[test]
    fn words_keep_elisions_and_hyphens_and_skip_numbers_acronyms_and_addresses() {
        assert_eq!(
            texts("Hello, l'école d’été — rock-and-roll x2 2024 a NASA www.example.com me@mail.org https://exmaple.com/page end-"),
            ["Hello", "l'école", "d’été", "rock-and-roll", "end"]
        );
    }

    #[test]
    fn english_flags_typos_and_suggests_fixes() {
        let mut speller = Speller::ready("en_US");
        assert_eq!(
            speller.misspelled_words("Teh quick fox don't recieve well-known NASA mail 2024"),
            ["Teh", "recieve"]
        );
        assert!(speller
            .wait_for_suggestions("recieve")
            .contains(&"receive".to_string()));
    }

    #[test]
    fn french_accepts_elisions_and_both_apostrophes() {
        let mut speller = Speller::ready("fr");
        assert_eq!(
            speller.misspelled_words(
                "L'école est souvant fermée aujourd’hui, jusqu'à ce qu’il pleuve."
            ),
            ["souvant"]
        );
        assert_eq!(speller.misspelled_words("Je vais à l'ecole"), ["l'ecole"]);
    }

    #[test]
    fn personal_and_ignored_words_are_accepted_and_saved_once() {
        let mut speller = Speller::ready("en_US");
        assert_eq!(speller.misspelled_words("Folioz notes"), ["Folioz"]);
        speller.add_personal("Folioz");
        assert!(speller.misspelled_words("Folioz notes").is_empty());
        assert_eq!(speller.take_personal_change().as_deref(), Some("Folioz"));
        assert_eq!(speller.take_personal_change(), None);
        speller.ignore_all("blorft");
        assert!(speller.misspelled_words("blorft").is_empty());
        speller.clear_personal();
        assert_eq!(speller.misspelled_words("Folioz"), ["Folioz"]);
    }

    /// The built-in dictionaries plus system ones, without looking at this computer.
    fn listed(system: &[&str]) -> Vec<DictionaryInfo> {
        let mut list: Vec<DictionaryInfo> = available().into_iter().take(2).collect();
        for code in system {
            list.push(DictionaryInfo {
                code: code.to_string(),
                name: language_name(code),
                source: Source::Files {
                    aff: PathBuf::from(format!("/nowhere/{code}.aff")),
                    dic: PathBuf::from(format!("/nowhere/{code}.dic")),
                    encoding: Encoding::Utf8,
                },
            });
        }
        list
    }

    #[test]
    fn automatic_language_follows_the_system_locale_plus_english() {
        let dictionaries = listed(&["de_DE", "en_GB", "fr_CA"]);
        let codes = |picked: Vec<usize>| -> Vec<&str> {
            picked
                .into_iter()
                .map(|index| dictionaries[index].code.as_str())
                .collect()
        };
        assert_eq!(dictionaries[0].code, "en_US");
        assert_eq!(dictionaries[1].name, "Français");
        // French locales use the French Folio ships unless the region has its own.
        assert_eq!(codes(pick("auto", "fr_FR", &dictionaries)), ["fr", "en_US"]);
        assert_eq!(codes(pick("auto", "fr_BE", &dictionaries)), ["fr", "en_US"]);
        assert_eq!(
            codes(pick("auto", "fr_CA", &dictionaries)),
            ["fr_CA", "en_US"]
        );
        assert_eq!(
            codes(pick("auto", "de_AT", &dictionaries)),
            ["de_DE", "en_US"]
        );
        // English needs no second English.
        assert_eq!(codes(pick("auto", "en_GB", &dictionaries)), ["en_GB"]);
        assert_eq!(codes(pick("auto", "C", &dictionaries)), ["en_US"]);
        assert_eq!(codes(pick("auto", "", &dictionaries)), ["en_US"]);
        assert_eq!(codes(pick("", "", &dictionaries)), ["en_US"]);
        // A chosen language is used alone; a missing one falls back to automatic.
        assert_eq!(codes(pick("en_US", "fr_FR", &dictionaries)), ["en_US"]);
        assert_eq!(codes(pick("fr", "en_US", &dictionaries)), ["fr"]);
        assert_eq!(
            codes(pick("xx_YY", "fr_FR", &dictionaries)),
            ["fr", "en_US"]
        );
        assert!(pick("off", "fr_FR", &dictionaries).is_empty());
    }

    #[test]
    fn listed_languages_have_distinct_names() {
        let dictionaries = available();
        assert_eq!(dictionaries[0].code, "en_US");
        assert_eq!(dictionaries[1].code, "fr");
        let names: HashSet<&str> = dictionaries.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names.len(), dictionaries.len(), "{dictionaries:?}");
    }

    #[test]
    fn automatic_checks_english_and_the_system_language() {
        let mut speller = Speller::ready_in("auto", "fr_FR");
        assert_eq!(speller.active_codes(), ["fr", "en_US"]);
        assert_eq!(
            speller.misspelled_words(
                "What matters this week: three things. L'école est souvant fermée, we recieve"
            ),
            ["souvant", "recieve"]
        );
        // Other scripts are not judged, and nothing is one long misspelled run.
        assert!(speller
            .misspelled_words("Москва 東京の天気は晴れです Ελλάδα")
            .is_empty());
        // Suggestions follow the language of the words around the typo.
        let english = "Today we recieve the letters from the office";
        let at = english.find("recieve").unwrap();
        let language = speller.language_near(english, at..at + 7);
        assert_eq!(language, Some(1), "English is the second language");
        let suggested = speller.wait_for_suggestions_in("recieve", language);
        assert_eq!(suggested.first().map(String::as_str), Some("receive"));
        let french = "Je vais souvant à la plage avec mes amis";
        let at = french.chars().count() - "à la plage avec mes amis".chars().count() - 8;
        assert_eq!(speller.language_near(french, at..at + 7), Some(0));
        assert_eq!(
            speller.status(),
            "Checking spelling in Français and English (US)."
        );
    }

    #[test]
    fn the_status_reads_in_french() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        assert_eq!(
            Speller::default().status(),
            "L’orthographe n’est pas vérifiée."
        );
        let speller = Speller::ready_in("auto", "fr_FR");
        // The space before ":" is a no-break space.
        assert_eq!(
            speller.status(),
            "Vérification de l’orthographe\u{a0}: Français et English (US)."
        );
    }

    #[test]
    fn a_load_failure_reads_in_the_language_shown() {
        use crate::i18n::{pin_thread_lang, Lang};
        pin_thread_lang(Lang::En);
        let mut speller = Speller::default();
        // The loader stops without an answer.
        let (tx, rx) = mpsc::channel();
        drop(tx);
        speller.load = Load::Loading(rx);
        speller.poll(&egui::Context::default());
        let status = speller.status();
        assert!(
            status.ends_with(": the dictionary could not be read"),
            "{status}"
        );
        // Switching the interface language rewords it.
        pin_thread_lang(Lang::Fr);
        let status = speller.status();
        assert!(
            status.ends_with("\u{a0}: le dictionnaire n’a pas pu être lu"),
            "{status}"
        );
        pin_thread_lang(Lang::En);
    }

    #[test]
    fn join_names_lists_with_commas_and_and() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        assert_eq!(join_names(&[]), "");
        assert_eq!(join_names(&["A"]), "A");
        assert_eq!(join_names(&["A", "B"]), "A and B");
        assert_eq!(join_names(&["A", "B", "C"]), "A, B and C");
    }

    #[test]
    fn suggestions_merge_in_the_texts_language_first() {
        let lists = || {
            vec![
                vec!["a".to_string(), "b".into(), "c".into()],
                vec![
                    "x".to_string(),
                    "b".into(),
                    "y".into(),
                    "z".into(),
                    "w".into(),
                ],
            ]
        };
        assert_eq!(
            merge_suggestions(lists(), false),
            ["a", "b", "c", "x", "y", "z"]
        );
        assert_eq!(
            merge_suggestions(lists(), true),
            ["a", "x", "b", "c", "y", "z"]
        );
    }

    #[test]
    fn hyphenated_words_use_the_users_words_and_non_breaking_hyphens() {
        let mut speller = Speller::ready("en_US");
        assert!(speller
            .misspelled_words("well\u{2011}known mother\u{2011}in\u{2011}law well-known")
            .is_empty());
        assert_eq!(speller.misspelled_words("Folioz-based"), ["Folioz-based"]);
        speller.add_personal("Folioz");
        assert!(speller
            .misspelled_words("Folioz-based Folioz\u{2011}based")
            .is_empty());
        speller.ignore_all("blorft");
        assert!(speller.misspelled_words("blorft-free l'blorft").is_empty());
    }

    #[test]
    fn long_text_without_spaces_is_tokenized_in_one_pass() {
        // Words joined by commas, as in pasted data: no space anywhere.
        let line = "word,".repeat(20_000);
        let started = std::time::Instant::now();
        let found = words(&line);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "{:?}",
            started.elapsed()
        );
        assert!(
            found.is_empty(),
            "a 100 KB run without spaces is data, not prose"
        );
        // Prose stays checked however long it is.
        let prose = "word, ".repeat(20_000);
        let started = std::time::Instant::now();
        assert_eq!(words(&prose).len(), 20_000);
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(texts("www.Example.com WWW.example.org wwwhat"), ["wwwhat"]);
    }

    #[test]
    fn system_dictionaries_are_read_in_their_character_set() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let utf8 = file("a.aff", b"# comment\nSET UTF-8\nTRY abc\n");
        let latin9 = file("b.aff", b"SET ISO8859-15\n");
        let default = file("c.aff", b"TRY abc\n");
        let koi8 = file("d.aff", b"SET KOI8-R\n");
        assert_eq!(declared_encoding(&utf8), Some(Encoding::Utf8));
        assert_eq!(declared_encoding(&latin9), Some(Encoding::Latin9));
        assert_eq!(declared_encoding(&default), Some(Encoding::Latin1));
        assert_eq!(
            declared_encoding(&koi8),
            None,
            "not offered: every word would be flagged"
        );
        assert_eq!(
            decode(vec![0x63, 0x9C, 0xBD, 0x75, 0x72], Encoding::Latin9),
            "c\u{9c}œur"
        );
        assert_eq!(decode(vec![0xE9, 0x74, 0xE9], Encoding::Latin1), "été");
        assert_eq!(decode("été".as_bytes().to_vec(), Encoding::Utf8), "été");
    }

    #[test]
    fn code_runs_are_not_checked() {
        let mut speller = Speller::ready("en_US");
        let mut text = StyledText {
            text: "call fooBarz now".to_string(),
            runs: Vec::new(),
        };
        text.apply_style_chars(5..11, |style| style.code = true);
        assert!(speller
            .misspelled(egui::Id::unique("code"), &text)
            .is_empty());
    }
}
