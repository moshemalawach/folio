//! The interface language: which one is active, and every message in it.
//!
//! Messages live in Fluent files under `assets/locales/<code>/`, one file per area of
//! the app, built into the binary. Code looks them up with the `t!` macro below.

use std::borrow::Cow;
use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, OnceLock};

use eframe::egui::{Key, KeyboardShortcut, ModifierNames};
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::FluentResource;

pub(crate) use fluent_bundle::FluentArgs;

/// Looks up a message in the interface language: `t!("id")`, or with variables
/// `t!("id", count = n, name = text)`. Gives a `Cow<'static, str>`, borrowed (no
/// allocation) for a message without variables.
macro_rules! t {
    ($id:literal) => {
        $crate::i18n::tr($id)
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = $crate::i18n::FluentArgs::new();
        $(args.set(stringify!($name), $value);)+
        $crate::i18n::tr_args($id, &args)
    }};
}
pub(crate) use t;

/// An interface language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Fr];

    /// The code kept in settings and read from `FOLIO_LANG`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    /// Reads "en" or "fr", or a locale such as "fr_FR.UTF-8", "fr-CA" or "en-US".
    pub fn from_code(code: &str) -> Option<Lang> {
        let language = code
            .split(['_', '-', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match language.as_str() {
            "en" => Some(Lang::En),
            "fr" => Some(Lang::Fr),
            _ => None,
        }
    }

    /// The language's own name, written the same in every interface language.
    pub fn endonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
        }
    }
}

static ACTIVE: AtomicU8 = AtomicU8::new(Lang::En as u8);

thread_local! {
    /// A test pins its own thread's language, so tests running in parallel keep the
    /// English default.
    static PINNED: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// The active interface language.
pub fn lang() -> Lang {
    PINNED
        .with(Cell::get)
        .unwrap_or_else(|| match ACTIVE.load(Ordering::Relaxed) {
            1 => Lang::Fr,
            _ => Lang::En,
        })
}

/// Switches the interface language (only this thread's, on a test thread that pinned one).
pub fn set_lang(lang: Lang) {
    let pinned = PINNED.with(|pinned| {
        let is_pinned = pinned.get().is_some();
        if is_pinned {
            pinned.set(Some(lang));
        }
        is_pinned
    });
    if !pinned {
        ACTIVE.store(lang as u8, Ordering::Relaxed);
    }
}

/// Gives this thread its own language, leaving every other thread's alone.
#[cfg(test)]
pub fn pin_thread_lang(lang: Lang) {
    PINNED.with(|pinned| pinned.set(Some(lang)));
}

/// The system's language when Folio speaks it, English otherwise. Read once: the
/// Settings dialog asks every frame, and on macOS the lookup goes through CoreFoundation.
pub fn system_lang() -> Lang {
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(|| {
        sys_locale::get_locale()
            .and_then(|locale| Lang::from_code(&locale))
            .unwrap_or(Lang::En)
    })
}

/// The language to start in: `FOLIO_LANG`, then the saved setting, then the system's.
pub fn startup_lang(setting: Option<&str>) -> Lang {
    std::env::var("FOLIO_LANG")
        .ok()
        .and_then(|code| Lang::from_code(&code))
        .or_else(|| setting.and_then(Lang::from_code))
        .unwrap_or_else(system_lang)
}

type Bundle = FluentBundle<FluentResource>;

/// Each language's messages, one file per area of the app.
const AREAS: [&str; 7] = [
    "common", "app", "canvas", "editor", "misc", "data", "ribbon",
];

const SOURCES: [[&str; 7]; 2] = [
    [
        include_str!("../assets/locales/en/common.ftl"),
        include_str!("../assets/locales/en/app.ftl"),
        include_str!("../assets/locales/en/canvas.ftl"),
        include_str!("../assets/locales/en/editor.ftl"),
        include_str!("../assets/locales/en/misc.ftl"),
        include_str!("../assets/locales/en/data.ftl"),
        include_str!("../assets/locales/en/ribbon.ftl"),
    ],
    [
        include_str!("../assets/locales/fr/common.ftl"),
        include_str!("../assets/locales/fr/app.ftl"),
        include_str!("../assets/locales/fr/canvas.ftl"),
        include_str!("../assets/locales/fr/editor.ftl"),
        include_str!("../assets/locales/fr/misc.ftl"),
        include_str!("../assets/locales/fr/data.ftl"),
        include_str!("../assets/locales/fr/ribbon.ftl"),
    ],
];

static BUNDLES: LazyLock<[Bundle; 2]> = LazyLock::new(|| Lang::ALL.map(|lang| build(lang).0));

/// One language's bundle, with every problem met while reading its files.
fn build(lang: Lang) -> (Bundle, Vec<String>) {
    let mut bundle =
        FluentBundle::new_concurrent(vec![lang.code().parse().expect("a valid language id")]);
    // egui would draw the bidi isolation marks Fluent puts around each variable.
    bundle.set_use_isolating(false);
    let mut problems = Vec::new();
    for (area, source) in AREAS.iter().zip(SOURCES[lang as usize]) {
        let resource =
            FluentResource::try_new(source.to_string()).unwrap_or_else(|(resource, errors)| {
                problems.extend(
                    errors
                        .iter()
                        .map(|e| format!("{}/{area}.ftl: {e:?}", lang.code())),
                );
                resource
            });
        if let Err(errors) = bundle.add_resource(resource) {
            problems.extend(
                errors
                    .iter()
                    .map(|e| format!("{}/{area}.ftl: {e:?}", lang.code())),
            );
        }
    }
    (bundle, problems)
}

/// A message without variables, in the interface language.
pub fn tr(id: &str) -> Cow<'static, str> {
    lookup(lang(), id, None)
}

