use std::borrow::Cow;
use std::ops::Range;

use chrono::{DateTime, Datelike, Local, TimeZone};

use crate::i18n::{self, t};

/// Message ids of the month and weekday names, so the page list builds no id per row.
const MONTHS: [&str; 12] = [
    "month-1", "month-2", "month-3", "month-4", "month-5", "month-6", "month-7", "month-8",
    "month-9", "month-10", "month-11", "month-12",
];
const SHORT_MONTHS: [&str; 12] = [
    "month-short-1",
    "month-short-2",
    "month-short-3",
    "month-short-4",
    "month-short-5",
    "month-short-6",
    "month-short-7",
    "month-short-8",
    "month-short-9",
    "month-short-10",
    "month-short-11",
    "month-short-12",
];
const WEEKDAYS: [&str; 7] = [
    "weekday-1",
    "weekday-2",
    "weekday-3",
    "weekday-4",
    "weekday-5",
    "weekday-6",
    "weekday-7",
];

/// The calendar's weekday initials, Monday first like `WEEKDAYS`.
const WEEKDAY_INITIALS: [&str; 7] = [
    "weekday-initial-1",
    "weekday-initial-2",
    "weekday-initial-3",
    "weekday-initial-4",
    "weekday-initial-5",
    "weekday-initial-6",
    "weekday-initial-7",
];

/// `month` counts from 1, as chrono gives it.
fn month_name(month: u32) -> Cow<'static, str> {
    i18n::tr(MONTHS[month as usize - 1])
}

fn short_month_name(month: u32) -> Cow<'static, str> {
    i18n::tr(SHORT_MONTHS[month as usize - 1])
}

/// How long ago a timestamp was, in the interface language. A sentence may need to tell
/// the two forms apart: French says "Modifié il y a 5 min" but "Modifié le 23 oct.".
#[derive(Debug, PartialEq, Eq)]
pub enum RelativeTime {
    /// "Just now", "5 min ago", "Yesterday", "3 days ago".
    Ago(String),
    /// A week or more back, a date: "Oct 23", or "Oct 23, 2025" in another year.
    Date(String),
}

pub fn relative_time(timestamp_ms: i64) -> String {
    match relative_time_of(timestamp_ms) {
        RelativeTime::Ago(text) | RelativeTime::Date(text) => text,
    }
}

pub fn relative_time_of(timestamp_ms: i64) -> RelativeTime {
    let now = Local::now().timestamp_millis();
    let delta = (now - timestamp_ms).max(0);
    let minutes = delta / 60_000;
    if minutes < 1 {
        return RelativeTime::Ago(t!("time-just-now").into_owned());
    }
    if minutes < 60 {
        return RelativeTime::Ago(t!("time-minutes-ago", count = minutes).into_owned());
    }
    let hours = minutes / 60;
    if hours < 24 {
        return RelativeTime::Ago(t!("time-hours-ago", count = hours).into_owned());
    }
    let Some(dt) = Local.timestamp_millis_opt(timestamp_ms).single() else {
        return RelativeTime::Ago(String::new());
    };
    let today = Local::now().date_naive();
    let date = dt.date_naive();
    let days = (today - date).num_days();
    if days == 1 {
        RelativeTime::Ago(t!("time-yesterday").into_owned())
    } else if days < 7 {
        RelativeTime::Ago(t!("time-days-ago", count = days).into_owned())
    } else if date.year() == today.year() {
        RelativeTime::Date(
            t!(
                "date-short",
                month = short_month_name(dt.month()),
                day = dt.day()
            )
            .into_owned(),
        )
    } else {
        RelativeTime::Date(
            t!(
                "date-short-year",
                month = short_month_name(dt.month()),
                day = dt.day(),
                year = dt.year()
            )
            .into_owned(),
        )
    }
}

