//! Find on page (Ctrl+F): where a query occurs in the text on a freeform page.

use std::ops::Range;

use crate::i18n::t;
use crate::model::{BlockKind, CanvasDoc, CanvasKind};
use crate::util;

/// Which text of a page item a match is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPlace {
    /// A paragraph of a note, by block id.
    Block(u64),
    /// A shape's label.
    Label,
    /// A table cell, by table block, row and column.
    Cell { block: u64, row: usize, col: usize },
    /// A frise milestone, by index.
    Milestone(usize, MilestoneField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilestoneField {
    Title,
    Date,
    Note,
}

impl MatchPlace {
    /// The note paragraph (or table) to bring into view, for matches inside notes.
    pub fn block(self) -> Option<u64> {
        match self {
            MatchPlace::Block(id) | MatchPlace::Cell { block: id, .. } => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    pub item: u64,
    pub place: MatchPlace,
    /// Char range within that text.
    pub range: Range<usize>,
}

/// The find bar, while it is open on a page.
#[derive(Debug, Default)]
pub struct FindState {
    pub query: String,
    pub matches: Vec<FindMatch>,
    /// Index into `matches` of the highlighted match.
    pub current: usize,
    /// Recompute matches and highlights at the end of this frame.
    pub dirty: bool,
    /// Give the query field the caret, with its text selected.
    pub focus: bool,
    /// The query field had keyboard focus last frame (egui drops it on Escape before
    /// the bar runs, so Escape is matched against this).
    pub field_focused: bool,
    /// Opened from global search: close quietly when nothing on the page matches.
    pub from_search: bool,
    /// Bring the current match into view once matches are up to date.
    pub reveal: bool,
    /// The keyboard was last on the page (a note, a label, a cell) rather than in the
    /// query field, so closing the bar leaves the caret there, not on the current match.
    pub typing_on_page: bool,
}

impl FindState {
    pub fn current_match(&self) -> Option<&FindMatch> {
        self.matches.get(self.current)
    }

    /// Moves to the next (or previous) match, wrapping around the page.
    pub fn step(&mut self, forward: bool) {
        let count = self.matches.len();
        if count == 0 {
            return;
        }
        self.current = if forward {
            (self.current + 1) % count
        } else {
            (self.current + count - 1) % count
        };
        self.dirty = true;
        self.reveal = true;
    }

    /// "2 of 5", "No matches" (in the UI language), or nothing while the query is empty.
    pub fn status(&self) -> String {
        if self.query.trim().is_empty() {
            String::new()
        } else if self.matches.is_empty() {
            t!("find-no-matches").into_owned()
        } else {
            t!(
                "find-position",
                current = self.current + 1,
                total = self.matches.len()
            )
            .into_owned()
        }
    }
}

/// Every match of `query` on the page, in reading order: items top to bottom, then
/// left to right (as `CanvasDoc::plain_text` reads them), then in order inside each item.
pub fn collect(doc: &CanvasDoc, query: &str) -> Vec<FindMatch> {
    let mut matches = Vec::new();
    if query.trim().is_empty() {
        return matches;
    }
    let mut items: Vec<_> = doc.items.iter().collect();
    items.sort_by_key(|item| (item.y as i32, item.x as i32));
    for item in items {
        let mut add = |place: MatchPlace, text: &str| {
            for range in util::find_all_folded(text, query) {
                matches.push(FindMatch {
                    item: item.id,
                    place,
                    range,
                });
            }
        };
        match &item.kind {
            CanvasKind::Text { doc } => {
                for block in &doc.blocks {
                    // A file in a note is found by its name and an equation by its source,
                    // as their items on the page are; a table by its cells, row by row.
                    let text = match &block.kind {
                        BlockKind::Table(table) => {
                            for (row, cells) in table.rows.iter().enumerate() {
                                for (col, cell) in cells.iter().enumerate() {
                                    let place = MatchPlace::Cell {
                                        block: block.id,
                                        row,
                                        col,
                                    };
                                    add(place, &cell.text.text);
                                }
                            }
                            continue;
                        }
                        BlockKind::File { name, .. } => name,
                        BlockKind::Equation { source, .. } => source,
                        _ => &block.text.text,
                    };
                    add(MatchPlace::Block(block.id), text);
                }
            }
            // Connector labels are never drawn, so they are not searched either.
            CanvasKind::Shape { label, .. } if !item.is_connector() => {
                add(MatchPlace::Label, label)
            }
            CanvasKind::Timeline { milestones, .. } => {
                for (index, milestone) in milestones.iter().enumerate() {
                    add(
                        MatchPlace::Milestone(index, MilestoneField::Title),
                        &milestone.title,
                    );
                    add(
                        MatchPlace::Milestone(index, MilestoneField::Date),
                        &milestone.date,
                    );
                    add(
                        MatchPlace::Milestone(index, MilestoneField::Note),
                        &milestone.note,
                    );
                }
            }
            // the desktop notebook finds attachments by their file name.
            CanvasKind::File { name, .. } => add(MatchPlace::Label, name),
            // The source is matched; the whole formula is highlighted.
            CanvasKind::Equation { source, .. } => add(MatchPlace::Label, source),
            _ => {}
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, CanvasItem, Document, Milestone, Rgb, ShapeKind, TableBlock};

    fn note(text: &str, x: f32, y: f32) -> CanvasItem {
        CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::paragraph(text)],
                },
            },
            x,
            y,
            300.0,
            40.0,
        )
    }

    fn shape(shape: ShapeKind, label: &str, y: f32) -> CanvasItem {
        CanvasItem::new(
            CanvasKind::Shape {
                shape,
                stroke: Rgb::INKS[0],
                fill: None,
                label: label.into(),
                flip_x: false,
                flip_y: false,
                thickness: None,
            },
            40.0,
            y,
            120.0,
            60.0,
        )
    }

    fn first_block(doc: &CanvasDoc, id: u64) -> u64 {
        doc.item(id).and_then(|item| item.doc()).unwrap().blocks[0].id
    }

    #[test]
    fn matches_follow_reading_order_and_skip_connectors() {
        let mut doc = CanvasDoc::default();
        let lower = doc.add(note("Moon and MOON", 40.0, 300.0));
        let upper = doc.add(note("moon", 400.0, 20.0));
        let table = doc.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::new(BlockKind::Table(TableBlock::from_strings(
                        &[vec!["a".into(), "honeymoon".into()]],
                        false,
                        0.0,
                    )))],
                },
            },
            40.0,
            500.0,
            200.0,
            60.0,
        ));
        doc.add(shape(ShapeKind::Arrow, "moon", 100.0));
        let label = doc.add(shape(ShapeKind::Rect, "Moonlight", 200.0));
        let frise = doc.add(CanvasItem::new(
            CanvasKind::Timeline {
                color: Rgb::INKS[2],
                milestones: vec![Milestone {
                    title: "Apollo".into(),
                    date: "1969".into(),
                    note: "First moon walk".into(),
                }],
            },
            40.0,
            700.0,
            600.0,
            200.0,
        ));
        let found: Vec<_> = collect(&doc, "moon")
            .into_iter()
            .map(|found| (found.item, found.place, found.range))
            .collect();
        assert_eq!(
            found,
            vec![
                (upper, MatchPlace::Block(first_block(&doc, upper)), 0..4),
                (label, MatchPlace::Label, 0..4),
                (lower, MatchPlace::Block(first_block(&doc, lower)), 0..4),
                (lower, MatchPlace::Block(first_block(&doc, lower)), 9..13),
                (
                    table,
                    MatchPlace::Cell {
                        block: first_block(&doc, table),
                        row: 0,
                        col: 1
                    },
                    5..9
                ),
                (frise, MatchPlace::Milestone(0, MilestoneField::Note), 6..10),
            ]
        );
        assert!(collect(&doc, "  ").is_empty());
    }

    #[test]
    fn stepping_wraps_and_the_status_counts() {
        let mut doc = CanvasDoc::default();
        doc.add(note("one two one one", 0.0, 0.0));
        let mut find = FindState {
            query: "one".into(),
            matches: collect(&doc, "one"),
            ..FindState::default()
        };
        assert_eq!(find.status(), "1 of 3");
        find.step(false);
        assert_eq!(find.current, 2);
        assert!(find.reveal && find.dirty);
        find.step(true);
        assert_eq!(find.current, 0);
        find.matches.clear();
        assert_eq!(find.status(), "No matches");
        find.query.clear();
        assert_eq!(find.status(), "");
    }

    #[test]
    fn the_status_reads_in_french() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        let mut doc = CanvasDoc::default();
        doc.add(note("un deux un un", 0.0, 0.0));
        let mut find = FindState {
            query: "un".into(),
            matches: collect(&doc, "un"),
            ..FindState::default()
        };
        assert_eq!(find.status(), "1 sur 3");
        find.matches.clear();
        assert_eq!(find.status(), "Aucun résultat");
    }

    #[test]
    fn attached_files_are_found_by_name_and_recordings_are_skipped() {
        let mut doc = CanvasDoc::default();
        let file = doc.add(CanvasItem::new(
            CanvasKind::File {
                attachment: "a1".into(),
                name: "Lesson plan.pdf".into(),
                size: 2048,
            },
            40.0,
            40.0,
            260.0,
            56.0,
        ));
        doc.add(CanvasItem::new(
            CanvasKind::Audio {
                attachment: "plan".into(),
                duration_ms: 0,
            },
            40.0,
            200.0,
            300.0,
            64.0,
        ));
        let found = collect(&doc, "PLAN");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (found[0].item, found[0].place, found[0].range.clone()),
            (file, MatchPlace::Label, 7..11)
        );
    }

    /// What Insert puts into a note at the caret is found as its item would be: a file by
    /// its name, an equation by its source; a recording has nothing to find.
    #[test]
    fn files_and_equations_in_notes_are_found() {
        use crate::model::BlockKind;
        let file = Block::new(BlockKind::File {
            attachment: "a1".into(),
            name: "Lesson plan.pdf".into(),
            size: 2048,
        });
        let equation = Block::new(BlockKind::Equation {
            source: "x=plan^2".into(),
            scale: crate::model::default_equation_size(),
        });
        let audio = Block::new(BlockKind::Audio {
            attachment: "plan".into(),
            duration_ms: 0,
        });
        let (file_id, equation_id) = (file.id, equation.id);
        let mut doc = CanvasDoc::default();
        let note = doc.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::paragraph("A plan"), file, equation, audio],
                },
            },
            40.0,
            40.0,
            300.0,
            200.0,
        ));
        let found: Vec<_> = collect(&doc, "plan")
            .into_iter()
            .map(|found| (found.item, found.place, found.range))
            .collect();
        assert_eq!(
            found[1..],
            [
                (note, MatchPlace::Block(file_id), 7..11),
                (note, MatchPlace::Block(equation_id), 2..6),
            ],
            "{found:?}"
        );
    }
}
