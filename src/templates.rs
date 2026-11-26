//! the desktop notebook's page templates, a few built in: each makes a new page with its notes laid
//! out, written in the current language (like every default Folio creates).

use std::borrow::Cow;

use eframe::egui::Ui;

use crate::canvas::NOTE_PAD_X;
use crate::i18n::t;
use crate::icons;
use crate::model::{
    Block, BlockKind, CanvasDoc, CanvasItem, CanvasKind, Document, StyledText, PAGE_CONTENT_TOP,
    PAGE_LEFT,
};
use crate::theme::Tokens;
use crate::widgets;

/// A template's note: wide enough for its headings and lists without growing as you type.
pub(crate) const NOTE_WIDTH: f32 = 560.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTemplate {
    Blank,
    LectureNotes,
    ToDoList,
    MeetingNotes,
}

impl PageTemplate {
    /// Insert › Page Templates lists them in this order.
    pub const ALL: [PageTemplate; 4] = [
        PageTemplate::Blank,
        PageTemplate::LectureNotes,
        PageTemplate::ToDoList,
        PageTemplate::MeetingNotes,
    ];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            PageTemplate::Blank => t!("ribbon-template-blank"),
            PageTemplate::LectureNotes => t!("ribbon-template-lecture"),
            PageTemplate::ToDoList => t!("ribbon-template-todo"),
            PageTemplate::MeetingNotes => t!("ribbon-template-meeting"),
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            PageTemplate::Blank => icons::DOCUMENT_ONE_PAGE_MULTIPLE,
            PageTemplate::LectureNotes => icons::BOOK_OPEN,
            PageTemplate::ToDoList => icons::CHECK_SQUARE,
            PageTemplate::MeetingNotes => icons::CALENDAR_BLANK,
        }
    }

    /// The new page's contents: one note where a page's first note goes (its text under
    /// the title's first letter), or nothing for Blank. Its items are empty, ready to type in.
    pub fn canvas(self) -> CanvasDoc {
        let blocks = match self {
            PageTemplate::Blank => return CanvasDoc::default(),
            PageTemplate::LectureNotes => vec![
                heading(t!("data-template-topic")),
                Block::paragraph(""),
                heading(t!("data-template-key-points")),
                Block::bullet(""),
                Block::bullet(""),
                Block::bullet(""),
                heading(t!("data-template-questions")),
                Block::bullet(""),
                heading(t!("data-template-summary")),
                Block::paragraph(""),
            ],
            PageTemplate::ToDoList => {
                let mut blocks = vec![heading(t!("data-template-to-do"))];
                blocks.extend((0..5).map(|_| Block::todo("", false)));
                blocks
            }
            PageTemplate::MeetingNotes => vec![
                Block::paragraph(t!(
                    "data-template-date",
                    date = crate::util::long_date(chrono::Local::now())
                )),
                heading(t!("data-template-attendees")),
                Block::bullet(""),
                heading(t!("data-template-agenda")),
                Block::new(BlockKind::Numbered),
                heading(t!("data-template-notes")),
                Block::paragraph(""),
                heading(t!("data-template-actions")),
                Block::todo("", false),
            ],
        };
        let doc = Document { blocks };
        // Its To Do boxes hang left of its text (`editor::tag_lead`), which starts under
        // the title like every other note's.
        let lead = crate::editor::tag_lead(&doc);
        let mut canvas = CanvasDoc::default();
        canvas.add(CanvasItem::new(
            CanvasKind::Text { doc },
            PAGE_LEFT - NOTE_PAD_X - lead,
            PAGE_CONTENT_TOP,
            NOTE_WIDTH + lead,
            40.0,
        ));
        canvas
    }

    /// Whether the page gets college rule lines, as the desktop notebook's lecture notes are ruled.
    pub fn ruled(self) -> bool {
        self == PageTemplate::LectureNotes
    }
}

/// A template's heading: the desktop notebook's Heading 2 style.
fn heading(text: Cow<'static, str>) -> Block {
    Block {
        text: StyledText::new(text),
        ..Block::new(BlockKind::heading(2))
    }
}

/// Insert › Page Templates ▾: one row per template; a click adds a page laid out from it.
pub fn template_list(ui: &mut Ui, t: Tokens) -> Option<PageTemplate> {
    ui.set_min_width(180.0);
    let mut picked = None;
    for template in PageTemplate::ALL {
        if widgets::menu_item(ui, Some(template.glyph()), &template.label(), t).clicked() {
            picked = Some(template);
            ui.close();
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{pin_thread_lang, Lang};

    /// The note a template lays out, and its blocks.
    fn note(canvas: &CanvasDoc) -> &[Block] {
        assert_eq!(canvas.items.len(), 1, "one note");
        &canvas.items[0].doc().expect("a note").blocks
    }

    #[test]
    fn templates_lay_out_their_notes_in_the_current_language() {
        pin_thread_lang(Lang::Fr);
        let canvas = PageTemplate::ToDoList.canvas();
        let blocks = note(&canvas);
        assert_eq!(blocks[0].text.text, "À faire");
        assert_eq!(blocks[0].kind, BlockKind::heading(2));
        let todos = blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Todo { checked: false }))
            .count();
        assert_eq!(todos, 5);

        pin_thread_lang(Lang::En);
        let canvas = PageTemplate::LectureNotes.canvas();
        let headings: Vec<&str> = note(&canvas)
            .iter()
            .filter(|block| block.kind == BlockKind::heading(2))
            .map(|block| block.text.text.as_str())
            .collect();
        assert_eq!(headings, ["Topic", "Key points", "Questions", "Summary"]);
        let item = &canvas.items[0];
        assert_eq!(
            (item.x, item.y, item.width),
            (PAGE_LEFT - NOTE_PAD_X, PAGE_CONTENT_TOP, NOTE_WIDTH)
        );
        assert!(!item.auto_width, "a template's note keeps its width");
        assert!(PageTemplate::LectureNotes.ruled());
        assert!(!PageTemplate::ToDoList.ruled());
        // The To Do boxes hang left of the text, which starts under the title.
        assert_eq!(
            PageTemplate::ToDoList.canvas().items[0].x,
            PAGE_LEFT - NOTE_PAD_X - crate::editor::TAG_SLOT
        );

        let canvas = PageTemplate::MeetingNotes.canvas();
        let blocks = note(&canvas);
        assert_eq!(
            blocks[0].text.text,
            format!("Date: {}", crate::util::long_date(chrono::Local::now())),
            "the meeting is dated today"
        );
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);

        assert!(PageTemplate::Blank.canvas().items.is_empty());
        assert!(PageTemplate::Blank.canvas().ink.is_empty());
    }
}