/// A moment in full, as the Versions pane shows it: the day, then the time as the page
/// header writes it ("October 1, 2026 · 11:09 AM", "1er octobre 2026 · 11:09").
pub fn full_date(timestamp_ms: i64) -> String {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|dt| {
            t!(
                "date-full",
                month = month_name(dt.month()),
                day = dt.day(),
                year = dt.year(),
                time = clock_time(dt)
            )
            .into_owned()
        })
        .unwrap_or_default()
}

/// A day written out in full, as the page header and the Meeting Notes template show it
/// ("Thursday, October 1, 2026", "jeudi 1er octobre 2026").
pub fn long_date(dt: DateTime<Local>) -> String {
    let weekday = i18n::tr(WEEKDAYS[dt.weekday().num_days_from_monday() as usize]);
    t!(
        "date-long",
        weekday = weekday,
        month = month_name(dt.month()),
        day = dt.day(),
        year = dt.year()
    )
    .into_owned()
}

/// A time of day as the page header shows it: "5:05 PM" in English, "17:05" in French.
pub fn clock_time(dt: DateTime<Local>) -> String {
    match i18n::lang() {
        i18n::Lang::En => dt.format("%-I:%M %p").to_string(),
        _ => dt.format("%H:%M").to_string(),
    }
}

/// A day as the desktop notebook's date stamps write it into notes (Insert › Date, Alt+Shift+D): the
/// short numeric date, "10/1/2026" in English, "01/10/2026" in French.
pub fn short_date(dt: DateTime<Local>) -> String {
    match i18n::lang() {
        i18n::Lang::En => dt.format("%-m/%-d/%Y").to_string(),
        _ => dt.format("%d/%m/%Y").to_string(),
    }
}

/// The time of day as Folio writes it into notes (Alt+Shift+T): 24-hour, "18:41".
pub fn time_of_day(dt: DateTime<Local>) -> String {
    dt.format("%H:%M").to_string()
}

/// A date and a time, as Alt+Shift+F writes them: "10/1/2026 11:09".
pub fn date_and_time(dt: DateTime<Local>) -> String {
    t!(
        "date-and-time",
        date = short_date(dt),
        time = time_of_day(dt)
    )
    .into_owned()
}

/// A calendar's heading: "October 2026", "octobre 2026".
pub fn month_year(year: i32, month: u32) -> String {
    t!("date-month-year", month = month_name(month), year = year).into_owned()
}

/// Whether a calendar week starts on Monday (French) or Sunday (English, as US Windows
/// shows it).
pub fn week_starts_monday() -> bool {
    !matches!(i18n::lang(), i18n::Lang::En)
}

/// The calendar's weekday initials in display order.
pub fn weekday_initials() -> [Cow<'static, str>; 7] {
    let first = if week_starts_monday() { 0 } else { 6 };
    std::array::from_fn(|column| i18n::tr(WEEKDAY_INITIALS[(first + column) % 7]))
}

