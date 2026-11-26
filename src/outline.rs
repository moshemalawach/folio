//! A section's pages in the desktop notebook order: a flat list where every page has an indent level
//! (0 = page, 1 = subpage, 2 = sub-subpage). A page's "family" is the page plus the
//! deeper pages right below it; they move, fold and change level together.

use std::ops::Range;

/// Deepest level: the desktop notebook allows two levels of subpages.
pub const MAX_LEVEL: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub level: u8,
}

fn levels(entries: &[Entry]) -> Vec<u8> {
    entries.iter().map(|entry| entry.level).collect()
}

fn index_of(entries: &[Entry], id: &str) -> Option<usize> {
    entries.iter().position(|entry| entry.id == id)
}

/// The page at `index` and the subpages under it.
pub fn family(levels: &[u8], index: usize) -> Range<usize> {
    let level = levels[index];
    let end = levels[index + 1..]
        .iter()
        .position(|&next| next <= level)
        .map_or(levels.len(), |offset| index + 1 + offset);
    index..end
}

/// Whether the row at `index` has subpages.
pub fn has_children(levels: &[u8], index: usize) -> bool {
    levels
        .get(index + 1)
        .is_some_and(|&next| next > levels[index])
}

/// Keeps levels valid: the first page is a top-level page and no page is more than one
/// level deeper than the page above it.
pub fn normalize(entries: &mut [Entry]) {
    let mut previous: Option<u8> = None;
    for entry in entries {
        let limit = previous.map_or(0, |level| (level + 1).min(MAX_LEVEL));
        entry.level = entry.level.min(limit);
        previous = Some(entry.level);
    }
}

fn relevel(family: &mut [Entry], root_level: u8) {
    let Some(old_root) = family.first().map(|entry| entry.level) else {
        return;
    };
    for entry in family {
        let level = entry.level as i16 - old_root as i16 + root_level as i16;
        entry.level = level.clamp(0, MAX_LEVEL as i16) as u8;
    }
}

/// Demotes (`delta > 0`) or promotes (`delta < 0`) a page with its subpages. A page can
/// only become a subpage of the page above it. Returns whether anything changed.
pub fn shift(entries: &mut [Entry], id: &str, delta: i8) -> bool {
    let Some(index) = index_of(entries, id) else {
        return false;
    };
    let before = levels(entries);
    let limit = if index == 0 {
        0
    } else {
        (before[index - 1] + 1).min(MAX_LEVEL)
    };
    let target = (before[index] as i16 + delta as i16).clamp(0, limit as i16) as u8;
    if target == before[index] {
        return false;
    }
    let range = family(&before, index);
    relevel(&mut entries[range], target);
    normalize(entries);
    levels(entries) != before
}

/// Removes a page and its subpages from the order and returns them.
pub fn take_family(entries: &mut Vec<Entry>, id: &str) -> Vec<Entry> {
    let Some(index) = index_of(entries, id) else {
        return Vec::new();
    };
    let range = family(&levels(entries), index);
    entries.drain(range).collect()
}

/// Inserts a family before the page `before` (at the end when `None` or not found). With
/// `level`, the family's first page moves to that level and its subpages follow.
pub fn insert_family(
    entries: &mut Vec<Entry>,
    mut moving: Vec<Entry>,
    before: Option<&str>,
    level: Option<u8>,
) {
    if moving.is_empty() {
        return;
    }
    if let Some(level) = level {
        relevel(&mut moving, level.min(MAX_LEVEL));
    }
    let at = before
        .and_then(|id| index_of(entries, id))
        .unwrap_or(entries.len());
    entries.splice(at..at, moving);
    normalize(entries);
}

/// Moves a page with its subpages before `before` (or to the end). Dropping a page inside
/// its own family does nothing. Returns whether the order changed.
pub fn place(entries: &mut Vec<Entry>, id: &str, before: Option<&str>, level: Option<u8>) -> bool {
    let original = entries.clone();
    let moving = take_family(entries, id);
    if moving.is_empty() || before.is_some_and(|b| moving.iter().any(|entry| entry.id == b)) {
        *entries = original;
        return false;
    }
    insert_family(entries, moving, before, level);
    *entries != original
}