/// A message with variables, in the interface language.
pub fn tr_args(id: &str, args: &FluentArgs) -> Cow<'static, str> {
    lookup(lang(), id, Some(args))
}

/// `value` with `digits` decimals and the language's decimal separator ("1.5", "1,5").
pub fn decimal(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let separator = tr("number-decimal-separator");
    if separator == "." {
        text
    } else {
        text.replace('.', &separator)
    }
}

/// A message without variables in a given language, whatever the interface speaks
/// (to measure a label in every language).
#[allow(dead_code)] // L4 measures toolbar labels with it
pub fn tr_in(lang: Lang, id: &str) -> Cow<'static, str> {
    lookup(lang, id, None)
}

/// A plain one-line message as a `&'static str`: such messages are borrowed straight
/// from the bundle. `fallback` covers a message that would ever need building.
fn tr_static(id: &str, fallback: &'static str) -> &'static str {
    match tr(id) {
        Cow::Borrowed(text) => text,
        Cow::Owned(_) => fallback,
    }
}

/// Modifier names in the interface language ("Ctrl", "Maj", "Alt").
fn named_modifiers() -> ModifierNames<'static> {
    ModifierNames {
        is_short: false,
        alt: tr_static("key-alt", "Alt"),
        ctrl: tr_static("key-ctrl", "Ctrl"),
        shift: tr_static("key-shift", "Shift"),
        mac_cmd: "Cmd",
        mac_alt: "Option",
        concat: "+",
    }
}

/// How shortcuts name their modifiers: egui's symbols on macOS (⌘ ⇧ ⌥), the
/// interface language's names elsewhere (French: Ctrl, Maj, Alt).
pub fn modifier_names() -> ModifierNames<'static> {
    if cfg!(target_os = "macos") {
        ModifierNames::SYMBOLS
    } else {
        named_modifiers()
    }
}