/// `timestamp_ms` moved to another day, keeping its local time of day.
pub fn with_date(timestamp_ms: i64, date: chrono::NaiveDate) -> Option<i64> {
    let local = Local.timestamp_millis_opt(timestamp_ms).single()?;
    Local
        .from_local_datetime(&date.and_time(local.time()))
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

/// `timestamp_ms` at another time of day, on the same local day.
pub fn with_time(timestamp_ms: i64, hour: u32, minute: u32) -> Option<i64> {
    let local = Local.timestamp_millis_opt(timestamp_ms).single()?;
    let time = chrono::NaiveTime::from_hms_opt(hour, minute, 0)?;
    Local
        .from_local_datetime(&local.date_naive().and_time(time))
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

pub fn file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    let text = if bytes < 1024 {
        t!("unit-bytes", size = bytes)
    } else if value < KB * KB {
        t!("unit-kilobytes", size = i18n::decimal(value / KB, 1))
    } else if value < KB * KB * KB {
        t!("unit-megabytes", size = i18n::decimal(value / (KB * KB), 1))
    } else {
        t!(
            "unit-gigabytes",
            size = i18n::decimal(value / (KB * KB * KB), 2)
        )
    };
    text.into_owned()
}

pub fn truncate(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Every place `needle` occurs in `haystack`, ignoring case, as char ranges of
/// `haystack`, first to last and never overlapping. Lowercasing can change lengths
/// ("İ" becomes two chars), so positions are mapped back char by char.
pub fn find_all_folded(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut lower = String::with_capacity(haystack.len());
    // For each char of `lower`, the index of the `haystack` char it came from.
    let mut origin = Vec::with_capacity(haystack.len());
    for (index, ch) in haystack.chars().enumerate() {
        for folded in ch.to_lowercase() {
            lower.push(folded);
            origin.push(index);
        }
    }
    let needle_chars = needle.chars().count();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let (mut from_byte, mut from_char) = (0, 0);
    while let Some(offset) = lower[from_byte..].find(&needle) {
        let start_byte = from_byte + offset;
        let start = from_char + lower[from_byte..start_byte].chars().count();
        let end = start + needle_chars;
        let range = origin[start]..origin[end - 1] + 1;
        if ranges.last().is_none_or(|last| range.start >= last.end) {
            ranges.push(range);
        }
        from_byte = start_byte + needle.len();
        from_char = end;
    }
    ranges
}

/// Byte ranges of `text` for char ranges given first to last (as `find_all_folded`
/// returns them), found in one pass over the text. Positions past the end stop there.
pub fn byte_ranges<'a>(
    text: &str,
    ranges: impl IntoIterator<Item = &'a Range<usize>>,
) -> Vec<Range<usize>> {
    let mut offsets = text
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()));
    // `byte` is where char number `char` starts.
    let (mut char, mut byte) = (0, offsets.next().unwrap_or(text.len()));
    let mut byte_of = |target: usize| {
        while char < target {
            let Some(at) = offsets.next() else {
                break;
            };
            char += 1;
            byte = at;
        }
        byte
    };
    ranges
        .into_iter()
        .map(|range| {
            let start = byte_of(range.start);
            start..byte_of(range.end).max(start)
        })
        .collect()
}

/// A shortcut as a menu row shows it, the desktop notebook's way and as the text menu writes it:
/// "Ctrl+X", "Ctrl+Alt+M" (French "Ctrl+Maj+N"), "⌘X" on macOS.
pub fn menu_shortcut(modifiers: egui::Modifiers, key: egui::Key) -> String {
    i18n::shortcut_text(
        &egui::KeyboardShortcut::new(modifiers, key),
        cfg!(target_os = "macos"),
    )
}

/// Consumes a press of `key` held with exactly `modifiers`. Unlike egui's `consume_key`, an
/// extra Shift or Alt does not match, so Ctrl+Tab and Ctrl+Shift+Tab, or Alt+↑ and
/// Alt+Shift+↑, stay apart.
pub fn consume_exact(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input_mut(|input| {
        let before = input.events.len();
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key { key: pressed, pressed: true, modifiers: held, .. }
                    if *pressed == key && held.matches_exact(modifiers)
            )
        });
        input.events.len() != before
    })
}

pub fn shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘{key}")
    } else {
        format!("{} {key}", i18n::modifier_names().ctrl)
    }
}

pub fn shift_shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘⇧{key}")
    } else {
        let names = i18n::modifier_names();
        format!("{} {} {key}", names.ctrl, names.shift)
    }
}

pub fn alt_shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘⌥{key}")
    } else {
        let names = i18n::modifier_names();
        format!("{} {} {key}", names.ctrl, names.alt)
    }
}

/// Shift and a key as the platform writes it: "⇧Tab" on macOS, "Shift Tab" elsewhere.
pub fn shift_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⇧{key}")
    } else {
        format!("{} {key}", i18n::modifier_names().shift)
    }
}

/// The command key with Alt and Shift: "⌘⌥⇧N" on macOS, "Ctrl Alt Shift N" elsewhere.
pub fn alt_shift_shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌘⌥⇧{key}")
    } else {
        let names = i18n::modifier_names();
        format!("{} {} {} {key}", names.ctrl, names.alt, names.shift)
    }
}