/// Alt+Shift+↑ / ↓: a page and its subpages move past the page above (to before it, or
/// before its parent when that is the row above) or past the page below with its
/// subpages, keeping their level where they can. Returns whether the order changed.
pub fn step(entries: &mut Vec<Entry>, id: &str, up: bool) -> bool {
    let Some(index) = index_of(entries, id) else {
        return false;
    };
    let all = levels(entries);
    let level = all[index];
    let before = if up {
        let Some(above) = (0..index).rev().find(|&i| all[i] <= level) else {
            return false;
        };
        Some(entries[above].id.clone())
    } else {
        let next = family(&all, index).end;
        if next >= all.len() {
            return false;
        }
        entries
            .get(family(&all, next).end)
            .map(|entry| entry.id.clone())
    };
    place(entries, id, before.as_deref(), Some(level))
}

/// Moves the single page `id` right below `after`: after `after`'s subpages at its level,
/// or as its first subpage.
pub fn put_after(entries: &mut Vec<Entry>, id: &str, after: &str, subpage: bool) -> bool {
    let Some(from) = index_of(entries, id) else {
        return false;
    };
    let mut entry = entries.remove(from);
    let Some(index) = index_of(entries, after) else {
        entries.insert(from, entry);
        return false;
    };
    let all = levels(entries);
    let (at, level) = if subpage {
        (index + 1, all[index] + 1)
    } else {
        (family(&all, index).end, all[index])
    };
    entry.level = level.min(MAX_LEVEL);
    entries.insert(at, entry);
    normalize(entries);
    true
}

/// Where Paste on the row at `index` puts a page (the desktop notebook pastes after the selected
/// page): before the row right below that row's subpages (`None`: at the end), at the
/// row's level. A page cut from the row `cut` of the same list stays where it is when it
/// is pasted on its own family: `None`.
pub fn paste_after(levels: &[u8], index: usize, cut: Option<usize>) -> Option<(Option<usize>, u8)> {
    if cut.is_some_and(|cut| family(levels, cut).contains(&index)) {
        return None;
    }
    let end = family(levels, index).end;
    Some(((end < levels.len()).then_some(end), levels[index]))
}

/// The pages `id` sits under, nearest first (its parent, then that page's parent…).
pub fn ancestors<'a>(entries: &'a [Entry], id: &str) -> Vec<&'a str> {
    let Some(index) = index_of(entries, id) else {
        return Vec::new();
    };
    let mut level = entries[index].level;
    let mut out = Vec::new();
    for entry in entries[..index].iter().rev() {
        if level == 0 {
            break;
        }
        if entry.level < level {
            out.push(entry.id.as_str());
            level = entry.level;
        }
    }
    out
}