/// A key's name in the interface language ("Entrée"); punctuation and arrows keep
/// egui's symbols.
pub fn key_name(key: Key) -> Cow<'static, str> {
    match key {
        Key::Enter => t!("key-enter"),
        Key::Escape => t!("key-esc"),
        Key::Delete => t!("key-del"),
        Key::Space => t!("key-space"),
        Key::Tab => t!("key-tab"),
        key => Cow::Borrowed(key.symbol_or_name()),
    }
}

/// A shortcut as menus show it: "Ctrl+Shift+=" (English), "Ctrl+Maj+=" (French),
/// "⇧⌘=" on a Mac, whatever the language.
pub fn shortcut_text(shortcut: &KeyboardShortcut, is_mac: bool) -> String {
    if is_mac {
        return shortcut.format(&ModifierNames::SYMBOLS, true);
    }
    let names = named_modifiers();
    let modifiers = names.format(&shortcut.modifiers, false);
    let key = key_name(shortcut.logical_key);
    if modifiers.is_empty() {
        key.into_owned()
    } else {
        format!("{modifiers}{}{key}", names.concat)
    }
}

fn lookup(lang: Lang, id: &str, args: Option<&FluentArgs>) -> Cow<'static, str> {
    let bundles: &'static [Bundle; 2] = &BUNDLES;
    lookup_in(bundles, lang, id, args)
}