/// Alt and Shift with a key, without the command key: "⌥⇧↑" on macOS, "Alt Shift ↑" elsewhere.
pub fn alt_shift_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("⌥⇧{key}")
    } else {
        let names = i18n::modifier_names();
        format!("{} {} {key}", names.alt, names.shift)
    }
}

pub fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A name as a file name: its letters and digits in lower case, the words joined by
/// "-" ("Week 1: Plans" → "week-1-plans"). Empty when the name has no letter or digit.
pub fn slugify(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// `slugify` cut to at most `FILE_SLUG_CHARS` characters (at a word break when there is
/// one), so a long title still makes a file name every file system takes.
pub fn file_slug(title: &str) -> String {
    const FILE_SLUG_CHARS: usize = 60;
    let slug = slugify(title);
    let Some((cut, _)) = slug.char_indices().nth(FILE_SLUG_CHARS) else {
        return slug;
    };
    let short = &slug[..cut];
    match short.rfind('-') {
        Some(word) if word > 0 => short[..word].to_string(),
        _ => short.to_string(),
    }
}

/// A whole web page titled `title` (escaped here) around `body` (already HTML), in the
/// interface's language, as the exports write them.
pub fn html_document(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"{}\"><head><meta charset=\"utf-8\"><title>{}</title></head><body>{body}</body></html>",
        i18n::lang().code(),
        html_escape(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{pin_thread_lang, Lang};
    use chrono::Timelike;

    #[test]
    fn dates_sizes_and_times_follow_the_interface_language() {
        let first = Local
            .with_ymd_and_hms(2026, 10, 1, 11, 9, 0)
            .single()
            .unwrap();
        let ms = first.timestamp_millis();
        let now = Local::now().timestamp_millis();
        pin_thread_lang(Lang::En);
        assert_eq!(full_date(ms), "October 1, 2026 · 11:09 AM");
        assert_eq!(long_date(first), "Thursday, October 1, 2026");
        assert_eq!(clock_time(first), "11:09 AM");
        // The date stamps put into notes are the desktop notebook's short dates.
        assert_eq!(short_date(first), "10/1/2026");
        assert_eq!(date_and_time(first), "10/1/2026 11:09");
        assert_eq!(file_size(512), "512 B");
        assert_eq!(file_size(1536), "1.5 KB");
        assert_eq!(relative_time(now - 5 * 60_000), "5 min ago");
        assert_eq!(relative_time(now - 3 * 3_600_000), "3 h ago");
        pin_thread_lang(Lang::Fr);
        assert_eq!(full_date(ms), "1er octobre 2026 · 11:09");
        assert_eq!(long_date(first), "jeudi 1er octobre 2026");
        assert_eq!(clock_time(first), "11:09");
        assert_eq!(short_date(first), "01/10/2026");
        assert_eq!(date_and_time(first), "01/10/2026 11:09");
        assert_eq!(file_size(1536), "1,5\u{a0}Ko");
        assert_eq!(file_size(3 * 1024 * 1024 * 1024), "3,00\u{a0}Go");
        assert_eq!(relative_time(now), "À l’instant");
        assert_eq!(relative_time(now - 5 * 60_000), "Il y a 5\u{a0}min");
        if !cfg!(target_os = "macos") {
            assert_eq!(shift_shortcut("N"), "Ctrl Maj N");
            use egui::{Key, Modifiers};
            assert_eq!(
                menu_shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::N),
                "Ctrl+Maj+N"
            );
            assert_eq!(menu_shortcut(Modifiers::NONE, Key::Delete), "Suppr");
            pin_thread_lang(Lang::En);
            assert_eq!(menu_shortcut(Modifiers::COMMAND, Key::X), "Ctrl+X");
            assert_eq!(
                menu_shortcut(Modifiers::COMMAND | Modifiers::ALT, Key::M),
                "Ctrl+Alt+M"
            );
        }
    }

    #[test]
    fn page_dates_move_by_day_or_by_time_and_calendars_follow_the_language() {
        let at = Local
            .with_ymd_and_hms(2026, 10, 1, 11, 9, 0)
            .single()
            .unwrap();
        let ms = at.timestamp_millis();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        let moved = Local
            .timestamp_millis_opt(with_date(ms, day).unwrap())
            .single()
            .unwrap();
        assert_eq!(
            (moved.date_naive(), moved.hour(), moved.minute()),
            (day, 11, 9)
        );
        let later = Local
            .timestamp_millis_opt(with_time(ms, 17, 5).unwrap())
            .single()
            .unwrap();
        assert_eq!(
            (later.date_naive(), later.hour(), later.minute()),
            (at.date_naive(), 17, 5)
        );
        assert_eq!(with_time(ms, 24, 0), None);
        pin_thread_lang(Lang::En);
        assert_eq!(month_year(2026, 10), "October 2026");
        assert_eq!(weekday_initials()[0], "Su");
        pin_thread_lang(Lang::Fr);
        assert_eq!(month_year(2026, 10), "octobre 2026");
        assert_eq!(weekday_initials()[0], "lu");
        pin_thread_lang(Lang::En);
    }

    #[test]
    fn relative_times_tell_dates_from_spans() {
        pin_thread_lang(Lang::Fr);
        let now = Local::now().timestamp_millis();
        assert_eq!(
            relative_time_of(now - 5 * 60_000),
            RelativeTime::Ago("Il y a 5\u{a0}min".into())
        );
        assert!(matches!(
            relative_time_of(now - 3 * 86_400_000),
            RelativeTime::Ago(_)
        ));
        let RelativeTime::Date(date) = relative_time_of(now - 30 * 86_400_000) else {
            panic!("a month back is a date");
        };
        assert_eq!(relative_time(now - 30 * 86_400_000), date);
        pin_thread_lang(Lang::En);
    }

    #[test]
    fn month_and_weekday_ids_name_their_messages() {
        pin_thread_lang(Lang::En);
        let names: Vec<_> = MONTHS
            .iter()
            .chain(&SHORT_MONTHS)
            .chain(&WEEKDAYS)
            .map(|id| i18n::tr(id))
            .collect();
        assert_eq!(
            (names[0].as_ref(), names[11].as_ref()),
            ("January", "December")
        );
        assert_eq!((names[12].as_ref(), names[23].as_ref()), ("Jan", "Dec"));
        assert_eq!(
            (names[24].as_ref(), names[30].as_ref()),
            ("Monday", "Sunday")
        );
    }

    #[test]
    fn utility_formatting() {
        assert!(shortcut("K").contains('K'));
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn folded_find_returns_every_match_as_char_ranges() {
        assert_eq!(
            find_all_folded("Apple apple APPLE", "apple"),
            vec![0..5, 6..11, 12..17]
        );
        assert_eq!(find_all_folded("aaaa", "aa"), vec![0..2, 2..4]);
        assert_eq!(find_all_folded("École ÉCOLE", "école"), vec![0..5, 6..11]);
        // "İ" lowercases to two chars; ranges still point at the original chars.
        assert_eq!(find_all_folded("İx İx", "x"), vec![1..2, 4..5]);
        assert!(find_all_folded("abc", "").is_empty());
        assert!(find_all_folded("abc", "z").is_empty());
    }

    #[test]
    fn char_ranges_become_byte_ranges_in_one_pass() {
        // "é" takes two bytes.
        assert_eq!(
            byte_ranges("été et été", &[0..3, 4..6, 7..10]),
            vec![0..5, 6..8, 9..14]
        );
        // Text that is a frame ahead (shorter) clamps the ranges to its end.
        assert_eq!(byte_ranges("ab", &[1..2, 3..5]), vec![1..2, 2..2]);
        assert!(byte_ranges("", &[]).is_empty());
    }
}