/// For each row `(level, collapsed)`: whether a folded page above hides it.
pub fn hidden(rows: &[(u8, bool)]) -> Vec<bool> {
    let mut out = Vec::with_capacity(rows.len());
    let mut hide_below: Option<u8> = None;
    for &(level, collapsed) in rows {
        if let Some(limit) = hide_below {
            if level > limit {
                out.push(true);
                continue;
            }
            hide_below = None;
        }
        out.push(false);
        if collapsed {
            hide_below = Some(level);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline(spec: &[(&str, u8)]) -> Vec<Entry> {
        spec.iter()
            .map(|&(id, level)| Entry {
                id: id.to_string(),
                level,
            })
            .collect()
    }

    fn ids_levels(entries: &[Entry]) -> Vec<(String, u8)> {
        entries
            .iter()
            .map(|entry| (entry.id.clone(), entry.level))
            .collect()
    }

    fn expect(spec: &[(&str, u8)]) -> Vec<(String, u8)> {
        ids_levels(&outline(spec))
    }

    #[test]
    fn family_spans_the_page_and_its_subpages() {
        let entries = outline(&[("A", 0), ("B", 1), ("C", 2), ("D", 1), ("E", 0)]);
        let all = levels(&entries);
        assert_eq!(family(&all, 0), 0..4);
        assert_eq!(family(&all, 1), 1..3);
        assert_eq!(family(&all, 2), 2..3);
        assert_eq!(family(&all, 4), 4..5);
        assert!(has_children(&all, 0));
        assert!(has_children(&all, 1));
        assert!(!has_children(&all, 2));
        assert!(!has_children(&all, 3));
        assert!(!has_children(&all, 4));
    }

    #[test]
    fn the_first_page_cannot_be_demoted() {
        let mut entries = outline(&[("A", 0), ("B", 0)]);
        assert!(!shift(&mut entries, "A", 1));
        assert_eq!(ids_levels(&entries), expect(&[("A", 0), ("B", 0)]));
    }

    #[test]
    fn demoting_moves_the_family_and_is_clamped_under_the_page_above() {
        let mut entries = outline(&[("A", 0), ("B", 0), ("C", 1)]);
        assert!(shift(&mut entries, "B", 1));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 1), ("C", 2)])
        );
        // The limit is the level of the page above plus one.
        assert!(!shift(&mut entries, "B", 1));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 1), ("C", 2)])
        );
    }

    #[test]
    fn promoting_moves_the_family_and_stops_at_the_top() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 2)]);
        assert!(!shift(&mut entries, "A", -1));
        assert!(shift(&mut entries, "B", -1));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 0), ("C", 1)])
        );
        assert!(!shift(&mut entries, "missing", 1));
    }

    #[test]
    fn alt_shift_arrows_step_a_page_with_its_subpages() {
        let mut entries = outline(&[("A", 0), ("B", 0), ("C", 0)]);
        assert!(step(&mut entries, "B", true));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("B", 0), ("A", 0), ("C", 0)])
        );
        assert!(!step(&mut entries, "B", true), "the first page stays");
        assert!(step(&mut entries, "B", false));
        assert!(step(&mut entries, "B", false));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("C", 0), ("B", 0)])
        );
        assert!(!step(&mut entries, "B", false), "the last page stays");
        let mut entries = outline(&[("A", 0), ("a", 1), ("B", 0), ("b", 1)]);
        assert!(step(&mut entries, "B", true));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("B", 0), ("b", 1), ("A", 0), ("a", 1)])
        );
        assert!(step(&mut entries, "B", false));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("a", 1), ("B", 0), ("b", 1)])
        );
    }

    #[test]
    fn place_moves_a_family_to_the_end() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)]);
        assert!(place(&mut entries, "B", None, None));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("D", 0), ("B", 1), ("C", 2)])
        );
    }

    #[test]
    fn place_with_a_level_relevels_the_family() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)]);
        assert!(place(&mut entries, "B", None, Some(0)));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("D", 0), ("B", 0), ("C", 1)])
        );
    }

    #[test]
    fn dropping_a_page_inside_its_own_family_does_nothing() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)]);
        assert!(!place(&mut entries, "B", Some("C"), None));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)])
        );
        assert!(!place(&mut entries, "B", Some("B"), None));
        assert!(!place(&mut entries, "missing", None, None));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)])
        );
    }

    #[test]
    fn moving_to_the_top_makes_a_top_level_page() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)]);
        assert!(place(&mut entries, "B", Some("A"), None));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("B", 0), ("C", 1), ("A", 0), ("D", 0)])
        );
    }

    #[test]
    fn put_after_goes_below_the_family_at_the_same_level() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 0), ("N", 0)]);
        assert!(put_after(&mut entries, "N", "A", false));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 1), ("N", 0), ("C", 0)])
        );
    }

    #[test]
    fn put_after_as_a_subpage_becomes_the_first_subpage() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 0), ("N", 0)]);
        assert!(put_after(&mut entries, "N", "A", true));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("N", 1), ("B", 1), ("C", 0)])
        );
    }

    #[test]
    fn put_after_a_missing_page_leaves_the_order_alone() {
        let mut entries = outline(&[("A", 0), ("N", 0), ("C", 0)]);
        assert!(!put_after(&mut entries, "N", "missing", false));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("N", 0), ("C", 0)])
        );
        assert!(!put_after(&mut entries, "missing", "A", false));
    }

    /// Cut a page, then Paste on another row, as the page list does it.
    fn paste(entries: &mut Vec<Entry>, id: &str, on: &str) -> bool {
        let index = index_of(entries, on).unwrap();
        let cut = index_of(entries, id);
        let Some((before, level)) = paste_after(&levels(entries), index, cut) else {
            return false;
        };
        let before = before.map(|at| entries[at].id.clone());
        place(entries, id, before.as_deref(), Some(level))
    }

    #[test]
    fn pasting_on_a_row_moves_past_the_neighbouring_page() {
        let start = outline(&[("A", 0), ("B", 1), ("C", 1), ("D", 0)]);
        let swapped = expect(&[("A", 0), ("C", 1), ("B", 1), ("D", 0)]);
        let all = levels(&start);
        // Before the next row at the row's level or above, at the row's level.
        assert_eq!(paste_after(&all, 2, None), Some((Some(3), 1)));
        assert_eq!(paste_after(&all, 0, None), Some((Some(3), 0)));
        // The last row's page goes at the end.
        assert_eq!(paste_after(&all, 3, None), Some((None, 0)));
        // A copy may go under its own original; a cut page never goes inside itself.
        assert_eq!(paste_after(&all, 1, Some(0)), None);
        assert_eq!(paste_after(&all, 3, Some(0)), Some((None, 0)));

        let mut entries = start.clone();
        assert!(paste(&mut entries, "B", "C"));
        assert_eq!(ids_levels(&entries), swapped);

        // Pasting a cut page on itself, or on its own subpage, leaves it where it is.
        let mut entries = start.clone();
        assert!(!paste(&mut entries, "B", "B"));
        assert!(!paste(&mut entries, "A", "C"));
        assert_eq!(entries, start);
    }

    #[test]
    fn pasting_on_a_row_skips_its_subpages() {
        let mut entries = outline(&[("A", 0), ("B", 0), ("C", 1), ("D", 0)]);
        assert!(paste(&mut entries, "A", "B"));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("B", 0), ("C", 1), ("A", 0), ("D", 0)])
        );
        // A subpage pasted on a top-level row becomes a top-level page after it.
        assert!(paste(&mut entries, "C", "D"));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("B", 0), ("A", 0), ("D", 0), ("C", 0)])
        );
    }

    #[test]
    fn folded_pages_hide_their_subpages() {
        assert_eq!(
            hidden(&[(0, true), (1, false), (2, false), (0, false)]),
            vec![false, true, true, false]
        );
        assert_eq!(
            hidden(&[(0, false), (1, true), (2, false), (1, false)]),
            vec![false, false, true, false]
        );
    }

    #[test]
    fn ancestors_walk_up_through_the_parents() {
        let entries = outline(&[("A", 0), ("B", 1), ("C", 1), ("D", 2), ("E", 0)]);
        assert_eq!(ancestors(&entries, "D"), vec!["C", "A"]);
        assert_eq!(ancestors(&entries, "B"), vec!["A"]);
        assert!(ancestors(&entries, "A").is_empty());
        assert!(ancestors(&entries, "E").is_empty());
        assert!(ancestors(&entries, "missing").is_empty());
    }

    #[test]
    fn normalize_repairs_levels() {
        let mut entries = outline(&[("A", 2), ("B", 0), ("C", 2)]);
        normalize(&mut entries);
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 0), ("C", 1)])
        );
    }

    #[test]
    fn take_and_insert_family_keep_subpages_together() {
        let mut entries = outline(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)]);
        let moving = take_family(&mut entries, "B");
        assert_eq!(ids_levels(&moving), expect(&[("B", 1), ("C", 2)]));
        assert_eq!(ids_levels(&entries), expect(&[("A", 0), ("D", 0)]));
        insert_family(&mut entries, moving, Some("D"), Some(MAX_LEVEL + 3));
        assert_eq!(
            ids_levels(&entries),
            expect(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)])
        );
        assert!(take_family(&mut entries, "missing").is_empty());
    }
}