fn lookup_in<'a>(
    bundles: &'a [Bundle; 2],
    lang: Lang,
    id: &str,
    args: Option<&FluentArgs>,
) -> Cow<'a, str> {
    // A message missing in that language falls back to English.
    for language in [lang, Lang::En] {
        let bundle = &bundles[language as usize];
        let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) else {
            continue;
        };
        let mut errors = Vec::new();
        let text = bundle.format_pattern(pattern, args, &mut errors);
        debug_assert!(errors.is_empty(), "message {id}: {errors:?}");
        return text;
    }
    if cfg!(debug_assertions) {
        panic!("no message {id}");
    }
    Cow::Owned(id.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use eframe::egui::Modifiers;
    use fluent_syntax::ast;

    /// Ids looked up through tables, which the scan of `t!("…")` calls below cannot see:
    /// util.rs's months and weekdays, canvas.rs's pen colours (`PEN_COLORS`), model.rs's
    /// highlight colours (`Rgb::TEXT_HIGHLIGHTS`), math.rs's ready-made equations
    /// (`READY_EQUATIONS`) and editor.rs's symbol sets (`SYMBOL_SETS`).
    const DYNAMIC: [&str; 6] = [
        "month-",
        "weekday-",
        "ribbon-color-",
        "color-",
        "equation-",
        "symbols-",
    ];

    /// Every message of a language, with the variables it uses.
    fn messages(lang: Lang) -> BTreeMap<String, BTreeSet<String>> {
        let mut out = BTreeMap::new();
        for source in SOURCES[lang as usize] {
            let resource = fluent_syntax::parser::parse(source)
                .unwrap_or_else(|(_, errors)| panic!("{}: {errors:?}", lang.code()));
            for entry in &resource.body {
                if let ast::Entry::Message(message) = entry {
                    let mut vars = BTreeSet::new();
                    if let Some(value) = &message.value {
                        pattern_vars(value, &mut vars);
                    }
                    for attribute in &message.attributes {
                        pattern_vars(&attribute.value, &mut vars);
                    }
                    out.insert(message.id.name.to_string(), vars);
                }
            }
        }
        out
    }

    fn pattern_vars(pattern: &ast::Pattern<&str>, out: &mut BTreeSet<String>) {
        for element in &pattern.elements {
            if let ast::PatternElement::Placeable { expression } = element {
                expression_vars(expression, out);
            }
        }
    }

    fn expression_vars(expression: &ast::Expression<&str>, out: &mut BTreeSet<String>) {
        match expression {
            ast::Expression::Select { selector, variants } => {
                inline_vars(selector, out);
                for variant in variants {
                    pattern_vars(&variant.value, out);
                }
            }
            ast::Expression::Inline(inline) => inline_vars(inline, out),
        }
    }

    fn inline_vars(inline: &ast::InlineExpression<&str>, out: &mut BTreeSet<String>) {
        match inline {
            ast::InlineExpression::VariableReference { id } => {
                out.insert(id.name.to_string());
            }
            ast::InlineExpression::Placeable { expression } => expression_vars(expression, out),
            ast::InlineExpression::FunctionReference { arguments, .. } => {
                for argument in &arguments.positional {
                    inline_vars(argument, out);
                }
                for argument in &arguments.named {
                    inline_vars(&argument.value, out);
                }
            }
            _ => {}
        }
    }

    /// Message ids written as literals in the code: `t!("id"…)`, `tr("id")`,
    /// `tr_args("id", …)`, `tr_in(lang, "id")` and `tr_static("id", …)`. Comment
    /// lines are skipped, and so are markers inside strings (this scanner's own list).
    fn ids_in_code() -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let code: String = std::fs::read_to_string(&path)
                    .unwrap()
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                for marker in ["t!(", "tr(", "tr_args(", "tr_in(", "tr_static("] {
                    for (at, _) in code.match_indices(marker) {
                        let before = code[..at].chars().next_back();
                        if before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '"') {
                            continue;
                        }
                        let mut rest = &code[at + marker.len()..];
                        // `tr_in` takes the language first.
                        if marker == "tr_in(" {
                            let Some(comma) = rest.find(',') else {
                                continue;
                            };
                            rest = &rest[comma + 1..];
                        }
                        let rest = rest.trim_start();
                        if let Some(rest) = rest.strip_prefix('"') {
                            if let Some(end) = rest.find('"') {
                                ids.insert(rest[..end].to_string());
                            }
                        }
                    }
                }
            }
        }
        ids
    }

    /// The text of a language's files, without comment lines.
    fn values(lang: Lang) -> Vec<String> {
        SOURCES[lang as usize]
            .iter()
            .flat_map(|source| source.lines())
            .filter(|line| !line.trim_start().starts_with('#'))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn every_language_loads_without_errors() {
        for lang in Lang::ALL {
            let (_, problems) = build(lang);
            assert!(problems.is_empty(), "{problems:#?}");
        }
    }

    #[test]
    fn every_message_exists_in_every_language_with_the_same_variables() {
        let english = messages(Lang::En);
        for lang in Lang::ALL {
            let other = messages(lang);
            let missing: Vec<_> = english
                .keys()
                .filter(|id| !other.contains_key(*id))
                .collect();
            let extra: Vec<_> = other
                .keys()
                .filter(|id| !english.contains_key(*id))
                .collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{}: missing {missing:?}, extra {extra:?}",
                lang.code()
            );
            for (id, vars) in &english {
                assert_eq!(&other[id], vars, "{}: the variables of {id}", lang.code());
            }
        }
    }

    #[test]
    fn every_message_the_code_names_exists() {
        let english = messages(Lang::En);
        let unknown: Vec<_> = ids_in_code()
            .into_iter()
            .filter(|id| !english.contains_key(id))
            .collect();
        assert!(
            unknown.is_empty(),
            "ids with no English message: {unknown:?}"
        );
    }

    #[test]
    fn every_message_is_used() {
        let used = ids_in_code();
        let unused: Vec<_> = messages(Lang::En)
            .into_keys()
            .filter(|id| !used.contains(id) && !DYNAMIC.iter().any(|p| id.starts_with(p)))
            .collect();
        assert!(unused.is_empty(), "messages no code uses: {unused:?}");
    }

    /// French typography problems in one line of a French file.
    pub(crate) fn typography_problems(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        let mut problems = Vec::new();
        for (at, &ch) in chars.iter().enumerate() {
            let before = at.checked_sub(1).map(|at| chars[at]);
            let after = chars.get(at + 1).copied();
            let ok = match ch {
                // A time: "11:09".
                ':' if before.is_some_and(|c| c.is_ascii_digit())
                    && after.is_some_and(|c| c.is_ascii_digit()) =>
                {
                    true
                }
                // A URL scheme: "https://".
                ':' if after == Some('/') && chars.get(at + 2) == Some(&'/') => true,
                ':' | ';' | '!' | '?' | '»' | '%' => before == Some('\u{a0}'),
                '«' => after == Some('\u{a0}'),
                _ => true,
            };
            if !ok {
                problems.push(format!("{ch:?} in {line:?}"));
            }
        }
        for bad in ["'", "..."] {
            if line.contains(bad) {
                problems.push(format!("{bad:?} in {line:?}"));
            }
        }
        problems
    }

    #[test]
    fn french_follows_french_typography() {
        let problems: Vec<String> = values(Lang::Fr)
            .iter()
            .flat_map(|line| typography_problems(line))
            .collect();
        assert!(
            problems.is_empty(),
            "use a no-break space (U+00A0) before : ; ! ? » % and after «, ’ and …: {problems:#?}"
        );
    }

    #[test]
    fn the_typography_check_catches_a_missing_space_too() {
        for bad in [
            "a = Langue: { $error }",
            "a = Langue : { $error }",
            "a = { $value }%",
            "a = Vraiment?",
            "a = «\u{a0}citation»",
            "a = « citation\u{a0}»",
            "a = l'heure",
            "a = Patientez...",
        ] {
            assert!(!typography_problems(bad).is_empty(), "{bad:?} passed");
        }
        for good in [
            "a = Langue\u{a0}: { $error }",
            "a = { $value }\u{a0}%",
            "a = Vraiment\u{a0}?",
            "a = «\u{a0}citation\u{a0}»",
            "a = { $day } { $month } · 11:09",
            "a = l’heure…",
            "a = https://… ou choisissez une page",
        ] {
            assert_eq!(typography_problems(good), Vec::<String>::new(), "{good:?}");
        }
    }

    #[test]
    fn messages_only_use_characters_the_fonts_have() {
        // Add a character here only after checking the bundled fonts draw it.
        let allowed = |ch: char| {
            matches!(ch, ' '..='~' | '\u{a0}'..='\u{ff}')
                || "œŒŸ‘’“”…–—€•↑↓←→↵⌘⇧⌥⌃".contains(ch)
                || ('\u{e000}'..='\u{f8ff}').contains(&ch)
        };
        for lang in Lang::ALL {
            let odd: Vec<String> = values(lang)
                .into_iter()
                .filter(|line| !line.chars().all(allowed))
                .collect();
            assert!(odd.is_empty(), "{}: {odd:#?}", lang.code());
        }
    }

    #[test]
    fn plain_messages_are_borrowed() {
        pin_thread_lang(Lang::Fr);
        assert_eq!(tr("common-untitled"), "Page sans titre");
        assert!(matches!(tr("common-untitled"), Cow::Borrowed(_)));
        pin_thread_lang(Lang::En);
        assert_eq!(tr("common-untitled"), "Untitled page");
    }

    #[test]
    fn a_message_missing_in_french_falls_back_to_english() {
        let bundle = |code: &str, source: &str| {
            let mut bundle = FluentBundle::new_concurrent(vec![code.parse().unwrap()]);
            bundle.set_use_isolating(false);
            bundle
                .add_resource(FluentResource::try_new(source.to_string()).unwrap())
                .unwrap();
            bundle
        };
        let bundles = [
            bundle("en", "both = Both\nenglish-only = Only { $what }\n"),
            bundle("fr", "both = Les deux\n"),
        ];
        assert_eq!(lookup_in(&bundles, Lang::Fr, "both", None), "Les deux");
        let mut args = FluentArgs::new();
        args.set("what", "English");
        assert_eq!(
            lookup_in(&bundles, Lang::Fr, "english-only", Some(&args)),
            "Only English"
        );
        assert_eq!(lookup_in(&bundles, Lang::En, "both", None), "Both");
    }

    #[test]
    fn a_message_can_be_read_in_any_language() {
        // Whatever this thread speaks, tr_in picks the language asked for.
        pin_thread_lang(Lang::Fr);
        assert_eq!(tr_in(Lang::En, "key-shift"), "Shift");
        assert_eq!(tr_in(Lang::Fr, "key-shift"), "Maj");
        pin_thread_lang(Lang::En);
        assert_eq!(tr_in(Lang::Fr, "common-untitled"), "Page sans titre");
        assert!(matches!(
            tr_in(Lang::Fr, "common-untitled"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn french_counts_zero_as_one() {
        let mut bundle = FluentBundle::new_concurrent(vec!["fr".parse().unwrap()]);
        let source = "n = { $count ->\n    [one] one\n   *[other] other\n}\n";
        bundle
            .add_resource(FluentResource::try_new(source.to_string()).unwrap())
            .unwrap();
        let pattern = bundle.get_message("n").unwrap().value().unwrap();
        for (count, form) in [(0, "one"), (1, "one"), (2, "other")] {
            let mut args = FluentArgs::new();
            args.set("count", count);
            let mut errors = Vec::new();
            assert_eq!(
                bundle.format_pattern(pattern, Some(&args), &mut errors),
                form
            );
        }
    }

    #[test]
    fn language_codes_read_locales() {
        assert_eq!(Lang::from_code("fr_FR.UTF-8"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("fr-CA"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("en_US"), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(Lang::from_code(""), None);
    }

    #[test]
    fn numbers_and_shortcuts_follow_the_interface_language() {
        let superscript = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Equals);
        let enter = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);
        pin_thread_lang(Lang::En);
        assert_eq!(decimal(1.25, 1), "1.2");
        assert_eq!(shortcut_text(&superscript, false), "Ctrl+Shift+=");
        assert_eq!(shortcut_text(&enter, false), "Ctrl+Enter");
        assert_eq!(shortcut_text(&superscript, true), "⇧⌘=");
        pin_thread_lang(Lang::Fr);
        assert_eq!(decimal(1.5, 1), "1,5");
        assert_eq!(shortcut_text(&superscript, false), "Ctrl+Maj+=");
        assert_eq!(shortcut_text(&enter, false), "Ctrl+Entrée");
        assert_eq!(shortcut_text(&superscript, true), "⇧⌘=");
        assert_eq!(key_name(Key::Escape), "Échap");
        if !cfg!(target_os = "macos") {
            let names = modifier_names();
            assert_eq!((names.ctrl, names.alt, names.shift), ("Ctrl", "Alt", "Maj"));
        }
    }

    #[test]
    fn shared_labels_follow_the_interface_language() {
        use crate::model::{display_title, BlockKind, LineTag, RuleLines, RuleSize};
        pin_thread_lang(Lang::Fr);
        assert_eq!(display_title("  "), "Page sans titre");
        assert_eq!(display_title("Notes"), "Notes");
        assert_eq!(
            LineTag::RememberForLater.label(),
            "À retenir pour plus tard"
        );
        assert_eq!(
            RuleLines::Lines(RuleSize::College).label(),
            "Lignes moyennes"
        );
        assert_eq!(BlockKind::Heading { level: 2 }.label(), "Titre 1");
        assert_eq!(BlockKind::PAGE_TITLE.label(), "Titre de la page");
        assert_eq!(BlockKind::Citation.label(), "Référence");
        pin_thread_lang(Lang::En);
        assert_eq!(BlockKind::Heading { level: 3 }.label(), "Heading 2");
        assert_eq!(BlockKind::Paragraph.label(), "Normal");
    }
}
