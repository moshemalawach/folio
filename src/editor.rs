#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use eframe::egui::text_edit::TextEditState;
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, Id, Key, KeyboardShortcut, Layout,
    Margin, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder,
};

use crate::db::Library;
use crate::i18n::t;
use crate::icons;
use crate::images::TextureCache;
use crate::math;
use crate::model::{
    display_title, normalize_link, points_to_px, pt_to_px, px_to_pt, Block, BlockKind, CustomTag,
    Document, Folds, FontFace, InlineStyle, LineTag, ListMarker, ListStart, Page, ParaAlign,
    ParaSpacing, Rgb, StyleRun, StyledText, TableBlock, TableCell, TagSymbol, FONT_SIZES,
    MAX_LEVEL, PAGE_LINK_PREFIX, SECTION_LINK_PREFIX, TABLE_AUTO_MAX, TABLE_MIN_COLUMN,
};
use crate::ribbon::{Cmd, PasteMode};
use crate::spell::Speller;
use crate::table::{CellRange, TableCmd};
use crate::tags::{TagGallery, TagKey};
use crate::theme::{self, Tokens};
use crate::util;
use crate::widgets;

/// Normal text: the desktop notebook's 11 pt (Calibri; Carlito here), single spaced.
pub(crate) const BODY_SIZE: f32 = 11.0 * 4.0 / 3.0;
pub(crate) const BODY_LINE: f32 = 18.0;
/// Space between a table cell's border and its text.
const CELL_PAD: egui::Vec2 = vec2(6.0, 3.0);

/// One outline level's indent (the desktop notebook's is about a quarter of an inch at 100 %).
pub(crate) const LEVEL_INDENT: f32 = 24.0;
/// The margin left of a note where its paragraph handles appear, in screen points.
pub(crate) const HANDLE_GUTTER: f32 = 16.0;
/// One tag's (or the To Do box's) place, hung left of its paragraph's text.
pub(crate) const TAG_SLOT: f32 = 18.0;
/// The bullet's or number's place before a list item's text (a wider number widens it).
pub(crate) const MARKER_WIDTH: f32 = 20.0;
/// The To Do box (the desktop notebook's is about this size at 100 %).
pub(crate) const CHECKBOX_SIZE: f32 = 12.0;
/// A level-1 bullet's radius.
pub(crate) const BULLET_RADIUS: f32 = 2.5;

/// A paragraph as laid out this frame, in the coordinates of the ui it was shown in (for a
/// note, the canvas scene). Paragraph handles, the selection across paragraphs, auto width
/// and the tests read these.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub block: u64,
    /// The whole row: tag column, marker and text.
    pub rect: Rect,
    /// The first line's top and bottom, where handles and markers line up.
    pub head: egui::Rangef,
    /// For text, its galley and where the galley's origin was drawn.
    pub text: Option<(Arc<egui::Galley>, egui::Pos2)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
}

fn has_mark(style: &InlineStyle, mark: Mark) -> bool {
    match mark {
        Mark::Bold => style.bold,
        Mark::Italic => style.italic,
        Mark::Underline => style.underline,
        Mark::Strike => style.strike,
        Mark::Superscript => style.superscript,
        Mark::Subscript => style.subscript,
    }
}

fn set_mark(style: &mut InlineStyle, mark: Mark, value: bool) {
    match mark {
        Mark::Bold => style.bold = value,
        Mark::Italic => style.italic = value,
        Mark::Underline => style.underline = value,
        Mark::Strike => style.strike = value,
        // Raised and lowered text exclude each other.
        Mark::Superscript => {
            style.superscript = value;
            style.subscript &= !value;
        }
        Mark::Subscript => {
            style.subscript = value;
            style.superscript &= !value;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ToolbarCommand {
    Toggle(Mark),
    Highlight(Option<Rgb>),
    Color(Option<Rgb>),
    Face(FontFace),
    Size(u8),
    Align(ParaAlign),
    ClearFormatting,
    SetKind(BlockKind),
    /// Home › Delete while typing: the selected part of the paragraph, else all of it.
    DeleteBlock,
    Undo,
    Redo,
    Tag(TagCommand),
    /// Home › Cut and Copy: the selected text, across paragraphs too.
    Cut,
    Copy,
    /// Home › Paste (plain text) at the caret.
    Paste,
    /// Home › Decrease Indent (-1) and Increase Indent (1): the outline level.
    Indent(i8),
    /// Home › Bullets ▾ and Numbering ▾: a list style, or `None` to end the list.
    ListMarker(Option<ListMarker>),
    /// Restart Numbering / Continue Numbering / Customize Numbering's Start at.
    ListStart(Option<ListStart>),
    /// Customize Numbering…'s OK: the numbering style, and the number the item at the
    /// caret starts at, as one undo step.
    Numbering(ListMarker, ListStart),
    /// Paragraph Spacing Options… for the paragraph or the selected paragraphs.
    Spacing(ParaSpacing),
}

/// A command from the right-click menu of note text.
#[derive(Debug, Clone, PartialEq)]
enum TextMenuCommand {
    Cut,
    Copy,
    Paste,
    Link,
    OpenLink,
    CopyLink,
    /// Copy Link to Paragraph: a `folio://page/<page>#p<block>` link to the paragraph.
    CopyParagraphLink,
    RemoveLink,
    /// A pick in the mini toolbar: the app runs it as the ribbon's (`EditorState::menu_cmd`).
    Format(ToolbarCommand),
    /// Replace the misspelled word at `range` (chars) with a suggestion.
    ReplaceWord {
        range: Range<usize>,
        word: String,
    },
    AddToDictionary(String),
    IgnoreWord(String),
}

pub struct ToolbarResult {
    pub changed: bool,
    /// A command ran; the caller should hand keyboard focus back to the text.
    pub interacted: bool,
}

/// What the ribbon shows for the text being typed: which marks are on, the font and
/// size, the alignment and paragraph kind, and whether undo and redo can run.
#[derive(Debug, Clone)]
pub(crate) struct TextState {
    pub marks: [bool; 6], // Bold, Italic, Underline, Strike, Superscript, Subscript
    /// Whether the text is highlighted or coloured. The ribbon's Highlight and Font
    /// Color buttons show their colour, not this state (as in the desktop notebook), so nothing reads
    /// it yet.
    #[allow(dead_code)]
    pub highlight: bool,
    #[allow(dead_code)]
    pub color: bool,
    pub face: FontFace,
    pub pt: f32,
    pub align: ParaAlign,
    pub kind: BlockKind,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Text is selected (in the paragraph, or across paragraphs): Cut and Copy can run.
    pub has_selection: bool,
    /// The list item's bullet or number style, picked or its level's default; `None` off
    /// a list. The Bullets and Numbering libraries check it.
    pub marker: Option<ListMarker>,
    /// The paragraph's outline level (0 at the left).
    #[allow(dead_code)]
    // no ribbon rule reads it: both indent buttons stay on, as the desktop notebook's
    pub level: u8,
    /// Its Paragraph Spacing Options, which their dialog opens with.
    pub spacing: ParaSpacing,
}

impl TextState {
    pub fn mark(&self, mark: Mark) -> bool {
        // `Mark`'s variants are declared in the order of `marks`.
        self.marks[mark as usize]
    }
}

/// `None` when no paragraph of `doc` is active (nothing to format). Computed as the old
/// editor toolbar computed its buttons' states.
pub(crate) fn text_state(doc: &Document, editor: &EditorState) -> Option<TextState> {
    let index = editor.active_index(doc)?;
    let block = &doc.blocks[index];
    let mark = |mark: Mark| editor.active_style_state(doc, |style| has_mark(style, mark));
    let shown = editor.shown_style(doc);
    // A table cell is typed like a paragraph, in its own alignment.
    let cell = match (&block.kind, editor.active_cell) {
        (BlockKind::Table(table), Some((row, col))) => {
            table.rows.get(row).and_then(|cells| cells.get(col))
        }
        _ => None,
    };
    let (default_px, _, default_face) = match cell {
        Some(_) => kind_defaults(&BlockKind::Paragraph),
        None => kind_defaults(&block.kind),
    };
    Some(TextState {
        marks: [
            Mark::Bold,
            Mark::Italic,
            Mark::Underline,
            Mark::Strike,
            Mark::Superscript,
            Mark::Subscript,
        ]
        .map(mark),
        highlight: editor.active_style_state(doc, |style| style.highlight.is_some()),
        color: editor.active_style_state(doc, |style| style.color.is_some()),
        face: shown.font.unwrap_or(default_face),
        pt: shown.size.map_or(px_to_pt(default_px), f32::from),
        align: cell.map_or(block.align, |cell| cell.align),
        // Asked every frame: a table's cells are not copied (no ribbon rule reads them).
        kind: block.kind.without_cells(),
        can_undo: !editor.undo_stack.is_empty(),
        can_redo: !editor.redo_stack.is_empty(),
        // Cells selected across the table count: Cut and Copy take them (D2).
        has_selection: editor.range_in(doc).is_some()
            || editor
                .selection
                .as_ref()
                .is_some_and(|range| !range.is_empty())
            || editor.cell_selection.is_some_and(|(owner, range)| {
                Some(owner) == editor.active_block && !range.is_single()
            }),
        marker: block.list_marker(),
        level: block.indent,
        spacing: block.spacing,
    })
}

/// A link being inserted or edited in the link dialog (drawn by the app).
#[derive(Debug, Clone, PartialEq)]
pub struct LinkEdit {
    /// The note's document: `"<page id>:<item id>"`.
    pub doc_id: String,
    pub block: u64,
    /// The chars the link covers, or just the caret for a new link.
    pub range: Range<usize>,
    pub text: String,
    pub url: String,
    /// The range already was a link, so the dialog offers "Remove link".
    pub existing: bool,
}

/// Where a right-click in note text happened.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextMenuTarget {
    pub block: u64,
    /// The selection (or the caret, when empty) the menu acts on.
    pub range: Range<usize>,
    /// The char under the pointer, for word-level commands (spelling, links).
    pub pointer_char: Option<usize>,
    /// The pointer where the menu was asked for, in screen points: the menu's card opens
    /// there, with the Mini Toolbar's above it.
    pub at: egui::Pos2,
}

/// The Mini Toolbar over a selection: the note and paragraph it formats, where it shows
/// (above where the selecting drag or click ended, in screen points), and its rect as last
/// drawn.
#[derive(Debug, Clone)]
pub(crate) struct MiniBar {
    doc_id: String,
    block: u64,
    at: egui::Pos2,
    rect: Option<Rect>,
}

#[derive(Debug, Clone)]
enum FocusRequest {
    Title {
        select_all: bool,
    },
    Block {
        id: u64,
        pos: usize,
    },
    /// Re-focus a block, keeping its caret and selection (after a toolbar click).
    Restore {
        id: u64,
    },
    /// Focus a block with `range` (chars) selected: the match find on page leaves.
    Select {
        id: u64,
        range: Range<usize>,
    },
    /// Focus a table cell with `range` (chars) selected (empty: just the caret).
    Cell {
        id: u64,
        row: usize,
        col: usize,
        range: Range<usize>,
    },
}

#[derive(Debug, Clone)]
enum BlockAction {
    ToggleChecked(u64),
    /// A click on a tag's box in the tag column (Call back, a custom box…).
    ToggleTagChecked(u64, TagTick),
    /// The equation block's session ended (its source field lost the keyboard); `enter`
    /// when Enter ended it. Applied after the note is drawn, as it may add or remove a
    /// block.
    FinishEquation {
        block: u64,
        enter: bool,
    },
    /// A command from a table cell's right-click menu. Applied after the note is drawn,
    /// as it may remove the table.
    TableCommand(TableCmd),
}

/// The tag a click in the tag column ticks or unticks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TagTick {
    Line(LineTag),
    /// A custom tag, by name.
    Custom(String),
}

/// The misspelled word a right-click landed on (char range into the block's text).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpellTarget {
    block: Id,
    range: Range<usize>,
    word: String,
    /// The language the words around it are in ([`Speller::language_near`]).
    language: Option<usize>,
}

/// What a file or recording card in a note asked for; the canvas carries it out with
/// the library, as it does for file and recording items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentAction {
    Open,
    SaveAs,
    TogglePlay,
    /// A PDF's Insert Printout: its pages go under the note.
    InsertPrintout,
}

/// What the canvas tells the editor about recordings, for their cards: the one being
/// recorded, the one playing (its progress from 0 to 1, and whether it plays), and the
/// lengths read from the files of the clips whose blocks do not know their own.
#[derive(Debug, Clone, Default)]
pub struct MediaView {
    pub recording: Option<String>,
    pub playing: Option<(String, f32, bool)>,
    pub lengths: HashMap<String, u64>,
    /// The PDFs in notes whose printout is on the page: their menu has no Insert
    /// Printout, which would stack new pages over it.
    pub printed: HashSet<String>,
}

/// A place in a note's text: a paragraph (block id) and a char index in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub block: u64,
    pub char: usize,
}

/// What Folio last copied from note text, with its formatting. The system clipboard only
/// carries the plain text, which is how a later paste is matched back to this.
#[derive(Debug, Clone)]
pub(crate) struct RichClip {
    pub plain: String,
    pub blocks: Vec<Block>,
    /// Copied from inside one paragraph or one cell: its text joins the line it is pasted into.
    pub inline: bool,
}

/// The last rich paste into a note, while the desktop notebook's Paste Options button shows under it:
/// enough to paste it again another way in place.
#[derive(Debug, Clone)]
pub(crate) struct PasteSession {
    pub doc_id: String,
    /// The note before the paste.
    before: Document,
    /// The note right after it: any other change ends the session.
    after: Document,
    at: crate::paste::PasteAt,
    source: Vec<Block>,
    inline: bool,
    caret_style: InlineStyle,
    pub mode: PasteMode,
    /// The block the pasted text ends in: the button sits under it.
    pub end_block: u64,
}

/// Format Painter: the formatting picked up, waiting for text to paint.
#[derive(Debug, Clone)]
pub(crate) struct FormatPainter {
    style: InlineStyle,
    align: ParaAlign,
    /// Turned on by a double-click: stays on until Esc or another click on the button.
    pub sticky: bool,
}

pub struct EditorState {
    pub active_block: Option<u64>,
    /// The table cell holding the caret (row, column), when the active block is a table.
    pub(crate) active_cell: Option<(usize, usize)>,
    /// Cells selected across a table (block id, range), by dragging or a Select command.
    pub(crate) cell_selection: Option<(u64, crate::table::CellRange)>,
    pub selection: Option<Range<usize>>,
    /// A selection that crosses paragraphs, as (anchor, focus). egui's text boxes each
    /// select inside one paragraph; the editor paints and edits this range itself. Block
    /// ids are unique across notes, so it is in whichever note holds both its ends.
    pub range: Option<(Pos, Pos)>,
    pub active_style: InlineStyle,
    pub hovered_block: Option<u64>,
    pub title_focused: bool,
    pub(crate) pending_style: Option<InlineStyle>,
    /// Set when Find Tags was chosen from a menu; the app opens the pane.
    pub find_tags_request: bool,
    /// Set when Customize Tags… was chosen from a menu; the app opens the dialog.
    pub customize_tags_request: bool,
    /// Set when the Mini Toolbar's Font Color › More Colors… was chosen; the app opens
    /// the colour dialog.
    pub more_font_colors_request: bool,
    /// The tag gallery's order and custom tags: the tag menu and Ctrl+1 … Ctrl+9 follow it.
    pub tags: TagGallery,
    /// Where `pending_style` was armed (block, caret), recorded on the first frame the block
    /// shows after arming. Moving the caret drops the armed style, as in the desktop notebook.
    pending_at: Option<(u64, usize)>,
    pending_focus: Option<FocusRequest>,
    undo_stack: Vec<Document>,
    redo_stack: Vec<Document>,
    /// The document the undo history belongs to. Canvas notes share one editor state,
    /// and one note's history must never be applied to another.
    history_owner: Option<String>,
    /// The block text edit that last had keyboard focus.
    text_widget: Option<Id>,
    /// The equation block being typed (its source field shows under it).
    pub(crate) equation_block: Option<u64>,
    focus_equation: bool,
    /// A link the user asked to follow; the app opens it and clears this.
    pub link_request: Option<String>,
    /// The link dialog to open; the app shows it and clears this.
    pub link_edit: Option<LinkEdit>,
    /// What a file or recording card in a note asked for (an attachment id and the
    /// action); the canvas carries it out and clears this.
    pub attachment_action: Option<(String, AttachmentAction)>,
    /// A picture block clicked this frame: the canvas makes its note the one being edited.
    pub(crate) picture_clicked: Option<u64>,
    /// A command from a picture block's menu (the block and the command): the canvas runs
    /// it.
    pub(crate) picture_request: Option<(u64, crate::canvas::PictureCmd)>,
    /// A change in a note that the page's history records, not the note's: its doc id and
    /// the note just before. A picture's corner drag (the press took the keyboard from the
    /// text, so Ctrl+Z is the page's), or a column border dragged in a note other than the
    /// one being typed in. The canvas takes it once the note is drawn.
    pub(crate) page_step: Option<(String, Document)>,
    /// The recording and playing clips, set by the canvas each frame for the cards.
    pub media: MediaView,
    pub(crate) menu_target: Option<TextMenuTarget>,
    /// A formatting command picked in the text menu's mini toolbar. The app takes it after
    /// the page is drawn and runs it as the ribbon's `Cmd::Text`, so it acts exactly as the
    /// ribbon's twin does.
    pub(crate) menu_cmd: Option<ToolbarCommand>,
    /// Options › General › Show Mini Toolbar on selection (on unless the user turned it
    /// off). Kept across [`Self::reset`]: it is a setting, not the page's state.
    pub(crate) mini_on_selection: bool,
    /// The Mini Toolbar shown over text just selected with the mouse, if any.
    pub(crate) mini_bar: Option<MiniBar>,
    /// Options › Advanced › Calculate mathematical expressions automatically (the desktop notebook's,
    /// on unless turned off): "95+83=" then Space or Enter writes the answer. Set by the
    /// app; kept across [`Self::reset`].
    pub(crate) auto_calc: bool,
    /// Find-on-page matches to highlight, per block widget (`block_id`), as char
    /// ranges and whether each is the current match. Filled by the canvas find bar.
    pub find_marks: HashMap<Id, Vec<(Range<usize>, bool)>>,
    /// This frame's rows of every document shown, by `doc_id`.
    pub(crate) rows: HashMap<String, Vec<Row>>,
    /// The rule spacing of the ruled page the document being shown sits on (set by the
    /// canvas around each note): every line then takes a whole number of spacings, so text
    /// sits on the rules, as in the desktop notebook.
    pub(crate) line_pitch: Option<f32>,
    /// The galley the last text box laid out, for its paragraph's row.
    galley_hint: Option<(Arc<egui::Galley>, egui::Pos2)>,
    /// Spell checking; kept across [`Self::reset`] so the dictionary loads once.
    pub spell: Speller,
    /// The note text copied last, with its formatting; kept across [`Self::reset`], so it
    /// pastes on another page too.
    pub(crate) rich_clip: Option<RichClip>,
    /// Set when note text was copied this frame: the canvas then forgets its item copy.
    pub(crate) copied: bool,
    /// The way the next paste goes, asked for by the ribbon's Paste (Keep Source
    /// Formatting) or its options, and when it was asked (dropped after a second).
    pub(crate) paste_mode: Option<PasteMode>,
    pub(crate) paste_mode_at: f64,
    /// The last rich paste, while its Paste Options button shows.
    pub(crate) paste_session: Option<PasteSession>,
    /// The Format Painter, while it is on. [`Self::reset`] ends it: a page switch puts it
    /// away.
    pub(crate) painter: Option<FormatPainter>,
    /// The formatting Ctrl+Alt+C copied, for Ctrl+Alt+V. Kept across pages, as a copy is.
    pub(crate) copied_format: Option<FormatPainter>,
    /// Where the last picture block was drawn (scene coordinates), for the UI tests.
    #[cfg(test)]
    pub(crate) last_picture_rect: Option<Rect>,
    /// The misspelled word the last right-click landed on, for the menu's suggestions.
    pub(crate) spell_target: Option<SpellTarget>,
    /// The block and caret of the last edit while the caret stays there: the word being
    /// typed is not flagged until it is finished, as in the desktop notebook.
    spell_edit: Option<(Id, usize)>,
    last_undo_at: f64,
    undo_applied: bool,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            active_block: None,
            active_cell: None,
            cell_selection: None,
            selection: None,
            range: None,
            active_style: InlineStyle::default(),
            hovered_block: None,
            title_focused: false,
            pending_style: None,
            find_tags_request: false,
            customize_tags_request: false,
            more_font_colors_request: false,
            tags: TagGallery::default(),
            pending_at: None,
            pending_focus: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_owner: None,
            text_widget: None,
            equation_block: None,
            focus_equation: false,
            link_request: None,
            link_edit: None,
            attachment_action: None,
            picture_clicked: None,
            picture_request: None,
            page_step: None,
            media: MediaView::default(),
            menu_target: None,
            menu_cmd: None,
            mini_on_selection: true,
            mini_bar: None,
            auto_calc: true,
            find_marks: HashMap::new(),
            rows: HashMap::new(),
            line_pitch: None,
            galley_hint: None,
            spell: Speller::default(),
            rich_clip: None,
            copied: false,
            paste_mode: None,
            paste_mode_at: 0.0,
            paste_session: None,
            painter: None,
            copied_format: None,
            #[cfg(test)]
            last_picture_rect: None,
            spell_target: None,
            spell_edit: None,
            last_undo_at: 0.0,
            undo_applied: false,
        }
    }
}

impl EditorState {
    pub fn reset(&mut self) {
        let spell = std::mem::take(&mut self.spell);
        let rich_clip = self.rich_clip.take();
        let copied_format = self.copied_format.take();
        *self = Self {
            spell,
            mini_on_selection: self.mini_on_selection,
            auto_calc: self.auto_calc,
            ..Self::default()
        };
        self.rich_clip = rich_clip;
        self.copied_format = copied_format;
    }

    pub fn focus_title(&mut self, select_all: bool) {
        self.pending_focus = Some(FocusRequest::Title { select_all });
    }

    /// A pending request to put the caret in the page title, `Some(select_all)`. The page
    /// header that draws the title takes it.
    pub fn take_title_focus(&mut self) -> Option<bool> {
        match self.pending_focus {
            Some(FocusRequest::Title { select_all }) => {
                self.pending_focus = None;
                Some(select_all)
            }
            _ => None,
        }
    }

    pub fn focus_block(&mut self, id: u64, pos: usize) {
        self.active_cell = None;
        self.pending_focus = Some(FocusRequest::Block { id, pos });
    }

    /// Puts the caret in a table cell, with `range` selected.
    pub(crate) fn focus_cell(&mut self, block: u64, row: usize, col: usize, range: Range<usize>) {
        self.active_block = Some(block);
        self.active_cell = Some((row, col));
        self.cell_selection = None;
        self.pending_focus = Some(FocusRequest::Cell {
            id: block,
            row,
            col,
            range,
        });
    }

    pub fn pending_block_focus(&self) -> Option<u64> {
        match self.pending_focus {
            Some(
                FocusRequest::Block { id, .. }
                | FocusRequest::Restore { id }
                | FocusRequest::Select { id, .. }
                | FocusRequest::Cell { id, .. },
            ) => Some(id),
            _ => None,
        }
    }

    /// Gives keyboard focus back to the active block without moving its caret.
    pub fn restore_focus(&mut self) {
        // A command that placed the caret itself (a new or deleted block, undo) already
        // asked for focus where it wants it; the old selection must not come back there.
        if matches!(
            self.pending_focus,
            Some(FocusRequest::Block { id, .. } | FocusRequest::Cell { id, .. })
                if self.active_block == Some(id)
        ) {
            return;
        }
        // An equation being typed gets its source field back, the caret where it was.
        if self.equation_block.is_some() && self.equation_block == self.active_block {
            self.focus_equation = true;
            return;
        }
        if let Some(id) = self.active_block {
            self.pending_focus = Some(FocusRequest::Restore { id });
        }
    }

    /// Starts typing equation block `id`: one undo step for the whole edit.
    pub(crate) fn begin_equation(&mut self, doc: &Document, id: u64, now: f64) {
        self.push_undo(doc, now);
        self.active_block = Some(id);
        self.equation_block = Some(id);
        self.focus_equation = true;
        self.selection = None;
        self.range = None;
    }

    /// Starts typing equation block `id`, just put into the note at the caret (whose
    /// insert was the undo step): its source field takes the keyboard, not the paragraph
    /// `insert_blocks_at_caret` left the caret in.
    pub(crate) fn start_equation(&mut self, id: u64) {
        self.active_block = Some(id);
        self.equation_block = Some(id);
        self.focus_equation = true;
        self.selection = None;
        self.pending_focus = None;
    }

    /// Puts the caret in a block with `range` (chars) selected, as closing find on page
    /// leaves the current match.
    pub fn focus_selection(&mut self, block: u64, range: Range<usize>) {
        self.pending_focus = Some(FocusRequest::Select { id: block, range });
    }

    /// Whether a note's text has the keyboard (so Ctrl+K means "link" and Ctrl+Shift+N
    /// "clear formatting", not search and "new notebook").
    pub fn text_has_focus(&self, ctx: &egui::Context) -> bool {
        self.text_widget
            .is_some_and(|id| ctx.memory(|memory| memory.has_focus(id)))
    }

    /// Arms `active_style` for the text typed next at the caret.
    fn arm_style(&mut self) {
        self.pending_style = Some(self.active_style.clone());
        self.pending_at = None;
    }

    pub fn clear_pending_focus(&mut self) {
        self.pending_focus = None;
    }

    pub fn ensure_block_doc(&mut self, doc: &Document) {
        if self.active_block.is_none() {
            self.active_block = doc.blocks.first().map(|block| block.id);
        }
    }

    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.selection = None;
        self.pending_style = None;
    }

    /// The caret comes into note `doc_id`: its history starts afresh, unless it is already
    /// that note's. An insert into a note not drawn yet (a new note at the insertion point)
    /// starts it this way, so its undo step survives the note's first frame.
    fn start_history(&mut self, doc_id: &str) {
        if self.history_owner.as_deref() != Some(doc_id) {
            self.clear_history();
            self.history_owner = Some(doc_id.to_string());
        }
    }

    /// Gives recording `attachment` its length in the undo and redo history too: the length
    /// came once it stopped, and taking back what was typed while it recorded must not
    /// bring back a clip without one.
    pub fn set_clip_length(&mut self, attachment: &str, length_ms: u64) {
        for doc in self.undo_stack.iter_mut().chain(self.redo_stack.iter_mut()) {
            doc.set_clip_length(attachment, length_ms);
        }
    }

    /// Drops the undo history if it belongs to one of `doc_ids`. A change the page makes to
    /// those notes behind the editor's back (a Shift+drag merge, recorded in the page's
    /// history) must never be crossed by the editor's undo, which would bring back the note
    /// from before it and wipe the paragraphs that came in.
    pub fn forget_history_of(&mut self, doc_ids: &[&str]) {
        if self
            .history_owner
            .as_deref()
            .is_some_and(|owner| doc_ids.contains(&owner))
        {
            self.clear_history();
            self.history_owner = None;
        }
    }

    fn push_undo(&mut self, doc: &Document, now: f64) {
        if self.undo_stack.last().is_some_and(|last| last == doc) {
            return;
        }
        self.undo_stack.push(doc.clone());
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.last_undo_at = now;
    }

    fn remember_if_needed(&mut self, doc: &Document, now: f64) {
        // Any new edit invalidates redo, even when it is folded into the last snapshot.
        self.redo_stack.clear();
        if self.undo_stack.last().is_some_and(|last| last == doc) {
            return;
        }
        if now - self.last_undo_at > 0.8 || self.undo_stack.is_empty() {
            self.undo_stack.push(doc.clone());
            if self.undo_stack.len() > 100 {
                self.undo_stack.remove(0);
            }
            self.last_undo_at = now;
        }
    }

    fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let caret = self.caret_place(doc);
        self.redo_stack.push(doc.clone());
        *doc = previous;
        self.after_history_step(doc, caret);
        true
    }

    fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let caret = self.caret_place(doc);
        self.undo_stack.push(doc.clone());
        *doc = next;
        self.after_history_step(doc, caret);
        true
    }

    /// Where the caret is before undo or redo swaps the note: its block's index and the
    /// id of the block above it.
    fn caret_place(&self, doc: &Document) -> (Option<usize>, Option<u64>) {
        let index = self.active_index(doc);
        let above = index
            .and_then(|index| index.checked_sub(1))
            .map(|index| doc.blocks[index].id);
        (index, above)
    }

    /// After undo or redo put `doc` back to another version: the caret goes back into a
    /// block that is still there, so the keyboard never ends up on nothing. A caret block
    /// that went (a row's or a paragraph's undo) hands it to the block above it, at its
    /// end (in a table, its last row), else to the nearest text or table; in a table it
    /// goes to a cell that is still there.
    fn after_history_step(&mut self, doc: &Document, (was, above): (Option<usize>, Option<u64>)) {
        self.selection = None;
        self.range = None;
        self.pending_style = None;
        self.cell_selection = None;
        self.undo_applied = true;
        self.last_undo_at = f64::NEG_INFINITY;
        // It ends the equation being typed: its session was the step (its block may be gone).
        self.equation_block = None;
        self.focus_equation = false;
        let takes_caret = |index: usize| {
            let kind = &doc.blocks[index].kind;
            kind.is_textual() || matches!(kind, BlockKind::Table(_))
        };
        let mut pos = 0;
        if self.active_index(doc).is_none() {
            let above = above
                .and_then(|id| doc.block_index(id))
                .filter(|&index| takes_caret(index));
            let nearest = || {
                let at = was.unwrap_or(0);
                (0..doc.blocks.len())
                    .filter(|&index| takes_caret(index))
                    .min_by_key(|&index| index.abs_diff(at))
            };
            // A note of pictures only: its first block, as before.
            let index = above
                .or_else(nearest)
                .or((!doc.blocks.is_empty()).then_some(0));
            self.active_block = index.map(|index| doc.blocks[index].id);
            self.active_cell = None;
            if let Some(index) = above {
                pos = usize::MAX;
                if let BlockKind::Table(table) = &doc.blocks[index].kind {
                    self.active_cell = Some((table.rows.len().saturating_sub(1), 0));
                }
            }
        }
        match self.active_index(doc).map(|index| &doc.blocks[index].kind) {
            Some(BlockKind::Table(table)) => {
                let (last_row, last_col) = (
                    table.rows.len().saturating_sub(1),
                    table.cols().saturating_sub(1),
                );
                self.active_cell = Some(match self.active_cell {
                    // Its row went (Tab or Enter added it): the end of the table.
                    Some((row, _)) if row > last_row => (last_row, last_col),
                    Some((row, col)) => (row, col.min(last_col)),
                    None => (0, 0),
                });
            }
            _ => self.active_cell = None,
        }
        if let Some(id) = self.active_block {
            self.pending_focus = Some(FocusRequest::Block { id, pos });
        }
    }

    pub(crate) fn active_index(&self, doc: &Document) -> Option<usize> {
        self.active_block.and_then(|id| doc.block_index(id))
    }

    /// The text edit holding the caret in `doc`: a paragraph's, or a table cell's.
    pub(crate) fn caret_widget(&self, doc: &Document, doc_id: &str) -> Option<Id> {
        let block = self.active_block?;
        let index = doc.block_index(block)?;
        Some(match (&doc.blocks[index].kind, self.active_cell) {
            (BlockKind::Table(_), Some((row, col))) => table_cell_id(doc_id, block, row, col),
            _ => block_id(doc_id, block),
        })
    }

    /// The text the caret is in: a paragraph's, or the active cell's. None in a table
    /// with no active cell, or on a picture or divider.
    pub(crate) fn active_text<'a>(&self, doc: &'a Document) -> Option<&'a StyledText> {
        let index = self.active_index(doc)?;
        match (&doc.blocks[index].kind, self.active_cell) {
            (BlockKind::Table(table), Some((row, col))) => {
                table.rows.get(row)?.get(col).map(|cell| &cell.text)
            }
            (kind, _) if kind.is_textual() => Some(&doc.blocks[index].text),
            _ => None,
        }
    }

    pub(crate) fn active_text_mut<'a>(&self, doc: &'a mut Document) -> Option<&'a mut StyledText> {
        let index = self.active_index(doc)?;
        let block = &mut doc.blocks[index];
        if block.kind.is_textual() {
            return Some(&mut block.text);
        }
        match (&mut block.kind, self.active_cell) {
            (BlockKind::Table(table), Some((row, col))) => table
                .rows
                .get_mut(row)?
                .get_mut(col)
                .map(|cell| &mut cell.text),
            _ => None,
        }
    }

    /// The active table cell itself (see [`Self::active_cell_mut`]).
    pub(crate) fn active_cell_ref<'a>(&self, doc: &'a Document) -> Option<&'a TableCell> {
        let index = self.active_index(doc)?;
        let (row, col) = self.active_cell?;
        match &doc.blocks[index].kind {
            BlockKind::Table(table) => table.rows.get(row)?.get(col),
            _ => None,
        }
    }

    /// The active table cell itself.
    pub(crate) fn active_cell_mut<'a>(&self, doc: &'a mut Document) -> Option<&'a mut TableCell> {
        let index = self.active_index(doc)?;
        let (row, col) = self.active_cell?;
        match &mut doc.blocks[index].kind {
            BlockKind::Table(table) => table.rows.get_mut(row)?.get_mut(col),
            _ => None,
        }
    }

    /// The paragraph being typed in, for the ribbon's Tags menu (its check marks).
    pub(crate) fn active_block<'a>(&self, doc: &'a Document) -> Option<&'a Block> {
        self.active_index(doc).map(|index| &doc.blocks[index])
    }

    fn active_style_state(&self, doc: &Document, predicate: impl Fn(&InlineStyle) -> bool) -> bool {
        if let Some(range) = self.range_in(doc) {
            let mut covered = range_segments(doc, range)
                .into_iter()
                .filter(|(_, chars)| !chars.is_empty())
                .peekable();
            return covered.peek().is_some()
                && covered
                    .all(|(index, chars)| doc.blocks[index].text.all_in_range(chars, &predicate));
        }
        if let Some(text) = self.active_text(doc) {
            if let Some(range) = &self.selection {
                return text.all_in_range(range.clone(), &predicate);
            }
        }
        predicate(&self.active_style)
    }

    /// The style the ribbon shows: the selection's first character, else what typing would use.
    pub(crate) fn shown_style(&self, doc: &Document) -> InlineStyle {
        if let Some((start, _)) = self.range_in(doc) {
            if let Some(index) = doc.block_index(start.block) {
                let text = &doc.blocks[index].text;
                return text.style_at(text.char_to_byte(start.char));
            }
        }
        if let (Some(text), Some(range)) = (self.active_text(doc), &self.selection) {
            return text.style_at(text.char_to_byte(range.start));
        }
        self.pending_style
            .clone()
            .unwrap_or_else(|| self.active_style.clone())
    }

    /// The selection across paragraphs, ordered (start, end), when it lies in `doc`.
    pub(crate) fn range_in(&self, doc: &Document) -> Option<(Pos, Pos)> {
        let (a, b) = self.range?;
        let key = |pos: Pos| doc.block_index(pos.block).map(|index| (index, pos.char));
        Some(if key(a)? <= key(b)? { (a, b) } else { (b, a) })
    }

    /// Drops the selection, leaving the caret at `at`.
    fn collapse_to(&mut self, at: Pos) {
        self.range = None;
        self.selection = None;
        self.focus_block(at.block, at.char);
    }

    /// How a paste arriving now goes: the ribbon's choice if fresh, Keep Text Only with
    /// Shift held (Ctrl+Shift+V), else Keep Source Formatting (the desktop notebook's default).
    pub(crate) fn take_paste_mode(&mut self, now: f64, shift: bool) -> PasteMode {
        match self.paste_mode.take() {
            Some(PasteMode::Default) if now - self.paste_mode_at < 1.0 => PasteMode::KeepSource,
            Some(mode) if now - self.paste_mode_at < 1.0 => mode,
            _ if shift => PasteMode::TextOnly,
            _ => PasteMode::KeepSource,
        }
    }
}

pub fn title_id(page: &Page) -> Id {
    Id::unique(("folio-page-title", page.id.as_str()))
}

pub(crate) fn block_id(doc_id: &str, block: u64) -> Id {
    Id::unique(("folio-block", doc_id, block))
}

pub(crate) fn table_cell_id(doc_id: &str, block: u64, row: usize, col: usize) -> Id {
    Id::unique(("folio-cell", doc_id, block, row, col))
}

fn cursor_range(ctx: &egui::Context, id: Id) -> Option<(usize, usize)> {
    let state = TextEditState::load(ctx, id)?;
    let range = state.cursor.char_range()?;
    let a = range.primary.index.0;
    let b = range.secondary.index.0;
    Some((a.min(b), a.max(b)))
}

/// Types `text` at the page title's caret, over its selection (a symbol picked with the
/// caret in the title), and gives the title the keyboard with the caret after it.
pub(crate) fn type_into_title(ctx: &egui::Context, page: &mut Page, text: &str) -> bool {
    let id = title_id(page);
    let count = page.title.chars().count();
    let (min, max) = cursor_range(ctx, id).map_or((count, count), |(min, max)| {
        (min.min(count), max.min(count))
    });
    let byte = |at: usize| {
        page.title
            .char_indices()
            .nth(at)
            .map_or(page.title.len(), |(byte, _)| byte)
    };
    let range = byte(min)..byte(max);
    page.title.replace_range(range, text);
    set_focus(ctx, id, min + text.chars().count(), None);
    true
}

pub(crate) fn set_focus(ctx: &egui::Context, id: Id, pos: usize, select_all: Option<usize>) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    if let Some(all) = select_all {
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(all))));
    } else {
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(pos))));
    }
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

/// Focuses a block's text edit with `range` selected (or the caret at its start).
fn set_focus_range(ctx: &egui::Context, id: Id, range: Range<usize>) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(range.start),
        CCursor::new(range.end),
    )));
    state.store(ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

/// The pointer in this ui's coordinates (canvas notes live in a zoomed, panned layer).
fn local_pointer(ui: &Ui) -> Option<egui::Pos2> {
    let ctx = ui.ctx();
    let pos = ctx.input(|input| input.pointer.hover_pos())?;
    Some(
        ctx.layer_transform_from_global(ui.layer_id())
            .map_or(pos, |transform| transform * pos),
    )
}

/// The char index of the glyph under `pos` (relative to the galley), if any.
fn char_under(galley: &egui::Galley, pos: egui::Pos2) -> Option<usize> {
    let mut index = 0;
    for row in &galley.rows {
        if pos.y >= row.min_y() && pos.y < row.max_y() {
            let x = pos.x - row.pos.x;
            // egui snaps glyphs to whole pixels, so a glyph reaches to where the next one
            // starts: no gap between two letters is "no char".
            let glyphs = &row.glyphs;
            return (0..glyphs.len())
                .find(|&column| {
                    let right = glyphs
                        .get(column + 1)
                        .map_or_else(|| glyphs[column].max_x(), |next| next.pos.x);
                    x >= glyphs[column].pos.x && x < right
                })
                .map(|column| index + column);
        }
        index += row.char_count_including_newline().0;
    }
    None
}

/// The link a menu acts on: under the pointer, around the selection, or before the caret.
fn target_link(text: &StyledText, target: &TextMenuTarget) -> Option<(Range<usize>, Arc<str>)> {
    if let Some(found) = target.pointer_char.and_then(|at| text.link_at(at)) {
        return Some(found);
    }
    let edit = link_edit_for(
        text,
        "",
        target.block,
        (target.range.start, target.range.end),
    );
    edit.existing.then(|| (edit.range, Arc::from(edit.url)))
}

fn link_tooltip(url: &str) -> String {
    let click = click_hint();
    if url.starts_with(PAGE_LINK_PREFIX) {
        t!("editor-link-tip-page", click = click.as_ref()).into_owned()
    } else if url.starts_with(SECTION_LINK_PREFIX) {
        t!("editor-link-tip-section", click = click.as_ref()).into_owned()
    } else {
        t!("editor-link-tip-url", url = url, click = click.as_ref()).into_owned()
    }
}

/// "Ctrl+Click" (or "⌘+Click"), in the language's words.
fn click_hint() -> std::borrow::Cow<'static, str> {
    if cfg!(target_os = "macos") {
        t!("key-cmd-click")
    } else {
        t!("key-ctrl-click")
    }
}

/// The text menu's minimum width: room for "Edit link…" and a shortcut, like the app's
/// other menus. `text_menu_width` widens it when a language's labels need more.
const TEXT_MENU_WIDTH: f32 = 230.0;

/// The mini toolbar's controls are 28 px high, and so are its square buttons.
const MINI_ROW: f32 = 28.0;
/// The room between two of the mini toolbar's controls (its `item_spacing.x`).
const MINI_GAP: f32 = 4.0;
/// The Font and Size boxes, as wide as the ribbon's.
const MINI_FONT_WIDTH: f32 = 120.0;
const MINI_SIZE_WIDTH: f32 = 44.0;
/// Styles ▸ and Tag ▸: a glyph and an arrow.
const MINI_MENU_WIDTH: f32 = 40.0;
/// The mini toolbar's first row, its widest: Font, Size, then Bold, Italic, Underline,
/// Highlight and Font Color, and a little room after it.
const MINI_TOOLBAR_WIDTH: f32 =
    MINI_FONT_WIDTH + MINI_SIZE_WIDTH + 5.0 * MINI_ROW + 6.0 * MINI_GAP + 12.0;
/// The Mini Toolbar's card above the text menu: its two rows, its margins and its 1 px
/// border.
const MINI_CARD_HEIGHT: f32 = 2.0 * MINI_ROW + 2.0 + 12.0 + 2.0;
/// The gap between the Mini Toolbar's card and the menu's.
const MINI_CARD_GAP: f32 = 6.0;

/// The text menu's width: at least `TEXT_MENU_WIDTH`, more when a language's labels and
/// shortcuts need it (measured once per language).
pub(crate) fn text_menu_width(ctx: &egui::Context) -> f32 {
    cached_width(ctx, "folio-text-menu", |ctx| {
        // Each row's right-hand side: its shortcut hint.
        let hint = |text: &str| widgets::text_width(ctx, text, theme::ui(11.5));
        let key = |key: Key| {
            hint(&shortcut_label(
                ctx,
                KeyboardShortcut::new(Modifiers::COMMAND, key),
            ))
        };
        let rows: [(std::borrow::Cow<'static, str>, f32); 13] = [
            (t!("common-cut"), key(Key::X)),
            (t!("common-copy"), key(Key::C)),
            (t!("common-paste"), key(Key::V)),
            (t!("editor-restart-numbering"), 0.0),
            (t!("editor-continue-numbering"), 0.0),
            (t!("editor-edit-link"), key(Key::K)),
            (t!("editor-open-link"), hint(&click_hint())),
            (t!("editor-copy-link"), 0.0),
            (t!("editor-copy-paragraph-link"), 0.0),
            (t!("editor-remove-link"), 0.0),
            (t!("editor-link"), key(Key::K)),
            (t!("spell-add-to-dictionary"), 0.0),
            (t!("spell-ignore-all"), 0.0),
        ];
        rows.iter()
            .map(|(label, right)| {
                // Button padding (10 + 10), glyph and its four spaces, label, a gap, shortcut.
                20.0 + widgets::text_width(
                    ctx,
                    &format!("{}    {label}", icons::SCISSORS),
                    theme::ui(12.5),
                ) + 16.0
                    + right
            })
            .fold(TEXT_MENU_WIDTH, f32::max)
    })
}

fn text_menu_item(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    shortcut: &str,
    enabled: bool,
    t: Tokens,
) -> egui::Response {
    // `widgets::menu_row`, no wider than the text menu, so the two never drift.
    let width = ui.available_width().min(text_menu_width(ui.ctx()));
    ui.scope(|ui| {
        ui.set_max_width(width);
        widgets::menu_row(ui, Some(glyph), label, shortcut, enabled, t)
    })
    .inner
}

/// The right-click menu of note text, in the desktop notebook's order: the spelling suggestions, Cut,
/// Copy and Paste, then the link commands. the desktop notebook's Mini Toolbar on its own card, then
/// the menu's own card under it. Both are one popup, so a click on the toolbar never
/// closes the menu. Returns the command picked, if any.
fn text_context_menu(
    ui: &mut Ui,
    doc: &Document,
    index: usize,
    editor: &mut EditorState,
    target: &TextMenuTarget,
    t: Tokens,
) -> Option<TextMenuCommand> {
    let text = &doc.blocks[index].text;
    let selected = !target.range.is_empty();
    let link = target_link(text, target);
    let ctx = ui.ctx().clone();
    let hint = |key: Key| shortcut_label(&ctx, KeyboardShortcut::new(Modifiers::COMMAND, key));
    let mut command = None;
    let width = text_menu_width(ui.ctx());
    let style = ui.style().clone();
    egui::Frame::menu(&style).show(ui, |ui| {
        ui.set_width(MINI_TOOLBAR_WIDTH);
        if let Some(format) = mini_toolbar(ui, doc, index, editor, t) {
            command = Some(TextMenuCommand::Format(format));
        }
    });
    // The cards are MINI_CARD_GAP apart, the popup's item spacing included, so the menu's
    // card starts at the pointer.
    ui.add_space((MINI_CARD_GAP - ui.spacing().item_spacing.y).max(0.0));
    egui::Frame::menu(&style).show(ui, |ui| {
        ui.set_width(width);
        // menu-section: spelling (it draws its own separator after it)
        if let Some(picked) = spelling_menu(ui, doc, index, editor, target, t) {
            command = Some(picked);
        }
        if text_menu_item(
            ui,
            icons::SCISSORS,
            &t!("common-cut"),
            &hint(Key::X),
            selected,
            t,
        )
        .clicked()
        {
            command = Some(TextMenuCommand::Cut);
        }
        if text_menu_item(
            ui,
            icons::COPY,
            &t!("common-copy"),
            &hint(Key::C),
            selected,
            t,
        )
        .clicked()
        {
            command = Some(TextMenuCommand::Copy);
        }
        if text_menu_item(
            ui,
            icons::CLIPBOARD_TEXT,
            &t!("common-paste"),
            &hint(Key::V),
            true,
            t,
        )
        .clicked()
        {
            command = Some(TextMenuCommand::Paste);
        }
        ui.separator();
        // A numbered item's count: the desktop notebook's Restart Numbering and Continue Numbering.
        if matches!(doc.blocks[index].kind, BlockKind::Numbered) {
            for (glyph, label, start) in [
                (
                    icons::ARROW_RESET,
                    t!("editor-restart-numbering"),
                    ListStart::At(1),
                ),
                (
                    icons::ARROW_RIGHT,
                    t!("editor-continue-numbering"),
                    ListStart::Continue,
                ),
            ] {
                if text_menu_item(ui, glyph, &label, "", true, t).clicked() {
                    command = Some(TextMenuCommand::Format(ToolbarCommand::ListStart(Some(
                        start,
                    ))));
                }
            }
            ui.separator();
        }
        if link.is_some() {
            let open = click_hint();
            if text_menu_item(
                ui,
                icons::ARROW_SQUARE_OUT,
                &t!("editor-open-link"),
                &open,
                true,
                t,
            )
            .clicked()
            {
                command = Some(TextMenuCommand::OpenLink);
            }
            if text_menu_item(
                ui,
                icons::LINK,
                &t!("editor-edit-link"),
                &hint(Key::K),
                true,
                t,
            )
            .clicked()
            {
                command = Some(TextMenuCommand::Link);
            }
        } else if text_menu_item(ui, icons::LINK, &t!("editor-link"), &hint(Key::K), true, t)
            .clicked()
        {
            command = Some(TextMenuCommand::Link);
        }
        // the desktop notebook's order: Copy Link to Paragraph after Link…, before Copy Link.
        if text_menu_item(
            ui,
            icons::COPY_LINK,
            &t!("editor-copy-paragraph-link"),
            "",
            true,
            t,
        )
        .clicked()
        {
            command = Some(TextMenuCommand::CopyParagraphLink);
        }
        if link.is_some() {
            if text_menu_item(ui, icons::COPY, &t!("editor-copy-link"), "", true, t).clicked() {
                command = Some(TextMenuCommand::CopyLink);
            }
            if text_menu_item(
                ui,
                icons::LINK_BREAK,
                &t!("editor-remove-link"),
                "",
                true,
                t,
            )
            .clicked()
            {
                command = Some(TextMenuCommand::RemoveLink);
            }
        }
    });
    if command.is_some() {
        ui.close();
    }
    command
}

/// the desktop notebook's mini toolbar, on its card above the text menu or over a selection, in two
/// rows: Font, Size, Bold, Italic, Underline, Highlight and Font Color over Bullets,
/// Numbering, the two indents, Styles ▸ and Tag ▸. Each control is a button named as its
/// ribbon twin is, and shows the paragraph's state as the ribbon does. Returns the command
/// picked.
fn mini_toolbar(
    ui: &mut Ui,
    doc: &Document,
    index: usize,
    editor: &mut EditorState,
    t: Tokens,
) -> Option<ToolbarCommand> {
    use ToolbarCommand as Do;
    let block = &doc.blocks[index];
    // What the ribbon shows for the paragraph the menu opened on; its kind's defaults when
    // the caret is in another one.
    let state = text_state(doc, editor).filter(|_| editor.active_index(doc) == Some(index));
    let (default_px, _, default_face) = kind_defaults(&block.kind);
    let face = state.as_ref().map_or(default_face, |state| state.face);
    let pt = state
        .as_ref()
        .map_or(px_to_pt(default_px), |state| state.pt);
    let marked = |mark: Mark| state.as_ref().is_some_and(|state| state.mark(mark));
    let mut picked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(MINI_GAP, 2.0);
        ui.horizontal(|ui| {
            let font = mini_combo(ui, MINI_FONT_WIDTH, &t!("editor-font"), face.label(), t);
            if let Some(face) = mini_submenu(ui, &font, |ui| font_list(ui, face)) {
                picked = Some(Do::Face(face));
            }
            let size = format!("{}", pt.round());
            let size = mini_combo(ui, MINI_SIZE_WIDTH, &t!("editor-font-size"), &size, t);
            if let Some(size) = mini_submenu(ui, &size, |ui| size_list(ui, pt)) {
                picked = Some(Do::Size(size));
            }
            for (mark, glyph, label) in [
                (Mark::Bold, icons::TEXT_B, t!("editor-bold")),
                (Mark::Italic, icons::TEXT_ITALIC, t!("editor-italic")),
                (
                    Mark::Underline,
                    icons::TEXT_UNDERLINE,
                    t!("editor-underline"),
                ),
            ] {
                if mini_button(ui, glyph, &label, Some(marked(mark)), t).clicked() {
                    picked = Some(Do::Toggle(mark));
                }
            }
            let highlight = mini_color_button(
                ui,
                icons::HIGHLIGHTER,
                Rgb::TEXT_HIGHLIGHTS[0].0,
                &t!("editor-highlight"),
                t,
            );
            if let Some(color) = mini_submenu(ui, &highlight, |ui| highlight_menu(ui, t)) {
                picked = Some(Do::Highlight(color));
            }
            let color = mini_color_button(
                ui,
                icons::TEXT_COLOR,
                crate::ribbon::FONT_RED,
                &t!("editor-text-color"),
                t,
            );
            match mini_submenu(ui, &color, |ui| text_color_menu(ui, t)) {
                Some(ColorPick::Color(color)) => picked = Some(Do::Color(color)),
                // A dialog, which no toolbar command opens: the app reads the request.
                Some(ColorPick::More) => editor.more_font_colors_request = true,
                None => {}
            }
        });
        ui.horizontal(|ui| {
            for (glyph, label, list) in [
                (
                    icons::TEXT_BULLET_LIST,
                    t!("ribbon-bullets"),
                    BlockKind::Bullet,
                ),
                (
                    icons::LIST_NUMBERS,
                    t!("ribbon-numbering"),
                    BlockKind::Numbered,
                ),
            ] {
                let on = same_kind(&block.kind, &list);
                if mini_button(ui, glyph, &label, Some(on), t).clicked() {
                    picked = Some(Do::SetKind(list));
                }
            }
            for (glyph, label, delta) in [
                (
                    icons::TEXT_INDENT_DECREASE_LTR,
                    t!("ribbon-decrease-indent"),
                    -1,
                ),
                (
                    icons::TEXT_INDENT_INCREASE_LTR,
                    t!("ribbon-increase-indent"),
                    1,
                ),
            ] {
                if mini_button(ui, glyph, &label, None, t).clicked() {
                    picked = Some(Do::Indent(delta));
                }
            }
            let styles = mini_menu_button(ui, icons::TEXT_HEADER_1, &t!("ribbon-group-styles"), t);
            if let Some(command) =
                mini_submenu(ui, &styles, |ui| styles_menu(ui, Some(&block.kind), t))
            {
                picked = Some(command);
            }
            let tag = mini_menu_button(ui, icons::TAG, &t!("editor-tag"), t);
            if let Some(command) = mini_submenu(ui, &tag, |ui| {
                tag_menu_contents(ui, Some(block), &editor.tags, t)
            }) {
                picked = Some(Do::Tag(command));
            }
        });
    });
    picked
}

/// Room for one mini-toolbar control, `width` wide: a button named `name`, as its ribbon
/// twin is, `selected` when it is a toggle.
fn mini_control(
    ui: &mut Ui,
    width: f32,
    name: &str,
    checked: Option<bool>,
) -> (Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(width, MINI_ROW), Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Button, true, name);
        info.selected = checked;
        info
    });
    (rect, response)
}

/// The ribbon's fills behind a control: grey with a grey frame when checked, a lighter
/// grey under the pointer.
fn mini_back(ui: &Ui, rect: Rect, response: &egui::Response, checked: bool, t: Tokens) {
    let painter = ui.painter();
    if checked {
        painter.rect_filled(rect, CornerRadius::same(4), t.nav_selected);
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, t.container_border),
            StrokeKind::Inside,
        );
    }
    if response.hovered() || response.has_focus() {
        painter.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
}

/// A square button with a glyph; a toggle when `checked` is given. Its name is its
/// tooltip.
fn mini_button(
    ui: &mut Ui,
    glyph: &str,
    name: &str,
    checked: Option<bool>,
    t: Tokens,
) -> egui::Response {
    let (rect, response) = mini_control(ui, MINI_ROW, name, checked);
    if ui.is_rect_visible(rect) {
        mini_back(ui, rect, &response, checked == Some(true), t);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            theme::icon(16.0),
            t.ink,
        );
    }
    response.on_hover_text(name)
}

/// The Font or Size box: the current value in a framed box with a chevron, as the
/// ribbon's combo boxes draw it. Hovering it opens its list (`mini_submenu`).
fn mini_combo(ui: &mut Ui, width: f32, name: &str, value: &str, t: Tokens) -> egui::Response {
    let (rect, response) = mini_control(ui, width, name, None);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let field = rect.shrink2(vec2(0.0, 2.0));
        let border = if response.hovered() {
            t.muted
        } else {
            t.line_strong
        };
        painter.rect_filled(field, CornerRadius::same(4), t.raised);
        painter.rect_stroke(
            field,
            CornerRadius::same(4),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        widgets::fitted_text(
            painter,
            pos2(field.left() + 6.0, field.center().y),
            Align2::LEFT_CENTER,
            value,
            theme::ui(12.0),
            t.ink,
            field.width() - 6.0 - 18.0,
        );
        painter.text(
            pos2(field.right() - 9.0, field.center().y),
            Align2::CENTER_CENTER,
            icons::CARET_DOWN,
            theme::icon(10.0),
            t.muted,
        );
    }
    response
}

/// Highlight or Font Color: the glyph over a bar of the colour it applies, as on the
/// ribbon. Hovering it opens the colours (`mini_submenu`).
fn mini_color_button(
    ui: &mut Ui,
    glyph: &str,
    color: Rgb,
    name: &str,
    t: Tokens,
) -> egui::Response {
    let (rect, response) = mini_control(ui, MINI_ROW, name, None);
    if ui.is_rect_visible(rect) {
        mini_back(ui, rect, &response, false, t);
        let center = rect.center() - vec2(0.0, 2.0);
        let painter = ui.painter();
        painter.text(
            center,
            Align2::CENTER_CENTER,
            glyph,
            theme::icon(16.0),
            t.ink,
        );
        painter.rect_filled(
            Rect::from_center_size(pos2(center.x, center.y + 7.5), vec2(12.0, 3.0)),
            CornerRadius::ZERO,
            color.to_color32(),
        );
    }
    response
}

/// Styles ▸ or Tag ▸: the glyph and a chevron. Hovering it opens its menu
/// (`mini_submenu`).
fn mini_menu_button(ui: &mut Ui, glyph: &str, name: &str, t: Tokens) -> egui::Response {
    let (rect, response) = mini_control(ui, MINI_MENU_WIDTH, name, None);
    if ui.is_rect_visible(rect) {
        mini_back(ui, rect, &response, false, t);
        let painter = ui.painter();
        painter.text(
            pos2(rect.left() + 6.0 + 8.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            theme::icon(16.0),
            t.ink,
        );
        painter.text(
            pos2(rect.right() - 9.0, rect.center().y),
            Align2::CENTER_CENTER,
            icons::CARET_DOWN,
            theme::icon(10.0),
            t.muted,
        );
    }
    response
}

/// Opens `contents` beside a mini-toolbar control as the text menu's submenu, as
/// `ui.menu_button` does inside a menu: shown while the control is hovered, and a pick
/// closes the whole menu.
fn mini_submenu<R>(
    ui: &Ui,
    control: &egui::Response,
    contents: impl FnOnce(&mut Ui) -> Option<R>,
) -> Option<R> {
    egui::menu::SubMenu::new()
        .show(ui, control, |ui| {
            let picked = contents(ui);
            if picked.is_some() {
                ui.close();
            }
            picked
        })
        .and_then(|shown| shown.inner)
}

/// How far the pointer may go from the Mini Toolbar over a selection before it goes. It
/// fades on the way, as Office's does.
const MINI_FAR: f32 = 160.0;

/// The Mini Toolbar over a selection: solid with the pointer on it or within 8 px, faded
/// further away, and gone (transparent) at `MINI_FAR`. Not drawn yet, it is faded.
fn mini_bar_opacity(pointer: Option<Pos2>, rect: Option<Rect>) -> f32 {
    match (pointer, rect) {
        (Some(pointer), Some(rect)) => {
            let distance = rect.distance_to_pos(pointer);
            if distance <= 8.0 {
                1.0
            } else {
                0.55 * (1.0 - distance / MINI_FAR).clamp(0.0, 1.0)
            }
        }
        _ => 0.55,
    }
}

/// Office's Mini Toolbar on a selection: it shows after text is selected with the mouse
/// (Options › General › Show Mini Toolbar on selection), and goes on a key, typing, a
/// press elsewhere on the page, a scroll, the end of the selection, or the pointer moving
/// well away.
fn track_mini_bar(ui: &Ui, doc: &Document, doc_id: &str, editor: &mut EditorState) {
    let ctx = ui.ctx();
    // The editor's own record of the selection: the text box's collapses when a press (on
    // the bar, on the ribbon) takes the keyboard from it, while the selection stays.
    let selected = |editor: &EditorState, block: u64| {
        (editor.active_block == Some(block)
            && editor
                .selection
                .as_ref()
                .is_some_and(|range| !range.is_empty()))
            || editor.range_in(doc).is_some()
    };
    // Over something other than the page: the bar itself, or one of its lists (Font,
    // Size, the colours…), which hang outside its card.
    let off_page = |pos: Pos2| {
        ctx.layer_id_at(pos)
            .is_some_and(|layer| layer != ui.layer_id() && layer != egui::LayerId::background())
    };
    let (pointer, press, keyed) = ctx.input(|input| {
        let keyed = input.events.iter().any(|event| match event {
            egui::Event::Key {
                key, pressed: true, ..
            } => !matches!(
                key,
                Key::ShiftLeft
                    | Key::ShiftRight
                    | Key::ControlLeft
                    | Key::ControlRight
                    | Key::AltLeft
                    | Key::AltRight
                    | Key::SuperLeft
                    | Key::SuperRight
            ),
            // Typing through an input method (not the IME's own comings and goings).
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => !text.is_empty(),
            egui::Event::Ime(egui::ImeEvent::Commit(_))
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Cut
            | egui::Event::Copy
            | egui::Event::MouseWheel { .. } => true,
            _ => false,
        });
        let press = input
            .pointer
            .any_pressed()
            .then(|| input.pointer.press_origin())
            .flatten();
        (input.pointer.latest_pos(), press, keyed)
    });
    if let Some(bar) = editor.mini_bar.as_ref().filter(|bar| bar.doc_id == doc_id) {
        let pressed_away = press
            .is_some_and(|pos| !bar.rect.is_some_and(|rect| rect.contains(pos)) && !off_page(pos));
        let far = pointer
            .zip(bar.rect)
            .is_some_and(|(pos, rect)| !off_page(pos) && rect.distance_to_pos(pos) > MINI_FAR);
        let gone = doc.block_index(bar.block).is_none() || !selected(editor, bar.block);
        if keyed || pressed_away || far || gone {
            editor.mini_bar = None;
        }
    }
    if !editor.mini_on_selection {
        return;
    }
    // Text is selected with the mouse by a drag across it, or by a double click (a word) or
    // a triple click (the paragraph), as in Office.
    let dragged = ctx.drag_stopped_id();
    let clicked = ctx.input(|input| {
        input
            .pointer
            .button_double_clicked(egui::PointerButton::Primary)
            || input
                .pointer
                .button_triple_clicked(egui::PointerButton::Primary)
    });
    let Some(at) = pointer.filter(|_| dragged.is_some() || clicked) else {
        return;
    };
    let block = doc
        .blocks
        .iter()
        .find(|block| {
            let id = block_id(doc_id, block.id);
            block.kind.is_textual()
                && (dragged == Some(id)
                    || clicked
                        && ctx.read_response(id).is_some_and(|response| {
                            response.double_clicked() || response.triple_clicked()
                        }))
        })
        .map(|block| block.id);
    if let Some(block) = block.filter(|block| selected(editor, *block)) {
        editor.mini_bar = Some(MiniBar {
            doc_id: doc_id.to_owned(),
            block,
            at,
            rect: None,
        });
    }
}

/// Draws the Mini Toolbar over a selection, above where the selecting drag or click ended,
/// faded by the pointer's distance. A pick runs as its ribbon twin does
/// (`EditorState::menu_cmd`), on the selection, which a press on the bar keeps; the bar
/// stays, as Office's does.
fn show_mini_bar(ui: &Ui, doc: &Document, doc_id: &str, editor: &mut EditorState, t: Tokens) {
    let Some(mut bar) = editor.mini_bar.clone().filter(|bar| bar.doc_id == doc_id) else {
        return;
    };
    let Some(index) = doc.block_index(bar.block) else {
        return;
    };
    let ctx = ui.ctx().clone();
    let pointer = ctx.input(|input| input.pointer.latest_pos());
    let area = egui::Area::new(Id::unique("folio-mini-bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(bar.at - vec2(24.0, MINI_CARD_HEIGHT + 14.0))
        .show(&ctx, |ui| {
            ui.multiply_opacity(mini_bar_opacity(pointer, bar.rect));
            egui::Frame::menu(ui.style())
                .show(ui, |ui| {
                    ui.set_width(MINI_TOOLBAR_WIDTH);
                    mini_toolbar(ui, doc, index, editor, t)
                })
                .inner
        });
    bar.rect = Some(area.response.rect);
    // A click on the bar that picks nothing (between its buttons, or on one that opens a
    // list) gives the keyboard back to the note with its selection, as a pick does: the
    // bar never keeps it.
    let released = ctx.input(|input| {
        input
            .pointer
            .any_released()
            .then(|| input.pointer.interact_pos())
            .flatten()
    });
    let clicked_bar = released.is_some_and(|pos| {
        area.response.rect.contains(pos) && ctx.layer_id_at(pos) == Some(area.response.layer_id)
    });
    if let Some(command) = area.inner {
        editor.menu_cmd = Some(command);
    } else if clicked_bar {
        editor.restore_focus();
    }
    editor.mini_bar = Some(bar);
}

/// Runs a text-menu command. The text edit lost focus to the menu, so every command
/// hands focus back to the block (with the selection) unless it moves the caret itself.
fn apply_text_menu_command(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    target: &TextMenuTarget,
    command: TextMenuCommand,
) -> bool {
    let Some(index) = doc.block_index(target.block) else {
        return false;
    };
    let id = block_id(doc_id, target.block);
    let range = target.range.clone();
    let now = ctx.input(|input| input.time);
    let selected: String = doc.blocks[index]
        .text
        .text
        .chars()
        .skip(range.start)
        .take(range.len())
        .collect();
    // Over a selection across paragraphs, Cut and Copy take all of it.
    if let Some(range) = editor.range_in(doc) {
        match command {
            TextMenuCommand::Cut => {
                copy_range(ctx, doc, editor, range);
                editor.push_undo(doc, now);
                let at = delete_range(doc, range);
                editor.collapse_to(at);
                return true;
            }
            TextMenuCommand::Copy => {
                copy_range(ctx, doc, editor, range);
                return false;
            }
            _ => {}
        }
    }
    editor.active_block = Some(target.block);
    // Copy and Cut keep the chars' formatting for a paste in Folio.
    let piece = || doc.blocks[index].text.slice_chars(range.clone());
    match command {
        TextMenuCommand::Cut => {
            if range.is_empty() {
                return false;
            }
            ctx.copy_text(selected);
            keep_inline_clip(editor, &doc.blocks[index].kind, piece());
            editor.push_undo(doc, now);
            doc.blocks[index].text.delete_chars(range.clone());
            editor.selection = None;
            editor.focus_block(target.block, range.start);
            true
        }
        TextMenuCommand::Copy => {
            if !range.is_empty() {
                ctx.copy_text(selected);
                keep_inline_clip(editor, &doc.blocks[index].kind, piece());
            }
            set_focus_range(ctx, id, range);
            false
        }
        TextMenuCommand::Paste => {
            // egui can't read the clipboard on demand; the platform answers this with a
            // paste event for the focused text next frame.
            set_focus_range(ctx, id, range);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            false
        }
        TextMenuCommand::Format(command) => {
            // The ribbon's routing runs it once the page is drawn, on the text the menu
            // opened on, so the mini toolbar does exactly what its ribbon twin does.
            editor.selection = (!range.is_empty()).then(|| range.clone());
            set_focus_range(ctx, id, range);
            editor.menu_cmd = Some(command);
            false
        }
        TextMenuCommand::Link => {
            // The menu said "Edit link…" for the link under the pointer (or around the
            // selection), even when the caret landed just before it.
            let text = &doc.blocks[index].text;
            editor.link_edit = Some(match target_link(text, target) {
                Some((link, url)) => LinkEdit {
                    doc_id: doc_id.to_string(),
                    block: target.block,
                    text: text
                        .text
                        .chars()
                        .skip(link.start)
                        .take(link.len())
                        .collect(),
                    range: link,
                    url: url.to_string(),
                    existing: true,
                },
                None => link_edit_for(text, doc_id, target.block, (range.start, range.end)),
            });
            false
        }
        TextMenuCommand::OpenLink => {
            if let Some((_, url)) = target_link(&doc.blocks[index].text, target) {
                editor.link_request = Some(url.to_string());
            }
            false
        }
        TextMenuCommand::CopyLink => {
            if let Some((_, url)) = target_link(&doc.blocks[index].text, target) {
                ctx.copy_text(url.to_string());
            }
            set_focus_range(ctx, id, range);
            false
        }
        TextMenuCommand::CopyParagraphLink => {
            // Note ids are "<page id>:<item id>".
            if let Some((page, _)) = doc_id.rsplit_once(':') {
                ctx.copy_text(crate::model::paragraph_link(page, doc.blocks[index].id));
            }
            set_focus_range(ctx, id, range);
            false
        }
        TextMenuCommand::RemoveLink => {
            let Some((link, _)) = target_link(&doc.blocks[index].text, target) else {
                return false;
            };
            editor.push_undo(doc, now);
            doc.blocks[index].text.set_link(link, None);
            set_focus_range(ctx, id, range);
            true
        }
        TextMenuCommand::ReplaceWord {
            range: misspelled,
            word,
        } => {
            editor.push_undo(doc, now);
            editor.selection = None;
            replace_chars(doc, index, misspelled, &word, editor);
            true
        }
        TextMenuCommand::AddToDictionary(word) => {
            editor.spell.add_personal(&word);
            set_focus_range(ctx, id, range);
            false
        }
        TextMenuCommand::IgnoreWord(word) => {
            editor.spell.ignore_all(&word);
            set_focus_range(ctx, id, range);
            false
        }
    }
}

/// Runs a ribbon formatting command on `doc`, a note's text: what the toolbar's buttons
/// did. The caller gives focus back to the text afterwards.
pub(crate) fn apply_command(
    ctx: &egui::Context,
    doc: &mut Document,
    editor: &mut EditorState,
    command: ToolbarCommand,
) -> ToolbarResult {
    editor.ensure_block_doc(doc);
    let now = ctx.input(|input| input.time);
    let mut changed = false;
    match command {
        ToolbarCommand::Undo => changed |= editor.undo(doc),
        ToolbarCommand::Redo => changed |= editor.redo(doc),
        ToolbarCommand::Toggle(mark) => {
            changed |= toggle_mark(doc, editor, mark, now);
        }
        ToolbarCommand::Highlight(color) => {
            changed |= set_highlight(doc, editor, color, now);
        }
        ToolbarCommand::Color(color) => {
            changed |= set_color(doc, editor, color, now);
        }
        ToolbarCommand::Face(face) => {
            changed |= apply_inline(doc, editor, now, |style| style.font = Some(face));
        }
        ToolbarCommand::Size(size) => {
            changed |= apply_inline(doc, editor, now, |style| style.size = Some(size));
        }
        ToolbarCommand::Align(align) => changed |= set_align(doc, editor, align, now),
        ToolbarCommand::ClearFormatting => changed |= clear_formatting(doc, editor, now),
        ToolbarCommand::SetKind(kind) => {
            if let Some(index) = editor.active_index(doc) {
                changed |= set_block_kind(doc, editor, index, kind, now);
            }
        }
        ToolbarCommand::DeleteBlock => {
            if let Some(range) = editor.range_in(doc) {
                // A selection across paragraphs goes whole, as the Delete key takes it.
                editor.push_undo(doc, now);
                let at = delete_range(doc, range);
                editor.active_block = Some(at.block);
                editor.collapse_to(at);
                changed = true;
            } else if let Some(index) = editor.active_index(doc) {
                let chars = doc.blocks[index].text.char_count();
                let part = editor
                    .selection
                    .clone()
                    .map(|range| range.start.min(chars)..range.end.min(chars))
                    .filter(|range| !range.is_empty() && range.len() < chars);
                editor.push_undo(doc, now);
                match part {
                    // Part of the paragraph selected: only that goes, as the desktop notebook's Delete
                    // takes the selection, and the caret stays where it was.
                    Some(range) => {
                        doc.blocks[index].text.delete_chars(range.clone());
                        editor.selection = None;
                        editor.focus_block(doc.blocks[index].id, range.start);
                        changed = true;
                    }
                    // A caret, or the whole paragraph selected: the paragraph goes.
                    None => changed |= delete_block(doc, editor, index),
                }
            }
        }
        ToolbarCommand::Tag(picked) => {
            if picked == TagCommand::FindTags {
                editor.find_tags_request = true;
            } else if let Some(index) = editor.active_index(doc) {
                changed |= apply_tag_command(doc, editor, index, picked, now);
            }
        }
        ToolbarCommand::Cut | ToolbarCommand::Copy => {
            let cut = matches!(command, ToolbarCommand::Cut);
            changed |= copy_or_cut(ctx, doc, editor, cut, now);
        }
        ToolbarCommand::Paste => {
            // egui can't read the clipboard on demand: the platform answers with a paste
            // event for the focused text next frame (as the text menu's Paste does).
            editor.restore_focus();
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        ToolbarCommand::Indent(delta) => {
            changed |= shift_level(doc, editor, delta, now);
        }
        ToolbarCommand::ListMarker(marker) => {
            changed |= set_list_marker(doc, editor, marker, now);
        }
        ToolbarCommand::ListStart(start) => {
            changed |= set_list_start(doc, editor, start, now);
        }
        ToolbarCommand::Numbering(marker, start) => {
            changed |= set_numbering(doc, editor, marker, start, now);
        }
        ToolbarCommand::Spacing(spacing) => {
            changed |= set_spacing(doc, editor, spacing, now);
        }
    }
    ToolbarResult {
        changed,
        interacted: true,
    }
}

/// Copies (and for Cut, removes) the selected text, as Ctrl+C and Ctrl+X do, for the
/// ribbon's Cut and Copy: a selection across paragraphs whole, cells selected across a
/// table as a table, else the part of the paragraph or cell being typed in. Formats are
/// kept for a paste in Folio.
fn copy_or_cut(
    ctx: &egui::Context,
    doc: &mut Document,
    editor: &mut EditorState,
    cut: bool,
    now: f64,
) -> bool {
    if let Some(range) = editor.range_in(doc) {
        copy_range(ctx, doc, editor, range);
        if !cut {
            return false;
        }
        editor.push_undo(doc, now);
        let at = delete_range(doc, range);
        editor.active_block = Some(at.block);
        editor.collapse_to(at);
        return true;
    }
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    let id = doc.blocks[index].id;
    if let Some((_, cells)) = editor
        .cell_selection
        .filter(|(owner, cells)| *owner == id && !cells.is_single())
    {
        return copy_cells(ctx, doc, index, cells, editor, cut, now);
    }
    let Some(text) = editor.active_text(doc) else {
        return false;
    };
    let chars = text.char_count();
    let Some(range) = editor
        .selection
        .clone()
        .map(|range| range.start.min(chars)..range.end.min(chars))
        .filter(|range| !range.is_empty())
    else {
        return false;
    };
    let piece = text.slice_chars(range.clone());
    ctx.copy_text(piece.text.clone());
    keep_inline_clip(editor, &doc.blocks[index].kind, piece);
    if !cut {
        return false;
    }
    editor.push_undo(doc, now);
    if let Some(text) = editor.active_text_mut(doc) {
        text.delete_chars(range.clone());
    }
    editor.selection = None;
    match (&doc.blocks[index].kind, editor.active_cell) {
        (BlockKind::Table(_), Some((row, col))) => {
            editor.focus_cell(id, row, col, range.start..range.start)
        }
        _ => editor.focus_block(id, range.start),
    }
    true
}

/// Home › Increase Indent (`delta` > 0) and Decrease Indent (`delta` < 0): Tab and
/// Shift+Tab's outline step, on the paragraph with the caret (or the selected ones) and
/// their subparagraphs. One undo step; nothing moves past the first or the last level.
fn shift_level(doc: &mut Document, editor: &mut EditorState, delta: i8, now: f64) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    let span = outline_span(doc, editor, index);
    undo_step(doc, editor, now, |doc| {
        let mut changed = false;
        for _ in 0..delta.unsigned_abs() {
            changed |= doc.shift_blocks(span.clone(), delta > 0);
        }
        changed
    })
}

/// Home › Bullets ▾ and Numbering ▾: makes the paragraph with the caret (or the selected
/// ones) list items in `marker`'s style, keeping their levels, or ends their list for
/// `None`. One undo step.
fn set_list_marker(
    doc: &mut Document,
    editor: &mut EditorState,
    marker: Option<ListMarker>,
    now: f64,
) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    let targets = list_targets(doc, editor, index);
    undo_step(doc, editor, now, |doc| mark_list(doc, &targets, marker))
}

/// The paragraphs a list style goes on: the one at `index`, or the selected ones, that
/// hold text.
fn list_targets(doc: &Document, editor: &EditorState, index: usize) -> Vec<usize> {
    targets(doc, editor, index)
        .into_iter()
        .filter(|&i| doc.blocks[i].kind.is_textual())
        .collect()
}

/// Gives `targets` `marker`'s list style, or ends their list for `None`. Whether any
/// changed.
fn mark_list(doc: &mut Document, targets: &[usize], marker: Option<ListMarker>) -> bool {
    let mut changed = false;
    for &i in targets {
        let block = &mut doc.blocks[i];
        let (kind, picked) = match marker {
            Some(marker) if marker.numbered() => (BlockKind::Numbered, Some(marker)),
            Some(marker) => (BlockKind::Bullet, Some(marker)),
            // "None" takes the bullet or number off; other paragraphs keep their style.
            None if matches!(block.kind, BlockKind::Bullet | BlockKind::Numbered) => {
                (BlockKind::Paragraph, None)
            }
            None => continue,
        };
        if block.kind != kind || block.marker != picked {
            set_kind(block, kind);
            block.marker = picked;
            changed = true;
        }
    }
    changed
}

/// Gives `block` `kind`. A paragraph made a numbered item counts on from the item before:
/// a start it kept from when it was one before (Restart Numbering, Start at) does not come
/// back, as the page would read it once reopened (`Document::repair`).
fn set_kind(block: &mut Block, kind: BlockKind) {
    if !matches!(block.kind, BlockKind::Numbered) {
        block.start = None;
    }
    block.kind = kind;
}

/// Restart Numbering, Continue Numbering or Customize Numbering's Start at, for the
/// numbered item at the caret: one undo step. Other paragraphs have no count to change.
fn set_list_start(
    doc: &mut Document,
    editor: &mut EditorState,
    start: Option<ListStart>,
    now: f64,
) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    if !matches!(doc.blocks[index].kind, BlockKind::Numbered) || doc.blocks[index].start == start {
        return false;
    }
    undo_step(doc, editor, now, |doc| {
        doc.blocks[index].start = start;
        true
    })
}

/// Customize Numbering…'s OK: `marker`'s style on the paragraph with the caret (or the
/// selected ones), then `start` on the item at the caret, which the style has just made
/// numbered. One undo step for both.
fn set_numbering(
    doc: &mut Document,
    editor: &mut EditorState,
    marker: ListMarker,
    start: ListStart,
    now: f64,
) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    let targets = list_targets(doc, editor, index);
    undo_step(doc, editor, now, |doc| {
        let changed = mark_list(doc, &targets, Some(marker));
        if !matches!(doc.blocks[index].kind, BlockKind::Numbered) {
            return changed;
        }
        // A start the item would get anyway is not kept: it counts on from the items
        // above, so one added above it later still moves its number on.
        let kept = doc.blocks[index].start.take();
        let natural = crate::model::list_numbers(&doc.blocks)[index];
        let start = match start {
            ListStart::At(number) if number as usize == natural => None,
            start => Some(start),
        };
        doc.blocks[index].start = start;
        changed | (start != kept)
    })
}

/// Paragraph Spacing Options…' OK: `spacing` on the paragraph with the caret, or on the
/// selected ones. One undo step.
fn set_spacing(
    doc: &mut Document,
    editor: &mut EditorState,
    mut spacing: ParaSpacing,
    now: f64,
) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    spacing.repair();
    let targets = targets(doc, editor, index);
    undo_step(doc, editor, now, |doc| {
        let mut changed = false;
        for i in targets {
            if doc.blocks[i].spacing != spacing {
                doc.blocks[i].spacing = spacing;
                changed = true;
            }
        }
        changed
    })
}

/// A width that depends only on the language's labels (and the display scale): measured
/// once per language and scale and kept in egui's memory, so every later frame is a cheap
/// read. It takes the context rather than a `Ui`, so tests can measure too.
fn cached_width(
    ctx: &egui::Context,
    key: &'static str,
    measure: impl FnOnce(&egui::Context) -> f32,
) -> f32 {
    cached_width_rev(ctx, key, 0, measure)
}

/// [`cached_width`] for labels that also change with a `revision`, as the tag gallery's do
/// when Customize Tags changes them.
fn cached_width_rev(
    ctx: &egui::Context,
    key: &'static str,
    revision: u64,
    measure: impl FnOnce(&egui::Context) -> f32,
) -> f32 {
    let id = Id::unique((
        key,
        revision,
        crate::i18n::lang().code(),
        ctx.pixels_per_point().to_bits(),
    ));
    if let Some(width) = ctx.data(|data| data.get_temp::<f32>(id)) {
        return width;
    }
    let width = measure(ctx);
    ctx.data_mut(|data| data.insert_temp(id, width));
    width
}

/// The note fonts as menu entries, each in its own face, the current one marked. Returns
/// the face clicked.
pub(crate) fn font_list(ui: &mut Ui, current: FontFace) -> Option<FontFace> {
    ui.set_min_width(200.0);
    let mut picked = None;
    for option in FontFace::ALL {
        let family = face_family(option, false, false).0;
        let item =
            egui::Button::new(RichText::new(option.label()).font(egui::FontId::new(15.0, family)))
                .frame(false)
                .selected(option == current);
        if ui.add(item).clicked() {
            picked = Some(option);
            ui.close();
        }
    }
    picked
}

/// The paragraph alignments as menu commands, the current one marked. Returns the one
/// clicked.
/// A pick in Home › Paragraph Alignment ▾.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignPick {
    Align(ParaAlign),
    /// Paragraph Spacing Options…: the app opens its dialog.
    Spacing,
}

/// Home › Paragraph Alignment ▾: the alignments, `current` ticked, then Paragraph Spacing
/// Options…, as the desktop notebook lists them. Returns the pick.
pub(crate) fn align_list(ui: &mut Ui, current: ParaAlign) -> Option<AlignPick> {
    ui.set_min_width(180.0);
    let mut picked = None;
    for (option, glyph, label, key) in [
        (
            ParaAlign::Left,
            icons::TEXT_ALIGN_LEFT,
            t!("editor-align-left"),
            Some(Key::L),
        ),
        (
            ParaAlign::Center,
            icons::TEXT_ALIGN_CENTER,
            t!("editor-align-center"),
            None,
        ),
        (
            ParaAlign::Right,
            icons::TEXT_ALIGN_RIGHT,
            t!("editor-align-right"),
            Some(Key::R),
        ),
    ] {
        let shortcut = key.map(|key| KeyboardShortcut::new(Modifiers::COMMAND, key));
        if menu_command(ui, glyph, &label, shortcut, current == option) {
            picked = Some(AlignPick::Align(option));
        }
    }
    ui.separator();
    // A command, not a choice: never ticked.
    let label = t!("editor-paragraph-spacing");
    if menu_command(ui, icons::TEXT_PARAGRAPH, &label, None, false) {
        picked = Some(AlignPick::Spacing);
    }
    picked
}

/// The font sizes as menu entries, the current one marked. Returns the size clicked.
pub(crate) fn size_list(ui: &mut Ui, pt: f32) -> Option<u8> {
    let mut picked = None;
    for size in FONT_SIZES {
        let item = egui::Button::new(RichText::new(size.to_string()).font(theme::ui(12.5)))
            .frame(false)
            .selected((pt - f32::from(size)).abs() < 0.01);
        if ui.add(item).clicked() {
            picked = Some(size);
            ui.close();
        }
    }
    picked
}

/// the desktop notebook's eleven styles as menu rows, in its order, each its name in its own look
/// (`style_preview`), the current one (`current`, the paragraph's kind) ticked; then Clear
/// Formatting. The ribbon's Styles menu and the text menu's mini toolbar share it. Returns
/// the command clicked.
pub(crate) fn styles_menu(
    ui: &mut Ui,
    current: Option<&BlockKind>,
    t: Tokens,
) -> Option<ToolbarCommand> {
    ui.set_min_width(180.0);
    let mut picked = None;
    for kind in BlockKind::styles() {
        let checked = current.is_some_and(|current| same_kind(current, &kind));
        // Each style's name in its own look, as the desktop notebook previews them.
        let label = kind.label();
        let job = LayoutJob::single_section(label.to_string(), style_preview(&kind, t));
        if widgets::checked_preview_item(ui, job, &label, checked, t).clicked() {
            picked = Some(ToolbarCommand::SetKind(kind));
        }
    }
    ui.separator();
    if widgets::checked_menu_item(
        ui,
        Some(icons::TEXT_CLEAR_FORMATTING),
        &t!("editor-clear-formatting"),
        false,
        t,
    )
    .clicked()
    {
        picked = Some(ToolbarCommand::ClearFormatting);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Whether two paragraph kinds are the same style: headings by their level, the others by
/// their kind alone (a To Do's state, a code block's language don't count).
pub(crate) fn same_kind(a: &BlockKind, b: &BlockKind) -> bool {
    match (a, b) {
        (BlockKind::Heading { level: a }, BlockKind::Heading { level: b }) => a == b,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// One command in a ribbon dropdown; closes the menu when clicked.
fn menu_command(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    shortcut: Option<KeyboardShortcut>,
    active: bool,
) -> bool {
    let mut button =
        egui::Button::new(RichText::new(format!("{glyph}   {label}")).font(theme::ui(12.5)))
            .frame(false)
            .selected(active);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut_label(ui.ctx(), shortcut));
    }
    let clicked = ui.add(button).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A shortcut as the ribbon shows it: egui spells punctuation out ("Ctrl+Shift+Equals"),
/// the desktop notebook shows the symbol ("Ctrl+Shift+="), in the interface language.
fn shortcut_label(ctx: &egui::Context, shortcut: KeyboardShortcut) -> String {
    crate::i18n::shortcut_text(&shortcut, ctx.os().is_mac())
}

/// the desktop notebook's tag list, in the gallery's order. Shared by the ribbon's Tag menu and the text
/// right-click menu.
pub(crate) fn tag_menu_contents(
    ui: &mut Ui,
    block: Option<&Block>,
    tags: &TagGallery,
    t: Tokens,
) -> Option<TagCommand> {
    // A fixed width: the rows fill it, and a popup's sizing pass would otherwise offer
    // them its whole default area. Wider when a language's names need it.
    ui.set_width(tag_menu_width(ui, tags));
    let mut picked = None;
    // 29 tags and the custom ones are taller than a menu should be: they scroll, with the
    // bar in view whenever they do (not only on hover), from the top each time the menu
    // opens. A menu not drawn last frame has just opened.
    let opened = ui.make_persistent_id("tag-menu-opened");
    let frame = ui.ctx().cumulative_frame_nr();
    let last = ui.data_mut(|data| {
        let last = data.get_temp::<u64>(opened);
        data.insert_temp(opened, frame);
        last
    });
    let mut rows = egui::ScrollArea::vertical()
        .id_salt("tag-menu-rows")
        .max_height(420.0);
    if last.is_none_or(|last| last + 1 < frame) {
        rows = rows.vertical_scroll_offset(0.0);
    }
    ui.scope(|ui| {
        ui.spacing_mut().scroll = tag_menu_scroll();
        rows.show(ui, |ui| {
            for &key in &tags.order {
                let on = block.is_some_and(|block| match key {
                    TagKey::Todo => matches!(block.kind, BlockKind::Todo { .. }),
                    TagKey::Line(tag) => block.tags.contains(&tag),
                    TagKey::Custom(index) => tags
                        .custom(index)
                        .is_some_and(|tag| custom_tag_state(block, &tag.name).is_some()),
                });
                let hint = tags.digit(key).map(tag_hint).unwrap_or_default();
                let color = tag_glyph_color(tags.color(key), t);
                if tag_menu_row(ui, tags.icon(key), color, &tags.label(key), &hint, on, t).clicked()
                {
                    picked = Some(TagCommand::of(key));
                }
            }
        });
    });
    ui.separator();
    if tag_menu_row(
        ui,
        icons::X,
        t.muted,
        &t!("editor-remove-tags"),
        &tag_hint(0),
        false,
        t,
    )
    .clicked()
    {
        picked = Some(TagCommand::Clear);
    }
    if tag_menu_row(
        ui,
        icons::TAG,
        t.muted,
        &t!("editor-customize-tags"),
        "",
        false,
        t,
    )
    .clicked()
    {
        picked = Some(TagCommand::Customize);
    }
    if tag_menu_row(
        ui,
        icons::MAGNIFYING_GLASS,
        t.muted,
        &t!("editor-find-tags"),
        "",
        false,
        t,
    )
    .clicked()
    {
        picked = Some(TagCommand::FindTags);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// One row of the Tag menu: a coloured glyph, the name, the shortcut and a tick when the
/// paragraph already has the tag.
fn tag_menu_row(
    ui: &mut Ui,
    glyph: &str,
    glyph_color: Color32,
    label: &str,
    hint: &str,
    checked: bool,
    t: Tokens,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width().max(230.0), 26.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    let y = rect.center().y;
    ui.painter().text(
        pos2(rect.left() + 12.0, y),
        Align2::CENTER_CENTER,
        glyph,
        theme::icon(13.0),
        glyph_color,
    );
    ui.painter().text(
        pos2(rect.left() + 28.0, y),
        Align2::LEFT_CENTER,
        label,
        theme::ui(12.5),
        t.ink,
    );
    ui.painter().text(
        pos2(rect.right() - 26.0, y),
        Align2::RIGHT_CENTER,
        hint,
        theme::ui(11.0),
        t.faint,
    );
    if checked {
        ui.painter().text(
            pos2(rect.right() - 10.0, y),
            Align2::CENTER_CENTER,
            icons::CHECK,
            theme::icon(11.0),
            t.accent,
        );
    }
    response
}

/// A tag's shortcut as its menu row shows it: Ctrl+1 … Ctrl+9, Ctrl+0 to remove them.
pub(crate) fn tag_hint(digit: u8) -> String {
    LEVEL_KEYS
        .get(usize::from(digit))
        .map(|key| util::menu_shortcut(egui::Modifiers::COMMAND, *key))
        .unwrap_or_default()
}

/// The Tag menu's scroll bar: the app's thin one (`theme`), but shown whenever the rows
/// scroll rather than only on hover, in room of its own beside them so it never covers
/// their ticks. It widens under the pointer, as the app's do.
fn tag_menu_scroll() -> egui::style::ScrollStyle {
    egui::style::ScrollStyle {
        bar_width: 8.0,
        floating_width: 4.0,
        floating_allocated_width: 6.0,
        dormant_handle_opacity: 0.45,
        ..egui::style::ScrollStyle::floating()
    }
}

/// The Tag menu's width: 240 px, or more when a language's tag names (the custom ones
/// included) and their shortcut hints need it, beside the rows' scroll bar.
fn tag_menu_width(ui: &Ui, tags: &TagGallery) -> f32 {
    cached_width_rev(ui.ctx(), "folio-tag-menu", tags.revision, |ctx| {
        let mut rows: Vec<(String, String)> = tags
            .order
            .iter()
            .map(|&key| {
                let hint = tags.digit(key).map(tag_hint).unwrap_or_default();
                (tags.label(key).into_owned(), hint)
            })
            .collect();
        rows.push((t!("editor-remove-tags").into_owned(), tag_hint(0)));
        rows.push((t!("editor-customize-tags").into_owned(), String::new()));
        rows.push((t!("editor-find-tags").into_owned(), String::new()));
        // Mirrors `tag_menu_row`: the label at +28, a gap, the hint right-aligned at -26.
        let bar = tag_menu_scroll().allocated_width();
        rows.iter()
            .map(|(label, hint)| {
                28.0 + widgets::text_width(ctx, label, theme::ui(12.5))
                    + 16.0
                    + widgets::text_width(ctx, hint, theme::ui(11.0))
                    + 26.0
            })
            .fold(240.0, f32::max)
            + bar
    })
}

pub(crate) fn color_swatch(
    ui: &mut Ui,
    color: Rgb,
    size: f32,
    selected: bool,
    t: Tokens,
) -> egui::Response {
    let hex = format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2);
    widgets::swatch(ui, color.to_color32(), size, selected, &hex, t)
}

/// `color_swatch` named in words (its tooltip and accessible name), as Office names its
/// highlight colours.
pub(crate) fn named_swatch(
    ui: &mut Ui,
    color: Rgb,
    size: f32,
    selected: bool,
    name: &str,
    t: Tokens,
) -> egui::Response {
    widgets::swatch(ui, color.to_color32(), size, selected, name, t)
}

/// The Text Highlight Color menu: Office's 15 highlighter colours, five to a row, then
/// No Color. Returns the pick, `Some(None)` for none.
pub(crate) fn highlight_menu(ui: &mut Ui, t: Tokens) -> Option<Option<Rgb>> {
    const SWATCH: f32 = 22.0;
    const GAP: f32 = 4.0;
    let mut picked = None;
    ui.set_min_width(5.0 * SWATCH + 4.0 * GAP + 24.0);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        for row in Rgb::TEXT_HIGHLIGHTS.chunks(5) {
            ui.horizontal(|ui| {
                for &(color, name) in row {
                    let name = crate::i18n::tr(name);
                    if named_swatch(ui, color, SWATCH, false, &name, t).clicked() {
                        picked = Some(Some(color));
                    }
                }
            });
        }
    });
    ui.separator();
    if widgets::menu_item(ui, None, &t!("editor-no-highlight"), t).clicked() {
        picked = Some(None);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// What the Font Color menu picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ColorPick {
    /// A colour, or `None` for Automatic.
    Color(Option<Rgb>),
    /// More Colors…
    More,
}

/// The Font Color menu, in the desktop notebook's layout: Automatic; Theme Colors, a row of Office's
/// theme colours over a block of their shades; Standard Colors; then More Colors….
pub(crate) fn text_color_menu(ui: &mut Ui, t: Tokens) -> Option<ColorPick> {
    const SWATCH: f32 = 16.0;
    const GAP: f32 = 4.0;
    let mut picked = None;
    let pad = ui.spacing().button_padding.x;
    ui.set_min_width(10.0 * SWATCH + 9.0 * GAP + 2.0 * pad);
    // Automatic: the theme's own ink, shown as a filled square.
    let automatic = widgets::tinted_menu_item(
        ui,
        Some(icons::SQUARE),
        Some(t.ink),
        &t!("editor-default-color"),
        t,
    );
    if ui.is_rect_visible(automatic.rect) {
        let width = widgets::text_width(ui.ctx(), icons::SQUARE, theme::ui(12.5));
        let center = pos2(
            automatic.rect.left() + pad + width / 2.0,
            automatic.rect.center().y,
        );
        let square = Rect::from_center_size(center, egui::Vec2::splat(width * 0.6));
        ui.painter()
            .rect_filled(square, CornerRadius::same(1), t.ink);
    }
    if automatic.clicked() {
        picked = Some(ColorPick::Color(None));
    }
    let mut row = |ui: &mut Ui, colors: &[Rgb]| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.add_space(pad);
            for &color in colors {
                if color_swatch(ui, color, SWATCH, false, t).clicked() {
                    picked = Some(ColorPick::Color(Some(color)));
                }
            }
        });
    };
    menu_heading(ui, &t!("editor-theme-colors"), t);
    ui.scope(|ui| {
        // The shades sit under the bases as one block, as in Office.
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().interact_size.y = SWATCH;
        row(ui, &Rgb::FONT_THEME[0]);
        ui.add_space(4.0);
        for shades in &Rgb::FONT_THEME[1..] {
            row(ui, shades);
        }
    });
    menu_heading(ui, &t!("editor-standard-colors"), t);
    row(ui, &Rgb::FONT_STANDARD);
    ui.separator();
    if widgets::menu_item(ui, Some(icons::PALETTE), &t!("ribbon-more-colors"), t).clicked() {
        picked = Some(ColorPick::More);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// The heading over a ribbon dropdown's grid ("Bullet Library"), as the colour menus
/// title theirs.
fn menu_heading(ui: &mut Ui, text: &str, t: Tokens) {
    ui.label(
        RichText::new(text)
            .font(theme::semibold(12.0))
            .color(t.muted),
    );
}

/// the desktop notebook's paste options, for Home › Paste ▾ and the Paste Options button: Keep Source
/// Formatting, Merge Formatting and Keep Text Only, `current` (the way the paste under the
/// button went) with its glyph on a highlighted square. Returns the option picked.
pub(crate) fn paste_menu(ui: &mut Ui, current: Option<PasteMode>, t: Tokens) -> Option<PasteMode> {
    ui.set_min_width(230.0);
    let mut picked = None;
    for mode in PasteMode::ALL {
        let row = widgets::checked_menu_item(
            ui,
            Some(mode.glyph()),
            &mode.label(),
            current == Some(mode),
            t,
        );
        if row.clicked() {
            picked = Some(mode);
        }
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// The ribbon's Format Painter. `double` is true for the second click of a double-click.
pub(crate) fn format_painter(doc: &Document, editor: &mut EditorState, double: bool) {
    if let Some(painter) = editor.painter.as_mut() {
        if double {
            painter.sticky = true;
        } else {
            editor.painter = None;
        }
        return;
    }
    editor.painter = format_at_caret(doc, editor, double);
}

/// The formatting at the caret, as the Format Painter picks it up: the text's look
/// (without its link) and the paragraph's or cell's alignment. None away from text.
fn format_at_caret(doc: &Document, editor: &EditorState, sticky: bool) -> Option<FormatPainter> {
    editor.active_text(doc)?;
    let mut style = editor.shown_style(doc);
    style.link = None;
    let align = match editor.active_cell {
        Some(_) => editor
            .active_cell_ref(doc)
            .map_or(ParaAlign::Left, |cell| cell.align),
        None => editor
            .active_index(doc)
            .map_or(ParaAlign::Left, |index| doc.blocks[index].align),
    };
    Some(FormatPainter {
        style,
        align,
        sticky,
    })
}

/// Paints the Format Painter's formatting onto the text selected (or, for a plain click,
/// the word under it), keeping links. A painter that is not sticky turns off.
fn paint_format(
    ctx: &egui::Context,
    doc: &mut Document,
    editor: &mut EditorState,
    range: Range<usize>,
) -> bool {
    let Some(painter) = editor.painter.clone() else {
        return false;
    };
    let painted = apply_format(ctx, doc, editor, &painter, range);
    if painted && !painter.sticky {
        editor.painter = None;
    }
    painted
}

/// Gives `format`'s look (links kept) and alignment to the selection across paragraphs,
/// if there is one, else to `range` of the text holding the caret, or the word at the
/// caret when that is empty. One undo step. True when something was painted.
fn apply_format(
    ctx: &egui::Context,
    doc: &mut Document,
    editor: &mut EditorState,
    format: &FormatPainter,
    range: Range<usize>,
) -> bool {
    let now = ctx.input(|input| input.time);
    let paint = |text: &mut StyledText, chars: Range<usize>| {
        text.apply_style_chars(chars, |style| {
            let link = style.link.take();
            *style = format.style.clone();
            style.link = link;
        });
    };
    // A drag (or Shift+arrows) across paragraphs: every paragraph it covers.
    if let Some(selection) = editor.range_in(doc) {
        let segments = range_segments(doc, selection);
        if segments.is_empty() {
            return false;
        }
        editor.push_undo(doc, now);
        for (index, chars) in segments {
            paint(&mut doc.blocks[index].text, chars);
            doc.blocks[index].align = format.align;
        }
        return true;
    }
    let Some(text) = editor.active_text(doc) else {
        return false;
    };
    let range = if range.is_empty() {
        word_at(&text.text, range.start)
    } else {
        range
    };
    if range.is_empty() {
        return false;
    }
    editor.push_undo(doc, now);
    if let Some(text) = editor.active_text_mut(doc) {
        paint(text, range);
    }
    match editor.active_cell {
        Some(_) => {
            if let Some(cell) = editor.active_cell_mut(doc) {
                cell.align = format.align;
            }
        }
        None => {
            if let Some(index) = editor.active_index(doc) {
                doc.blocks[index].align = format.align;
            }
        }
    }
    true
}

/// Whether the mouse button came up this frame over text edit `response`, or at the end of
/// a drag that selected in it: when the Format Painter paints. The release of the ribbon
/// click that turned the painter on is neither, though that edit gets the focus back in the
/// same frame.
fn painter_release(ui: &Ui, response: &egui::Response) -> bool {
    (response.contains_pointer() || response.drag_stopped())
        && ui.input(|input| input.pointer.primary_released())
}

/// The word around `at` (chars): up to whitespace on either side.
fn word_at(text: &str, at: usize) -> Range<usize> {
    let chars: Vec<char> = text.chars().collect();
    let at = at.min(chars.len());
    let start = (0..at)
        .rev()
        .take_while(|i| !chars[*i].is_whitespace())
        .last()
        .unwrap_or(at);
    let end = (at..chars.len())
        .find(|i| chars[*i].is_whitespace())
        .unwrap_or(chars.len());
    start..end
}

/// A dropdown grid's button: `text` (a list marker, a symbol) in `font`, 28 × 28, named by
/// that text, and outlined in the accent when it is the current choice.
pub(crate) fn glyph_button(
    ui: &mut Ui,
    text: &str,
    font: egui::FontId,
    current: bool,
    t: Tokens,
) -> egui::Response {
    glyph_tile(ui, vec2(28.0, 28.0), text, font, current, t)
}

/// A [`glyph_button`] of any `size`, its text kept inside it (Customize Numbering's
/// formats, which can be as wide as "(viii)").
pub(crate) fn glyph_tile(
    ui: &mut Ui,
    size: egui::Vec2,
    text: &str,
    font: egui::FontId,
    current: bool,
    t: Tokens,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    let (width, color) = if current {
        (1.5, t.accent)
    } else {
        (1.0, t.line)
    };
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(width, color),
        StrokeKind::Inside,
    );
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, t.ink);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, text));
    response
}

/// `cells` (their text, and whether each is the current choice) as rows of `per_row`
/// glyph buttons, 2 px apart. Returns the index of the one clicked.
fn glyph_grid(
    ui: &mut Ui,
    cells: &[(String, bool)],
    per_row: usize,
    font: egui::FontId,
    t: Tokens,
) -> Option<usize> {
    let mut picked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for (row, chunk) in cells.chunks(per_row).enumerate() {
            ui.horizontal(|ui| {
                for (column, (text, current)) in chunk.iter().enumerate() {
                    if glyph_button(ui, text, font.clone(), *current, t).clicked() {
                        picked = Some(row * per_row + column);
                    }
                }
            });
        }
    });
    picked
}

/// A pick in Home › Bullets ▾ or Numbering ▾.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerPick {
    /// A list style, or `None` to end the list.
    Marker(Option<ListMarker>),
    /// Numbering ▾ › Customize Numbering…: the app opens its dialog.
    Customize,
}

/// Home › Bullets ▾ (`numbered` false) and Numbering ▾: the desktop notebook's library of markers, four
/// to a row, each as it shows before a list's first item, `current` outlined; then None,
/// which ends the list, and for numbering Customize Numbering…. Returns the pick.
pub(crate) fn marker_library(
    ui: &mut Ui,
    numbered: bool,
    current: Option<ListMarker>,
    t: Tokens,
) -> Option<MarkerPick> {
    let (heading, markers): (_, &[ListMarker]) = if numbered {
        (t!("ribbon-numbering-library"), &ListMarker::NUMBERS)
    } else {
        (t!("ribbon-bullet-library"), &ListMarker::BULLETS)
    };
    ui.set_min_width(4.0 * 28.0 + 3.0 * 2.0);
    menu_heading(ui, &heading, t);
    let cells: Vec<(String, bool)> = markers
        .iter()
        .map(|&marker| (marker.text(1), current == Some(marker)))
        .collect();
    let mut picked = glyph_grid(ui, &cells, 4, theme::body(15.0), t)
        .map(|index| MarkerPick::Marker(Some(markers[index])));
    ui.separator();
    if widgets::menu_item(ui, None, &t!("ribbon-list-none"), t).clicked() {
        picked = Some(MarkerPick::Marker(None));
    }
    if numbered && widgets::menu_item(ui, None, &t!("ribbon-customize-numbering"), t).clicked() {
        picked = Some(MarkerPick::Customize);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Insert › Symbol's grid, eight to a row: maths, Greek, arrows, then currency and
/// typography.
pub(crate) const SYMBOLS: [char; 48] = [
    '±', '×', '÷', '≠', '≈', '≤', '≥', '∞', '√', '∑', '∫', '∆', 'π', '°', '‰', 'µ', //
    'α', 'β', 'γ', 'δ', 'θ', 'λ', 'σ', 'Ω', '→', '←', '↑', '↓', '⇒', '⇔', '∈', '∀', //
    '€', '£', '¥', '¢', '©', '®', '™', '§', '«', '»', '—', '…', '½', '¼', '¾', '²',
];

/// More Symbols…'s categories: (name id, symbols with their search names). Names are
/// LaTeX's where LaTeX has one, so they read the same in every language.
pub(crate) const SYMBOL_SETS: [(&str, &[(char, &str)]); 8] = [
    (
        "symbols-maths",
        &[
            ('±', "pm"),
            ('∓', "mp"),
            ('×', "times"),
            ('÷', "div"),
            ('≠', "neq"),
            ('≈', "approx"),
            ('≡', "equiv"),
            ('≤', "leq"),
            ('≥', "geq"),
            ('∞', "infty"),
            ('√', "sqrt"),
            ('∛', "cbrt"),
            ('∑', "sum"),
            ('∏', "prod"),
            ('∫', "int"),
            ('∂', "partial"),
            ('∇', "nabla"),
            ('∆', "delta increment"),
            ('∝', "propto"),
            ('∠', "angle"),
            ('°', "degree"),
            ('′', "prime"),
            ('″', "second"),
            ('‰', "permille"),
            ('∈', "in"),
            ('∉', "notin"),
            ('⊂', "subset"),
            ('⊃', "supset"),
            ('⊆', "subseteq"),
            ('⊇', "supseteq"),
            ('∪', "cup union"),
            ('∩', "cap intersection"),
            ('∅', "emptyset"),
            ('∀', "forall"),
            ('∃', "exists"),
            ('¬', "neg not"),
            ('∧', "wedge and"),
            ('∨', "vee or"),
            ('⇒', "implies"),
            ('⇔', "iff"),
            ('∴', "therefore"),
            ('∵', "because"),
        ],
    ),
    (
        "symbols-greek",
        &[
            ('α', "alpha"),
            ('β', "beta"),
            ('γ', "gamma"),
            ('δ', "delta"),
            ('ε', "epsilon"),
            ('ζ', "zeta"),
            ('η', "eta"),
            ('θ', "theta"),
            ('ι', "iota"),
            ('κ', "kappa"),
            ('λ', "lambda"),
            ('µ', "mu micro"),
            ('ν', "nu"),
            ('ξ', "xi"),
            ('π', "pi"),
            ('ρ', "rho"),
            ('σ', "sigma"),
            ('τ', "tau"),
            ('υ', "upsilon"),
            ('φ', "phi"),
            ('χ', "chi"),
            ('ψ', "psi"),
            ('ω', "omega"),
            ('Γ', "Gamma"),
            ('Δ', "Delta"),
            ('Θ', "Theta"),
            ('Λ', "Lambda"),
            ('Ξ', "Xi"),
            ('Π', "Pi"),
            ('Σ', "Sigma"),
            ('Φ', "Phi"),
            ('Ψ', "Psi"),
            ('Ω', "Omega"),
        ],
    ),
    (
        "symbols-arrows",
        &[
            ('←', "leftarrow"),
            ('→', "rightarrow"),
            ('↑', "uparrow"),
            ('↓', "downarrow"),
            ('↔', "leftrightarrow"),
            ('↕', "updownarrow"),
            ('⇐', "Leftarrow"),
            ('⇒', "Rightarrow"),
            ('⇑', "Uparrow"),
            ('⇓', "Downarrow"),
            ('⇔', "Leftrightarrow"),
            ('↗', "nearrow"),
            ('↘', "searrow"),
            ('↖', "nwarrow"),
            ('↙', "swarrow"),
            ('↦', "mapsto"),
            ('↺', "circlearrowleft"),
            ('↻', "circlearrowright"),
        ],
    ),
    (
        "symbols-currency",
        &[
            ('€', "euro"),
            ('£', "pound"),
            ('¥', "yen"),
            ('¢', "cent"),
            ('$', "dollar"),
            ('₹', "rupee"),
            ('₽', "ruble"),
            ('₩', "won"),
            ('₺', "lira"),
            ('¤', "currency"),
        ],
    ),
    (
        "symbols-typography",
        &[
            ('«', "guillemet left"),
            ('»', "guillemet right"),
            ('‹', "lsaquo"),
            ('›', "rsaquo"),
            ('“', "ldquo"),
            ('”', "rdquo"),
            ('‘', "lsquo"),
            ('’', "rsquo apostrophe"),
            ('„', "bdquo"),
            ('—', "emdash"),
            ('–', "endash"),
            ('…', "ldots ellipsis"),
            ('•', "bullet"),
            ('·', "cdot middot"),
            ('§', "section"),
            ('¶', "pilcrow paragraph"),
            ('†', "dagger"),
            ('‡', "ddagger"),
            ('©', "copyright"),
            ('®', "registered"),
            ('™', "trademark"),
            ('№', "numero"),
        ],
    ),
    (
        "symbols-numbers",
        &[
            ('½', "half"),
            ('⅓', "third"),
            ('⅔', "two thirds"),
            ('¼', "quarter"),
            ('¾', "three quarters"),
            ('⅛', "eighth"),
            ('⁰', "sup0"),
            ('¹', "sup1"),
            ('²', "sup2 squared"),
            ('³', "sup3 cubed"),
            ('⁴', "sup4"),
            ('⁵', "sup5"),
            ('⁶', "sup6"),
            ('⁷', "sup7"),
            ('⁸', "sup8"),
            ('⁹', "sup9"),
            ('ⁿ', "supn"),
            ('₀', "sub0"),
            ('₁', "sub1"),
            ('₂', "sub2"),
            ('₃', "sub3"),
            ('₄', "sub4"),
        ],
    ),
    (
        "symbols-shapes",
        &[
            ('△', "triangle"),
            ('▲', "blacktriangle"),
            ('□', "square"),
            ('■', "blacksquare"),
            ('○', "circle"),
            ('●', "blackcircle"),
            ('◇', "diamond"),
            ('◆', "blackdiamond"),
            ('✓', "checkmark"),
            ('✗', "ballotx"),
            ('♠', "spades"),
            ('♣', "clubs"),
            ('♥', "hearts"),
            ('♦', "diamonds"),
            ('♪', "note"),
            ('☺', "smiley"),
        ],
    ),
    (
        "symbols-letters",
        &[
            ('À', "Agrave"),
            ('Â', "Acirc"),
            ('Æ', "AElig"),
            ('Ç', "Ccedil"),
            ('É', "Eacute"),
            ('È', "Egrave"),
            ('Ê', "Ecirc"),
            ('Ë', "Euml"),
            ('Î', "Icirc"),
            ('Ï', "Iuml"),
            ('Ô', "Ocirc"),
            ('Œ', "OElig"),
            ('Ù', "Ugrave"),
            ('Û', "Ucirc"),
            ('Ü', "Uuml"),
            ('Ÿ', "Yuml"),
            ('à', "agrave"),
            ('â', "acirc"),
            ('æ', "aelig"),
            ('ç', "ccedil"),
            ('é', "eacute"),
            ('è', "egrave"),
            ('ê', "ecirc"),
            ('ë', "euml"),
            ('î', "icirc"),
            ('ï', "iuml"),
            ('ô', "ocirc"),
            ('œ', "oelig"),
            ('ù', "ugrave"),
            ('û', "ucirc"),
            ('ü', "uuml"),
            ('ÿ', "yuml"),
            ('ß', "szlig"),
            ('ñ', "ntilde"),
        ],
    ),
];

/// What Insert › Symbol ▾ picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolPick {
    /// A symbol, which goes in at the caret.
    Symbol(char),
    /// More Symbols…
    More,
}

/// Insert › Symbol ▾: the symbols used lately (`recent`, newest first) in a row of eight
/// under "Recently used", then `SYMBOLS` as a grid of buttons in the note font, then More
/// Symbols…. Returns the pick.
pub(crate) fn symbol_grid(ui: &mut Ui, recent: &[char], t: Tokens) -> Option<SymbolPick> {
    let font = theme::body(16.0);
    let mut picked = None;
    if !recent.is_empty() {
        let recent = &recent[..recent.len().min(8)];
        menu_heading(ui, &t!("editor-recent-symbols"), t);
        let cells: Vec<(String, bool)> = recent
            .iter()
            .map(|symbol| (symbol.to_string(), false))
            .collect();
        if let Some(index) = glyph_grid(ui, &cells, 8, font.clone(), t) {
            picked = Some(SymbolPick::Symbol(recent[index]));
        }
        ui.add_space(4.0);
    }
    let cells: Vec<(String, bool)> = SYMBOLS
        .iter()
        .map(|symbol| (symbol.to_string(), false))
        .collect();
    if let Some(index) = glyph_grid(ui, &cells, 8, font, t) {
        picked = Some(SymbolPick::Symbol(SYMBOLS[index]));
    }
    ui.separator();
    let more = t!("editor-more-symbols");
    if widgets::menu_item(ui, Some(icons::SYMBOLS), &more, t).clicked() {
        picked = Some(SymbolPick::More);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Insert › Equation ▾: Office's ready-made equations, each under its name and drawn as a
/// note draws an equation, then Insert New Equation. Returns the command picked.
pub(crate) fn equation_gallery(ui: &mut Ui, t: Tokens) -> Option<Cmd> {
    // Wide enough that the longest (the trig identities) keep this size and their ½ reads.
    const WIDTH: f32 = 340.0;
    const SIZE: f32 = 18.0;
    let pad = ui.spacing().button_padding.x;
    ui.set_min_width(WIDTH);
    let mut picked = None;
    egui::ScrollArea::vertical()
        .max_height(460.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (index, (name, source)) in math::READY_EQUATIONS.iter().enumerate() {
                let name = crate::i18n::tr(name);
                let layout = math::layout_source(ui.ctx(), source, SIZE);
                // One longer than any of Office's would shrink to fit the row.
                let scale = ((WIDTH - 2.0 * pad) / layout.size.x.max(1.0)).min(1.0);
                let heading =
                    ui.painter()
                        .layout_no_wrap(name.to_string(), theme::semibold(11.5), t.muted);
                let heading_height = heading.size().y;
                let height = 6.0 + heading_height + 4.0 + layout.size.y * scale + 8.0;
                let (rect, response) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_ref())
                });
                if ui.is_rect_visible(rect) {
                    let painter = ui.painter();
                    if response.hovered() || response.has_focus() {
                        painter.rect_filled(rect, CornerRadius::same(3), t.hover);
                    }
                    let top = rect.min + vec2(pad, 6.0);
                    painter.galley(top, heading, t.muted);
                    let at = top + vec2(0.0, heading_height + 4.0);
                    math::paint(painter, &layout, at, scale, t.ink);
                }
                if response.clicked() {
                    picked = Some(Cmd::ReadyEquation(index as u8));
                }
            }
        });
    ui.separator();
    let new = t!("ribbon-insert-new-equation");
    if widgets::menu_item(ui, Some(icons::MATH_OPERATIONS), &new, t).clicked() {
        picked = Some(Cmd::Equation);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

pub fn show_document(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    library: &Library,
    textures: &mut TextureCache,
    t: Tokens,
    bottom_space: f32,
) -> bool {
    editor.ensure_block_doc(doc);
    let ctx = ui.ctx().clone();
    // Reserved now so the selection across paragraphs is painted behind their text.
    let range_shape = ui.painter().add(egui::Shape::Noop);
    click_range(ui, doc, doc_id, editor);
    if editor
        .active_block
        .is_some_and(|block| doc.block_index(block).is_some())
        && editor.history_owner.as_deref() != Some(doc_id)
    {
        editor.start_history(doc_id);
    }
    // The caret going into a folded paragraph (find, tags, undo) unfolds what hides it.
    let mut revealed = false;
    if let Some(index) = editor
        .pending_block_focus()
        .and_then(|block| doc.block_index(block))
    {
        revealed = doc.expand_to(index);
    }
    apply_pending_doc_focus(&ctx, doc, doc_id, editor);
    // Typing only reaches the note holding keyboard focus, and every other change pushes
    // its own undo step, so only that note needs a copy to diff against (not every note on
    // the page, every frame).
    let focused = ctx.memory(|memory| memory.focused());
    let before = focused
        .is_some_and(|focused| doc_holds(doc, doc_id, focused))
        .then(|| doc.clone());
    let caret_focused = editor
        .caret_widget(doc, doc_id)
        .is_some_and(|widget| ctx.memory(|memory| memory.has_focus(widget)));
    let mut changed = revealed | handle_shortcuts(&ctx, doc, doc_id, editor);
    // A key that put the caret's paragraph under a folded one (Tab, Alt+Shift+→, undo)
    // unfolds it: a paragraph that is not drawn can't keep the keyboard.
    let caret = editor
        .pending_block_focus()
        .or(editor.active_block.filter(|_| caret_focused));
    if let Some(index) = caret.and_then(|block| doc.block_index(block)) {
        changed |= doc.expand_to(index);
    }
    // Enter/Backspace move focus; apply it before this frame's text edits run.
    apply_pending_doc_focus(&ctx, doc, doc_id, editor);
    editor.hovered_block = None;
    if doc.blocks.is_empty() {
        doc.blocks.push(Block::paragraph(""));
    }

    let mut block_actions: Vec<BlockAction> = Vec::new();
    // This frame's rows replace last frame's (keeping the allocation).
    match editor.rows.get_mut(doc_id) {
        Some(rows) => rows.clear(),
        None => {
            editor.rows.insert(doc_id.to_owned(), Vec::new());
        }
    }
    let lead = tag_lead(doc);
    // Paragraphs follow each other with no gap, as in the desktop notebook. egui gives every text box at
    // least the theme's control height (28 px, for the ribbon's boxes), which would space
    // 18 px lines 28 px apart: a paragraph is as tall as its lines.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.spacing_mut().interact_size.y = 0.0;
    let numbers = crate::model::list_numbers(&doc.blocks);
    let len = doc.blocks.len();
    let mut folds = Folds::default();
    // A press anywhere but on the Paste Options button (or its open menu) moves on from
    // the paste: the button goes. A press on the button opens its menu the same frame.
    let pressed_away = ui.input(|input| input.pointer.primary_pressed())
        && !egui::Popup::is_any_open(&ctx)
        && editor
            .paste_session
            .as_ref()
            .is_some_and(|session| session.doc_id == doc_id);
    if pressed_away {
        let over_button = ui
            .input(|input| input.pointer.interact_pos())
            .and_then(|pos| ctx.layer_id_at(pos))
            .is_some_and(|layer| layer.id == paste_options_id(doc_id));
        if !over_button {
            editor.paste_session = None;
        }
    }
    // Where the pasted text ends, for the Paste Options button under it.
    let mut end_rect: Option<Rect> = None;
    let top = ui.cursor().top();
    for index in 0..len {
        if folds.hides(doc, index) {
            continue;
        }
        // On a ruled page, text after a picture or a divider starts on the next rule.
        if let Some(pitch) = editor
            .line_pitch
            .filter(|_| doc.blocks[index].kind.is_textual())
        {
            let offset = ui.cursor().top() - top;
            let gap = ((offset - 0.01) / pitch).ceil().max(0.0) * pitch - offset;
            if gap > 0.01 {
                ui.add_space(gap);
            }
        }
        let number = numbers.get(index).copied().unwrap_or(0);
        let block_top = ui.cursor().min.y;
        changed |= show_block(
            ui,
            doc,
            doc_id,
            index,
            number,
            lead,
            editor,
            library,
            textures,
            t,
            &mut block_actions,
        );
        let ends_paste = editor.paste_session.as_ref().is_some_and(|session| {
            session.doc_id == doc_id
                && doc
                    .blocks
                    .get(index)
                    .is_some_and(|block| block.id == session.end_block)
        });
        if ends_paste {
            end_rect = Some(Rect::from_min_max(
                pos2(ui.min_rect().left(), block_top),
                pos2(ui.min_rect().right(), ui.cursor().min.y),
            ));
        }
    }
    if !block_actions.is_empty() {
        if editor.history_owner.as_deref() == Some(doc_id) {
            changed |= apply_block_actions(&ctx, doc, doc_id, editor, block_actions);
        } else {
            // A box ticked in another note than the one being typed in: its undo step must
            // not land in that note's history (undo there would bring this note's text in),
            // and that history must survive. The page's own undo still covers the tick.
            let undo_stack = std::mem::take(&mut editor.undo_stack);
            let redo_stack = std::mem::take(&mut editor.redo_stack);
            let last_undo_at = editor.last_undo_at;
            changed |= apply_block_actions(&ctx, doc, doc_id, editor, block_actions);
            editor.undo_stack = undo_stack;
            editor.redo_stack = redo_stack;
            editor.last_undo_at = last_undo_at;
        }
    }
    if bottom_space > 0.0 {
        ui.add_space(bottom_space);
    }
    track_range_drag(ui, doc, doc_id, editor);
    sync_range_focus(&ctx, doc, doc_id, editor);
    paint_range(ui, doc, doc_id, editor, range_shape);
    track_mini_bar(ui, doc, doc_id, editor);
    show_mini_bar(ui, doc, doc_id, editor, t);
    // Any other change to the note (typing, a command) or Esc ends the paste's options.
    let session_over = editor.paste_session.as_ref().is_some_and(|session| {
        session.doc_id == doc_id
            && (*doc != session.after || ui.input(|input| input.key_pressed(Key::Escape)))
    });
    if session_over {
        editor.paste_session = None;
    }
    let repasted = end_rect
        .filter(|_| editor.paste_session.is_some())
        .is_some_and(|rect| paste_options_button(ui, doc, doc_id, editor, rect, t));
    changed |= repasted;
    let undo_applied = editor.undo_applied;
    editor.undo_applied = false;
    if changed {
        // Another paste option redoes the paste in place: its undo step is still the one
        // the paste pushed, so one Ctrl+Z takes it back.
        if !undo_applied && !repasted {
            let now = ctx.input(|input| input.time);
            match &before {
                Some(before) => editor.remember_if_needed(before, now),
                // Not being typed in: the change already pushed its own undo step (if this
                // note owns the history at all).
                None if editor.history_owner.as_deref() == Some(doc_id) => {
                    editor.redo_stack.clear();
                }
                None => {}
            }
        }
        if editor.selection.is_none() && editor.pending_style.is_none() {
            let caret = editor
                .caret_widget(doc, doc_id)
                .and_then(|id| cursor_range(&ctx, id));
            if let (Some((_, caret)), Some(text)) = (caret, editor.active_text(doc)) {
                editor.active_style = text.style_for_caret(caret);
            }
        }
    }
    changed
}

/// Whether `focused` is one of this document's text edits (a paragraph or a cell).
fn doc_holds(doc: &Document, doc_id: &str, focused: Id) -> bool {
    doc.blocks.iter().any(|block| {
        block_id(doc_id, block.id) == focused
            || matches!(&block.kind, BlockKind::Table(table) if table.rows.iter().enumerate().any(|(row, cells)| {
                (0..cells.len()).any(|col| table_cell_id(doc_id, block.id, row, col) == focused)
            }))
    })
}

/// The chars of each text paragraph a range covers, in order. A paragraph whose only
/// covered part is its line break gets an empty range.
pub(crate) fn range_segments(
    doc: &Document,
    (start, end): (Pos, Pos),
) -> Vec<(usize, Range<usize>)> {
    let (Some(first), Some(last)) = (doc.block_index(start.block), doc.block_index(end.block))
    else {
        return Vec::new();
    };
    (first..=last)
        .filter(|&index| doc.blocks[index].kind.is_textual())
        .map(|index| {
            let count = doc.blocks[index].text.char_count();
            let from = if index == first {
                start.char.min(count)
            } else {
                0
            };
            let to = if index == last {
                end.char.min(count)
            } else {
                count
            };
            (index, from..to.max(from))
        })
        .collect()
}

/// The paragraphs a range covers, cut to it, formats kept: what a copy carries.
pub(crate) fn range_fragment(doc: &Document, range: (Pos, Pos)) -> Vec<Block> {
    range_segments(doc, range)
        .into_iter()
        .map(|(index, chars)| {
            let mut block = doc.blocks[index].clone();
            let mut middle = block.text.split_off_chars(chars.start);
            middle.split_off_chars(chars.len());
            block.text = middle;
            block
        })
        .collect()
}

/// A range as plain text, one paragraph per line.
fn range_text(doc: &Document, range: (Pos, Pos)) -> String {
    range_fragment(doc, range)
        .iter()
        .map(|block| block.text.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Copies a selection across paragraphs: its text to the clipboard, a line per paragraph,
/// and the paragraphs themselves, formats kept, for a paste in Folio.
fn copy_range(ctx: &egui::Context, doc: &Document, editor: &mut EditorState, range: (Pos, Pos)) {
    let plain = range_text(doc, range);
    ctx.copy_text(plain.clone());
    editor.rich_clip = Some(RichClip {
        plain,
        blocks: range_fragment(doc, range),
        inline: false,
    });
    editor.copied = true;
}

/// Records chars copied from inside one paragraph (of `kind`) or one cell (a paragraph),
/// formats kept, for a paste in Folio. egui's text edit or the caller puts their plain
/// text on the clipboard.
fn keep_inline_clip(editor: &mut EditorState, kind: &BlockKind, piece: StyledText) {
    let kind = if kind.is_textual() {
        kind.clone()
    } else {
        BlockKind::Paragraph
    };
    let mut block = Block::new(kind);
    block.text = piece;
    editor.rich_clip = Some(RichClip {
        plain: block.text.text.clone(),
        blocks: vec![block],
        inline: true,
    });
    editor.copied = true;
}

/// What a paste of `text` carries besides the text: the blocks Folio copied (when the
/// clipboard still holds that copy), or the HTML another app put there. The bool is
/// "inline" (a copy from inside one line).
pub(crate) fn rich_source(editor: &EditorState, text: &str) -> Option<(Vec<Block>, bool)> {
    let same = |a: &str, b: &str| a.replace("\r\n", "\n") == b.replace("\r\n", "\n");
    if let Some(clip) = editor
        .rich_clip
        .as_ref()
        .filter(|clip| same(&clip.plain, text))
    {
        return Some((clip.blocks.clone(), clip.inline));
    }
    let html = crate::clipboard::read_html()?;
    let blocks = crate::clipboard::html_blocks(&html);
    crate::clipboard::matches_text(&blocks, text).then_some((blocks, false))
}

/// Pastes into `doc` at `at` and opens a Paste Options session for it. The caret goes to
/// the end of what was pasted. The caller has pushed the undo step.
pub(crate) fn paste_with_options(
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    at: crate::paste::PasteAt,
    source: Vec<Block>,
    inline: bool,
    mode: PasteMode,
    caret_style: InlineStyle,
) -> bool {
    let before = doc.clone();
    let Some(caret) = crate::paste::paste_into(doc, &at, &source, inline, mode, &caret_style)
    else {
        return false;
    };
    focus_caret(editor, caret);
    editor.paste_session = Some(PasteSession {
        doc_id: doc_id.to_string(),
        before,
        after: doc.clone(),
        at,
        source,
        inline,
        caret_style,
        mode,
        end_block: caret.0,
    });
    true
}

/// Puts the caret where a paste left it: in a paragraph, or in a table cell.
fn focus_caret(editor: &mut EditorState, (block, cell, at): crate::paste::Caret) {
    editor.selection = None;
    match cell {
        Some((row, col)) => editor.focus_cell(block, row, col, at..at),
        None => editor.focus_block(block, at),
    }
}

/// The area (and layer) of note `doc_id`'s Paste Options button.
fn paste_options_id(doc_id: &str) -> Id {
    Id::unique(("paste-options", doc_id))
}

/// the desktop notebook's Paste Options: a small button under what was just pasted. It pastes it
/// again another way (the note goes back to before the paste first), with no new undo
/// step. True when the note changed.
fn paste_options_button(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    end: Rect,
    t: Tokens,
) -> bool {
    let ctx = ui.ctx().clone();
    // Scrolled out of the page's view, it waits (unclipped, it would float over the
    // ribbon) until the pasted text comes back.
    if !ui.clip_rect().contains(end.left_bottom()) {
        return false;
    }
    // The note lives in the zoomed page; the button keeps its size, so it goes in an Area
    // at the screen position under the pasted text.
    let anchor = ctx
        .layer_transform_to_global(ui.layer_id())
        .map_or(end.left_bottom(), |transform| transform * end.left_bottom())
        + vec2(0.0, 2.0);
    let current = editor.paste_session.as_ref().map(|session| session.mode);
    let name = t!("paste-options");
    let mut picked = None;
    egui::Area::new(paste_options_id(doc_id))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor)
        .show(&ctx, |ui| {
            // the desktop notebook's is small: the glyphs and a narrow margin, 22 px high.
            ui.spacing_mut().button_padding = vec2(4.0, 2.0);
            ui.spacing_mut().interact_size.y = 22.0;
            let button = ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("{} {}", icons::CLIPBOARD_TEXT, icons::CARET_DOWN))
                            .font(theme::icon(14.0)),
                    )
                    .min_size(vec2(34.0, 22.0)),
                )
                .on_hover_text(name.as_ref());
            button
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_ref()));
            egui::Popup::menu(&button).show(|ui| {
                picked = paste_menu(ui, current, t);
            });
        });
    let Some(mode) = picked else {
        return false;
    };
    let Some(session) = editor.paste_session.as_mut() else {
        return false;
    };
    *doc = session.before.clone();
    let Some(caret) = crate::paste::paste_into(
        doc,
        &session.at,
        &session.source,
        session.inline,
        mode,
        &session.caret_style,
    ) else {
        // That way does not paste here: the note keeps the paste it had.
        *doc = session.after.clone();
        return false;
    };
    session.after = doc.clone();
    session.mode = mode;
    session.end_block = caret.0;
    focus_caret(editor, caret);
    true
}

/// A paste into the note being typed in, taken from egui when it carries formatting
/// (a copy in Folio, HTML from another app), several lines, or a mode the ribbon or
/// Ctrl+Shift+V asked for, or when it replaces a selection across paragraphs: Folio
/// places it (Paste Options). A plain one-line paste stays the text edit's, which links
/// addresses as typed ones are. True when the note changed.
fn intercept_paste(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    active: u64,
    cell: Option<(usize, usize)>,
    (min, max): (usize, usize),
    now: f64,
) -> Option<bool> {
    let text = ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Paste(text) => Some(text.clone()),
            _ => None,
        })
    })?;
    let shift = ctx.input(|input| input.modifiers.shift);
    let mode = editor.take_paste_mode(now, shift);
    let rich = rich_source(editor, &text);
    let lines = text.contains('\n') && cell.is_none();
    // P4's selection across paragraphs (never in a cell) is replaced by the paste.
    let range = cell.is_none().then(|| editor.range_in(doc)).flatten();
    if rich.is_none() && !lines && mode == PasteMode::KeepSource && range.is_none() {
        return None;
    }
    ctx.input_mut(|input| {
        input
            .events
            .retain(|event| !matches!(event, egui::Event::Paste(_)))
    });
    // The text at the caret (over a range, the text it begins with) gives Merge and Keep
    // Text Only their look.
    let armed = editor.pending_style.take();
    let caret_style = match range {
        Some((start, _)) => range_style(doc, start),
        None => armed.unwrap_or_else(|| editor.active_style.clone()),
    };
    // Plain text has no formatting of its own to keep: it takes the caret's, as typing
    // (and egui's paste) would, keeping the links made of its addresses.
    let (source, inline) = rich.unwrap_or_else(|| {
        let mut blocks = crate::paste::plain_blocks(&text);
        for block in &mut blocks {
            let styles: Vec<InlineStyle> = block
                .text
                .char_styles()
                .into_iter()
                .map(|style| InlineStyle {
                    link: style.link,
                    ..caret_style.clone()
                })
                .collect();
            block.text.rebuild(&styles);
        }
        (blocks, false)
    });
    editor.push_undo(doc, now);
    let at = match range {
        Some(range) => {
            let start = delete_range(doc, range);
            editor.range = None;
            crate::paste::PasteAt {
                block: start.block,
                cell: None,
                range: start.char..start.char,
            }
        }
        None => crate::paste::PasteAt {
            block: active,
            cell,
            range: min..max,
        },
    };
    let pasted = paste_with_options(doc, doc_id, editor, at, source, inline, mode, caret_style);
    Some(pasted || range.is_some())
}

/// Copies (and for Cut, clears) the cells of `range` as a table of their own: the
/// clipboard gets a line per row with a tab between cells, a paste in Folio the table.
/// True when the table changed.
fn copy_cells(
    ctx: &egui::Context,
    doc: &mut Document,
    index: usize,
    range: CellRange,
    editor: &mut EditorState,
    cut: bool,
    now: f64,
) -> bool {
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return false;
    };
    let part = crate::table::sub_table(table, range);
    // `plain_text` drops empty rows; a paste matches the clip by exactly this string.
    let plain = part.plain_text();
    ctx.copy_text(plain.clone());
    editor.rich_clip = Some(RichClip {
        plain,
        blocks: vec![Block::new(BlockKind::Table(part))],
        inline: false,
    });
    editor.copied = true;
    if !cut {
        return false;
    }
    editor.push_undo(doc, now);
    if let BlockKind::Table(table) = &mut doc.blocks[index].kind {
        crate::table::clear(table, range);
    }
    true
}

/// Deletes what a range covers: the start paragraph keeps its text before the range and
/// takes the end paragraph's text after it, and everything between goes (pictures too).
/// Returns where the caret goes.
fn delete_range(doc: &mut Document, (start, end): (Pos, Pos)) -> Pos {
    let (Some(first), Some(last)) = (doc.block_index(start.block), doc.block_index(end.block))
    else {
        return start;
    };
    let count = doc.blocks[first].text.char_count();
    if first == last {
        doc.blocks[first]
            .text
            .delete_chars(start.char.min(count)..end.char.min(count));
        return start;
    }
    let mut last_text = doc.blocks[last].text.clone();
    let tail = last_text.split_off_chars(end.char);
    doc.blocks[first]
        .text
        .delete_chars(start.char.min(count)..count);
    doc.blocks[first].text.append(&tail);
    doc.blocks.drain(first + 1..=last);
    start
}

/// The format a range begins with, which text typed or pasted over it takes.
fn range_style(doc: &Document, start: Pos) -> InlineStyle {
    doc.block_index(start.block)
        .map_or_else(InlineStyle::default, |index| {
            let styled = &doc.blocks[index].text;
            if start.char < styled.char_count() {
                styled.style_at(styled.char_to_byte(start.char))
            } else {
                styled.style_for_caret(start.char)
            }
        })
}

/// Typing over a range: one undo step, the text taking the format the range began with,
/// and the caret after it. (A paste over one goes through [`intercept_paste`].)
fn replace_range(
    doc: &mut Document,
    editor: &mut EditorState,
    range: (Pos, Pos),
    text: &str,
    now: f64,
) {
    editor.push_undo(doc, now);
    let style = range_style(doc, range.0);
    let at = delete_range(doc, range);
    let Some(index) = doc.block_index(at.block) else {
        return;
    };
    let styled = &mut doc.blocks[index].text;
    let mut updated = styled.text.clone();
    updated.insert_str(styled.char_to_byte(at.char), text);
    let caret = at.char + text.chars().count();
    styled.sync_after_edit(&updated, style, Some(caret));
    editor.collapse_to(Pos { char: caret, ..at });
}

/// Where a pointer at `pointer` falls among a note's rows: the char nearest it in the
/// paragraph it is over (or the nearest one above or below).
fn pos_under(rows: &[Row], pointer: egui::Pos2) -> Option<Pos> {
    let distance = |row: &Row| {
        if pointer.y < row.rect.top() {
            row.rect.top() - pointer.y
        } else {
            (pointer.y - row.rect.bottom()).max(0.0)
        }
    };
    let row = rows
        .iter()
        .filter(|row| row.text.is_some())
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
    let (galley, origin) = row.text.as_ref()?;
    Some(Pos {
        block: row.block,
        char: galley.cursor_from_pos(pointer - *origin).index.0,
    })
}

/// Where `pos` is drawn (its caret's rect), from this frame's rows of the note.
fn pos_rect(rows: &[Row], pos: Pos) -> Option<Rect> {
    let (galley, origin) = rows
        .iter()
        .find(|row| row.block == pos.block)?
        .text
        .as_ref()?;
    Some(
        galley
            .pos_from_cursor(CCursor::new(pos.char))
            .translate(origin.to_vec2()),
    )
}

/// One screen line down or up from `pos`, into the next or previous paragraph at its
/// first or last line, keeping the column. `None` without rows for it, or when no text
/// is drawn there (the note's first or last line, or a table or picture next to it):
/// the nearest text is then the line it started on.
fn step_line(rows: &[Row], pos: Pos, down: bool) -> Option<Pos> {
    let at = pos_rect(rows, pos)?;
    let dy = if down { at.height() } else { -at.height() };
    let to = pos_under(rows, at.center() + vec2(0.0, dy))?;
    let moved = pos_rect(rows, to)?.center().y - at.center().y;
    (moved * dy.signum() > at.height() / 2.0).then_some(to)
}

/// PgUp / PgDn while typing: the caret goes `dy` (page units) down or up the note, to the
/// text drawn where it would then be on screen (the nearest line of the note past its
/// first or last). False with no caret in one of the note's paragraphs.
pub(crate) fn caret_by(
    ctx: &egui::Context,
    doc: &Document,
    doc_id: &str,
    editor: &mut EditorState,
    dy: f32,
) -> bool {
    if editor.active_cell.is_some() {
        return false;
    }
    let Some(block) = editor
        .active_block
        .filter(|block| doc.block_index(*block).is_some())
    else {
        return false;
    };
    let Some((_, caret)) = cursor_range(ctx, block_id(doc_id, block)) else {
        return false;
    };
    let Some(rows) = editor.rows.get(doc_id) else {
        return false;
    };
    let Some(to) = pos_rect(rows, Pos { block, char: caret })
        .and_then(|at| pos_under(rows, at.center() + vec2(0.0, dy)))
    else {
        return false;
    };
    editor.range = None;
    editor.selection = None;
    editor.focus_block(to.block, to.char);
    true
}

/// Whether all the note's text is selected: a selection from its first paragraph's start
/// to its last one's end (the second Ctrl+A), or, in a one-paragraph note, its whole text.
/// A further Ctrl+A selects the page, as in the desktop notebook.
pub(crate) fn whole_note_selected(doc: &Document, editor: &EditorState) -> bool {
    let hidden = doc.hidden_blocks();
    let texts: Vec<usize> = (0..doc.blocks.len())
        .filter(|i| doc.blocks[*i].kind.is_textual() && !hidden[*i])
        .collect();
    let (Some(&first), Some(&last)) = (texts.first(), texts.last()) else {
        return false;
    };
    let start = Pos {
        block: doc.blocks[first].id,
        char: 0,
    };
    let end = Pos {
        block: doc.blocks[last].id,
        char: doc.blocks[last].text.char_count(),
    };
    match editor.range_in(doc) {
        Some(range) => range == (start, end),
        None => {
            first == last
                && end.char > 0
                && editor.active_block == Some(end.block)
                && editor.selection == Some(0..end.char)
        }
    }
}

/// Where Shift+`key` takes a selection's moving end: ↑ / ↓ by screen line (as the note
/// was drawn last frame), the rest as `step_pos` goes.
fn moved_end(doc: &Document, doc_id: &str, editor: &EditorState, pos: Pos, key: Key) -> Pos {
    let line = match key {
        Key::ArrowUp | Key::ArrowDown => editor
            .rows
            .get(doc_id)
            .and_then(|rows| step_line(rows, pos, key == Key::ArrowDown)),
        _ => None,
    };
    line.unwrap_or_else(|| step_pos(doc, pos, key))
}

/// The rectangles covering chars `chars` of a galley drawn with its origin at `origin`. A
/// covered line break shows as a short block after the text, as text boxes show one.
fn selection_rects(
    galley: &egui::Galley,
    origin: egui::Pos2,
    chars: Range<usize>,
    line_break: bool,
) -> Vec<Rect> {
    let start = galley.pos_from_cursor(CCursor::new(chars.start));
    let end = galley.pos_from_cursor(CCursor::new(chars.end));
    let mut rects = Vec::new();
    for row in &galley.rows {
        let rect = row.rect();
        if rect.top() < start.top() - 0.5 || rect.top() > end.top() + 0.5 {
            continue;
        }
        let left = if (rect.top() - start.top()).abs() < 0.5 {
            start.left()
        } else {
            rect.left()
        };
        let right = if (rect.top() - end.top()).abs() < 0.5 {
            end.left()
        } else {
            rect.right()
        };
        if right > left {
            rects.push(
                Rect::from_x_y_ranges(left..=right, rect.y_range()).translate(origin.to_vec2()),
            );
        }
    }
    if line_break {
        let at = end.translate(origin.to_vec2());
        rects.push(Rect::from_min_size(
            at.left_top(),
            vec2(at.height() * 0.4, at.height()),
        ));
    }
    rects
}

/// Paints the selection across paragraphs behind their text, in egui's selection colour
/// (the focused paragraph's text box paints its own part over it in the same colour).
fn paint_range(
    ui: &Ui,
    doc: &Document,
    doc_id: &str,
    editor: &EditorState,
    shape: egui::layers::ShapeIdx,
) {
    let Some(range) = editor.range_in(doc) else {
        return;
    };
    let Some(rows) = editor.rows.get(doc_id) else {
        return;
    };
    let color = ui.visuals().selection.bg_fill;
    let mut shapes = Vec::new();
    for (index, chars) in range_segments(doc, range) {
        let block = doc.blocks[index].id;
        let Some((galley, origin)) = rows
            .iter()
            .find(|row| row.block == block)
            .and_then(|row| row.text.as_ref())
        else {
            continue;
        };
        for rect in selection_rects(galley, *origin, chars, block != range.1.block) {
            shapes.push(egui::Shape::rect_filled(rect, CornerRadius::ZERO, color));
        }
    }
    ui.painter().set(shape, egui::Shape::Vec(shapes));
}

/// Dragging from one paragraph's text into another's selects across them. egui keeps the
/// drag on the first paragraph's text box, which selects up to its edge; this sets the
/// range from where the press began to the char under the pointer.
fn track_range_drag(ui: &Ui, doc: &Document, doc_id: &str, editor: &mut EditorState) {
    let Some(dragged) = ui.ctx().dragged_id() else {
        return;
    };
    let Some(rows) = editor.rows.get(doc_id) else {
        return;
    };
    let Some(from) = rows
        .iter()
        .find(|row| row.text.is_some() && block_id(doc_id, row.block) == dragged)
        .map(|row| row.block)
    else {
        return;
    };
    let Some(to) = local_pointer(ui).and_then(|pointer| pos_under(rows, pointer)) else {
        return;
    };
    let anchor = match (editor.range_in(doc), editor.range) {
        (Some(_), Some((anchor, _))) => anchor,
        _ => Pos {
            block: from,
            char: TextEditState::load(ui.ctx(), dragged)
                .and_then(|state| state.cursor.char_range())
                .map_or(0, |range| range.secondary.index.0),
        },
    };
    // Back in the paragraph it began in: egui's own selection says it all.
    editor.range = (anchor.block != to.block).then_some((anchor, to));
}

/// Shift+click in another paragraph of the note being typed in selects from the caret (or
/// the selection's anchor) to there. Any other press on the page drops the selection.
fn click_range(ui: &Ui, doc: &Document, doc_id: &str, editor: &mut EditorState) {
    let ctx = ui.ctx();
    let Some(press) = ctx.input(|input| {
        input
            .pointer
            .primary_pressed()
            .then(|| input.pointer.press_origin())
            .flatten()
    }) else {
        return;
    };
    let local = ctx
        .layer_transform_from_global(ui.layer_id())
        .map_or(press, |transform| transform * press);
    // Only presses on the page itself count, not those on a menu, a dialog or the ribbon.
    // A canvas's scene layer is not an area, so egui finds the panel layer under it there.
    let above = ctx.layer_id_at(press);
    if above.is_some_and(|layer| layer != ui.layer_id() && layer != egui::LayerId::background())
        || !ui.clip_rect().contains(local)
    {
        return;
    }
    if !ctx.input(|input| input.modifiers.shift) {
        if editor.range_in(doc).is_some() {
            editor.range = None;
        }
        return;
    }
    let Some(active) = editor
        .active_block
        .filter(|block| doc.block_index(*block).is_some())
    else {
        return;
    };
    // Last frame's rows: this frame's are not laid out yet.
    let Some(to) = editor
        .rows
        .get(doc_id)
        .and_then(|rows| pos_under(rows, local))
    else {
        return;
    };
    let anchor = match (editor.range_in(doc), editor.range) {
        (Some(_), Some((anchor, _))) => Some(anchor),
        _ => TextEditState::load(ctx, block_id(doc_id, active))
            .and_then(|state| state.cursor.char_range())
            .map(|range| Pos {
                block: active,
                char: range.secondary.index.0,
            }),
    };
    if let Some(anchor) = anchor.filter(|anchor| anchor.block != to.block) {
        editor.range = Some((anchor, to));
    }
}

/// Keeps the focused paragraph's own text-box selection to its part of the range, with
/// its caret where the range moves, so its caret blinks at the range's moving end.
fn sync_range_focus(ctx: &egui::Context, doc: &Document, doc_id: &str, editor: &EditorState) {
    let (Some((start, end)), Some((anchor, focus))) = (editor.range_in(doc), editor.range) else {
        return;
    };
    let Some(focused) = ctx.memory(|memory| memory.focused()) else {
        return;
    };
    if ctx.dragged_id() == Some(focused) {
        return;
    }
    let Some((index, chars)) = range_segments(doc, (start, end))
        .into_iter()
        .find(|(index, _)| block_id(doc_id, doc.blocks[*index].id) == focused)
    else {
        return;
    };
    let block = doc.blocks[index].id;
    let forward = focus == end;
    let (secondary, primary) = if block == focus.block {
        (if forward { chars.start } else { chars.end }, focus.char)
    } else if block == anchor.block {
        (anchor.char, if forward { chars.end } else { chars.start })
    } else if forward {
        (chars.start, chars.end)
    } else {
        (chars.end, chars.start)
    };
    let mut state = TextEditState::load(ctx, focused).unwrap_or_default();
    let current = state
        .cursor
        .char_range()
        .map(|range| (range.secondary.index.0, range.primary.index.0));
    if current != Some((secondary, primary)) {
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(secondary),
            CCursor::new(primary),
        )));
        state.store(ctx, focused);
    }
}

/// What a paragraph handle asked for. The canvas records the undo step and applies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleAction {
    /// Select the paragraph with its subparagraphs (a click).
    Select(u64),
    /// Fold or unfold its subparagraphs (a double-click, or a click on "+").
    Toggle(u64),
    /// Move it, subparagraphs along, before `before` (to the end when `None`), as dragging
    /// its handle does.
    Move {
        block: u64,
        before: Option<u64>,
    },
    /// The handle's menu: Cut, Copy or Delete the paragraph with its subparagraphs.
    Cut(u64),
    Copy(u64),
    Delete(u64),
    /// Copy Link to Paragraph: a link to the paragraph on the clipboard.
    CopyLink(u64),
    /// Select › Level n (the handle's menu, or the note's): show levels 1 to n of the
    /// note's outline, as Alt+Shift+n does; 0 (All Levels) shows them all.
    ShowLevels(u8),
}

/// The deepest level Select › offers, as the desktop notebook's menu does.
const MENU_LEVELS: u8 = 5;

/// Select ›'s rows: Level 1 to 5, then All Levels, each with its Alt+Shift shortcut.
/// Returns the levels to show (0: all) when one is picked. The note container's menu
/// shows them too.
pub(crate) fn level_rows(ui: &mut Ui, t: Tokens) -> Option<u8> {
    let rows: Vec<(u8, String, String)> = (1..=MENU_LEVELS)
        .chain([0])
        .map(|levels| {
            let label = if levels == 0 {
                t!("editor-all-levels").into_owned()
            } else {
                t!("editor-level", level = levels).into_owned()
            };
            let hint = util::menu_shortcut(
                Modifiers::ALT | Modifiers::SHIFT,
                LEVEL_KEYS[usize::from(levels)],
            );
            (levels, label, hint)
        })
        .collect();
    ui.set_width(crate::canvas::menu_width(
        ui,
        rows.iter()
            .map(|(_, label, hint)| ("", label.as_str(), hint.as_str())),
    ));
    let mut picked = None;
    for (levels, label, hint) in &rows {
        if widgets::menu_row(ui, None, label, hint, true, t).clicked() {
            picked = Some(*levels);
            ui.close();
        }
    }
    picked
}

/// A paragraph handle's right-click menu, as the desktop notebook's gripper has it: Cut, Copy and
/// Delete for the paragraph with its subparagraphs, Copy Link to Paragraph, then Select ›
/// with the levels.
fn handle_menu(ui: &mut Ui, block: u64, t: Tokens) -> Option<HandleAction> {
    let rows = [
        (
            icons::SCISSORS,
            t!("common-cut"),
            util::menu_shortcut(Modifiers::COMMAND, Key::X),
            HandleAction::Cut(block),
        ),
        (
            icons::COPY,
            t!("common-copy"),
            util::menu_shortcut(Modifiers::COMMAND, Key::C),
            HandleAction::Copy(block),
        ),
        (
            icons::TRASH,
            t!("common-delete"),
            util::menu_shortcut(Modifiers::NONE, Key::Delete),
            HandleAction::Delete(block),
        ),
        (
            icons::COPY_LINK,
            t!("editor-copy-paragraph-link"),
            String::new(),
            HandleAction::CopyLink(block),
        ),
    ];
    let select = t!("editor-select-levels");
    ui.set_width(crate::canvas::menu_width(
        ui,
        rows.iter()
            .map(|(glyph, label, hint, _)| (*glyph, label.as_ref(), hint.as_str()))
            .chain([(icons::SELECTION_ALL, select.as_ref(), "")]),
    ));
    let mut picked = None;
    for (glyph, label, hint, action) in rows {
        if widgets::menu_row(ui, Some(glyph), &label, &hint, true, t).clicked() {
            picked = Some(action);
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button(
        crate::canvas::submenu_text(icons::SELECTION_ALL, &select, t),
        |ui| {
            if let Some(levels) = level_rows(ui, t) {
                picked = Some(HandleAction::ShowLevels(levels));
            }
        },
    );
    picked
}

/// A paragraph handle's size, in screen points.
const HANDLE_SIZE: egui::Vec2 = egui::vec2(10.0, 12.0);

/// the desktop notebook's paragraph handles in the margin left of a note (`left`..`right` is its frame,
/// in scene coordinates): the one for the paragraph under the pointer while `show`, and a
/// "+" on every folded paragraph. Call it after the note's text is shown (it reads this
/// frame's rows).
pub fn outline_handles(
    ui: &mut Ui,
    doc: &Document,
    doc_id: &str,
    editor: &EditorState,
    left: f32,
    right: f32,
    zoom: f32,
    show: bool,
    t: Tokens,
) -> Option<HandleAction> {
    let rows = editor.rows.get(doc_id)?;
    // Nothing to draw (no pointer near, nothing folded, no handle dragged): this runs for
    // every note every frame, so leave at once.
    let dragging_any = ui.ctx().dragged_id().is_some() || ui.ctx().drag_stopped_id().is_some();
    // A handle's open menu keeps its handle up, wherever the pointer went.
    let menu_any = egui::Popup::is_any_open(ui.ctx());
    if !show && !dragging_any && !menu_any && !doc.blocks.iter().any(|block| block.collapsed) {
        return None;
    }
    let px = |points: f32| points / zoom;
    let pointer = local_pointer(ui);
    // A note against the page's left edge keeps its handles on the page.
    let handle_x = (left - px(HANDLE_GUTTER / 2.0)).max(px(HANDLE_SIZE.x / 2.0 + 1.0));
    let mut action = None;
    // Rows come in document order: look each up from where the last one was.
    let mut next = 0;
    for row in rows {
        let found = (next..doc.blocks.len())
            .find(|&index| doc.blocks[index].id == row.block)
            .or_else(|| doc.block_index(row.block));
        let Some(index) = found else {
            continue;
        };
        next = index + 1;
        let folded = doc.blocks[index].collapsed && doc.has_children(index);
        let id = Id::unique(("folio-paragraph-handle", doc_id, row.block));
        let band = Rect::from_x_y_ranges(left - px(HANDLE_GUTTER)..=right, row.rect.y_range());
        let hovered = show && pointer.is_some_and(|pointer| band.contains(pointer));
        // A dragged handle stays up wherever the pointer goes, and on the frame it is let go,
        // when egui no longer counts it as dragged, so the drop is seen.
        let dragging = ui.ctx().is_being_dragged(id) || ui.ctx().drag_stopped_id() == Some(id);
        // egui keeps a context menu open by the id of the widget it belongs to.
        let menu = menu_any && egui::Popup::is_id_open(ui.ctx(), id.with("popup"));
        if !(hovered || folded || dragging || menu) {
            continue;
        }
        let rect = Rect::from_center_size(pos2(handle_x, row.head.center()), HANDLE_SIZE / zoom);
        let tip = if folded {
            t!("editor-handle-expand")
        } else {
            t!("editor-handle-tip")
        };
        let response = ui
            .interact(rect.expand(px(2.0)), id, Sense::click_and_drag())
            .on_hover_text(tip.as_ref());
        let ink = if response.hovered() || response.dragged() {
            t.ink
        } else {
            t.muted
        };
        if folded {
            let square = Rect::from_center_size(rect.center(), vec2(px(9.0), px(9.0)));
            ui.painter().rect_stroke(
                square,
                CornerRadius::ZERO,
                Stroke::new(px(1.0), ink),
                StrokeKind::Inside,
            );
            for half in [vec2(px(2.5), 0.0), vec2(0.0, px(2.5))] {
                ui.painter().line_segment(
                    [square.center() - half, square.center() + half],
                    Stroke::new(px(1.0), ink),
                );
            }
        } else {
            // the desktop notebook's gripper: a light tab with a pointed right side, outlined a shade
            // darker, that darkens under the pointer.
            let (fill, edge) = if response.hovered() || response.dragged() {
                (t.container_border, t.muted)
            } else {
                (t.hover, t.container_border)
            };
            let point = px(4.0);
            let tab = vec![
                rect.left_top(),
                pos2(rect.right() - point, rect.top()),
                pos2(rect.right(), rect.center().y),
                pos2(rect.right() - point, rect.bottom()),
                rect.left_bottom(),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                tab,
                fill,
                Stroke::new(px(1.0), edge),
            ));
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if folded {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Move
            });
        }
        if response.double_clicked() {
            action = Some(HandleAction::Toggle(row.block));
        } else if response.clicked() {
            action = Some(if folded {
                HandleAction::Toggle(row.block)
            } else {
                HandleAction::Select(row.block)
            });
        }
        if response.dragged() {
            if let Some(pointer) = pointer {
                let (_, y) = drop_target(doc, rows, index, pointer.y);
                ui.painter()
                    .hline(left..=right, y, Stroke::new(px(2.0), t.accent));
            }
        }
        if response.drag_stopped() {
            if let Some(pointer) = pointer {
                let (before, _) = drop_target(doc, rows, index, pointer.y);
                action = Some(HandleAction::Move {
                    block: row.block,
                    before,
                });
            }
        }
        response.context_menu(|ui| {
            if let Some(picked) = handle_menu(ui, row.block, t) {
                action = Some(picked);
            }
        });
    }
    action
}

/// Where a dragged paragraph (at `index`, with its subparagraphs) lands for a pointer at
/// height `y`: before which block (`None`: the end), and the height of the line showing it.
fn drop_target(doc: &Document, rows: &[Row], index: usize, y: f32) -> (Option<u64>, f32) {
    let family = doc.family(index);
    let outside = rows.iter().filter(|row| {
        doc.block_index(row.block)
            .is_some_and(|i| !family.contains(&i))
    });
    for row in outside {
        if y < row.rect.center().y {
            return (Some(row.block), row.rect.top());
        }
    }
    (None, rows.last().map_or(y, |row| row.rect.bottom()))
}

/// Applies a handle's action. In the note being typed in (its history's owner) this
/// records the editor's undo step; for any other note the canvas recorded a page step.
/// Returns whether the document changed.
pub fn apply_handle_action(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    action: HandleAction,
    now: f64,
) -> bool {
    let owner = editor.history_owner.as_deref() == Some(doc_id);
    let step =
        |doc: &mut Document, editor: &mut EditorState, command: &dyn Fn(&mut Document) -> bool| {
            if owner {
                undo_step(doc, editor, now, command)
            } else {
                command(doc)
            }
        };
    match action {
        HandleAction::Select(block) => {
            select_family(doc, editor, block);
            false
        }
        HandleAction::Toggle(block) => {
            let Some(index) = doc.block_index(block) else {
                return false;
            };
            let changed = step(doc, editor, &|doc: &mut Document| {
                if !doc.has_children(index) {
                    return false;
                }
                doc.blocks[index].collapsed = !doc.blocks[index].collapsed;
                true
            });
            reveal_caret(doc, editor);
            changed
        }
        HandleAction::Move { block, before } => {
            let Some(index) = doc.block_index(block) else {
                return false;
            };
            let before = before.and_then(|id| doc.block_index(id));
            step(doc, editor, &|doc: &mut Document| {
                doc.move_family_before(index, before).is_some()
            })
        }
        HandleAction::Copy(block) => {
            if let Some(range) = family_range(doc, block) {
                copy_range(ctx, doc, editor, range);
            }
            false
        }
        HandleAction::CopyLink(block) => {
            // Note ids are "<page id>:<item id>".
            if let Some((page, _)) = doc_id.rsplit_once(':') {
                ctx.copy_text(crate::model::paragraph_link(page, block));
            }
            false
        }
        HandleAction::Cut(block) | HandleAction::Delete(block) => {
            let Some(index) = doc.block_index(block) else {
                return false;
            };
            if matches!(action, HandleAction::Cut(_)) {
                if let Some(range) = family_range(doc, block) {
                    copy_range(ctx, doc, editor, range);
                }
            }
            // The paragraphs go whole, folded ones too; a note keeps one paragraph.
            let family = doc.family(index);
            let changed = step(doc, editor, &|doc: &mut Document| {
                doc.blocks.drain(family.clone());
                if doc.blocks.is_empty() {
                    doc.blocks.push(Block::paragraph(""));
                }
                true
            });
            if owner {
                editor.range = None;
                let next = &doc.blocks[index.min(doc.blocks.len() - 1)];
                editor.focus_block(next.id, 0);
            }
            changed
        }
        HandleAction::ShowLevels(levels) => {
            let changed = step(doc, editor, &|doc: &mut Document| doc.show_levels(levels));
            reveal_caret(doc, editor);
            changed
        }
    }
}

/// A paragraph with its subparagraphs (folded ones too), from its first char to its last
/// paragraph's end, for the handle's Cut and Copy.
fn family_range(doc: &Document, block: u64) -> Option<(Pos, Pos)> {
    let family = doc.family(doc.block_index(block)?);
    let last = &doc.blocks[family.end - 1];
    Some((
        Pos { block, char: 0 },
        Pos {
            block: last.id,
            char: last.text.char_count(),
        },
    ))
}

/// Selects a paragraph with its subparagraphs, as a click on its handle does. Paragraphs
/// a fold hides stay out of it, as with Ctrl+A: the caret can't go where nothing shows.
fn select_family(doc: &Document, editor: &mut EditorState, block: u64) {
    let Some(index) = doc.block_index(block) else {
        return;
    };
    let hidden = doc.hidden_blocks();
    let family = doc.family(index);
    let text = |i: &usize| doc.blocks[*i].kind.is_textual() && !hidden[*i];
    let first = family.clone().find(text);
    let last = family.rev().find(text);
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    let start = Pos {
        block: doc.blocks[first].id,
        char: 0,
    };
    let end = Pos {
        block: doc.blocks[last].id,
        char: doc.blocks[last].text.char_count(),
    };
    if first == last {
        editor.range = None;
        editor.focus_selection(start.block, 0..end.char);
    } else {
        editor.range = Some((start, end));
        editor.pending_focus = Some(FocusRequest::Block {
            id: end.block,
            pos: end.char,
        });
    }
}

/// egui ignores focus requests for widgets behind the dialog it showed last frame, so a
/// request made as a dialog closes (Cancel in the link dialog) waits until it is gone.
fn behind_dialog(ctx: &egui::Context) -> bool {
    ctx.memory(|memory| memory.top_modal_layer().is_some())
}

/// Focuses a block again after a ribbon click. egui collapses the selection of a text box
/// that lost focus, so put back the selection the editor remembered, as the desktop notebook keeps it.
fn restore_block_focus(
    ctx: &egui::Context,
    id: Id,
    editor: &EditorState,
    block: u64,
    chars: usize,
) {
    let selection = editor
        .selection
        .clone()
        .filter(|_| editor.active_block == Some(block));
    if let Some(range) = selection {
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(range.start.min(chars)),
            CCursor::new(range.end.min(chars)),
        )));
        state.store(ctx, id);
    }
    ctx.memory_mut(|memory| memory.request_focus(id));
}

/// Several documents (canvas notes) share one editor state, so a focus request is only
/// consumed by the document that owns the block.
fn apply_pending_doc_focus(
    ctx: &egui::Context,
    doc: &Document,
    doc_id: &str,
    editor: &mut EditorState,
) {
    if behind_dialog(ctx) {
        return;
    }
    let block = match editor.pending_focus {
        Some(
            FocusRequest::Block { id, .. }
            | FocusRequest::Restore { id }
            | FocusRequest::Select { id, .. }
            | FocusRequest::Cell { id, .. },
        ) => id,
        // Title focus is handled by the canvas title row.
        Some(FocusRequest::Title { .. }) | None => return,
    };
    let Some(index) = doc.block_index(block) else {
        return;
    };
    let Some(request) = editor.pending_focus.take() else {
        return;
    };
    let editor_id = block_id(doc_id, block);
    let chars = doc.blocks[index].text.char_count();
    match request {
        FocusRequest::Block { pos, .. } => {
            match &doc.blocks[index].kind {
                // A table has no text edit of its own: the caret goes to the cell it was in
                // (undo, redo), while that is still there, else the first.
                BlockKind::Table(table) if !table.rows.is_empty() && table.cols() > 0 => {
                    let (row, col) = editor
                        .active_cell
                        .filter(|&(row, col)| {
                            editor.active_block == Some(block)
                                && row < table.rows.len()
                                && col < table.cols()
                        })
                        .unwrap_or((0, 0));
                    let chars = table.rows[row]
                        .get(col)
                        .map_or(0, |cell| cell.text.char_count());
                    set_focus(
                        ctx,
                        table_cell_id(doc_id, block, row, col),
                        pos.min(chars),
                        None,
                    );
                    editor.active_cell = Some((row, col));
                }
                _ => set_focus(ctx, editor_id, pos.min(chars), None),
            }
            editor.selection = None;
        }
        FocusRequest::Restore { .. } => {
            // In a table, the cell with the caret gets the keyboard back.
            let widget = editor.caret_widget(doc, doc_id).unwrap_or(editor_id);
            let chars = editor
                .active_text(doc)
                .map_or(chars, StyledText::char_count);
            restore_block_focus(ctx, widget, editor, block, chars);
        }
        FocusRequest::Select { range, .. } => {
            let range = range.start.min(chars)..range.end.min(chars);
            set_focus_range(ctx, editor_id, range.clone());
            editor.selection = (!range.is_empty()).then_some(range);
        }
        FocusRequest::Cell {
            row, col, range, ..
        } => {
            let chars = match &doc.blocks[index].kind {
                BlockKind::Table(table) => table
                    .rows
                    .get(row)
                    .and_then(|cells| cells.get(col))
                    .map_or(0, |cell| cell.text.char_count()),
                _ => 0,
            };
            let range = range.start.min(chars)..range.end.min(chars);
            set_focus_range(ctx, table_cell_id(doc_id, block, row, col), range.clone());
            editor.active_cell = Some((row, col));
            editor.selection = (!range.is_empty()).then_some(range);
        }
        FocusRequest::Title { .. } => {}
    }
    // The caret left the table, if it was in one.
    if !matches!(doc.blocks[index].kind, BlockKind::Table(_)) {
        editor.active_cell = None;
    }
    editor.active_block = Some(block);
    ctx.request_repaint();
}

/// the desktop notebook's Ctrl+0 … Ctrl+9 follow the digit keys, whatever they type. On AZERTY (and
/// other layouts) the digit row types symbols, and egui names a press after its symbol when
/// it has a key for it: Ctrl+4 arrives as "'" and Ctrl+6 as "-", which would strike the
/// text through. Such presses are read by the key's position instead. Ctrl+Alt and
/// Ctrl+Shift are left alone (they type characters on many layouts), and so are numpad
/// keys that act as arrows.
fn read_digits_by_position(ctx: &egui::Context) {
    let digit = |key: Key| {
        matches!(
            key,
            Key::Num0
                | Key::Num1
                | Key::Num2
                | Key::Num3
                | Key::Num4
                | Key::Num5
                | Key::Num6
                | Key::Num7
                | Key::Num8
                | Key::Num9
        )
    };
    let symbol = |key: Key| {
        matches!(
            key,
            Key::Colon
                | Key::Comma
                | Key::Backslash
                | Key::Slash
                | Key::Pipe
                | Key::Questionmark
                | Key::Exclamationmark
                | Key::OpenBracket
                | Key::CloseBracket
                | Key::OpenCurlyBracket
                | Key::CloseCurlyBracket
                | Key::Backtick
                | Key::Minus
                | Key::Period
                | Key::Plus
                | Key::Equals
                | Key::Semicolon
                | Key::Quote
        )
    };
    ctx.input_mut(|input| {
        for event in &mut input.events {
            if let egui::Event::Key {
                key,
                physical_key: Some(physical),
                modifiers,
                ..
            } = event
            {
                if modifiers.command
                    && !modifiers.alt
                    && !modifiers.shift
                    && digit(*physical)
                    && symbol(*key)
                {
                    *key = *physical;
                }
            }
        }
    });
}

/// Takes one Alt+Shift press of `keys` out of this frame's input, matching the key it
/// typed or its position (US keyboards report Alt+Shift+1 as "!", AZERTY as "1"). The
/// character the press may also have typed ("!", "+"), which arrives right after it, goes
/// too, so the shortcut never types into the paragraph.
fn take_alt_shift(ctx: &egui::Context, keys: &[Key]) -> Option<Key> {
    ctx.input_mut(|input| {
        let at = input.events.iter().position(|event| {
            matches!(event, egui::Event::Key { key, physical_key, pressed: true, modifiers, .. }
                if modifiers.alt && modifiers.shift && !modifiers.command
                    && (keys.contains(key)
                        || physical_key.is_some_and(|physical| keys.contains(&physical))))
        })?;
        let event = input.events.remove(at);
        if matches!(input.events.get(at), Some(egui::Event::Text(_))) {
            input.events.remove(at);
        }
        match event {
            egui::Event::Key {
                key, physical_key, ..
            } => Some(if keys.contains(&key) {
                key
            } else {
                physical_key.unwrap_or(key)
            }),
            _ => None,
        }
    })
}

/// The digit keys 0 to 9: Alt+Shift+digit shows outline levels, Ctrl+digit tags.
const LEVEL_KEYS: [Key; 10] = [
    Key::Num0,
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
    Key::Num7,
    Key::Num8,
    Key::Num9,
];

/// The blocks an outline command moves or indents: the selected paragraphs (when the
/// selection crosses paragraphs) or the one with the caret, with their subparagraphs.
fn outline_span(doc: &Document, editor: &EditorState, index: usize) -> Range<usize> {
    let ends = editor.range_in(doc).and_then(|(start, end)| {
        Some((doc.block_index(start.block)?, doc.block_index(end.block)?))
    });
    match ends {
        Some((first, last)) => first..doc.family(last).end,
        None => doc.family(index),
    }
}

/// the desktop notebook's outline keys on the paragraph with the caret and its subparagraphs:
/// Alt+Shift+↑/↓ move it past the paragraph above or below, Alt+Shift+→/← indent and
/// outdent it, Alt+Shift+1…9 show that many levels of the note (0: all of them), and
/// Alt+Shift++ (or =) / Alt+Shift+- unfold and fold it.
fn outline_keys(
    ctx: &egui::Context,
    doc: &mut Document,
    editor: &mut EditorState,
    now: f64,
) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    let span = outline_span(doc, editor, index);
    if let Some(key) = take_alt_shift(ctx, &[Key::ArrowUp, Key::ArrowDown]) {
        return undo_step(doc, editor, now, |doc| {
            doc.move_blocks(span, key == Key::ArrowUp).is_some()
        });
    }
    if let Some(key) = take_alt_shift(ctx, &[Key::ArrowRight, Key::ArrowLeft]) {
        return undo_step(doc, editor, now, |doc| {
            doc.shift_blocks(span, key == Key::ArrowRight)
        });
    }
    if let Some(key) = take_alt_shift(ctx, &LEVEL_KEYS) {
        let levels = LEVEL_KEYS
            .iter()
            .position(|level| *level == key)
            .unwrap_or(0) as u8;
        let changed = undo_step(doc, editor, now, |doc| doc.show_levels(levels));
        reveal_caret(doc, editor);
        return changed;
    }
    if take_alt_shift(ctx, &[Key::Plus, Key::Equals]).is_some() {
        return undo_step(doc, editor, now, |doc| {
            std::mem::take(&mut doc.blocks[index].collapsed)
        });
    }
    if take_alt_shift(ctx, &[Key::Minus]).is_some() {
        return undo_step(doc, editor, now, |doc| {
            let fold = doc.has_children(index) && !doc.blocks[index].collapsed;
            doc.blocks[index].collapsed |= fold;
            fold
        });
    }
    false
}

/// After folding, the caret can't stay in a paragraph that went out of sight: it goes to
/// the end of the visible paragraph that folded it away.
fn reveal_caret(doc: &Document, editor: &mut EditorState) {
    let Some(index) = editor.active_index(doc) else {
        return;
    };
    let hidden = doc.hidden_blocks();
    if !hidden[index] {
        return;
    }
    if let Some(shown) = (0..index).rev().find(|above| !hidden[*above]) {
        let block = &doc.blocks[shown];
        editor.focus_block(block.id, block.text.char_count());
    }
}

/// Where a moving selection end goes for an arrow key, Home or End: by char across
/// paragraphs, by paragraph up and down (keeping the column where it can), past what a
/// folded paragraph hides.
fn step_pos(doc: &Document, pos: Pos, key: Key) -> Pos {
    let Some(index) = doc.block_index(pos.block) else {
        return pos;
    };
    let hidden = doc.hidden_blocks();
    let text = |i: usize| doc.blocks[i].kind.is_textual() && !hidden[i];
    let count = |i: usize| doc.blocks[i].text.char_count();
    let at = |i: usize, char: usize| Pos {
        block: doc.blocks[i].id,
        char,
    };
    let previous = (0..index).rev().find(|i| text(*i));
    let next = (index + 1..doc.blocks.len()).find(|i| text(*i));
    match key {
        Key::ArrowLeft if pos.char > 0 => Pos {
            char: pos.char - 1,
            ..pos
        },
        Key::ArrowLeft => previous.map_or(pos, |i| at(i, count(i))),
        Key::ArrowRight if pos.char < count(index) => Pos {
            char: pos.char + 1,
            ..pos
        },
        Key::ArrowRight => next.map_or(pos, |i| at(i, 0)),
        Key::ArrowUp => previous.map_or(Pos { char: 0, ..pos }, |i| at(i, pos.char.min(count(i)))),
        Key::ArrowDown => next.map_or(
            Pos {
                char: count(index),
                ..pos
            },
            |i| at(i, pos.char.min(count(i))),
        ),
        Key::Home => Pos { char: 0, ..pos },
        Key::End => Pos {
            char: count(index),
            ..pos
        },
        _ => pos,
    }
}

/// Ctrl+arrows' steps, across paragraphs: ← / → by word (to the start of the word before,
/// or the end of the word after; from a paragraph's start or end into the next one), ↑ to
/// the start of this paragraph or the one above, ↓ to the end of this one or the one below.
fn ctrl_step(doc: &Document, pos: Pos, key: Key) -> Pos {
    let Some(index) = doc.block_index(pos.block) else {
        return pos;
    };
    let chars: Vec<char> = doc.blocks[index].text.text.chars().collect();
    let word = |at: usize| chars.get(at).is_some_and(|c| c.is_alphanumeric());
    let mut at = pos.char.min(chars.len());
    match key {
        Key::ArrowRight if at == chars.len() => step_pos(doc, pos, key),
        Key::ArrowRight => {
            while at < chars.len() && !word(at) {
                at += 1;
            }
            while word(at) {
                at += 1;
            }
            Pos { char: at, ..pos }
        }
        Key::ArrowLeft if at == 0 => step_pos(doc, pos, key),
        Key::ArrowLeft => {
            while at > 0 && !word(at - 1) {
                at -= 1;
            }
            while at > 0 && word(at - 1) {
                at -= 1;
            }
            Pos { char: at, ..pos }
        }
        Key::ArrowUp if at > 0 => Pos { char: 0, ..pos },
        Key::ArrowUp => Pos {
            char: 0,
            ..step_pos(doc, pos, Key::ArrowLeft)
        },
        Key::ArrowDown if at < chars.len() => Pos {
            char: chars.len(),
            ..pos
        },
        Key::ArrowDown => {
            let below = step_pos(doc, pos, Key::ArrowRight);
            let end = doc
                .block_index(below.block)
                .map_or(below.char, |i| doc.blocks[i].text.char_count());
            Pos { char: end, ..below }
        }
        _ => pos,
    }
}

const RANGE_KEYS: [Key; 6] = [
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Home,
    Key::End,
];

/// Shift+↓ or Shift+→ at the end of a paragraph (Shift+↑ or Shift+← at its start), with or
/// without Ctrl, carries the selection on into the next (previous) paragraph. Ctrl+A with
/// the whole paragraph already selected selects the whole note, as the desktop notebook's repeated
/// Ctrl+A does.
fn start_range_keys(
    ctx: &egui::Context,
    doc: &Document,
    doc_id: &str,
    editor: &mut EditorState,
) -> bool {
    // Cheap first: the text box's state (with its undo history) is only loaded for these keys.
    let wanted = ctx.input(|input| {
        (input.modifiers.shift && RANGE_KEYS[..4].iter().any(|key| input.key_pressed(*key)))
            || (input.modifiers.command && input.key_pressed(Key::A))
    });
    if !wanted {
        return false;
    }
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    let block = doc.blocks[index].id;
    let Some(cursor) = TextEditState::load(ctx, block_id(doc_id, block))
        .and_then(|state| state.cursor.char_range())
    else {
        return false;
    };
    let (anchor, caret) = (cursor.secondary.index.0, cursor.primary.index.0);
    let count = doc.blocks[index].text.char_count();
    if anchor.min(caret) == 0
        && anchor.max(caret) == count
        && ctx.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::A))
        })
    {
        let hidden = doc.hidden_blocks();
        let texts: Vec<usize> = (0..doc.blocks.len())
            .filter(|i| doc.blocks[*i].kind.is_textual() && !hidden[*i])
            .collect();
        if let (Some(&first), Some(&last)) = (texts.first(), texts.last()) {
            if first != last {
                let end = Pos {
                    block: doc.blocks[last].id,
                    char: doc.blocks[last].text.char_count(),
                };
                editor.range = Some((
                    Pos {
                        block: doc.blocks[first].id,
                        char: 0,
                    },
                    end,
                ));
                editor.pending_focus = Some(FocusRequest::Block {
                    id: end.block,
                    pos: end.char,
                });
            }
        }
        return true;
    }
    // Ctrl+Shift+arrows from a paragraph's edge carry the selection into the next or
    // previous one, by word or by paragraph.
    let ctrl_keys: &[Key] = match (caret == 0, caret == count) {
        (true, true) => &[
            Key::ArrowUp,
            Key::ArrowLeft,
            Key::ArrowDown,
            Key::ArrowRight,
        ],
        (true, false) => &[Key::ArrowUp, Key::ArrowLeft],
        (false, true) => &[Key::ArrowDown, Key::ArrowRight],
        (false, false) => &[],
    };
    if let Some(key) = ctrl_keys.iter().copied().find(|key| {
        ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, *key))
    }) {
        let focus = ctrl_step(doc, Pos { block, char: caret }, key);
        if focus.block != block {
            editor.range = Some((
                Pos {
                    block,
                    char: anchor,
                },
                focus,
            ));
            editor.pending_focus = Some(FocusRequest::Block {
                id: focus.block,
                pos: focus.char,
            });
        }
        return true;
    }
    let keys: &[Key] = match (caret == 0, caret == count) {
        (true, true) => &[
            Key::ArrowUp,
            Key::ArrowLeft,
            Key::ArrowDown,
            Key::ArrowRight,
        ],
        (true, false) => &[Key::ArrowUp, Key::ArrowLeft],
        (false, true) => &[Key::ArrowDown, Key::ArrowRight],
        (false, false) => return false,
    };
    let Some(key) = keys
        .iter()
        .copied()
        .find(|key| ctx.input_mut(|input| input.consume_key(Modifiers::SHIFT, *key)))
    else {
        return false;
    };
    let focus = moved_end(doc, doc_id, editor, Pos { block, char: caret }, key);
    if focus.block != block {
        editor.range = Some((
            Pos {
                block,
                char: anchor,
            },
            focus,
        ));
        editor.pending_focus = Some(FocusRequest::Block {
            id: focus.block,
            pos: focus.char,
        });
    }
    true
}

/// Keys while a selection crosses paragraphs: Shift+arrows (and Shift+Home/End,
/// Ctrl+Shift+arrows) move its moving end; arrows, Home and End drop it at its start or end,
/// as in the desktop notebook. `None` when none of these was pressed (formats, tags and levels then act
/// on the whole selection).
fn range_keys(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    now: f64,
) -> Option<bool> {
    let (start, end) = editor.range_in(doc)?;
    let (anchor, focus) = editor.range?;
    // Clipboard and typed text first: egui's text box would apply them to one paragraph.
    // (A paste over the range is `intercept_paste`'s, which ran before this.)
    let (mut copy, mut cut, mut typed) = (false, false, String::new());
    ctx.input_mut(|input| {
        input.events.retain(|event| match event {
            egui::Event::Copy => {
                copy = true;
                false
            }
            egui::Event::Cut => {
                cut = true;
                false
            }
            egui::Event::Text(text) => {
                typed.push_str(text);
                false
            }
            _ => true,
        })
    });
    if copy || cut {
        copy_range(ctx, doc, editor, (start, end));
    }
    let deleting = cut
        || ctx.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Backspace)
                || input.consume_key(Modifiers::NONE, Key::Delete)
        });
    if deleting {
        editor.push_undo(doc, now);
        let at = delete_range(doc, (start, end));
        editor.collapse_to(at);
        return Some(true);
    }
    if copy {
        return Some(false);
    }
    if !typed.is_empty() {
        replace_range(doc, editor, (start, end), &typed, now);
        return Some(true);
    }
    // Shift+Enter puts a line break in place of the selection.
    if ctx.input_mut(|input| input.consume_key(Modifiers::SHIFT, Key::Enter)) {
        replace_range(doc, editor, (start, end), "\n", now);
        return Some(true);
    }
    if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
        // One undo step brings back the selected text.
        editor.push_undo(doc, now);
        let at = delete_range(doc, (start, end));
        editor.range = None;
        split_paragraph(doc, editor, at.block, at.char, at.char);
        return Some(true);
    }
    // Ctrl+Shift+arrows move its moving end by word or by paragraph.
    if let Some(key) = [
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::ArrowUp,
        Key::ArrowDown,
    ]
    .into_iter()
    .find(|key| {
        ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, *key))
    }) {
        let focus = ctrl_step(doc, focus, key);
        if focus == anchor {
            editor.collapse_to(focus);
        } else {
            editor.range = Some((anchor, focus));
            editor.pending_focus = Some(FocusRequest::Block {
                id: focus.block,
                pos: focus.char,
            });
        }
        return Some(false);
    }
    // Shifted first: a plain match ignores an extra Shift.
    if let Some(key) = RANGE_KEYS
        .into_iter()
        .find(|key| ctx.input_mut(|input| input.consume_key(Modifiers::SHIFT, *key)))
    {
        let focus = moved_end(doc, doc_id, editor, focus, key);
        if focus == anchor {
            editor.collapse_to(focus);
        } else {
            editor.range = Some((anchor, focus));
            editor.pending_focus = Some(FocusRequest::Block {
                id: focus.block,
                pos: focus.char,
            });
        }
        return Some(false);
    }
    if let Some(key) = RANGE_KEYS
        .into_iter()
        .find(|key| ctx.input_mut(|input| input.consume_key(Modifiers::NONE, *key)))
    {
        let at = if matches!(key, Key::ArrowLeft | Key::ArrowUp | Key::Home) {
            start
        } else {
            end
        };
        editor.collapse_to(at);
        return Some(false);
    }
    None
}

fn handle_shortcuts(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
) -> bool {
    let mut changed = false;
    let Some(active) = editor.active_block else {
        return changed;
    };
    // The paragraph's text edit, or the table cell's with the caret.
    let active_id = editor
        .caret_widget(doc, doc_id)
        .unwrap_or_else(|| block_id(doc_id, active));
    let active_focused = ctx.memory(|memory| memory.has_focus(active_id));
    if !active_focused {
        return changed;
    }
    read_digits_by_position(ctx);

    let redo_shortcut = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    let undo_shortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
    let ctrl_y = KeyboardShortcut::new(Modifiers::CTRL, Key::Y);
    if ctx.input_mut(|input| input.consume_shortcut(&redo_shortcut))
        || (cfg!(not(target_os = "macos"))
            && ctx.input_mut(|input| input.consume_shortcut(&ctrl_y)))
    {
        changed |= editor.redo(doc);
    } else if ctx.input_mut(|input| input.consume_shortcut(&undo_shortcut)) {
        changed |= editor.undo(doc);
    }

    // Ctrl+K inserts or edits a link (the desktop notebook). Consume it even without a caret: egui's
    // text edit would otherwise delete the rest of the paragraph.
    if ctx.input_mut(|input| {
        input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::K))
    }) {
        begin_link(ctx, doc, doc_id, editor);
        return changed;
    }

    let Some((min, max)) = cursor_range(ctx, active_id) else {
        return changed;
    };

    let copying = ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy | egui::Event::Cut))
    });
    if copying
        && min < max
        && !editor
            .cell_selection
            .is_some_and(|(_, range)| !range.is_single())
    {
        // egui's text edit copies the chars; Folio keeps their formatting for a paste here.
        if let (Some(text), Some(index)) = (editor.active_text(doc), editor.active_index(doc)) {
            let piece = text.slice_chars(min..max);
            keep_inline_clip(editor, &doc.blocks[index].kind, piece);
        }
    }

    let now = ctx.input(|input| input.time);
    // In a table cell, the cell keys come first; nothing below may split, merge, indent,
    // retype or tag the table block itself. (The caret is in a cell when its text edit is
    // a cell's, as `caret_widget` decides.)
    let in_cell = editor.active_cell.is_some() && active_id != block_id(doc_id, active);
    // A paste that carries formatting, several lines or an asked-for mode is Folio's to
    // place (Paste Options); a plain one-line paste stays egui's (it autolinks as typed).
    let cell = editor.active_cell.filter(|_| in_cell);
    if let Some(pasted) = intercept_paste(ctx, doc, doc_id, editor, active, cell, (min, max), now) {
        return changed | pasted;
    }
    if in_cell {
        changed |= table_keys(ctx, doc, doc_id, editor, now);
    }
    // the desktop notebook's Ctrl+Alt+C copies the formatting at the caret and Ctrl+Alt+V gives it to
    // the selection (or the word at the caret), as often as wanted. Not when the keys
    // typed a character: Ctrl+Alt is AltGr on Windows, which types on many layouts.
    let typed = ctx.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(_)))
    });
    let format_key = |key: Key| {
        !typed
            && ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(
                    Modifiers::COMMAND | Modifiers::ALT,
                    key,
                ))
            })
    };
    if format_key(Key::C) {
        if let Some(format) = format_at_caret(doc, editor, false) {
            editor.copied_format = Some(format);
        }
        return changed;
    }
    if format_key(Key::V) {
        if let Some(format) = editor.copied_format.clone() {
            changed |= apply_format(ctx, doc, editor, &format, min..max);
        }
        return changed;
    }
    if !in_cell && outline_keys(ctx, doc, editor, now) {
        return true;
    }
    // A selection across paragraphs takes the keys that edit or move it. A selection in a
    // table cell stays in the cell.
    if !in_cell && editor.range_in(doc).is_some() {
        if let Some(edited) = range_keys(ctx, doc, doc_id, editor, now) {
            return changed | edited;
        }
    } else if !in_cell && start_range_keys(ctx, doc, doc_id, editor) {
        return changed;
    }
    // Ctrl+↑ / Ctrl+↓: the desktop notebook's previous and next paragraph (egui's own keys go to the
    // paragraph's start or end). Ctrl+Shift+↑ / ↓ are the range keys' (S2.11).
    if !in_cell {
        for (key, down) in [(Key::ArrowUp, false), (Key::ArrowDown, true)] {
            if util::consume_exact(ctx, Modifiers::COMMAND, key) {
                if let Some(index) = editor.active_index(doc) {
                    paragraph_step(doc, editor, index, min, down);
                }
                return changed;
            }
        }
    }
    // the desktop notebook's inline maths: Space or Enter right after "95+83+416=" writes the answer
    // first; the key then does what it always does (types the space, splits the line).
    let (min, max) = match inline_answer(ctx, doc, editor, in_cell, (min, max)) {
        Some(answer) if insert_at_caret(doc, doc_id, editor, &answer, (min, max), now) => {
            changed = true;
            let at = min + answer.chars().count();
            (at, at)
        }
        _ => (min, max),
    };
    let bold = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
    if ctx.input_mut(|input| input.consume_shortcut(&bold)) {
        changed |= toggle_mark(doc, editor, Mark::Bold, now);
    }
    let italic = KeyboardShortcut::new(Modifiers::COMMAND, Key::I);
    if ctx.input_mut(|input| input.consume_shortcut(&italic)) {
        changed |= toggle_mark(doc, editor, Mark::Italic, now);
    }
    let underline = KeyboardShortcut::new(Modifiers::COMMAND, Key::U);
    if ctx.input_mut(|input| input.consume_shortcut(&underline)) {
        changed |= toggle_mark(doc, editor, Mark::Underline, now);
    }
    let strike = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
    if ctx.input_mut(|input| input.consume_shortcut(&strike)) {
        changed |= toggle_mark(doc, editor, Mark::Strike, now);
    }
    // Ctrl+Shift+= raises and Ctrl+= lowers. Shortcut matching ignores an extra Shift, so the
    // shifted forms ("=" with Shift, or "+") are checked first.
    let superscript = [
        KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Equals),
        KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus),
    ];
    let subscript = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
    if superscript
        .iter()
        .any(|shortcut| ctx.input_mut(|input| input.consume_shortcut(shortcut)))
    {
        changed |= toggle_mark(doc, editor, Mark::Superscript, now);
    } else if ctx.input_mut(|input| input.consume_shortcut(&subscript)) {
        changed |= toggle_mark(doc, editor, Mark::Subscript, now);
    }
    // Ctrl+Shift+> / Ctrl+Shift+< step the size. egui has no "<"/">" keys: US keyboards
    // report "." and ",", ISO keyboards the key beside the left Shift. These must run before
    // Ctrl+. (bullets) below, which would otherwise match the shifted press too.
    let shifted = Modifiers::COMMAND | Modifiers::SHIFT;
    let grow = [
        KeyboardShortcut::new(shifted, Key::Period),
        KeyboardShortcut::new(shifted, Key::IntlBackslash),
    ];
    let shrink = [
        KeyboardShortcut::new(shifted, Key::Comma),
        KeyboardShortcut::new(Modifiers::COMMAND, Key::IntlBackslash),
    ];
    if grow
        .iter()
        .any(|shortcut| ctx.input_mut(|input| input.consume_shortcut(shortcut)))
    {
        changed |= grow_text(doc, editor, true, now);
    } else if shrink
        .iter()
        .any(|shortcut| ctx.input_mut(|input| input.consume_shortcut(shortcut)))
    {
        changed |= grow_text(doc, editor, false, now);
    }
    let clear = KeyboardShortcut::new(shifted, Key::N);
    if ctx.input_mut(|input| input.consume_shortcut(&clear)) {
        changed |= clear_formatting(doc, editor, now);
    }
    let date = KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::D);
    if ctx.input_mut(|input| input.consume_shortcut(&date)) {
        changed |= insert_at_caret(doc, doc_id, editor, &today(), (min, max), now);
    }
    let time = KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::T);
    if ctx.input_mut(|input| input.consume_shortcut(&time)) {
        changed |= insert_at_caret(doc, doc_id, editor, &now_time(), (min, max), now);
    }
    let date_time = KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::F);
    if ctx.input_mut(|input| input.consume_shortcut(&date_time)) {
        changed |= insert_at_caret(doc, doc_id, editor, &now_date_time(), (min, max), now);
    }
    // the desktop notebook's Alt+= puts an equation at the caret.
    let equation = KeyboardShortcut::new(Modifiers::ALT, Key::Equals);
    if ctx.input_mut(|input| input.consume_shortcut(&equation)) {
        // The key may also arrive as typed text, which the paragraph must not get.
        ctx.input_mut(|input| {
            input.events.retain(
                |event| !matches!(event, egui::Event::Text(text) if text == "=" || text == "≠"),
            );
        });
        let block = Block::new(BlockKind::Equation {
            source: String::new(),
            scale: crate::model::default_equation_size(),
        });
        let id = block.id;
        // `insert_blocks_at_caret` pushes the undo step.
        if insert_blocks_at_caret(ctx, doc, doc_id, editor, vec![block]) {
            editor.start_equation(id);
            changed = true;
        }
    }
    // the desktop notebook's paragraph shortcuts: Ctrl+. bullets, Ctrl+/ numbering, Ctrl+Alt+1…6
    // Heading 1–6 (repeating one turns the block back into Normal text), and the tags'
    // Ctrl+0 … Ctrl+9.
    if let Some(index) = editor.active_index(doc) {
        // A table keeps its kind and takes no tags while the caret is in one of its cells.
        if !in_cell {
            let alt = Modifiers::COMMAND | Modifiers::ALT;
            let heading = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
            ]
            .into_iter()
            .position(|key| {
                ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(alt, key)))
            });
            let kind = if let Some(n) = heading {
                Some(BlockKind::heading(n as u8 + 1))
            } else if ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Period))
            }) {
                Some(BlockKind::Bullet)
            } else if ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Slash))
            }) {
                Some(BlockKind::Numbered)
            } else {
                None
            };
            if let Some(kind) = kind {
                changed |= set_block_kind(doc, editor, index, kind, now);
            }
            // Ctrl+1 … Ctrl+9 apply the gallery's first nine tags and Ctrl+0 removes them. A
            // tag with a box goes on, then is ticked, then comes off, as Ctrl+1 does To Do;
            // the others go on and off. Ctrl+Alt (AltGr on Windows) and Ctrl+Shift
            // combinations type characters on many layouts; leave them.
            let plain = ctx.input(|input| !input.modifiers.alt && !input.modifiers.shift);
            if plain {
                for digit in 1..=9u8 {
                    let key = LEVEL_KEYS[usize::from(digit)];
                    if !ctx.input_mut(|input| {
                        input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key))
                    }) {
                        continue;
                    }
                    changed |= match editor.tags.at_digit(digit) {
                        Some(TagKey::Todo) => cycle_todo(doc, editor, index, now),
                        Some(TagKey::Line(tag)) if tag.checkable() => {
                            cycle_line_tag(doc, editor, index, tag, now)
                        }
                        Some(TagKey::Line(tag)) => toggle_line_tag(doc, editor, index, tag, now),
                        Some(TagKey::Custom(custom)) => {
                            cycle_or_toggle_custom(doc, editor, index, custom, now)
                        }
                        None => false,
                    };
                }
                if ctx.input_mut(|input| {
                    input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0))
                }) {
                    changed |= clear_line_tags(doc, editor, index, now);
                }
            }
        }
        // Ctrl+L / Ctrl+R align, as in the desktop notebook (center is in the ribbon; Ctrl+E is search).
        let align = if ctx.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::L))
        }) {
            Some(ParaAlign::Left)
        } else if ctx.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::R))
        }) {
            Some(ParaAlign::Right)
        } else {
            None
        };
        if let Some(align) = align {
            changed |= set_align(doc, editor, align, now);
        }
    }

    if !in_cell
        && min == max
        && min == 0
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Backspace))
    {
        changed |= backspace_start(doc, editor, active);
        return changed;
    }

    // Tab and Shift+Tab move the paragraph a level, its subparagraphs along, as in the desktop notebook
    // (block text edits lock focus, so Tab never jumps out of the note).
    if !in_cell
        && editor
            .active_index(doc)
            .is_some_and(|index| doc.blocks[index].kind.is_textual())
    {
        let shift_tab = KeyboardShortcut::new(Modifiers::SHIFT, Key::Tab);
        let tab = KeyboardShortcut::new(Modifiers::NONE, Key::Tab);
        if ctx.input_mut(|input| input.consume_shortcut(&shift_tab)) {
            changed |= shift_level(doc, editor, -1, now);
        } else if ctx.input_mut(|input| input.consume_shortcut(&tab)) {
            // After the text of a plain paragraph, Tab starts a table, as in the desktop notebook;
            // anywhere else it moves the paragraph a level.
            let at_end = editor.active_index(doc).filter(|index| {
                editor.range.is_none()
                    && min == max
                    && min > 0
                    && min == doc.blocks[*index].text.char_count()
            });
            let tabled = at_end.is_some_and(|index| tab_into_table(doc, editor, index, now));
            changed |= tabled || shift_level(doc, editor, 1, now);
        }
    }

    // Up at the start and Down at the end go to the paragraph above or below (into a
    // table's last or first row), past what a folded paragraph hides. In a cell, the cell
    // keys did this.
    if !in_cell
        && min == max
        && min == 0
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowUp))
    {
        if let Some(index) = editor.active_index(doc) {
            focus_beside(doc, editor, index, false);
        }
        return changed;
    }
    if !in_cell && min == max {
        if let Some(index) = editor.active_index(doc) {
            let count = doc.blocks[index].text.char_count();
            if min == count
                && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown))
            {
                focus_beside(doc, editor, index, true);
                return changed;
            }
        }
    }

    let active_kind = editor
        .active_index(doc)
        .map(|index| doc.blocks[index].kind.clone());
    // Shift+Enter is a line break inside the paragraph, as in the desktop notebook: the text box types
    // it. (`consume_key` would take it too: Shift is ignored there.)
    let line_break = ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. }
                if modifiers.shift)
        })
    });
    if !in_cell
        && !line_break
        && !matches!(active_kind, Some(BlockKind::Code))
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
    {
        changed |= split_block(doc, editor, active, min, max, now);
    }
    changed
}

/// Today's date as it is written into notes (Insert › Date, Alt+Shift+D): the desktop notebook's short
/// date, "10/29/2026", "29/10/2026".
pub fn today() -> String {
    util::short_date(chrono::Local::now())
}

/// The time of day as it is written into notes (Alt+Shift+T).
pub fn now_time() -> String {
    util::time_of_day(chrono::Local::now())
}

/// Today's date and the time, as they are written into notes (Alt+Shift+F).
pub fn now_date_time() -> String {
    util::date_and_time(chrono::Local::now())
}

/// Inserts `text` at the caret of the active block (replacing any selection), keeping
/// the caret after it. The caret is read from the text edit's stored state, so this
/// also works right after a ribbon click took focus away. The equation being typed takes
/// the text into its source, at the source field's caret; a picture, a file, a recording
/// or a finished equation has the text typed in the paragraph after it.
pub fn insert_text(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    text: &str,
) -> bool {
    let Some(active) = editor.active_block else {
        return false;
    };
    editor.start_history(doc_id);
    if editor.equation_block == Some(active) {
        return insert_into_equation(ctx, doc, doc_id, editor, active, text);
    }
    let now = ctx.input(|input| input.time);
    let Some(index) = doc.block_index(active) else {
        return false;
    };
    // A paragraph or a table cell takes the text at its caret.
    let Some(end) = editor.active_text(doc).map(StyledText::char_count) else {
        return insert_after_block(doc, editor, index, text, now);
    };
    // A caret move not made yet (one waits while a dialog is up, as More Symbols… stays
    // up between its inserts) is where the caret is.
    let range = match &editor.pending_focus {
        Some(FocusRequest::Block { id, pos }) if *id == active => (*pos, *pos),
        Some(FocusRequest::Cell {
            id,
            row,
            col,
            range,
        }) if *id == active && editor.active_cell == Some((*row, *col)) => (range.start, range.end),
        _ => editor
            .caret_widget(doc, doc_id)
            .and_then(|id| cursor_range(ctx, id))
            .unwrap_or((end, end)),
    };
    insert_at_caret(doc, doc_id, editor, text, range, now)
}

/// Types `text` into the source of equation block `id`, the one being typed, at its source
/// field's caret (replacing what it selects); the field keeps the keyboard, the caret after
/// the text. Part of the equation's editing session, which is its undo step.
fn insert_into_equation(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    id: u64,
    text: &str,
) -> bool {
    let Some(index) = doc.block_index(id) else {
        return false;
    };
    let BlockKind::Equation { source, .. } = &mut doc.blocks[index].kind else {
        return false;
    };
    let field = equation_source_id(doc_id, id);
    let count = source.chars().count();
    let (min, max) = cursor_range(ctx, field).unwrap_or((count, count));
    let (min, max) = (min.min(count), max.min(count));
    let byte = |chars: usize| {
        source
            .char_indices()
            .nth(chars)
            .map_or(source.len(), |(at, _)| at)
    };
    let (start, end) = (byte(min), byte(max));
    source.replace_range(start..end, text);
    set_focus(ctx, field, min + text.chars().count(), None);
    editor.focus_equation = true;
    true
}

/// Types `text` in the paragraph after block `index` (a picture, a file, a recording, a
/// finished equation): the empty one that follows it, else a new one at its level, as
/// `insert_blocks_at_caret` leaves after what it puts in. One undo step; the caret after
/// the text.
fn insert_after_block(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    text: &str,
    now: f64,
) -> bool {
    editor.push_undo(doc, now);
    // A folded paragraph keeps its hidden subparagraphs: the text goes after them.
    let next = if doc.blocks[index].collapsed && doc.has_children(index) {
        doc.family(index).end
    } else {
        index + 1
    };
    let reuse = doc.blocks.get(next).is_some_and(|block| {
        matches!(block.kind, BlockKind::Paragraph) && block.text.text.is_empty()
    });
    if !reuse {
        let mut paragraph = Block::paragraph("");
        paragraph.indent = doc.blocks[index].indent;
        doc.blocks.insert(next, paragraph);
    }
    editor.active_block = Some(doc.blocks[next].id);
    editor.selection = None;
    editor.range = None;
    type_at(doc, editor, next, text, (0, 0));
    true
}

/// The answer to write at the caret before this frame's Space or Enter: the paragraph's
/// text up to the caret ends with an expression and "=" (`calc::answer`). None in a
/// table cell, a code block, with a selection, or with Options › Advanced's maths off.
fn inline_answer(
    ctx: &egui::Context,
    doc: &Document,
    editor: &EditorState,
    in_cell: bool,
    (min, max): (usize, usize),
) -> Option<String> {
    if !editor.auto_calc || in_cell || min != max {
        return None;
    }
    let key = ctx.input(|input| {
        input.events.iter().any(|event| match event {
            egui::Event::Text(text) => text == " ",
            egui::Event::Key {
                key: Key::Enter,
                pressed: true,
                modifiers,
                ..
            } => modifiers.is_none(),
            _ => false,
        })
    });
    if !key {
        return None;
    }
    let block = editor
        .active_block(doc)
        .filter(|block| block.kind.is_textual() && block.kind != BlockKind::Code)?;
    let before: String = block.text.text.chars().take(min).collect();
    let answer = crate::calc::answer(&before, crate::i18n::tr("number-decimal-separator") == ",")?;
    // "2+2=|4": the answer is already there after the caret, so Space only types a space.
    let after: String = block.text.text.chars().skip(min).collect();
    (!after.trim_start().starts_with(&answer)).then_some(answer)
}

fn insert_at_caret(
    doc: &mut Document,
    _doc_id: &str,
    editor: &mut EditorState,
    text: &str,
    range: (usize, usize),
    now: f64,
) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    if editor.active_text(doc).is_none() {
        return false;
    }
    editor.push_undo(doc, now);
    match editor.active_cell {
        // In a table cell: typed into the cell, which keeps the caret after it.
        Some((row, col)) if !doc.blocks[index].kind.is_textual() => {
            let style = editor
                .pending_style
                .clone()
                .unwrap_or_else(|| editor.active_style.clone());
            let Some(cell) = editor.active_text_mut(doc) else {
                return false;
            };
            let caret = type_into(cell, text, range, style);
            let id = doc.blocks[index].id;
            editor.focus_cell(id, row, col, caret..caret);
        }
        _ => type_at(doc, editor, index, text, range),
    }
    true
}

/// Puts `text` in place of chars `min..max` of textual block `index`, in the style typing
/// there would take, and asks for the caret after it.
fn type_at(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    text: &str,
    range: (usize, usize),
) {
    let style = editor
        .pending_style
        .clone()
        .unwrap_or_else(|| editor.active_style.clone());
    let caret = type_into(&mut doc.blocks[index].text, text, range, style);
    let id = doc.blocks[index].id;
    editor.focus_block(id, caret);
}

/// Puts `text` in place of chars `min..max` of `block`, in `style`; returns the caret
/// after it.
fn type_into(
    block: &mut StyledText,
    text: &str,
    (min, max): (usize, usize),
    style: InlineStyle,
) -> usize {
    let start = block.char_to_byte(min.min(block.char_count()));
    let end = block.char_to_byte(max.min(block.char_count()));
    let mut updated = block.text.clone();
    updated.replace_range(start..end, text);
    let caret = min + text.chars().count();
    block.sync_after_edit(&updated, style, Some(caret));
    caret
}

/// Puts `blocks` into the note at the caret, where the desktop notebook puts a picture, a file or an
/// equation inserted while you type: before the paragraph when the caret is at its start
/// (or it is empty), after it at its end, and between its two halves otherwise. A
/// selection is replaced. The blocks take the paragraph's outline level, and the caret
/// goes to the paragraph after them. One undo step.
pub(crate) fn insert_blocks_at_caret(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    mut blocks: Vec<Block>,
) -> bool {
    if blocks.is_empty() {
        return false;
    }
    editor.start_history(doc_id);
    let now = ctx.input(|input| input.time);
    editor.push_undo(doc, now);
    // A selection across paragraphs goes first, as typing over it takes it.
    let mut caret_at = None;
    if let Some(range) = editor.range_in(doc) {
        let at = delete_range(doc, range);
        editor.range = None;
        editor.active_block = Some(at.block);
        caret_at = Some(at.char);
    }
    let Some(index) = editor.active_index(doc) else {
        doc.blocks.extend(blocks);
        let paragraph = Block::paragraph("");
        let id = paragraph.id;
        doc.blocks.push(paragraph);
        editor.active_block = Some(id);
        editor.selection = None;
        editor.focus_block(id, 0);
        return true;
    };
    let level = doc.blocks[index].indent;
    for block in &mut blocks {
        block.indent = level;
    }
    let textual = doc.blocks[index].kind.is_textual();
    let count = doc.blocks[index].text.char_count();
    let (min, max) = caret_at
        .map(|caret| (caret, caret))
        .or_else(|| {
            editor
                .active_block
                .and_then(|id| cursor_range(ctx, block_id(doc_id, id)))
        })
        .unwrap_or((count, count));
    let (min, max) = (min.min(count), max.min(count));
    if textual && max > min {
        doc.blocks[index].text.delete_chars(min..max);
    }
    let count = doc.blocks[index].text.char_count();
    let caret = min.min(count);
    // A folded paragraph keeps its hidden subparagraphs: what goes after it goes after
    // them, as Enter puts its new paragraph.
    let next = if doc.blocks[index].collapsed && doc.has_children(index) {
        doc.family(index).end
    } else {
        index + 1
    };
    let (at, caret_block) = if !textual || caret >= count && count > 0 {
        (next, None)
    } else if caret == 0 {
        (index, Some(doc.blocks[index].id))
    } else {
        let rest = doc.blocks[index].text.split_off_chars(caret);
        let head = &doc.blocks[index];
        let mut tail = Block::new(continued_kind(&head.kind));
        tail.text = rest;
        tail.indent = head.indent;
        tail.align = head.align;
        tail.marker = head.marker;
        carry_tags(head, &mut tail);
        let id = tail.id;
        doc.blocks.insert(next, tail);
        (next, Some(id))
    };
    let after = at + blocks.len();
    for (offset, block) in blocks.into_iter().enumerate() {
        doc.blocks.insert(at + offset, block);
    }
    let id = caret_block.unwrap_or_else(|| match doc.blocks.get(after) {
        Some(next) if next.kind == BlockKind::Paragraph && next.text.text.is_empty() => next.id,
        _ => {
            let mut paragraph = Block::paragraph("");
            paragraph.indent = level;
            let id = paragraph.id;
            doc.blocks.insert(after, paragraph);
            id
        }
    });
    editor.active_block = Some(id);
    editor.selection = None;
    editor.focus_block(id, 0);
    true
}

/// The link dialog's starting point for a caret or selection: inside an existing link
/// it edits that whole link, otherwise it links the selection (or inserts at the caret).
fn link_edit_for(
    text: &StyledText,
    doc_id: &str,
    block: u64,
    (min, max): (usize, usize),
) -> LinkEdit {
    let found = if min == max {
        min.checked_sub(1).and_then(|before| text.link_at(before))
    } else {
        text.link_at(min).filter(|(range, _)| max <= range.end)
    };
    let (range, url, existing) = match found {
        Some((range, url)) => (range, url.to_string(), true),
        None => (min..max, String::new(), false),
    };
    let display = text
        .text
        .chars()
        .skip(range.start)
        .take(range.len())
        .collect();
    LinkEdit {
        doc_id: doc_id.to_string(),
        block,
        range,
        text: display,
        url,
        existing,
    }
}

/// Opens the link dialog at the active block's caret (Ctrl+K, Insert → Link).
pub fn begin_link(
    ctx: &egui::Context,
    doc: &Document,
    doc_id: &str,
    editor: &mut EditorState,
) -> bool {
    let Some(block) = editor.active_block else {
        return false;
    };
    let Some(index) = doc.block_index(block) else {
        return false;
    };
    if !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let text = &doc.blocks[index].text;
    let count = text.char_count();
    let id = block_id(doc_id, block);
    // From the ribbon the text edit has lost focus, and egui collapses the selection of an
    // unfocused text edit; the editor remembers it (as for the Home ribbon buttons).
    let focused = ctx.memory(|memory| memory.has_focus(id));
    let remembered = editor
        .selection
        .clone()
        .filter(|_| !focused)
        .map(|range| (range.start.min(count), range.end.min(count)));
    let range = remembered
        .or_else(|| cursor_range(ctx, id))
        .unwrap_or((count, count));
    editor.link_edit = Some(link_edit_for(text, doc_id, block, range));
    true
}

/// Applies the link dialog: replaces the range with the display text (if it changed)
/// and links it, or removes the link when the address is empty.
pub fn apply_link(doc: &mut Document, editor: &mut EditorState, edit: &LinkEdit, now: f64) -> bool {
    let Some(index) = doc.block_index(edit.block) else {
        return false;
    };
    if !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let url = normalize_link(&edit.url);
    let count = doc.blocks[index].text.char_count();
    let start = edit.range.start.min(count);
    let mut end = edit.range.end.min(count).max(start);
    let display = if edit.text.is_empty() {
        url.clone().unwrap_or_default()
    } else {
        edit.text.clone()
    };
    if url.is_none() && start == end {
        return false;
    }
    let text = &doc.blocks[index].text;
    let current: String = text.text.chars().skip(start).take(end - start).collect();
    let retext = !display.is_empty() && display != current;
    let relink = !text.all_in_range(start..end, |style| style.link.as_deref() == url.as_deref());
    editor.active_block = Some(edit.block);
    if !retext && !relink {
        // Saved untouched: nothing changes, so no undo step; just give the caret back.
        editor.focus_block(edit.block, end);
        return false;
    }
    editor.push_undo(doc, now);
    let text = &mut doc.blocks[index].text;
    if retext {
        let mut style = if start < end {
            text.style_at(text.char_to_byte(start))
        } else {
            text.style_for_caret(start)
        };
        style.link = None;
        let (from, to) = (text.char_to_byte(start), text.char_to_byte(end));
        let mut updated = text.text.clone();
        updated.replace_range(from..to, &display);
        end = start + display.chars().count();
        text.sync_after_edit(&updated, style, Some(end));
    }
    text.set_link(start..end, url.map(Arc::from));
    editor.focus_block(edit.block, end);
    true
}

/// A tag menu choice, shared by the ribbon's Tag menu and the text right-click menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagCommand {
    Todo,
    Toggle(LineTag),
    /// The gallery's custom tag at this index (`TagGallery::custom`).
    Custom(u16),
    Clear,
    FindTags,
    /// Opens Customize Tags.
    Customize,
}

impl TagCommand {
    /// What choosing a gallery tag does: To Do, a built-in tag or a custom one on or off.
    pub(crate) fn of(key: TagKey) -> Self {
        match key {
            TagKey::Todo => TagCommand::Todo,
            TagKey::Line(tag) => TagCommand::Toggle(tag),
            TagKey::Custom(index) => TagCommand::Custom(index),
        }
    }
}

fn apply_tag_command(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    command: TagCommand,
    now: f64,
) -> bool {
    // A table takes no tags (Find Tags and Customize Tags still open).
    if editor.active_cell.is_some()
        && !matches!(command, TagCommand::FindTags | TagCommand::Customize)
    {
        return false;
    }
    match command {
        TagCommand::Todo => {
            set_block_kind(doc, editor, index, BlockKind::Todo { checked: false }, now)
        }
        TagCommand::Toggle(tag) => toggle_line_tag(doc, editor, index, tag, now),
        TagCommand::Custom(custom) => toggle_custom_tag(doc, editor, index, custom, now),
        TagCommand::Clear => clear_line_tags(doc, editor, index, now),
        TagCommand::FindTags => {
            editor.find_tags_request = true;
            false
        }
        TagCommand::Customize => {
            editor.customize_tags_request = true;
            false
        }
    }
}

/// Adds or removes one tag on a paragraph (or each selected one, on or off as the paragraph
/// at `index` decides), keeping tags in menu order.
fn toggle_line_tag(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    tag: LineTag,
    now: f64,
) -> bool {
    if !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let on = !doc.blocks[index].tags.contains(&tag);
    editor.push_undo(doc, now);
    for i in targets(doc, editor, index) {
        let tags = &mut doc.blocks[i].tags;
        let present = tags.iter().position(|existing| *existing == tag);
        match (on, present) {
            (true, None) => {
                tags.push(tag);
                tags.sort_by_key(|tag| tag.rank());
            }
            (false, Some(at)) => {
                tags.remove(at);
                doc.blocks[i].checked_tags.retain(|checked| *checked != tag);
            }
            _ => {}
        }
    }
    true
}

/// Adds or removes one of the gallery's custom tags, by name, as [`toggle_line_tag`] does a
/// built-in one. Each paragraph keeps its own copy of the tag's look, unticked.
fn toggle_custom_tag(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    custom: u16,
    now: f64,
) -> bool {
    let Some(tag) = editor.tags.custom(custom).cloned() else {
        return false;
    };
    if !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let on = custom_tag_state(&doc.blocks[index], &tag.name).is_none();
    editor.push_undo(doc, now);
    for i in targets(doc, editor, index) {
        let block = &mut doc.blocks[i];
        match (on, custom_tag_state(block, &tag.name)) {
            (true, None) => set_custom_tag_state(block, &tag, Some(false)),
            (false, Some(_)) => set_custom_tag_state(block, &tag, None),
            _ => {}
        }
    }
    true
}

/// The next step of the desktop notebook's Ctrl+digit on a tag that can be ticked: the tag, then the
/// tag ticked, then no tag. `None` is no tag; `Some(ticked)` the tag.
fn next_tick(state: Option<bool>) -> Option<bool> {
    match state {
        None => Some(false),
        Some(false) => Some(true),
        Some(true) => None,
    }
}

fn line_tag_state(block: &Block, tag: LineTag) -> Option<bool> {
    block
        .tags
        .contains(&tag)
        .then(|| block.checked_tags.contains(&tag))
}

/// Puts a built-in tag on a paragraph unticked or ticked, or takes it off (`None`).
fn set_line_tag_state(block: &mut Block, tag: LineTag, state: Option<bool>) {
    block.tags.retain(|existing| *existing != tag);
    block.checked_tags.retain(|existing| *existing != tag);
    if let Some(ticked) = state {
        block.tags.push(tag);
        block.tags.sort_by_key(|tag| tag.rank());
        if ticked {
            block.checked_tags.push(tag);
            block.checked_tags.sort_by_key(|tag| tag.rank());
        }
    }
}

fn custom_tag_state(block: &Block, name: &str) -> Option<bool> {
    let mut named = block.custom_tags.iter().filter(|tag| tag.name == name);
    named.next().map(|tag| tag.checked)
}

/// Puts a custom tag on a paragraph unticked or ticked, keeping its place, or takes every
/// tag of its name off (`None`).
fn set_custom_tag_state(block: &mut Block, tag: &CustomTag, state: Option<bool>) {
    let Some(ticked) = state else {
        block
            .custom_tags
            .retain(|existing| existing.name != tag.name);
        return;
    };
    let mut found = false;
    for existing in block.custom_tags.iter_mut().filter(|t| t.name == tag.name) {
        existing.checked = ticked;
        found = true;
    }
    if !found {
        block.custom_tags.push(CustomTag {
            checked: ticked,
            ..tag.clone()
        });
    }
}

/// Ctrl+digit on a built-in tag that can be ticked (Call back, Discuss with manager…):
/// [`next_tick`], as Ctrl+1 does To Do. The paragraph at `index` decides; the selected
/// paragraphs all follow. One undo step.
fn cycle_line_tag(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    tag: LineTag,
    now: f64,
) -> bool {
    if editor.active_cell.is_some() || !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let state = next_tick(line_tag_state(&doc.blocks[index], tag));
    editor.push_undo(doc, now);
    for i in targets(doc, editor, index) {
        set_line_tag_state(&mut doc.blocks[i], tag, state);
    }
    true
}

/// [`cycle_line_tag`] for the gallery's custom tag at `custom`, whose symbol is a box.
fn cycle_custom_tag(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    custom: u16,
    now: f64,
) -> bool {
    let Some(tag) = editor.tags.custom(custom).cloned() else {
        return false;
    };
    if editor.active_cell.is_some() || !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let state = next_tick(custom_tag_state(&doc.blocks[index], &tag.name));
    editor.push_undo(doc, now);
    for i in targets(doc, editor, index) {
        set_custom_tag_state(&mut doc.blocks[i], &tag, state);
    }
    true
}

/// Ctrl+digit on a custom tag: the tick cycle when its symbol is a box, else on and off.
fn cycle_or_toggle_custom(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    custom: u16,
    now: f64,
) -> bool {
    if editor.active_cell.is_some() {
        return false;
    }
    match editor.tags.custom(custom) {
        Some(tag) if tag.symbol.checkable() => cycle_custom_tag(doc, editor, index, custom, now),
        Some(_) => toggle_custom_tag(doc, editor, index, custom, now),
        None => false,
    }
}

/// Whether a paragraph has tags besides To Do: built-in or custom.
fn has_tags(block: &Block) -> bool {
    !block.tags.is_empty() || !block.custom_tags.is_empty()
}

/// Takes every tag but To Do off a paragraph, ticks included.
fn untag(block: &mut Block) {
    block.tags.clear();
    block.checked_tags.clear();
    block.custom_tags.clear();
}

/// The tags Enter gives the new paragraph: the same ones, unticked, as the desktop notebook's Enter on a
/// ticked To Do starts an unticked one.
fn carry_tags(from: &Block, to: &mut Block) {
    to.tags = from.tags.clone();
    to.custom_tags = from
        .custom_tags
        .iter()
        .map(|tag| CustomTag {
            checked: false,
            ..tag.clone()
        })
        .collect();
}

/// Ctrl+0: removes every tag from a paragraph (or each selected one), To Do included.
fn clear_line_tags(doc: &mut Document, editor: &mut EditorState, index: usize, now: f64) -> bool {
    let clearing: Vec<usize> = targets(doc, editor, index)
        .into_iter()
        .filter(|&i| {
            has_tags(&doc.blocks[i]) || matches!(doc.blocks[i].kind, BlockKind::Todo { .. })
        })
        .collect();
    if clearing.is_empty() {
        return false;
    }
    editor.push_undo(doc, now);
    for i in clearing {
        let block = &mut doc.blocks[i];
        untag(block);
        if matches!(block.kind, BlockKind::Todo { .. }) {
            block.kind = BlockKind::Paragraph;
        }
    }
    true
}

/// The paragraphs a paragraph command acts on: every text paragraph of a selection that
/// crosses paragraphs, else the one at `index`.
fn targets(doc: &Document, editor: &EditorState, index: usize) -> Vec<usize> {
    match editor.range_in(doc) {
        Some(range) => range_segments(doc, range)
            .into_iter()
            .map(|(index, _)| index)
            .collect(),
        None => vec![index],
    }
}

/// Ctrl+1, as in the desktop notebook: a To Do box, then ticked, then plain text again. The active
/// paragraph's state decides; the selected paragraphs all follow. One undo step.
fn cycle_todo(doc: &mut Document, editor: &mut EditorState, index: usize, now: f64) -> bool {
    if editor.active_cell.is_some() || !doc.blocks[index].kind.is_textual() {
        return false;
    }
    let kind = match doc.blocks[index].kind {
        BlockKind::Todo { checked: false } => BlockKind::Todo { checked: true },
        BlockKind::Todo { checked: true } => BlockKind::Paragraph,
        _ => BlockKind::Todo { checked: false },
    };
    let changing: Vec<usize> = targets(doc, editor, index)
        .into_iter()
        .filter(|&i| doc.blocks[i].kind.is_textual() && doc.blocks[i].kind != kind)
        .collect();
    if changing.is_empty() {
        return false;
    }
    editor.push_undo(doc, now);
    for i in changing {
        doc.blocks[i].kind = kind.clone();
    }
    true
}

/// Sets a textual block's kind. Choosing the kind the block already has turns it back
/// into plain text, like the list buttons in the desktop notebook and Word.
fn set_block_kind(
    doc: &mut Document,
    editor: &mut EditorState,
    index: usize,
    kind: BlockKind,
    now: f64,
) -> bool {
    // A table keeps its kind while the caret is in one of its cells.
    if editor.active_cell.is_some() {
        return false;
    }
    let block = &doc.blocks[index];
    if !block.kind.is_textual() {
        return false;
    }
    let same = std::mem::discriminant(&block.kind) == std::mem::discriminant(&kind)
        && (!matches!(kind, BlockKind::Heading { .. }) || block.kind == kind);
    let kind = if same && kind != BlockKind::Paragraph {
        BlockKind::Paragraph
    } else {
        kind
    };
    let changing: Vec<usize> = targets(doc, editor, index)
        .into_iter()
        .filter(|&i| doc.blocks[i].kind.is_textual() && doc.blocks[i].kind != kind)
        .collect();
    if changing.is_empty() {
        return false;
    }
    editor.push_undo(doc, now);
    for i in changing {
        set_kind(&mut doc.blocks[i], kind.clone());
    }
    true
}

/// Runs a paragraph command as one undo step, leaving no step when nothing changed.
fn undo_step(
    doc: &mut Document,
    editor: &mut EditorState,
    now: f64,
    command: impl FnOnce(&mut Document) -> bool,
) -> bool {
    let before = doc.clone();
    if !command(doc) {
        return false;
    }
    editor.push_undo(&before, now);
    true
}

fn backspace_start(doc: &mut Document, editor: &mut EditorState, active: u64) -> bool {
    let Some(index) = doc.block_index(active) else {
        return false;
    };
    let now = 0.0;
    if doc.blocks[index].indent > 0 {
        editor.push_undo(doc, now);
        doc.blocks[index].indent -= 1;
        return true;
    }
    if has_tags(&doc.blocks[index]) {
        editor.push_undo(doc, now);
        untag(&mut doc.blocks[index]);
        return true;
    }
    // A list item, heading, quote or code block first becomes plain text.
    if !matches!(doc.blocks[index].kind, BlockKind::Paragraph)
        && (doc.blocks[index].kind.is_list() || index == 0)
    {
        editor.push_undo(doc, now);
        doc.blocks[index].kind = BlockKind::Paragraph;
        return true;
    }
    if index == 0 {
        return false;
    }
    editor.push_undo(doc, now);
    // Under a folded paragraph: unfold it first, so nothing goes into (or out of) a
    // paragraph that isn't shown. The next Backspace joins the lines as usual.
    if doc.hidden_blocks()[index - 1] {
        doc.expand_to(index - 1);
        editor.focus_block(active, 0);
        return true;
    }
    if !doc.blocks[index - 1].kind.is_textual() {
        // Backspace after a divider or picture removes that block, not this text.
        doc.blocks.remove(index - 1);
        editor.active_block = Some(active);
        editor.focus_block(active, 0);
        return true;
    }
    let current = doc.blocks.remove(index);
    let previous = &mut doc.blocks[index - 1];
    let pos = previous.text.char_count();
    previous.text.append(&current.text);
    let id = previous.id;
    editor.active_block = Some(id);
    editor.focus_block(id, pos);
    true
}

/// The kind of the paragraph that goes on after one of `kind` is split: a list stays the
/// same list (a To Do starts unchecked), code stays code, anything else is plain text.
fn continued_kind(kind: &BlockKind) -> BlockKind {
    match kind {
        BlockKind::Bullet => BlockKind::Bullet,
        BlockKind::Numbered => BlockKind::Numbered,
        BlockKind::Todo { .. } => BlockKind::Todo { checked: false },
        BlockKind::Code => BlockKind::Code,
        _ => BlockKind::Paragraph,
    }
}

fn split_block(
    doc: &mut Document,
    editor: &mut EditorState,
    active: u64,
    char_pos: usize,
    selection_end: usize,
    now: f64,
) -> bool {
    if doc.block_index(active).is_none() {
        return false;
    }
    editor.push_undo(doc, now);
    split_paragraph(doc, editor, active, char_pos, selection_end)
}

/// Enter: [`split_block`] without its undo step, for a caller that recorded one.
fn split_paragraph(
    doc: &mut Document,
    editor: &mut EditorState,
    active: u64,
    char_pos: usize,
    selection_end: usize,
) -> bool {
    let Some(index) = doc.block_index(active) else {
        return false;
    };
    if selection_end > char_pos {
        doc.blocks[index].text.delete_chars(char_pos..selection_end);
        editor.selection = None;
    }
    if !matches!(doc.blocks[index].kind, BlockKind::Code) {
        doc.blocks[index].text.autolink_word_before(char_pos);
    }
    let current_kind = doc.blocks[index].kind.clone();
    if doc.blocks[index].text.text.is_empty() && current_kind.is_list() {
        if doc.blocks[index].indent > 0 {
            doc.blocks[index].indent -= 1;
        } else {
            // Leaving the list leaves the line plain: in the desktop notebook the To Do box and the
            // other tags are peers, and one Enter on an empty line takes them all off.
            doc.blocks[index].kind = BlockKind::Paragraph;
            untag(&mut doc.blocks[index]);
        }
        return true;
    }
    // Enter on an empty tagged line drops the tag, like leaving a list.
    if doc.blocks[index].text.text.is_empty() && has_tags(&doc.blocks[index]) {
        untag(&mut doc.blocks[index]);
        return true;
    }
    // the desktop notebook carries the format at the caret (or one armed for typing) into the new
    // paragraph, so a font or size lasts beyond one paragraph. A heading's next line starts
    // as plain text.
    let carried = editor
        .pending_style
        .clone()
        .unwrap_or_else(|| doc.blocks[index].text.style_for_caret(char_pos));
    let carry =
        !matches!(current_kind, BlockKind::Heading { .. }) && carried != InlineStyle::default();
    let new_kind = continued_kind(&current_kind);
    // A folded paragraph keeps its hidden subparagraphs and stays folded, as in the desktop notebook:
    // the new paragraph comes after them, and Enter at its start opens a line above it.
    let folded = doc.blocks[index].collapsed && doc.has_children(index);
    if folded && char_pos == 0 && !doc.blocks[index].text.text.is_empty() {
        let mut block = Block::new(new_kind);
        block.indent = doc.blocks[index].indent;
        block.marker = doc.blocks[index].marker;
        block.align = doc.blocks[index].align;
        carry_tags(&doc.blocks[index], &mut block);
        doc.blocks.insert(index, block);
        editor.active_block = Some(active);
        editor.focus_block(active, 0);
        editor.pending_style = None;
        return true;
    }
    let at = if folded {
        doc.family(index).end
    } else {
        index + 1
    };
    let mut rest = doc.blocks[index].text.split_off_chars(char_pos);
    let empty = rest.text.is_empty();
    if empty {
        rest = StyledText::default();
    }
    let mut block = Block::new(new_kind);
    block.text = rest;
    block.indent = doc.blocks[index].indent;
    block.marker = doc.blocks[index].marker;
    block.align = doc.blocks[index].align;
    carry_tags(&doc.blocks[index], &mut block);
    let id = block.id;
    doc.blocks.insert(at, block);
    editor.active_block = Some(id);
    editor.focus_block(id, 0);
    if empty && carry {
        editor.active_style = carried;
        editor.arm_style();
    } else {
        editor.pending_style = None;
    }
    true
}

fn delete_block(doc: &mut Document, editor: &mut EditorState, index: usize) -> bool {
    // The selection belonged to the block going away.
    editor.selection = None;
    if doc.blocks.len() <= 1 {
        if let Some(block) = doc.blocks.get_mut(index) {
            block.kind = BlockKind::Paragraph;
            untag(block);
            block.text = StyledText::default();
            editor.active_block = Some(block.id);
            editor.focus_block(block.id, 0);
            return true;
        }
        return false;
    }
    let removed = doc.blocks.remove(index);
    let next = doc
        .blocks
        .get(index)
        .or_else(|| doc.blocks.last())
        .map(|block| block.id);
    if let Some(id) = next {
        editor.active_block = Some(id);
        editor.focus_block(
            id,
            doc.block_index(id)
                .map_or(0, |i| doc.blocks[i].text.char_count()),
        );
    }
    let _ = removed;
    true
}

/// Applies a character format to the selection, or arms it for the text typed next when
/// nothing is selected. Returns whether the document changed.
fn apply_inline(
    doc: &mut Document,
    editor: &mut EditorState,
    now: f64,
    f: impl Fn(&mut InlineStyle),
) -> bool {
    if let Some(range) = editor.range_in(doc) {
        editor.push_undo(doc, now);
        for (index, chars) in range_segments(doc, range) {
            doc.blocks[index].text.apply_style_chars(chars, &f);
        }
        return true;
    }
    // The paragraph or the table cell with the caret.
    if let Some(range) = editor.selection.clone() {
        if editor.active_text(doc).is_some() {
            editor.push_undo(doc, now);
            if let Some(text) = editor.active_text_mut(doc) {
                text.apply_style_chars(range, &f);
            }
            return true;
        }
    }
    f(&mut editor.active_style);
    editor.arm_style();
    false
}

fn toggle_mark(doc: &mut Document, editor: &mut EditorState, mark: Mark, now: f64) -> bool {
    if editor.active_index(doc).is_none() {
        return false;
    }
    let on = if editor.range_in(doc).is_some() {
        !editor.active_style_state(doc, |style| has_mark(style, mark))
    } else {
        match (&editor.selection, editor.active_text(doc)) {
            (Some(range), Some(text)) => {
                !text.all_in_range(range.clone(), |style| has_mark(style, mark))
            }
            _ => !has_mark(&editor.active_style, mark),
        }
    };
    apply_inline(doc, editor, now, |style| set_mark(style, mark, on))
}

fn set_highlight(
    doc: &mut Document,
    editor: &mut EditorState,
    color: Option<Rgb>,
    now: f64,
) -> bool {
    apply_inline(doc, editor, now, |style| style.highlight = color)
}

fn set_color(doc: &mut Document, editor: &mut EditorState, color: Option<Rgb>, now: f64) -> bool {
    apply_inline(doc, editor, now, |style| style.color = color)
}

/// The next size up or down the desktop notebook's list from `pt` (which may fall between entries).
fn step_size(pt: f32, bigger: bool) -> u8 {
    if bigger {
        FONT_SIZES
            .into_iter()
            .find(|size| f32::from(*size) > pt + 0.01)
            .unwrap_or(FONT_SIZES[FONT_SIZES.len() - 1])
    } else {
        FONT_SIZES
            .into_iter()
            .rev()
            .find(|size| f32::from(*size) < pt - 0.01)
            .unwrap_or(FONT_SIZES[0])
    }
}

/// Ctrl+Shift+> / Ctrl+Shift+<: every character steps from its own size.
fn grow_text(doc: &mut Document, editor: &mut EditorState, bigger: bool, now: f64) -> bool {
    if let Some(range) = editor.range_in(doc) {
        editor.push_undo(doc, now);
        for (index, chars) in range_segments(doc, range) {
            let base = px_to_pt(kind_defaults(&doc.blocks[index].kind).0);
            doc.blocks[index].text.apply_style_chars(chars, |style| {
                style.size = Some(step_size(style.size.map_or(base, f32::from), bigger));
            });
        }
        return true;
    }
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    // A cell's text is sized as a paragraph's.
    let base = if editor.active_cell.is_some() {
        px_to_pt(kind_defaults(&BlockKind::Paragraph).0)
    } else {
        px_to_pt(kind_defaults(&doc.blocks[index].kind).0)
    };
    apply_inline(doc, editor, now, |style| {
        style.size = Some(step_size(style.size.map_or(base, f32::from), bigger));
    })
}

fn set_align(doc: &mut Document, editor: &mut EditorState, align: ParaAlign, now: f64) -> bool {
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    // In a table, the cell with the caret takes the alignment.
    if editor.active_cell.is_some() {
        let Some(cell) = editor.active_cell_mut(doc) else {
            return false;
        };
        if cell.align == align {
            return false;
        }
        editor.push_undo(doc, now);
        if let Some(cell) = editor.active_cell_mut(doc) {
            cell.align = align;
        }
        return true;
    }
    let changing: Vec<usize> = targets(doc, editor, index)
        .into_iter()
        .filter(|&i| doc.blocks[i].kind.is_textual() && doc.blocks[i].align != align)
        .collect();
    if changing.is_empty() {
        return false;
    }
    editor.push_undo(doc, now);
    for i in changing {
        doc.blocks[i].align = align;
    }
    true
}

/// What Clear Formatting leaves of a character's style: only its link (the desktop notebook keeps
/// hyperlinks).
fn cleared_style(style: &InlineStyle) -> InlineStyle {
    InlineStyle {
        link: style.link.clone(),
        ..InlineStyle::default()
    }
}

/// Ctrl+Shift+N: back to plain text. A selection loses its character formats (links stay);
/// with nothing selected the whole paragraph does, and a heading, quote or code block
/// becomes text.
fn clear_formatting(doc: &mut Document, editor: &mut EditorState, now: f64) -> bool {
    if let Some(range) = editor.range_in(doc) {
        editor.push_undo(doc, now);
        for (index, chars) in range_segments(doc, range) {
            doc.blocks[index]
                .text
                .apply_style_chars(chars, |style| *style = cleared_style(style));
            let block = &mut doc.blocks[index];
            block.align = ParaAlign::Left;
            if matches!(
                block.kind,
                BlockKind::Heading { .. }
                    | BlockKind::Quote
                    | BlockKind::Code
                    | BlockKind::Citation
            ) {
                block.kind = BlockKind::Paragraph;
            }
        }
        editor.active_style = InlineStyle::default();
        editor.pending_style = None;
        return true;
    }
    let Some(index) = editor.active_index(doc) else {
        return false;
    };
    if editor.active_text(doc).is_none() {
        return false;
    }
    editor.active_style = InlineStyle::default();
    editor.pending_style = None;
    if let Some(range) = editor.selection.clone() {
        editor.push_undo(doc, now);
        if let Some(text) = editor.active_text_mut(doc) {
            text.apply_style_chars(range, |style| *style = cleared_style(style));
        }
        return true;
    }
    // In a table cell, the whole cell loses its formats and its alignment; the table
    // keeps its kind.
    if editor.active_cell.is_some() {
        let Some(cell) = editor.active_cell_mut(doc) else {
            return false;
        };
        let unformatted = cell
            .text
            .runs
            .iter()
            .all(|run| cleared_style(&run.style) == run.style);
        if unformatted && cell.align.is_left() {
            return false;
        }
        editor.push_undo(doc, now);
        if let Some(cell) = editor.active_cell_mut(doc) {
            let count = cell.text.char_count();
            cell.text
                .apply_style_chars(0..count, |style| *style = cleared_style(style));
            cell.align = ParaAlign::Left;
        }
        return true;
    }
    let block = &doc.blocks[index];
    let styled_kind = matches!(
        block.kind,
        BlockKind::Heading { .. } | BlockKind::Quote | BlockKind::Citation | BlockKind::Code
    );
    let unformatted = block
        .text
        .runs
        .iter()
        .all(|run| cleared_style(&run.style) == run.style);
    if unformatted && !styled_kind && block.align.is_left() {
        return false;
    }
    editor.push_undo(doc, now);
    let block = &mut doc.blocks[index];
    let count = block.text.char_count();
    block
        .text
        .apply_style_chars(0..count, |style| *style = cleared_style(style));
    block.align = ParaAlign::Left;
    if styled_kind {
        block.kind = BlockKind::Paragraph;
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn show_block(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    number: usize,
    lead: f32,
    editor: &mut EditorState,
    library: &Library,
    textures: &mut TextureCache,
    t: Tokens,
    actions: &mut Vec<BlockAction>,
) -> bool {
    let block_id_value = doc.blocks[index].id;
    // Not a table's cells, every frame: `show_table` works on the document itself.
    let kind = doc.blocks[index].kind.without_cells();
    // the desktop notebook's paragraphs have no space between them; only rules, pictures and file or
    // recording cards get air.
    let (top_padding, bottom_padding) = match kind {
        BlockKind::Divider => (8.0, 8.0),
        BlockKind::Image { .. }
        | BlockKind::Drawing { .. }
        | BlockKind::File { .. }
        | BlockKind::Audio { .. } => (4.0, 4.0),
        // A table breathes like a picture.
        BlockKind::Table(_) => (6.0, 6.0),
        _ => (0.0, 0.0),
    };
    let pitch = editor.line_pitch;
    // Paragraph Spacing Options' space before and after; on a ruled page by whole rules,
    // so the text stays on them, as in the desktop notebook.
    let spacing = doc.blocks[index].spacing;
    let space = |pt: f32| {
        let px = points_to_px(pt);
        match pitch {
            Some(pitch) if px > 0.0 => (px / pitch).ceil() * pitch,
            _ => px,
        }
    };
    let top_padding = top_padding + space(spacing.before);
    let bottom_padding = bottom_padding + space(spacing.after);
    ui.add_space(top_padding);
    let mut changed = false;

    let tag_slots = tag_column(&doc.blocks[index]);
    let todo = matches!(kind, BlockKind::Todo { .. });
    let marker = marker_width(ui.ctx(), &doc.blocks[index], number, t);
    let tags_width = (tag_slots.len() + usize::from(todo)) as f32 * TAG_SLOT;
    let level = f32::from(doc.blocks[index].indent.min(MAX_LEVEL)) * LEVEL_INDENT;
    // Its tags, box and bullet sit in the middle of a first line made taller by Line
    // Spacing At Least, as its text does.
    let slot_height = kind_defaults(&kind)
        .1
        .max(points_to_px(spacing.line_at_least));
    // A centred or right-aligned paragraph takes its tags and list marker along, as in
    // the desktop notebook: they sit just left of its first line, so they are drawn once the text is
    // laid out. Their slots stay (empty), so the text keeps its width.
    let follows_text =
        !doc.blocks[index].align.is_left() && kind.is_textual() && !matches!(kind, BlockKind::Code);
    let row = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // What follows the text: a tag, or the To Do box or list marker (`None`).
        let mut lead_slots: Vec<(Rect, Option<&TagSlot>)> = Vec::new();
        // A left-aligned list item's marker slot: its marker goes on the text's first
        // baseline, so it is painted once the text is laid out.
        let mut marker_slot: Option<Rect> = None;
        // Tags hang left of the text: this paragraph's tags (and To Do box) sit right
        // before its marker and text, wherever its level puts them.
        ui.add_space((lead + level - tags_width).max(0.0));
        for slot in &tag_slots {
            let sense = if slot.tick.is_some() && !follows_text {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(vec2(TAG_SLOT, slot_height), sense);
            if follows_text {
                lead_slots.push((rect, Some(slot)));
                continue;
            }
            actions.extend(show_tag_slot(
                ui.painter(),
                response,
                slot,
                block_id_value,
                t,
            ));
        }
        if todo || marker > 0.0 {
            let width = if todo { TAG_SLOT } else { marker };
            let sense = if todo && !follows_text {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(vec2(width, slot_height), sense);
            if follows_text {
                lead_slots.push((rect, None));
            } else if todo {
                paint_gutter_glyph(
                    ui.painter(),
                    rect,
                    &doc.blocks[index],
                    number,
                    None,
                    pitch,
                    t,
                );
                if response.clicked() {
                    actions.push(BlockAction::ToggleChecked(block_id_value));
                }
            } else {
                marker_slot = Some(rect);
            }
        }
        let content_start = ui.cursor().min;
        // Where the first line starts, once the text is shown.
        let mut first_line = content_start.x;
        match kind.clone() {
            BlockKind::Divider => {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                let y = rect.center().y;
                ui.painter().line_segment(
                    [pos2(rect.left(), y), pos2(rect.right(), y)],
                    Stroke::new(1.0, t.line_strong),
                );
                if response.clicked() {
                    editor.active_block = Some(block_id_value);
                }
            }
            BlockKind::Image {
                attachment,
                width,
                meta,
            } => {
                let column = ui.available_width();
                let response = show_image(
                    ui,
                    &attachment,
                    width,
                    false,
                    doc_id,
                    library,
                    textures,
                    t,
                    &meta,
                );
                #[cfg(test)]
                {
                    editor.last_picture_rect = Some(response.rect);
                }
                if response.clicked() {
                    editor.active_block = Some(block_id_value);
                    editor.picture_clicked = Some(block_id_value);
                }
                let active = editor.active_block == Some(block_id_value);
                // The corner handle resizes, keeping the picture's proportions. It shows
                // while the picture is hovered or picked, and while the pointer is on it
                // (half of it lies outside the picture).
                let corner = response.rect.right_bottom();
                let handle = Rect::from_center_size(corner, vec2(14.0, 14.0));
                if response.hovered() || active || ui.rect_contains_pointer(handle) {
                    let drag = ui.interact(
                        handle,
                        ui.make_persistent_id(("picture-size", doc_id, block_id_value)),
                        Sense::drag(),
                    );
                    ui.painter()
                        .circle(corner, 4.5, t.raised, Stroke::new(1.5, t.accent));
                    if drag.hovered() || drag.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    }
                    if drag.drag_started() {
                        // Pressing the handle picks the picture, as a click does. The press
                        // took the keyboard from the text, so Ctrl+Z is the page's: the drag
                        // is one page step, after the typing before it (Delete and the
                        // Picture tab's commands on it are page steps too).
                        editor.active_block = Some(block_id_value);
                        editor.picture_clicked = Some(block_id_value);
                        editor.page_step = Some((doc_id.to_owned(), doc.clone()));
                    }
                    if drag.dragged() && column > 0.0 {
                        let fraction = ((response.rect.width() + drag.drag_delta().x) / column)
                            .clamp(0.1, 1.0);
                        if let BlockKind::Image { width, .. } = &mut doc.blocks[index].kind {
                            if *width != fraction {
                                *width = fraction;
                                changed = true;
                            }
                        }
                    }
                }
                let mut picked = None;
                response.context_menu(|ui| picked = picture_menu(ui, t));
                if let Some(command) = picked {
                    editor.active_block = Some(block_id_value);
                    editor.picture_request = Some((block_id_value, command));
                }
            }
            BlockKind::Drawing { attachment, height } => {
                let response = show_image(
                    ui,
                    &attachment,
                    1.0,
                    true,
                    doc_id,
                    library,
                    textures,
                    t,
                    &crate::model::PictureMeta::default(),
                );
                let _ = height;
                if response.clicked() {
                    editor.active_block = Some(block_id_value);
                }
            }
            BlockKind::File {
                attachment,
                name,
                size,
            } => {
                let (rect, response) = ui.allocate_exact_size(
                    vec2(260.0_f32.min(ui.available_width()), 56.0),
                    Sense::click(),
                );
                // Find on page matches its name, as on a file item.
                let marks = editor
                    .find_marks
                    .get(&block_id(doc_id, block_id_value))
                    .cloned()
                    .unwrap_or_default();
                crate::canvas::paint_card(
                    ui,
                    t,
                    crate::canvas::file_glyph(&name),
                    t.accent,
                    &name,
                    &util::file_size(size),
                    rect,
                    &marks,
                );
                if response.clicked() {
                    editor.active_block = Some(block_id_value);
                }
                if response.double_clicked() {
                    editor.attachment_action = Some((attachment.clone(), AttachmentAction::Open));
                }
                response.context_menu(|ui| {
                    if widgets::menu_item(ui, Some(icons::ARROW_SQUARE_OUT), &t!("common-open"), t)
                        .clicked()
                    {
                        editor.attachment_action =
                            Some((attachment.clone(), AttachmentAction::Open));
                        ui.close();
                    }
                    if widgets::menu_item(
                        ui,
                        Some(icons::DOWNLOAD_SIMPLE),
                        &t!("canvas-menu-save-as"),
                        t,
                    )
                    .clicked()
                    {
                        editor.attachment_action =
                            Some((attachment.clone(), AttachmentAction::SaveAs));
                        ui.close();
                    }
                    // A PDF not yet printed out: Insert Printout, as on a file card.
                    if crate::canvas::is_pdf_name(&name)
                        && !editor.media.printed.contains(&attachment)
                    {
                        ui.separator();
                        if widgets::menu_item(
                            ui,
                            Some(icons::FILE_PDF),
                            &t!("printout-menu-insert"),
                            t,
                        )
                        .clicked()
                        {
                            editor.attachment_action =
                                Some((attachment.clone(), AttachmentAction::InsertPrintout));
                            ui.close();
                        }
                    }
                });
            }
            BlockKind::Audio {
                attachment,
                duration_ms,
            } => {
                show_recording_card(ui, editor, &attachment, duration_ms, block_id_value, t);
            }
            BlockKind::Equation { source, scale } => {
                let editing = editor.equation_block == Some(block_id_value);
                let layout = math::layout_source(ui.ctx(), &source, scale);
                // The formula, with its source field under it while it is typed.
                changed |= ui
                    .vertical(|ui| {
                        let size = (layout.size + vec2(8.0, 8.0)).max(vec2(48.0, 28.0));
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(size.x.min(ui.available_width()), size.y),
                            Sense::click(),
                        );
                        // Find on page matches the source; the whole formula is
                        // highlighted, as an equation item is.
                        if let Some(marks) =
                            editor.find_marks.get(&block_id(doc_id, block_id_value))
                        {
                            let current = marks.iter().any(|(_, current)| *current);
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(3),
                                theme::find_highlight(t, current),
                            );
                        }
                        if editing || response.hovered() {
                            ui.painter().rect_stroke(
                                rect,
                                CornerRadius::same(3),
                                Stroke::new(1.0, t.line_strong),
                                StrokeKind::Inside,
                            );
                        }
                        if layout.is_empty() {
                            ui.painter().text(
                                rect.left_center() + vec2(4.0, 0.0),
                                Align2::LEFT_CENTER,
                                "…",
                                theme::body(scale * 0.8),
                                t.faint,
                            );
                        } else {
                            math::paint(
                                ui.painter(),
                                &layout,
                                rect.min + vec2(4.0, 4.0),
                                1.0,
                                t.ink,
                            );
                        }
                        if response.clicked() {
                            if editing {
                                // A click on the formula leaves the source field typing.
                                editor.focus_equation = true;
                            } else {
                                // Another equation of this note being typed ends first.
                                if let Some(other) = editor.equation_block {
                                    actions.push(BlockAction::FinishEquation {
                                        block: other,
                                        enter: false,
                                    });
                                }
                                // The caret comes into this note: its history starts, as
                                // `show_document` starts it, before the session's step.
                                editor.start_history(doc_id);
                                let now = ui.input(|input| input.time);
                                editor.begin_equation(doc, block_id_value, now);
                                // A new session starts with the caret at the end.
                                set_focus(
                                    ui.ctx(),
                                    equation_source_id(doc_id, block_id_value),
                                    source.chars().count(),
                                    None,
                                );
                            }
                        }
                        if editing {
                            equation_source_field(
                                ui, doc, doc_id, index, editor, &layout, t, actions,
                            )
                        } else {
                            false
                        }
                    })
                    .inner;
            }
            BlockKind::Code => {
                let (edited, line) = show_text_edit(ui, doc, doc_id, index, editor, t, true, false);
                changed |= edited;
                first_line = line;
            }
            BlockKind::Table(_) => {
                changed |= show_table(ui, doc, doc_id, index, editor, t, actions);
            }
            kind => {
                changed |= paste_page_link(ui, doc, doc_id, index, editor, library);
                let quote = matches!(kind, BlockKind::Quote);
                let (edited, line) = ui
                    .scope(|ui| show_text_edit(ui, doc, doc_id, index, editor, t, false, quote))
                    .inner;
                changed |= edited;
                first_line = line;
            }
        }
        // The text's first baseline, where a text marker ("1.", "–"…) sits.
        let baseline = editor
            .galley_hint
            .as_ref()
            .filter(|_| marker_slot.is_some() || !lead_slots.is_empty())
            .and_then(|(galley, origin)| first_baseline(galley).map(|y| origin.y + y));
        if let Some(rect) = marker_slot {
            paint_gutter_glyph(
                ui.painter(),
                rect,
                &doc.blocks[index],
                number,
                baseline,
                pitch,
                t,
            );
        }
        if !lead_slots.is_empty() {
            let shift = vec2((first_line - content_start.x).max(0.0), 0.0);
            for (slot, (rect, tag)) in lead_slots.iter().enumerate() {
                let rect = rect.translate(shift);
                let id = ui.make_persistent_id(("lead", doc_id, block_id_value, slot));
                match tag {
                    Some(slot) => {
                        // On top of the text, as the box below.
                        let sense = if slot.tick.is_some() {
                            Sense::click_and_drag()
                        } else {
                            Sense::hover()
                        };
                        let response = ui.interact(rect, id, sense);
                        actions.extend(show_tag_slot(
                            ui.painter(),
                            response,
                            slot,
                            block_id_value,
                            t,
                        ));
                    }
                    None => {
                        paint_gutter_glyph(
                            ui.painter(),
                            rect,
                            &doc.blocks[index],
                            number,
                            baseline,
                            pitch,
                            t,
                        );
                        // On top of the text: a click (or a drag) on the box is the box's.
                        if matches!(kind, BlockKind::Todo { .. })
                            && ui.interact(rect, id, Sense::click_and_drag()).clicked()
                        {
                            actions.push(BlockAction::ToggleChecked(block_id_value));
                        }
                    }
                }
            }
        }
        let _ = ui;
    });
    let pointer_inside = ui
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|pointer| row.response.rect.contains(pointer));
    if row.response.hovered() || pointer_inside {
        editor.hovered_block = Some(block_id_value);
    }
    // Record where this paragraph went, for handles, selections and auto width.
    let text = editor.galley_hint.take();
    let head = text
        .as_ref()
        .and_then(|(galley, origin)| {
            galley
                .rows
                .first()
                .map(|line| egui::Rangef::new(origin.y + line.min_y(), origin.y + line.max_y()))
        })
        .unwrap_or_else(|| {
            egui::Rangef::new(
                row.response.rect.top(),
                row.response.rect.top() + slot_height,
            )
        });
    if let Some(rows) = editor.rows.get_mut(doc_id) {
        rows.push(Row {
            block: block_id_value,
            rect: row.response.rect,
            head,
            text,
        });
    }
    ui.add_space(bottom_padding);
    changed
}

/// The source field of equation block `block` in note `doc_id`.
fn equation_source_id(doc_id: &str, block: u64) -> Id {
    Id::unique(("equation-source", doc_id, block))
}

/// The one-line source field under the equation block being typed (block `index`), with
/// a live preview above it and any parse error under it. While it has the keyboard the
/// note counts as being typed in. Once it has lost the keyboard (Enter, Esc, Tab or a
/// click elsewhere) the session ends (`finish_equation`); a press waits for its release,
/// so what the click does (the ribbon's Undo, Copy LaTeX) still sees it open, and an open
/// menu waits for its pick. Returns whether the source changed.
#[allow(clippy::too_many_arguments)]
fn equation_source_field(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    editor: &mut EditorState,
    layout: &math::MathLayout,
    t: Tokens,
    actions: &mut Vec<BlockAction>,
) -> bool {
    let block = doc.blocks[index].id;
    let BlockKind::Equation { source, .. } = &doc.blocks[index].kind else {
        return false;
    };
    let mut buffer = source.clone();
    let id = equation_source_id(doc_id, block);
    if editor.focus_equation && !behind_dialog(ui.ctx()) {
        editor.focus_equation = false;
        // The caret stays where the field last had it (at the end for a new one).
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let font = theme::mono(13.0);
    ui.add_space(2.0);
    let response = ui.add(
        TextEdit::singleline(&mut buffer)
            .id(id)
            .desired_width(ui.available_width())
            .font(font.clone())
            .text_color(t.ink)
            .hint_text(
                RichText::new(t!("editor-equation-hint"))
                    .font(font)
                    .color(t.faint),
            ),
    );
    let focused = response.has_focus();
    if focused {
        editor.text_widget = Some(id);
        // The equation is what is typed in: a ribbon click gives the keyboard back here.
        // (Alt+= at the end of a paragraph draws that paragraph first, still focused for
        // the frame, and it made itself the active block.)
        editor.active_block = Some(block);
    }
    let mut changed = false;
    if response.changed() {
        if let BlockKind::Equation { source, .. } = &mut doc.blocks[index].kind {
            *source = buffer;
            changed = true;
        }
    }
    if let Some(error) = &layout.error {
        ui.add_space(2.0);
        ui.label(RichText::new(error).font(theme::ui(11.0)).color(t.danger));
    }
    // A menu opened from the ribbon (Insert › Symbol) took the keyboard: what it inserts
    // goes into this source, so the session waits for it to close.
    let menu_open = egui::Popup::is_any_open(ui.ctx());
    if !focused
        && !editor.focus_equation
        && !menu_open
        && !ui.input(|input| input.pointer.any_down())
    {
        let enter = ui.input(|input| input.key_pressed(Key::Enter));
        actions.push(BlockAction::FinishEquation { block, enter });
    }
    changed
}

/// Whether note `doc_id` has the keyboard: the paragraph or table cell with the caret, or
/// the source field of the equation block being typed in it. The canvas then counts the note as
/// being typed in (its undo session, the ribbon's text commands).
pub(crate) fn has_keyboard(
    ctx: &egui::Context,
    doc: &Document,
    doc_id: &str,
    editor: &EditorState,
) -> bool {
    // The caret's text edit: a paragraph's, or a table cell's.
    let caret = editor
        .caret_widget(doc, doc_id)
        .is_some_and(|widget| ctx.memory(|memory| memory.has_focus(widget)));
    let equation = editor.equation_block.is_some_and(|block| {
        doc.block_index(block).is_some()
            && ctx.memory(|memory| memory.has_focus(equation_source_id(doc_id, block)))
    });
    caret || equation
}

/// The LaTeX of the equation block being typed, when it is in `doc` and not blank: what
/// the Equation tab's Copy LaTeX copies.
pub(crate) fn equation_latex(doc: &Document, editor: &EditorState) -> Option<String> {
    let index = doc.block_index(editor.equation_block?)?;
    match &doc.blocks[index].kind {
        BlockKind::Equation { source, .. } if !source.trim().is_empty() => {
            Some(math::latex_of(source))
        }
        _ => None,
    }
}

/// A recording in a note, drawn as the canvas draws a recording: its play circle at the
/// left, its title and its length (or that it is recording), and while it plays, how far
/// it has got. The canvas plays it (`attachment_action`), as it does a recording item.
fn show_recording_card(
    ui: &mut Ui,
    editor: &mut EditorState,
    attachment: &str,
    duration_ms: u64,
    block: u64,
    t: Tokens,
) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(300.0_f32.min(ui.available_width()), 64.0),
        Sense::click(),
    );
    let playing = editor
        .media
        .playing
        .as_ref()
        .filter(|(clip, _, _)| clip == attachment)
        .map(|(_, progress, on)| (*progress, *on));
    let recording = editor.media.recording.as_deref() == Some(attachment);
    let glyph = if playing.is_some_and(|(_, on)| on) {
        icons::PAUSE
    } else {
        icons::PLAY
    };
    // A clip stopped while its page was not open (or one from before lengths were kept)
    // has its length read from its file, as a recording item does.
    let length = match duration_ms {
        0 => editor.media.lengths.get(attachment).copied().unwrap_or(0),
        ms => ms,
    };
    let detail = if recording {
        t!("canvas-audio-recording-now").into_owned()
    } else {
        crate::audio::format_time(length)
    };
    crate::canvas::paint_card(
        ui,
        t,
        glyph,
        t.accent,
        &t!("item-recording"),
        &detail,
        rect,
        &[],
    );
    let play = Rect::from_min_max(rect.min, pos2(rect.left() + 48.0, rect.bottom()));
    ui.painter().circle_stroke(
        pos2(rect.left() + 26.0, rect.center().y),
        16.0,
        Stroke::new(1.5, t.accent),
    );
    if let Some((progress, _)) = playing {
        let (left, right) = (rect.left() + 48.0, rect.right() - 14.0);
        let y = rect.bottom() - 9.0;
        ui.painter().line_segment(
            [
                pos2(left, y),
                pos2(left + (right - left) * progress.clamp(0.0, 1.0), y),
            ],
            Stroke::new(2.0, t.accent),
        );
    }
    if response
        .hover_pos()
        .is_some_and(|pointer| play.contains(pointer))
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        editor.active_block = Some(block);
        if response
            .interact_pointer_pos()
            .is_some_and(|pointer| play.contains(pointer))
        {
            editor.attachment_action = Some((attachment.to_owned(), AttachmentAction::TogglePlay));
        }
    }
    response.context_menu(|ui| {
        if widgets::menu_item(ui, Some(icons::PLAY), &t!("canvas-menu-play-pause"), t).clicked() {
            editor.attachment_action = Some((attachment.to_owned(), AttachmentAction::TogglePlay));
            ui.close();
        }
        if widgets::menu_item(
            ui,
            Some(icons::DOWNLOAD_SIMPLE),
            &t!("canvas-menu-save-as"),
            t,
        )
        .clicked()
        {
            editor.attachment_action = Some((attachment.to_owned(), AttachmentAction::SaveAs));
            ui.close();
        }
    });
}

/// The page a pasted text links to, when the text is nothing but a link to a page, and
/// the paragraph's block id for a link to one of its paragraphs.
fn pasted_page_link(text: &str) -> Option<(&str, Option<u64>)> {
    let (id, block) = crate::model::parse_page_link(text)?;
    (!id.is_empty() && !text.contains(char::is_whitespace)).then_some((id, block))
}

/// The longest a pasted paragraph link's text gets, `…` included.
const PARAGRAPH_LINK_CHARS: usize = 60;

/// What a pasted link to a paragraph reads: the paragraph's text on one line, shortened.
/// `None` when the paragraph is gone or has no text.
fn paragraph_link_text(library: &Library, page: &str, block: u64) -> Option<String> {
    let page = library.get_page(page).ok().flatten()?;
    let text = page
        .canvas
        .items
        .iter()
        .filter_map(crate::model::CanvasItem::doc)
        .find_map(|doc| {
            doc.block_index(block)
                .map(|at| doc.blocks[at].text.text.clone())
        })?;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| util::truncate(&text, PARAGRAPH_LINK_CHARS))
}

/// A lone link to a page pasted into the focused block goes in as a link reading the
/// page's title, as in the desktop notebook: one undo step, the caret after it. A link to a paragraph
/// reads as the paragraph's text instead, while it is there. Anything else (several
/// lines, other addresses, a page that is gone or in the trash) is left to the text edit,
/// which pastes it as it always has.
fn paste_page_link(
    ui: &Ui,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    editor: &mut EditorState,
    library: &Library,
) -> bool {
    let block = doc.blocks[index].id;
    let id = block_id(doc_id, block);
    if !ui.memory(|memory| memory.has_focus(id)) {
        return false;
    }
    let pasted = ui.input(|input| {
        input
            .events
            .iter()
            .enumerate()
            .find_map(|(at, event)| match event {
                egui::Event::Paste(text) => Some((at, text.clone())),
                _ => None,
            })
    });
    let Some((at, url)) = pasted else {
        return false;
    };
    let Some((page, paragraph)) = pasted_page_link(&url) else {
        return false;
    };
    let Some(title) = library.page_title(page).ok().flatten() else {
        return false;
    };
    let title = paragraph
        .and_then(|block| paragraph_link_text(library, page, block))
        .unwrap_or_else(|| display_title(&title).into_owned());
    ui.input_mut(|input| input.events.remove(at));
    let now = ui.input(|input| input.time);
    let text = &doc.blocks[index].text;
    let count = text.char_count();
    let (min, max) = cursor_range(ui.ctx(), id).map_or((count, count), |(min, max)| {
        (min.min(count), max.min(count))
    });
    editor.push_undo(doc, now);
    let style = editor
        .pending_style
        .take()
        .unwrap_or_else(|| editor.active_style.clone());
    let text = &mut doc.blocks[index].text;
    let mut updated = text.text.clone();
    updated.replace_range(text.char_to_byte(min)..text.char_to_byte(max), &title);
    let end = min + title.chars().count();
    text.sync_after_edit(&updated, style, Some(end));
    text.set_link(min..end, Some(Arc::from(url.as_str())));
    // The text edit shows the new text this frame, with the caret after the link.
    let mut state = TextEditState::load(ui.ctx(), id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(end))));
    state.store(ui.ctx(), id);
    editor.selection = None;
    editor.active_block = Some(block);
    editor.title_focused = false;
    true
}

/// One tag in a paragraph's tag column.
#[derive(Debug, Clone, PartialEq)]
struct TagSlot {
    symbol: TagSymbol,
    /// Its name, shown on hover.
    label: String,
    checked: bool,
    /// What a click ticks, for a tag with a box.
    tick: Option<TagTick>,
}

/// A paragraph's tag column, as the desktop notebook orders it: its icon tags, then its custom tags that
/// have a symbol. Its To Do box comes after them, next to the text.
fn tag_column(block: &Block) -> Vec<TagSlot> {
    let line = block
        .tags
        .iter()
        .filter(|tag| tag.background().is_none())
        .map(|&tag| TagSlot {
            symbol: TagSymbol::Tag(tag),
            label: tag.label().into_owned(),
            checked: tag.checkable() && block.checked_tags.contains(&tag),
            tick: tag.checkable().then_some(TagTick::Line(tag)),
        });
    let custom = block
        .custom_tags
        .iter()
        .filter(|tag| tag.symbol != TagSymbol::None)
        .map(|tag| TagSlot {
            symbol: tag.symbol,
            label: tag.name.clone(),
            checked: tag.symbol.checkable() && tag.checked,
            tick: tag
                .symbol
                .checkable()
                .then(|| TagTick::Custom(tag.name.clone())),
        });
    line.chain(custom).collect()
}

/// Paints one tag of the tag column on `response`'s rect and names it on hover. Returns the
/// tick a click on its box asks for.
fn show_tag_slot(
    painter: &egui::Painter,
    response: egui::Response,
    slot: &TagSlot,
    block: u64,
    t: Tokens,
) -> Option<BlockAction> {
    paint_symbol(painter, response.rect, slot.symbol, slot.checked, t);
    let clicked = response.clicked();
    if slot.tick.is_some() {
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::CheckBox, true, slot.checked, &slot.label)
        });
    }
    response.on_hover_text(&slot.label);
    let tick = slot.tick.clone().filter(|_| clicked)?;
    Some(BlockAction::ToggleTagChecked(block, tick))
}

/// A tag column symbol: To Do's box, or a built-in tag's icon. It sits 12 px below
/// `rect`'s top, where a paragraph's first line is (Customize Tags draws its list with it).
pub(crate) fn paint_symbol(
    painter: &egui::Painter,
    rect: Rect,
    symbol: TagSymbol,
    checked: bool,
    t: Tokens,
) {
    match symbol {
        TagSymbol::None => {}
        TagSymbol::Checkbox => paint_todo_box(painter, rect.center(), checked, t),
        TagSymbol::Tag(tag) => paint_tag_icon(painter, rect, tag, checked, t),
    }
}

/// A built-in tag's icon in its colour; Project A and B are solid squares, as in the desktop notebook.
/// A ticked tag gets a tick over the icon's lower right.
fn paint_tag_icon(painter: &egui::Painter, rect: Rect, tag: LineTag, checked: bool, t: Tokens) {
    let center = pos2(rect.center().x, rect.top() + 12.0);
    let color = tag_ink(tag, t);
    // Fluent's numbered boxes come ticked: a To Do priority tag not yet ticked is an empty
    // box holding its number, and ticked, the glyph (its own tick, no other).
    let number = match tag {
        LineTag::TodoPriority1 => Some("1"),
        LineTag::TodoPriority2 => Some("2"),
        _ => None,
    };
    if matches!(tag, LineTag::ProjectA | LineTag::ProjectB) {
        let square = Rect::from_center_size(center, vec2(9.0, 9.0));
        painter.rect_filled(square, egui::epaint::CornerRadiusF32::same(1.5), color);
    } else if let Some(number) = number.filter(|_| !checked) {
        let square = Rect::from_center_size(center, vec2(CHECKBOX_SIZE, CHECKBOX_SIZE));
        painter.rect_stroke(
            square,
            CornerRadius::same(2),
            Stroke::new(1.2, color),
            StrokeKind::Inside,
        );
        painter.text(
            center,
            Align2::CENTER_CENTER,
            number,
            theme::semibold(8.5),
            color,
        );
    } else {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            tag.icon(),
            theme::icon(12.5),
            color,
        );
    }
    if checked && number.is_none() {
        painter.text(
            center + vec2(3.0, 3.0),
            Align2::CENTER_CENTER,
            icons::CHECK,
            theme::icon(10.0),
            t.ink,
        );
    }
}

/// A tag's colour in the tag column. On dark mode's page the darker ones (Discuss with
/// manager's navy, Call back's blue) are lifted toward white until they read at 3:1.
fn tag_ink(tag: LineTag, t: Tokens) -> Color32 {
    tag_glyph_color(Some(tag.color()), t)
}

/// A tag's glyph colour on the page, in a menu, a pane or a dialog: the accent for To Do
/// (`None`), else the tag's own, lifted in dark mode where it would not read.
pub(crate) fn tag_glyph_color(color: Option<Rgb>, t: Tokens) -> Color32 {
    match color {
        None => t.accent,
        Some(color) if t.dark => theme::lifted_on(color.to_color32(), t.raised, 3.0),
        Some(color) => color.to_color32(),
    }
}

/// The tags hung left of a paragraph's text: its icon tags, its custom tags with a symbol
/// and its To Do box (see [`tag_column`]).
fn tag_slots(block: &Block) -> usize {
    block
        .tags
        .iter()
        .filter(|tag| tag.background().is_none())
        .count()
        + block
            .custom_tags
            .iter()
            .filter(|tag| tag.symbol != TagSymbol::None)
            .count()
        + usize::from(matches!(block.kind, BlockKind::Todo { .. }))
}

/// How far a note's tags (and To Do boxes) hang out left of its text: the most any
/// paragraph's tags reach past its level's indent. the desktop notebook hangs tags in the margin, so a
/// tagged note's text starts where an untagged one's would, and a frame that holds tags
/// reaches that much further left.
pub(crate) fn tag_lead(doc: &Document) -> f32 {
    doc.blocks
        .iter()
        .map(|block| {
            let level = f32::from(block.indent.min(MAX_LEVEL)) * LEVEL_INDENT;
            (tag_slots(block) as f32 * TAG_SLOT - level).max(0.0)
        })
        .fold(0.0, f32::max)
}

/// The width a note's text wants unwrapped: its widest shown paragraph or equation, with
/// its tags' hang (`tag_lead`), its level's indent and its marker. Notes that size
/// themselves grow to it. Laid out only when the note changed, never per frame.
pub(crate) fn natural_width(ctx: &egui::Context, doc: &Document, t: Tokens) -> f32 {
    let lead = tag_lead(doc);
    let hidden = doc.hidden_blocks();
    let numbers = crate::model::list_numbers(&doc.blocks);
    doc.blocks
        .iter()
        .zip(hidden)
        .zip(numbers)
        .filter(|((block, hidden), _)| {
            !hidden && (block.kind.is_textual() || matches!(block.kind, BlockKind::Equation { .. }))
        })
        .map(|((block, _), number)| {
            // An equation is as wide as its formula is drawn, so it grows the note as it is
            // typed, as the desktop notebook's container grows round one.
            if let BlockKind::Equation { source, scale } = &block.kind {
                let formula = math::layout_source(ctx, source, *scale).size.x;
                return lead
                    + f32::from(block.indent.min(MAX_LEVEL)) * LEVEL_INDENT
                    + (formula + 8.0).max(48.0);
            }
            let job = layout_job(
                &block.text.text,
                &block.text.runs,
                &block.kind,
                block.checked(),
                matches!(block.kind, BlockKind::Quote),
                TagLook::default(),
                Align::LEFT,
                f32::INFINITY,
                t,
                &[],
            );
            let text = ctx.fonts_mut(|fonts| fonts.layout_job(job)).size().x;
            lead + f32::from(block.indent.min(MAX_LEVEL)) * LEVEL_INDENT
                + marker_width(ctx, block, number, t)
                + text
        })
        .fold(0.0, f32::max)
}

/// The space between a text marker's right edge and the item's text.
const MARKER_GAP: f32 = 4.0;

/// A text marker ("1.", "(iv)", "–"…) laid out as the item's own text is: in its font and
/// size (its first character's), in the ink, on the page's rule pitch.
fn marker_job(block: &Block, text: String, pitch: Option<f32>, t: Tokens) -> LayoutJob {
    let first = style_at_runs(&block.text.runs, 0);
    let style = InlineStyle {
        font: first.font,
        size: first.size,
        ..InlineStyle::default()
    };
    let mut format = text_format(&style, &block.kind, false, false, None, t);
    format.color = t.ink;
    let mut job = LayoutJob::single_section(text, format);
    if let Some(pitch) = pitch {
        snap_lines(&mut job, pitch);
    }
    job
}

/// The room a list item's bullet or number takes before its text: the usual slot, wider
/// when a number such as "(12)" or "viii." needs it.
fn marker_width(ctx: &egui::Context, block: &Block, number: usize, t: Tokens) -> f32 {
    match block.list_marker() {
        None => 0.0,
        Some(ListMarker::Disc | ListMarker::Circle | ListMarker::Square) => MARKER_WIDTH,
        Some(marker) => {
            let job = marker_job(block, String::new(), None, t);
            let font = job.sections[0].format.font_id.clone();
            let width = widgets::text_width(ctx, &marker.text(number), font);
            MARKER_WIDTH.max((width + MARKER_GAP).ceil())
        }
    }
}

/// How far below its galley's top a galley's first line has its baseline (`None` when the
/// line is empty).
fn first_baseline(galley: &egui::Galley) -> Option<f32> {
    let row = galley.rows.first()?;
    row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y)
}

/// Paints a paragraph's To Do box or list marker on its first line. Bullets and numbers
/// change with the level, as in the desktop notebook (• ◦ ▪ and 1. a. i.), unless the item has a marker
/// of its own. The dot, ring and square are shapes, centred on the line; other markers are
/// text, right-aligned in the slot on the text's baseline (`baseline`, when it has one).
fn paint_gutter_glyph(
    painter: &egui::Painter,
    rect: Rect,
    block: &Block,
    number: usize,
    baseline: Option<f32>,
    pitch: Option<f32>,
    t: Tokens,
) {
    let center = rect.center();
    if let Some(marker) = block.list_marker() {
        match marker {
            ListMarker::Disc => {
                painter.circle_filled(center, BULLET_RADIUS, t.ink);
            }
            ListMarker::Circle => {
                painter.circle_stroke(center, BULLET_RADIUS, Stroke::new(1.0, t.ink));
            }
            ListMarker::Square => {
                painter.rect_filled(
                    Rect::from_center_size(center, vec2(BULLET_RADIUS * 1.8, BULLET_RADIUS * 1.8)),
                    CornerRadius::ZERO,
                    t.ink,
                );
            }
            marker => {
                let galley = painter.layout_job(marker_job(block, marker.text(number), pitch, t));
                let top = match (baseline, first_baseline(&galley)) {
                    (Some(text), Some(own)) => text - own,
                    _ => rect.top(),
                };
                let left = rect.right() - MARKER_GAP - galley.size().x;
                painter.galley(pos2(left, top), galley, t.ink);
            }
        }
        return;
    }
    if let BlockKind::Todo { checked } = block.kind {
        paint_todo_box(painter, center, checked, t);
    }
}

/// To Do's box, ticked or not. Custom tags with a box use it too.
fn paint_todo_box(painter: &egui::Painter, center: Pos2, checked: bool, t: Tokens) {
    let rect = Rect::from_center_size(center, vec2(CHECKBOX_SIZE, CHECKBOX_SIZE));
    painter.rect_filled(
        rect,
        CornerRadius::same(2),
        if checked { t.accent } else { t.raised },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, if checked { t.accent } else { t.muted }),
        StrokeKind::Inside,
    );
    if checked {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            icons::CHECK,
            theme::icon(9.0),
            theme::on_fill(t.accent),
        );
    }
}

/// The chars of `new` an edit of `old` inserted, ending at the caret when there is one
/// (egui leaves it after what was typed or pasted). Empty for a deletion.
fn inserted_chars(old: &str, new: &str, caret: Option<usize>) -> Range<usize> {
    let (old_len, new_len) = (old.chars().count(), new.chars().count());
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old
        .chars()
        .rev()
        .zip(new.chars().rev())
        .take(old_len.min(new_len) - prefix)
        .take_while(|(a, b)| a == b)
        .count();
    let len = new_len - prefix - suffix;
    match caret {
        Some(caret) if caret >= len && caret <= new_len => caret - len..caret,
        _ => prefix..prefix + len,
    }
}

/// Links the web addresses an edit completed: each one a space or line break was typed
/// (or pasted) after, and, for a paste, the address it ends with.
fn autolink_inserted(text: &mut StyledText, inserted: Range<usize>, pasted: bool) {
    if inserted.is_empty() {
        return;
    }
    let chars: Vec<char> = text.text.chars().collect();
    for at in inserted.clone() {
        if chars.get(at).is_some_and(|ch| ch.is_whitespace()) {
            text.autolink_word_before(at);
        }
    }
    let ends_word = chars.get(inserted.end).is_none_or(|ch| ch.is_whitespace());
    if pasted && ends_word {
        text.autolink_word_before(inserted.end);
    }
}

/// Shows a block's text. Returns whether it changed, and where (x) its first line starts.
fn show_text_edit(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    editor: &mut EditorState,
    t: Tokens,
    code: bool,
    quote: bool,
) -> (bool, f32) {
    let block_id_value = doc.blocks[index].id;
    let id = block_id(doc_id, block_id_value);
    let old_text = doc.blocks[index].text.text.clone();
    let mut buffer = old_text.clone();
    let kind = doc.blocks[index].kind.clone();
    let runs = doc.blocks[index].text.runs.clone();
    let checked = doc.blocks[index].checked();
    let look = TagLook::of(&doc.blocks[index]);
    let (halign, text_align) = match doc.blocks[index].align {
        ParaAlign::Left => (Align::LEFT, Align2::LEFT_TOP),
        ParaAlign::Center => (Align::Center, Align2::CENTER_TOP),
        ParaAlign::Right => (Align::RIGHT, Align2::RIGHT_TOP),
    };
    let insertion_style = editor
        .pending_style
        .clone()
        .unwrap_or_else(|| editor.active_style.clone());
    let marks = editor.find_marks.get(&id).cloned().unwrap_or_default();
    let pitch = editor.line_pitch;
    let least = points_to_px(doc.blocks[index].spacing.line_at_least);
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = layout_job(
            text.as_str(),
            &runs,
            &kind,
            checked,
            quote,
            look,
            halign,
            wrap_width,
            t,
            &marks,
        );
        lines_at_least(&mut job, least);
        if let Some(pitch) = pitch {
            snap_lines(&mut job, pitch);
        }
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let font = if code {
        theme::mono(BODY_SIZE)
    } else {
        theme::body(BODY_SIZE)
    };
    let prior = cursor_range(ui.ctx(), id);
    let response = TextEdit::multiline(&mut buffer)
        .id(id)
        .lock_focus(true)
        .frame(egui::Frame::NONE)
        .margin(Margin::ZERO)
        .desired_width(f32::INFINITY)
        .desired_rows(1)
        .align(text_align)
        .font(font)
        .layouter(&mut layouter)
        .show(ui);
    // the desktop notebook's notes show no placeholder text; a screen reader still hears the
    // paragraph's style (Normal, Heading 1…).
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_label(kind.label().into_owned());
    });
    let pointer = local_pointer(ui).filter(|pos| response.response.rect.contains(*pos));
    // egui paints the galley at `galley_pos - galley.rect.left()`; that left edge is
    // negative in centred and right-aligned paragraphs.
    let galley_origin = response.galley_pos - vec2(response.galley.rect.left(), 0.0);
    editor.galley_hint = Some((response.galley.clone(), galley_origin));
    // Where the first line starts (the tags and list marker of an aligned paragraph go
    // just left of it; an empty paragraph's line starts where its caret is).
    let first_line = galley_origin.x + response.galley.rows.first().map_or(0.0, |row| row.pos.x);
    let pointer_char =
        pointer.and_then(|pos| char_under(&response.galley, pos - galley_origin.to_vec2()));
    // egui moves the caret on any press; a right-click inside the selection keeps it, as
    // in every word processor, so the menu acts on what was selected.
    let mut cursor = response.cursor_range;
    if pointer.is_some()
        && ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Secondary))
    {
        if let (Some((min, max)), Some(at)) = (prior, cursor.map(|range| range.primary.index.0)) {
            if min < max && (min..=max).contains(&at) {
                let kept = CCursorRange::two(CCursor::new(min), CCursor::new(max));
                let mut state = response.state.clone();
                state.cursor.set_char_range(Some(kept));
                state.store(ui.ctx(), id);
                cursor = Some(kept);
            }
        }
    }
    // Remember which misspelled word a right-click landed on, for the menu's suggestions.
    if response.response.secondary_clicked() {
        editor.spell_target = None;
        if let (Some(at), false) = (pointer_char, code) {
            let text = &doc.blocks[index].text;
            let ranges = editor.spell.misspelled(id, text);
            if let Some(range) = ranges.iter().find(|range| range.contains(&at)) {
                editor.spell_target = Some(SpellTarget {
                    block: id,
                    range: range.clone(),
                    word: text
                        .text
                        .chars()
                        .skip(range.start)
                        .take(range.len())
                        .collect(),
                    language: editor.spell.language_near(&text.text, range.clone()),
                });
            }
        }
    }
    let mut changed = false;
    if buffer != old_text {
        let caret = response
            .cursor_range
            .map(|range| range.primary.index.0.max(range.secondary.index.0));
        doc.blocks[index]
            .text
            .sync_after_edit(&buffer, insertion_style, caret);
        // A typed web address becomes a link once a space follows it, and a pasted one at
        // once, as in the desktop notebook.
        if !matches!(doc.blocks[index].kind, BlockKind::Code) {
            let pasted = ui.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Paste(_)))
            });
            let inserted = inserted_chars(&old_text, &buffer, caret);
            autolink_inserted(&mut doc.blocks[index].text, inserted, pasted);
        }
        editor.pending_style = None;
        editor.active_block = Some(block_id_value);
        editor.title_focused = false;
        doc.blocks[index].text.normalize();
        changed = true;
        autoformat_list(doc, index, editor, caret);
    }
    if response.response.has_focus() {
        editor.text_widget = Some(id);
        if editor.active_block != Some(block_id_value) {
            editor.pending_style = None;
        }
        editor.active_block = Some(block_id_value);
        // A paragraph that takes the caret leaves the table.
        editor.active_cell = None;
        editor.title_focused = false;
        if let Some(range) = cursor {
            let a = range.primary.index.0;
            let b = range.secondary.index.0;
            let min = a.min(b);
            let max = a.max(b);
            if min == max {
                editor.selection = None;
                if editor.pending_style.is_some() {
                    match editor.pending_at {
                        None => editor.pending_at = Some((block_id_value, max)),
                        // The caret moved since the style was armed: the desktop notebook drops it.
                        Some(at) if at != (block_id_value, max) => editor.pending_style = None,
                        Some(_) => {}
                    }
                }
                if editor.pending_style.is_none() {
                    editor.active_style = doc.blocks[index].text.style_for_caret(max);
                }
            } else {
                editor.selection = Some(min..max);
                editor.pending_style = None;
            }
        }
    }
    // The Format Painter paints the text selected (or the word clicked) once the mouse
    // button comes up over it.
    if editor.painter.is_some() && response.response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    }
    if editor.painter.is_some()
        && response.response.has_focus()
        && painter_release(ui, &response.response.response)
    {
        if let Some(range) = response.cursor_range {
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            changed |= paint_format(ui.ctx(), doc, editor, a.min(b)..a.max(b));
        }
    }
    // As in the desktop notebook, the word being typed is only flagged once it is finished: the edit
    // is remembered until the caret moves away from where it left it.
    let caret = cursor
        .filter(|range| response.response.has_focus() && range.primary == range.secondary)
        .map(|range| range.primary.index.0);
    if changed {
        editor.spell_edit = caret.map(|at| (id, at));
    } else if editor
        .spell_edit
        .is_some_and(|(block, at)| block == id && caret != Some(at))
    {
        editor.spell_edit = None;
    }
    // TextEdit lays the galley out again after an edit, so it matches the text unless
    // an autoformat rewrote it (then the next frame paints).
    if !code && doc.blocks[index].text.text == buffer {
        let ranges = editor.spell.misspelled(id, &doc.blocks[index].text);
        let typing = editor
            .spell_edit
            .filter(|(block, _)| *block == id)
            .map(|(_, at)| at);
        let painter = ui.painter();
        for (left, right, y) in squiggle_segments(&response.galley, galley_origin, &ranges, typing)
        {
            wavy_line(painter, left, right, y, t.danger);
        }
    }
    if response.response.hovered() {
        editor.hovered_block = Some(block_id_value);
    }

    // Links: hover shows the address; Ctrl+click (⌘+click) opens it.
    let link = pointer_char
        .and_then(|at| doc.blocks[index].text.link_at(at))
        .map(|(_, url)| url);
    if let Some(url) = link {
        let command_held = ui.input(|input| input.modifiers.command);
        if command_held {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if command_held && response.response.clicked() {
            editor.link_request = Some(url.to_string());
        } else {
            let _ = response
                .response
                .response
                .clone()
                .on_hover_text_at_pointer(link_tooltip(&url));
        }
    }

    // The right-click menu.
    if response.response.secondary_clicked() {
        let (min, max) = cursor.map_or((0, 0), |range| {
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            (a.min(b), a.max(b))
        });
        // The note lives in the zoomed page; the menu opens at the screen point.
        let at = response
            .response
            .interact_pointer_pos()
            .unwrap_or(response.response.rect.left_bottom());
        let at = ui
            .ctx()
            .layer_transform_to_global(ui.layer_id())
            .map_or(at, |transform| transform * at);
        editor.menu_target = Some(TextMenuTarget {
            block: block_id_value,
            range: min..max,
            pointer_char,
            at,
        });
    }
    let target = editor
        .menu_target
        .clone()
        .filter(|target| target.block == block_id_value);
    if let Some(target) = target {
        let mut command = None;
        // the desktop notebook's Mini Toolbar floats above the menu: the popup has no frame of its own,
        // and draws the toolbar's card above the pointer and the menu's card at it.
        let menu = egui::Popup::context_menu(&response.response)
            .frame(egui::Frame::NONE)
            .at_position(target.at - vec2(0.0, MINI_CARD_HEIGHT + MINI_CARD_GAP))
            .show(|ui| {
                command = text_context_menu(ui, doc, index, editor, &target, t);
            });
        if let Some(command) = command {
            changed |= apply_text_menu_command(ui.ctx(), doc, doc_id, editor, &target, command);
        } else if menu.is_some() && ui.input(|input| input.key_pressed(Key::Escape)) {
            // Escape closed the menu (and egui dropped the text focus): back to typing,
            // with the selection, as after a command.
            set_focus_range(ui.ctx(), id, target.range);
        }
    }
    (changed, first_line)
}

/// What a cell did this frame.
#[derive(Default)]
struct CellOutput {
    changed: bool,
    /// A command picked from the cell's right-click menu, run once the whole table is
    /// drawn (it may change the grid).
    command: Option<crate::table::TableCmd>,
}

/// One table cell: a text edit like a paragraph's, without its marker, tags, hint or
/// spelling squiggles. `width` is the room for the text inside the cell's padding.
#[allow(clippy::too_many_arguments)]
fn show_cell(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    (row, col): (usize, usize),
    width: f32,
    editor: &mut EditorState,
    t: Tokens,
) -> CellOutput {
    let mut output = CellOutput::default();
    let block = doc.blocks[index].id;
    let id = table_cell_id(doc_id, block, row, col);
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return output;
    };
    let Some(cell) = table.rows.get(row).and_then(|cells| cells.get(col)) else {
        return output;
    };
    let old_text = cell.text.text.clone();
    let runs = cell.text.runs.clone();
    let (halign, text_align) = match cell.align {
        ParaAlign::Left => (Align::LEFT, Align2::LEFT_TOP),
        ParaAlign::Center => (Align::Center, Align2::CENTER_TOP),
        ParaAlign::Right => (Align::RIGHT, Align2::RIGHT_TOP),
    };
    let insertion_style = editor
        .pending_style
        .clone()
        .unwrap_or_else(|| editor.active_style.clone());
    let marks = editor.find_marks.get(&id).cloned().unwrap_or_default();
    let kind = BlockKind::Paragraph;
    let text_t = cell_tokens(t, cell.shading);
    if text_t.ink != t.ink {
        // The caret too, which would be as light as the text.
        ui.visuals_mut().text_cursor.stroke.color = text_t.ink;
    }
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = layout_job(
            text.as_str(),
            &runs,
            &kind,
            false,
            false,
            TagLook::default(),
            halign,
            wrap_width,
            text_t,
            &marks,
        );
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut buffer = old_text.clone();
    let response = TextEdit::multiline(&mut buffer)
        .id(id)
        .lock_focus(true)
        .frame(egui::Frame::NONE)
        .margin(Margin::ZERO)
        .desired_width(width)
        .desired_rows(1)
        .align(text_align)
        // The note text's font, exactly as `show_text_edit` sets it for a paragraph.
        .font(theme::body(BODY_SIZE))
        .layouter(&mut layouter)
        .show(ui);
    if buffer != old_text {
        let caret = response
            .cursor_range
            .map(|range| range.primary.index.0.max(range.secondary.index.0));
        let pasted = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)))
        });
        if let BlockKind::Table(table) = &mut doc.blocks[index].kind {
            let text = &mut table.rows[row][col].text;
            text.sync_after_edit(&buffer, insertion_style, caret);
            autolink_inserted(text, inserted_chars(&old_text, &buffer, caret), pasted);
            text.normalize();
        }
        editor.pending_style = None;
        output.changed = true;
    }
    if response.response.has_focus() {
        editor.text_widget = Some(id);
        if editor.active_block != Some(block) {
            // Coming into the table from elsewhere: an old cell selection is stale.
            editor.cell_selection = None;
        }
        if editor.active_block != Some(block) || editor.active_cell != Some((row, col)) {
            editor.pending_style = None;
        }
        editor.active_block = Some(block);
        editor.active_cell = Some((row, col));
        editor.title_focused = false;
        if let Some(range) = response.cursor_range {
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            let (min, max) = (a.min(b), a.max(b));
            if min == max {
                editor.selection = None;
                if editor.pending_style.is_none() {
                    if let Some(text) = editor.active_text(doc) {
                        editor.active_style = text.style_for_caret(max);
                    }
                }
            } else {
                editor.selection = Some(min..max);
                editor.pending_style = None;
            }
        }
    }
    // The Format Painter, as in a paragraph.
    if editor.painter.is_some() && response.response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    }
    if editor.painter.is_some()
        && response.response.has_focus()
        && painter_release(ui, &response.response.response)
    {
        if let Some(range) = response.cursor_range {
            let (a, b) = (range.primary.index.0, range.secondary.index.0);
            output.changed |= paint_format(ui.ctx(), doc, editor, a.min(b)..a.max(b));
        }
    }
    // Links: hover shows the address; Ctrl+click opens it, as in a paragraph.
    let pointer = local_pointer(ui).filter(|pos| response.response.rect.contains(*pos));
    let origin = response.galley_pos - vec2(response.galley.rect.left(), 0.0);
    let link = pointer
        .and_then(|pos| char_under(&response.galley, pos - origin.to_vec2()))
        .and_then(|at| match &doc.blocks[index].kind {
            BlockKind::Table(table) => table.rows[row][col].text.link_at(at).map(|(_, url)| url),
            _ => None,
        });
    if let Some(url) = link {
        let command_held = ui.input(|input| input.modifiers.command);
        if command_held {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if command_held && response.response.clicked() {
            editor.link_request = Some(url.to_string());
        }
    }

    // The right-click menu: the clipboard, then the desktop notebook's Table submenu. Cells selected
    // across this table stay selected for the command picked.
    if response.response.secondary_clicked() {
        editor.cell_selection = editor.cell_selection.filter(|(owner, _)| *owner == block);
    }
    let mut picked = None;
    let menu = response.response.context_menu(|ui| {
        picked = cell_menu(ui, editor, &doc.blocks[index], t);
    });
    // Where the caret was, for the paste and for typing on after the menu.
    let caret = || cursor_range(ui.ctx(), id).map_or(0..0, |(min, max)| min..max);
    match picked {
        Some(CellMenu::Table(command)) => output.command = Some(command),
        Some(CellMenu::Copy { cut }) => {
            if copy_cell_text(ui.ctx(), doc, doc_id, index, (row, col), editor, cut) {
                output.changed = true;
            } else {
                // Back to typing in the cell, the caret where it was.
                set_focus_range(ui.ctx(), id, caret());
            }
        }
        Some(CellMenu::Paste) => {
            // egui can't read the clipboard on demand; the platform answers this with a
            // paste event for the focused cell next frame.
            set_focus_range(ui.ctx(), id, caret());
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        None => {
            if menu.is_some() && ui.input(|input| input.key_pressed(Key::Escape)) {
                // Escape closed the menu (and egui dropped the text focus): back to
                // typing in the cell, as after a command.
                set_focus_range(ui.ctx(), id, caret());
            }
        }
    }
    output
}

/// The tokens a cell's text is laid out with. The shades are light, so in dark mode a
/// shaded cell keeps dark ink, as highlighted text does: light ink would vanish on it.
fn cell_tokens(t: Tokens, shading: Option<Rgb>) -> Tokens {
    if t.dark && shading.is_some() {
        Tokens {
            ink: Rgb::INKS[0].to_color32(),
            ..t
        }
    } else {
        t
    }
}

/// A column's width: the one its border was dragged to, or what its widest line needs
/// (the desktop notebook widens a column as you type), within TABLE_MIN_COLUMN..=TABLE_AUTO_MAX.
fn column_width(ui: &Ui, table: &TableBlock, col: usize, t: Tokens) -> f32 {
    let set = table.widths.get(col).copied().unwrap_or(0.0);
    if set > 0.0 {
        return set;
    }
    let widest = table
        .rows
        .iter()
        .filter_map(|row| row.get(col))
        .map(|cell| {
            let job = layout_job(
                &cell.text.text,
                &cell.text.runs,
                &BlockKind::Paragraph,
                false,
                false,
                TagLook::default(),
                Align::LEFT,
                f32::INFINITY,
                t,
                &[],
            );
            ui.fonts_mut(|fonts| fonts.layout_job(job)).size().x
        })
        .fold(0.0, f32::max);
    (widest + 2.0 * CELL_PAD.x + 2.0).clamp(TABLE_MIN_COLUMN, TABLE_AUTO_MAX)
}

/// A table in a note: rows of cells, each its own text edit, with borders, shading, a
/// drag selection across cells, and column borders that drag to set a width.
fn show_table(
    ui: &mut Ui,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    editor: &mut EditorState,
    t: Tokens,
    actions: &mut Vec<BlockAction>,
) -> bool {
    let block = doc.blocks[index].id;
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return false;
    };
    let rows = table.rows.len();
    let cols = table.cols();
    let header = table.header;
    let hide_borders = table.hide_borders;
    let shading: Vec<Vec<Option<Rgb>>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(|cell| cell.shading).collect())
        .collect();
    let widths: Vec<f32> = (0..cols)
        .map(|col| column_width(ui, table, col, t))
        .collect();
    let in_table = editor.active_block == Some(block) && editor.active_cell.is_some();
    let selected = editor
        .cell_selection
        .filter(|(owner, range)| *owner == block && !range.is_single())
        .map(|(_, range)| range);
    let origin = ui.cursor().min;
    let total: f32 = widths.iter().sum();
    let mut changed = false;
    let mut command = None;
    let mut cells = vec![vec![Rect::NOTHING; cols]; rows];
    let mut y = origin.y;
    for row in 0..rows {
        // Shading goes behind the text, once the row's height is known.
        let backdrop = ui.painter().add(egui::Shape::Noop);
        let mut height: f32 = 0.0;
        let mut x = origin.x;
        for (col, width) in widths.iter().enumerate() {
            let inner = Rect::from_min_size(
                pos2(x, y) + CELL_PAD,
                vec2((width - 2.0 * CELL_PAD.x).max(8.0), f32::INFINITY),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::top_down(Align::Min)),
            );
            let cell = show_cell(
                &mut child,
                doc,
                doc_id,
                index,
                (row, col),
                inner.width(),
                editor,
                t,
            );
            changed |= cell.changed;
            command = command.or(cell.command);
            height = height.max(child.min_rect().height());
            x += width;
        }
        let row_height = height + 2.0 * CELL_PAD.y;
        let mut fills = Vec::new();
        let mut x = origin.x;
        for (col, width) in widths.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(x, y), vec2(*width, row_height));
            cells[row][col] = rect;
            let fill = shading
                .get(row)
                .and_then(|cells| cells.get(col))
                .copied()
                .flatten()
                .map(Rgb::to_color32)
                .or((header && row == 0).then_some(t.sunken));
            if let Some(fill) = fill {
                fills.push(egui::Shape::rect_filled(rect, CornerRadius::ZERO, fill));
            }
            if selected.is_some_and(|range| range.contains(row, col)) {
                fills.push(egui::Shape::rect_filled(
                    rect,
                    CornerRadius::ZERO,
                    theme::with_alpha(t.selection, 170),
                ));
            }
            x += width;
        }
        ui.painter().set(backdrop, egui::Shape::Vec(fills));
        y += row_height;
    }
    let table_rect = Rect::from_min_max(origin, pos2(origin.x + total, y));
    ui.allocate_rect(table_rect, Sense::hover());
    // Typing widened (or narrowed) a column that fits its text: show it at its new width
    // at once instead of wrapped for a frame, as a note that widens as you type does.
    if changed {
        if let BlockKind::Table(table) = &doc.blocks[index].kind {
            let moved =
                (0..cols).any(|col| (column_width(ui, table, col, t) - widths[col]).abs() >= 0.5);
            if moved {
                ui.ctx().request_discard("a table column fitted its text");
            }
        }
    }

    // Borders: thin grey lines, as the desktop notebook's; hidden ones show dashed while typing in it.
    let solid = Stroke::new(1.0, t.container_border);
    let dashed = Stroke::new(1.0, t.faint);
    let mut lines: Vec<[Pos2; 2]> = Vec::new();
    let mut x = origin.x;
    lines.push([pos2(x, origin.y), pos2(x, y)]);
    for width in &widths {
        x += width;
        lines.push([pos2(x, origin.y), pos2(x, y)]);
    }
    lines.push([pos2(origin.x, origin.y), pos2(origin.x + total, origin.y)]);
    for row in &cells {
        let bottom = row[0].bottom();
        lines.push([pos2(origin.x, bottom), pos2(origin.x + total, bottom)]);
    }
    for line in lines {
        if !hide_borders {
            ui.painter().line_segment(line, solid);
        } else if in_table {
            ui.painter()
                .extend(egui::Shape::dashed_line(&line, dashed, 3.0, 3.0));
        }
    }

    // A column's right border drags to set its width (one undo step per drag).
    let now = ui.input(|input| input.time);
    // A border being dragged is not a drag across cells.
    let mut resizing = false;
    let mut x = origin.x;
    for (col, width) in widths.iter().enumerate() {
        x += width;
        let handle = Rect::from_x_y_ranges(x - 3.0..=x + 3.0, table_rect.y_range());
        let response = ui.interact(
            handle,
            ui.make_persistent_id(("table-column", doc_id, block, col)),
            Sense::drag(),
        );
        resizing |= response.is_pointer_button_down_on() || response.dragged();
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
        if response.drag_started() {
            if editor.history_owner.as_deref() == Some(doc_id) {
                editor.push_undo(doc, now);
            } else {
                // Not the note being typed in: its step must not land in that note's
                // history (Ctrl+Z there would bring this note's text in). The page's
                // history takes it.
                editor.page_step = Some((doc_id.to_owned(), doc.clone()));
            }
            // The press took the keyboard from the cell: give it back, so the caret stays
            // where it was and Ctrl+Z takes the new width back, as in the desktop notebook.
            editor.restore_focus();
        }
        if response.dragged() {
            if let BlockKind::Table(table) = &mut doc.blocks[index].kind {
                if let Some(set) = table.widths.get_mut(col) {
                    let current = if *set > 0.0 { *set } else { *width };
                    *set = (current + response.drag_delta().x).clamp(TABLE_MIN_COLUMN, 2400.0);
                    changed = true;
                }
            }
        }
    }

    // Dragging from one cell into another selects the cells between; a new press in the
    // table clears it.
    let to_local = |pos: Pos2| {
        ui.ctx()
            .layer_transform_from_global(ui.layer_id())
            .map_or(pos, |transform| transform * pos)
    };
    let (pressed, down, from, now_at) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.press_origin(),
            input.pointer.interact_pos(),
        )
    });
    // A press in the table starts over; presses elsewhere (the ribbon's Select and
    // Delete buttons) keep the selection for the command they give. So does a press on a
    // menu open over the table (the cell menu's Table rows, a ribbon dropdown).
    let over_menu = egui::Popup::is_any_open(ui.ctx());
    let pressed_here = pressed
        && !over_menu
        && from
            .map(to_local)
            .is_some_and(|pos| table_rect.contains(pos));
    if pressed_here
        && editor
            .cell_selection
            .is_some_and(|(owner, _)| owner == block)
    {
        editor.cell_selection = None;
    }
    let hit = |pos: Pos2| {
        (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (row, col)))
            .find(|&(row, col)| cells[row][col].contains(pos))
    };
    if down && !resizing && !over_menu {
        if let (Some(from), Some(at)) = (
            from.map(to_local).and_then(hit),
            now_at.map(to_local).and_then(hit),
        ) {
            if from != at {
                editor.cell_selection = Some((block, crate::table::CellRange::spanning(from, at)));
            } else if editor
                .cell_selection
                .is_some_and(|(owner, _)| owner == block)
            {
                // Back in the cell it began in: the cell's own text selection says it all.
                editor.cell_selection = None;
            }
        }
    }

    // The cell menu's command runs once the whole note is drawn: it may delete the table.
    if let Some(command) = command {
        actions.push(BlockAction::TableCommand(command));
    }
    changed
}

/// The length in chars of cell (`row`, `col`) of the table at `index`; 0 when there is
/// no such cell.
fn cell_chars(doc: &Document, index: usize, row: usize, col: usize) -> usize {
    match &doc.blocks[index].kind {
        BlockKind::Table(table) => table
            .rows
            .get(row)
            .and_then(|cells| cells.get(col))
            .map_or(0, |cell| cell.text.char_count()),
        _ => 0,
    }
}

/// Keys in a table cell, as in the desktop notebook (see the design notes in P6): Tab and Shift+Tab
/// move along the cells and Tab in the last one adds a row (a column in a one-row table),
/// Enter in the last cell adds a row and Enter in an empty last row leaves the table,
/// Enter at a row's start adds a row above, Ctrl+Enter adds a row below, Ctrl+Alt+R and
/// Ctrl+Alt+E add a column, Up and Down at a cell's edge go to the next cell up or down
/// (and out of the table), Delete clears selected cells, and Delete twice at an empty
/// row's start removes it. True when the table changed.
fn table_keys(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    now: f64,
) -> bool {
    let (Some(block), Some((row, col))) = (editor.active_block, editor.active_cell) else {
        return false;
    };
    let Some(index) = doc.block_index(block) else {
        return false;
    };
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return false;
    };
    let (rows, cols) = (table.rows.len(), table.cols());
    let Some(cells) = table.rows.get(row).filter(|_| col < cols) else {
        return false;
    };
    let row_empty = cells.iter().all(|cell| cell.text.text.is_empty());
    let whole_row = CellRange::cell(row, 0).full_rows(table);
    let len = cell_chars(doc, index, row, col);
    let Some((min, max)) = cursor_range(ctx, table_cell_id(doc_id, block, row, col)) else {
        return false;
    };
    // Typing or an arrow key ends a selection of several cells. A typed character also
    // means Ctrl+Alt was AltGr (Windows), which types on many layouts (AltGr+E is €).
    let (typed, arrow) = ctx.input(|input| {
        let typed = input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(_)));
        let arrow = [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
        ]
        .into_iter()
        .any(|key| input.key_pressed(key));
        (typed, arrow)
    });
    if typed || arrow {
        editor.cell_selection = None;
    }
    let key =
        |modifiers: Modifiers, key: Key| ctx.input_mut(|input| input.consume_key(modifiers, key));
    // Up and Down without modifiers only: `consume_key` also takes Shift and Alt+Shift
    // presses, which select in the cell or (in a paragraph) move it.
    let plain = |key: Key| {
        ctx.input_mut(|input| {
            let before = input.events.len();
            input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::Key { key: pressed, pressed: true, modifiers, .. }
                        if *pressed == key && modifiers.is_none()
                )
            });
            input.events.len() != before
        })
    };
    let last_cell = row + 1 == rows && col + 1 == cols;
    let edit = |doc: &mut Document, f: &dyn Fn(&mut TableBlock)| {
        if let BlockKind::Table(table) = &mut doc.blocks[index].kind {
            f(table);
        }
    };

    // the desktop notebook's Delete, Delete at the start of an empty row: the first selects the row,
    // the second removes it (a table keeps at least one row).
    if row_empty && rows > 1 && col == 0 && min == max && min == 0 && plain(Key::Delete) {
        if editor.cell_selection == Some((block, whole_row)) {
            editor.push_undo(doc, now);
            edit(doc, &|table| {
                crate::table::delete_rows(table, row, row);
            });
            editor.cell_selection = None;
            editor.focus_cell(block, row.min(rows - 2), 0, 0..0);
            return true;
        }
        editor.cell_selection = Some((block, whole_row));
        return false;
    }

    // Selected cells: Copy and Cut take them as a table, Delete and Backspace clear them.
    if let Some((_, range)) = editor
        .cell_selection
        .filter(|(owner, range)| *owner == block && !range.is_single())
    {
        let (copy, cut) = ctx.input(|input| {
            (
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy)),
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Cut)),
            )
        });
        if copy || cut {
            // Taken out of the queue: the cell's text edit would copy its own text.
            ctx.input_mut(|input| {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Copy | egui::Event::Cut))
            });
            return copy_cells(ctx, doc, index, range, editor, cut, now);
        }
        if key(Modifiers::NONE, Key::Delete) || key(Modifiers::NONE, Key::Backspace) {
            editor.push_undo(doc, now);
            edit(doc, &|table| crate::table::clear(table, range));
            editor.cell_selection = None;
            return true;
        }
    }
    // Shift+Tab first: consume_key(NONE, Tab) also matches Shift+Tab.
    if key(Modifiers::SHIFT, Key::Tab) {
        let flat = row * cols + col;
        if flat > 0 {
            let (r, c) = ((flat - 1) / cols, (flat - 1) % cols);
            editor.focus_cell(block, r, c, 0..cell_chars(doc, index, r, c));
        }
        return false;
    }
    if key(Modifiers::NONE, Key::Tab) {
        if last_cell {
            editor.push_undo(doc, now);
            // A one-row table grows a column, as the desktop notebook's does; a taller one a row.
            if rows == 1 {
                edit(doc, &|table| crate::table::insert_column(table, cols));
                editor.focus_cell(block, 0, cols, 0..0);
            } else {
                edit(doc, &|table| crate::table::insert_rows(table, rows, 1));
                editor.focus_cell(block, rows, 0, 0..0);
            }
            return true;
        }
        let flat = row * cols + col + 1;
        let (r, c) = (flat / cols, flat % cols);
        editor.focus_cell(block, r, c, 0..cell_chars(doc, index, r, c));
        return false;
    }
    if key(Modifiers::COMMAND, Key::Enter) {
        editor.push_undo(doc, now);
        edit(doc, &|table| crate::table::insert_rows(table, row + 1, 1));
        editor.focus_cell(block, row + 1, col, 0..0);
        return true;
    }
    if !typed && key(Modifiers::COMMAND | Modifiers::ALT, Key::R) {
        editor.push_undo(doc, now);
        edit(doc, &|table| crate::table::insert_column(table, col + 1));
        editor.focus_cell(block, row, col + 1, 0..0);
        return true;
    }
    if !typed && key(Modifiers::COMMAND | Modifiers::ALT, Key::E) {
        editor.push_undo(doc, now);
        edit(doc, &|table| crate::table::insert_column(table, col));
        editor.focus_cell(block, row, col, 0..0);
        return true;
    }
    // Plain Enter only: Alt+Enter (the desktop notebook's paragraph in a cell) and Shift+Enter reach the
    // cell's text edit as a line break, even at the end of the last cell.
    if row + 1 == rows && min == max {
        if row_empty && rows > 1 && plain(Key::Enter) {
            // Enter again in the empty row it added: leave the table, as the desktop notebook does, onto
            // the empty line under it (Insert › Table leaves one), else a new one.
            editor.push_undo(doc, now);
            edit(doc, &|table| {
                crate::table::delete_rows(table, row, row);
            });
            let level = doc.blocks[index].indent;
            let empty_below = doc.blocks.get(index + 1).filter(|below| {
                below.kind.is_textual() && below.text.text.is_empty() && below.indent == level
            });
            let id = match empty_below {
                Some(below) => below.id,
                None => {
                    let mut paragraph = Block::paragraph("");
                    paragraph.indent = level;
                    let id = paragraph.id;
                    doc.blocks.insert(index + 1, paragraph);
                    id
                }
            };
            editor.focus_block(id, 0);
            return true;
        }
        if last_cell && max == len && !row_empty && plain(Key::Enter) {
            editor.push_undo(doc, now);
            edit(doc, &|table| crate::table::insert_rows(table, rows, 1));
            editor.focus_cell(block, rows, 0, 0..0);
            return true;
        }
    }
    // Enter at the start of a row below the first adds an empty row above it, as in
    // the desktop notebook; the caret goes into it.
    if row > 0 && col == 0 && min == max && min == 0 && plain(Key::Enter) {
        editor.push_undo(doc, now);
        edit(doc, &|table| crate::table::insert_rows(table, row, 1));
        editor.focus_cell(block, row, 0, 0..0);
        return true;
    }
    if min == max && min == 0 && plain(Key::ArrowUp) {
        if row > 0 {
            let end = cell_chars(doc, index, row - 1, col);
            editor.focus_cell(block, row - 1, col, end..end);
        } else {
            focus_beside(doc, editor, index, false);
        }
        return false;
    }
    if min == max && max == len && plain(Key::ArrowDown) {
        if row + 1 < rows {
            editor.focus_cell(block, row + 1, col, 0..0);
        } else {
            focus_beside(doc, editor, index, true);
        }
        return false;
    }
    false
}

/// Runs a Table tab or cell-menu command on the table holding the caret: over the cells
/// selected across the table, else the active cell. One undo step. True when the note
/// changed (the Select commands only select).
pub(crate) fn table_command(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    command: TableCmd,
) -> bool {
    let (Some(block), Some((row, col))) = (editor.active_block, editor.active_cell) else {
        return false;
    };
    let Some(index) = doc.block_index(block) else {
        return false;
    };
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return false;
    };
    let (row, col) = (
        row.min(table.rows.len().saturating_sub(1)),
        col.min(table.cols() - 1),
    );
    // Where the caret is in that cell: it stays there when the cell does.
    let caret_range = cursor_range(ctx, table_cell_id(doc_id, block, row, col));
    let range = editor
        .cell_selection
        .filter(|(owner, _)| *owner == block)
        .map_or(CellRange::cell(row, col), |(_, range)| range)
        .clamped(table);
    let now = ctx.input(|input| input.time);
    match command {
        TableCmd::SelectTable => {
            editor.cell_selection = Some((block, CellRange::whole(table)));
            return false;
        }
        TableCmd::SelectColumns => {
            editor.cell_selection = Some((block, range.full_columns(table)));
            return false;
        }
        TableCmd::SelectRows => {
            editor.cell_selection = Some((block, range.full_rows(table)));
            return false;
        }
        TableCmd::SelectCell => {
            let end = cell_chars(doc, index, row, col);
            editor.focus_cell(block, row, col, 0..end);
            return false;
        }
        _ => {}
    }
    editor.push_undo(doc, now);
    let BlockKind::Table(table) = &mut doc.blocks[index].kind else {
        return false;
    };
    // Where the caret goes afterwards: (row, col), or None when the table went.
    let mut caret = Some((row, col));
    match command {
        TableCmd::DeleteTable => caret = None,
        TableCmd::DeleteColumns => {
            if crate::table::delete_columns(table, range.left, range.right) {
                caret = Some((row, range.left.min(table.cols() - 1)));
            } else {
                caret = None;
            }
        }
        TableCmd::DeleteRows => {
            if crate::table::delete_rows(table, range.top, range.bottom) {
                caret = Some((range.top.min(table.rows.len() - 1), col));
            } else {
                caret = None;
            }
        }
        TableCmd::InsertAbove => {
            crate::table::insert_rows(table, range.top, 1);
            caret = Some((range.top, col));
        }
        TableCmd::InsertBelow => {
            crate::table::insert_rows(table, range.bottom + 1, 1);
            caret = Some((range.bottom + 1, col));
        }
        TableCmd::InsertLeft => {
            crate::table::insert_column(table, range.left);
            caret = Some((row, range.left));
        }
        TableCmd::InsertRight => {
            crate::table::insert_column(table, range.right + 1);
            caret = Some((row, range.right + 1));
        }
        TableCmd::HideBorders => table.hide_borders = !table.hide_borders,
        TableCmd::Shading(color) => crate::table::shade(table, range, color),
        TableCmd::Align(align) => crate::table::align(table, range, align),
        TableCmd::SortAscending | TableCmd::SortDescending => {
            crate::table::sort(table, col, command == TableCmd::SortDescending)
        }
        TableCmd::HeaderRow => table.header = !table.header,
        TableCmd::SelectTable
        | TableCmd::SelectColumns
        | TableCmd::SelectRows
        | TableCmd::SelectCell => {}
    }
    editor.cell_selection = None;
    match caret {
        Some((to_row, to_col)) => {
            // The same place in the same cell, clamped; at the end of any other cell.
            let end = cell_chars(doc, index, to_row, to_col);
            let range = caret_range
                .filter(|_| (to_row, to_col) == (row, col))
                .map_or(end..end, |(min, max)| min.min(end)..max.min(end));
            editor.focus_cell(block, to_row, to_col, range);
        }
        None => {
            // The table goes: the caret goes to the line before it (at its end), else to
            // the one after it (at its start), else to a new empty line in its place.
            let landed =
                focus_beside(doc, editor, index, false) || focus_beside(doc, editor, index, true);
            let indent = doc.blocks[index].indent;
            doc.blocks.remove(index);
            if !landed {
                let mut paragraph = Block::paragraph("");
                paragraph.indent = indent;
                let id = paragraph.id;
                doc.blocks.insert(index, paragraph);
                editor.focus_block(id, 0);
            }
            // That block has the caret now, so giving the keyboard back after the command
            // (`restore_focus`) keeps this request.
            editor.active_block = editor.pending_block_focus();
            editor.selection = None;
        }
    }
    true
}

/// What the Table tab shows for the table holding the caret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TableInfo {
    pub header: bool,
    pub hide_borders: bool,
    /// The active cell's alignment.
    pub align: ParaAlign,
}

/// `None` unless the caret is in a table cell of `doc`.
pub(crate) fn table_info(doc: &Document, editor: &EditorState) -> Option<TableInfo> {
    let index = editor.active_index(doc)?;
    let (row, col) = editor.active_cell?;
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return None;
    };
    Some(TableInfo {
        header: table.header,
        hide_borders: table.hide_borders,
        align: table.rows.get(row)?.get(col)?.align,
    })
}

/// the desktop notebook's "type, then Tab": plain paragraph `index`, with text and the caret at its
/// end, becomes a one-row table of two columns, its text (with its formatting) in the
/// first cell and the caret in the second. A table always has a line after it to keep
/// typing on, as `insert_table` leaves one. One undo step.
fn tab_into_table(doc: &mut Document, editor: &mut EditorState, index: usize, now: f64) -> bool {
    let block = &doc.blocks[index];
    if block.kind != BlockKind::Paragraph
        || block.text.text.is_empty()
        || block.text.text.contains('\n')
        || has_tags(block)
    {
        return false;
    }
    editor.push_undo(doc, now);
    let mut table = TableBlock::new(1, 2);
    table.rows[0][0].text = doc.blocks[index].text.clone();
    let block = &mut doc.blocks[index];
    block.kind = BlockKind::Table(table);
    block.text = StyledText::default();
    let (id, level) = (block.id, block.indent);
    if index + 1 == doc.blocks.len() {
        let mut paragraph = Block::paragraph("");
        paragraph.indent = level;
        doc.blocks.push(paragraph);
    }
    editor.focus_cell(id, 0, 1, 0..0);
    true
}

/// Puts an empty `rows` × `cols` table at the caret of note `doc_id`, as the desktop notebook does: in
/// place of an empty paragraph, else after the paragraph holding the caret, at its level.
/// The caret goes to the first cell. Returns the table's block id.
pub(crate) fn insert_table(
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    rows: usize,
    cols: usize,
    now: f64,
) -> u64 {
    // The note's own history takes the step, even in a note not drawn yet (a new one),
    // as every insert at the caret does.
    editor.start_history(doc_id);
    editor.push_undo(doc, now);
    let mut table = Block::new(BlockKind::Table(TableBlock::new(rows, cols)));
    let id = table.id;
    // A new note (`add_text`) has only asked for the caret in its first line so far.
    let at = editor.active_index(doc).or_else(|| {
        editor
            .pending_block_focus()
            .and_then(|block| doc.block_index(block))
    });
    // A table always has a line after it to keep typing on.
    let line_after = |doc: &mut Document, table_at: usize| {
        if table_at + 1 == doc.blocks.len() {
            let mut paragraph = Block::paragraph("");
            paragraph.indent = doc.blocks[table_at].indent;
            doc.blocks.push(paragraph);
        }
    };
    match at {
        Some(index)
            if doc.blocks[index].kind.is_textual()
                && doc.blocks[index].text.text.is_empty()
                && editor.active_cell.is_none() =>
        {
            table.indent = doc.blocks[index].indent;
            doc.blocks[index] = table;
            line_after(doc, index);
        }
        Some(index) => {
            table.indent = doc.blocks[index].indent;
            // After what a folded paragraph hides, as Enter puts its new paragraph.
            let next = if doc.blocks[index].collapsed && doc.has_children(index) {
                doc.family(index).end
            } else {
                index + 1
            };
            doc.blocks.insert(next, table);
            line_after(doc, next);
        }
        None => {
            doc.blocks.push(table);
            line_after(doc, doc.blocks.len() - 1);
        }
    }
    editor.focus_cell(id, 0, 0, 0..0);
    id
}

/// The cell menu's Copy and Cut: the selected chars (or the whole cell) to the clipboard,
/// with their formatting kept for a paste in Folio. True when Cut removed text.
fn copy_cell_text(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    index: usize,
    (row, col): (usize, usize),
    editor: &mut EditorState,
    cut: bool,
) -> bool {
    let block = doc.blocks[index].id;
    let now = ctx.input(|input| input.time);
    // The caret goes to this cell: its step is this note's, not the history of another
    // note being typed in (whose Ctrl+Z would bring this note's text in).
    if cut {
        editor.start_history(doc_id);
    }
    // Over cells selected across the table (the one clicked among them), the menu's Copy
    // and Cut take them all, as Ctrl+C and Ctrl+X do.
    if let Some((_, range)) = editor
        .cell_selection
        .filter(|(owner, range)| *owner == block && !range.is_single() && range.contains(row, col))
    {
        let changed = copy_cells(ctx, doc, index, range, editor, cut, now);
        if cut {
            editor.cell_selection = None;
            editor.focus_cell(block, row, col, 0..0);
        }
        return changed;
    }
    let BlockKind::Table(table) = &doc.blocks[index].kind else {
        return false;
    };
    let Some(text) = table.rows.get(row).and_then(|cells| cells.get(col)) else {
        return false;
    };
    let text = &text.text;
    let chars = text.char_count();
    // The selection is this cell's only while the caret is in it.
    let range = editor
        .selection
        .clone()
        .filter(|_| editor.active_block == Some(block) && editor.active_cell == Some((row, col)))
        .map(|range| range.start.min(chars)..range.end.min(chars))
        .filter(|range| !range.is_empty())
        .unwrap_or(0..chars);
    if range.is_empty() {
        // An empty cell: nothing to copy, and the clipboard keeps what it had.
        return false;
    }
    let piece = text.slice_chars(range.clone());
    ctx.copy_text(piece.text.clone());
    keep_inline_clip(editor, &BlockKind::Paragraph, piece);
    if !cut {
        return false;
    }
    editor.push_undo(doc, now);
    if let BlockKind::Table(table) = &mut doc.blocks[index].kind {
        table.rows[row][col].text.delete_chars(range.clone());
    }
    editor.selection = None;
    editor.focus_cell(block, row, col, range.start..range.start);
    true
}

/// A row picked from a table cell's right-click menu.
enum CellMenu {
    Copy { cut: bool },
    Paste,
    Table(TableCmd),
}

/// The right-click menu of a table cell: the clipboard, then the desktop notebook's Table submenu.
fn cell_menu(ui: &mut Ui, editor: &EditorState, block: &Block, t: Tokens) -> Option<CellMenu> {
    let BlockKind::Table(table) = &block.kind else {
        return None;
    };
    let _ = editor;
    let mut picked = None;
    ui.set_min_width(190.0);
    let item = |ui: &mut Ui, glyph: &str, label: &str| {
        widgets::menu_item(ui, Some(glyph), label, t).clicked()
    };
    if item(ui, icons::SCISSORS, &t!("common-cut")) {
        picked = Some(CellMenu::Copy { cut: true });
    }
    if item(ui, icons::COPY, &t!("common-copy")) {
        picked = Some(CellMenu::Copy { cut: false });
    }
    if item(ui, icons::CLIPBOARD_TEXT, &t!("common-paste")) {
        picked = Some(CellMenu::Paste);
    }
    ui.separator();
    ui.menu_button(submenu_label(icons::TABLE, &t!("table-menu"), t), |ui| {
        ui.set_min_width(190.0);
        let mut command = None;
        let mut rows = |ui: &mut Ui, rows: &[(&str, &str, TableCmd)]| {
            for (glyph, label, row_command) in rows {
                if item(ui, glyph, label) {
                    command = Some(*row_command);
                }
            }
        };
        rows(
            ui,
            &[
                (
                    icons::TABLE_MOVE_ABOVE,
                    &t!("table-insert-above"),
                    TableCmd::InsertAbove,
                ),
                (
                    icons::TABLE_MOVE_BELOW,
                    &t!("table-insert-below"),
                    TableCmd::InsertBelow,
                ),
                (
                    icons::TABLE_MOVE_LEFT,
                    &t!("table-insert-left"),
                    TableCmd::InsertLeft,
                ),
                (
                    icons::TABLE_MOVE_RIGHT,
                    &t!("table-insert-right"),
                    TableCmd::InsertRight,
                ),
            ],
        );
        ui.separator();
        rows(
            ui,
            &[
                (
                    icons::TABLE_DISMISS,
                    &t!("table-delete-table"),
                    TableCmd::DeleteTable,
                ),
                (
                    icons::TABLE_DELETE_COLUMN,
                    &t!("table-delete-columns"),
                    TableCmd::DeleteColumns,
                ),
                (
                    icons::TABLE_DELETE_ROW,
                    &t!("table-delete-rows"),
                    TableCmd::DeleteRows,
                ),
            ],
        );
        ui.separator();
        rows(
            ui,
            &[
                (
                    icons::TABLE,
                    &t!("table-select-table"),
                    TableCmd::SelectTable,
                ),
                (
                    icons::TABLE_FREEZE_COLUMN,
                    &t!("table-select-columns"),
                    TableCmd::SelectColumns,
                ),
                (
                    icons::TABLE_FREEZE_ROW,
                    &t!("table-select-rows"),
                    TableCmd::SelectRows,
                ),
                (
                    icons::TABLE_CELL_EDIT,
                    &t!("table-select-cell"),
                    TableCmd::SelectCell,
                ),
            ],
        );
        ui.separator();
        if checked_menu_row(
            ui,
            icons::BORDER_NONE,
            &t!("table-hide-borders"),
            table.hide_borders,
            t,
        ) {
            command = Some(TableCmd::HideBorders);
        }
        ui.menu_button(
            submenu_label(icons::PAINT_BUCKET, &t!("table-shading"), t),
            |ui| {
                if let Some(color) = shading_menu(ui, t) {
                    command = Some(TableCmd::Shading(color));
                }
            },
        );
        if item(ui, icons::TEXT_SORT_ASCENDING, &t!("table-sort-ascending")) {
            command = Some(TableCmd::SortAscending);
        }
        if item(
            ui,
            icons::TEXT_SORT_DESCENDING,
            &t!("table-sort-descending"),
        ) {
            command = Some(TableCmd::SortDescending);
        }
        if checked_menu_row(
            ui,
            icons::TABLE_FREEZE_ROW,
            &t!("table-header-row"),
            table.header,
            t,
        ) {
            command = Some(TableCmd::HeaderRow);
        }
        if let Some(command) = command {
            picked = Some(CellMenu::Table(command));
        }
    });
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// A submenu's row, written as egui's submenu rows line up with `widgets::menu_item`'s.
fn submenu_label(glyph: &str, label: &str, t: Tokens) -> RichText {
    RichText::new(format!("{glyph}    {label}"))
        .font(theme::ui(12.5))
        .color(t.ink)
}

/// A menu row that is on or off: its glyph on a highlighted square while on. True when
/// clicked.
fn checked_menu_row(ui: &mut Ui, glyph: &str, label: &str, on: bool, t: Tokens) -> bool {
    widgets::checked_menu_item(ui, Some(glyph), label, on, t).clicked()
}

/// The Shading menu: "No Color", then the shades as swatches in two rows of five.
/// `Some(None)` clears the shading. It closes itself once something is picked.
pub(crate) fn shading_menu(ui: &mut Ui, t: Tokens) -> Option<Option<Rgb>> {
    let mut picked = None;
    if widgets::menu_item(ui, None, &t!("table-no-color"), t).clicked() {
        picked = Some(None);
    }
    for row in crate::table::SHADES.chunks(5) {
        ui.horizontal(|ui| {
            for shade in row {
                let (rect, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                let edge = if response.hovered() {
                    Stroke::new(1.5, t.ink)
                } else {
                    Stroke::new(1.0, t.line_strong)
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(2), shade.to_color32());
                ui.painter()
                    .rect_stroke(rect, CornerRadius::same(2), edge, StrokeKind::Inside);
                if response.clicked() {
                    picked = Some(Some(*shade));
                }
            }
        });
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Ctrl+↑ / Ctrl+↓, the desktop notebook's previous and next paragraph. The caret goes to the start of
/// paragraph `index` (from inside it) or of the one above, or to the start of the one
/// below (the end of the last). A selection is dropped.
fn paragraph_step(
    doc: &Document,
    editor: &mut EditorState,
    index: usize,
    caret: usize,
    down: bool,
) {
    let hidden = doc.hidden_blocks();
    let text = |i: &usize| doc.blocks[*i].kind.is_textual() && !hidden[*i];
    let block = &doc.blocks[index];
    let (id, char) = if down {
        (index + 1..doc.blocks.len())
            .find(text)
            .map_or((block.id, block.text.char_count()), |i| {
                (doc.blocks[i].id, 0)
            })
    } else if caret > 0 {
        (block.id, 0)
    } else {
        (0..index)
            .rev()
            .find(text)
            .map_or((block.id, 0), |i| (doc.blocks[i].id, 0))
    };
    editor.range = None;
    editor.selection = None;
    editor.focus_block(id, char);
}

/// Puts the caret in the block before (`after` false: at its end) or after (at its start)
/// the block at `index`, stepping into a table's last or first row (its first cell), and
/// past what a folded paragraph hides. False when there is no such block.
fn focus_beside(doc: &Document, editor: &mut EditorState, index: usize, after: bool) -> bool {
    let hidden = doc.hidden_blocks();
    let lands = |i: &usize| {
        !hidden[*i]
            && (doc.blocks[*i].kind.is_textual()
                || matches!(doc.blocks[*i].kind, BlockKind::Table(_)))
    };
    let found = if after {
        (index + 1..doc.blocks.len()).find(lands)
    } else {
        (0..index).rev().find(lands)
    };
    let Some(target) = found else {
        return false;
    };
    let id = doc.blocks[target].id;
    match &doc.blocks[target].kind {
        BlockKind::Table(table) => {
            let row = if after {
                0
            } else {
                table.rows.len().saturating_sub(1)
            };
            let end = if after {
                0
            } else {
                cell_chars(doc, target, row, 0)
            };
            editor.focus_cell(id, row, 0, end..end);
        }
        _ if after => editor.focus_block(id, 0),
        _ => editor.focus_block(id, doc.blocks[target].text.char_count()),
    }
    true
}

/// Where the red wavy lines under misspelled words go, as `(left, right, y)` per word
/// and row. `ranges` are char ranges into the galley's text, in order; the one holding
/// `typing` (the caret of an edit in progress) is skipped. `origin` is where the galley's
/// local coordinates start (`galley_pos - galley.rect.left()`).
fn squiggle_segments(
    galley: &egui::Galley,
    origin: egui::Pos2,
    ranges: &[Range<usize>],
    typing: Option<usize>,
) -> Vec<(f32, f32, f32)> {
    let mut segments = Vec::new();
    let mut row_start = 0;
    // Ranges that end before the current row are done; rows and ranges are both in
    // order, so this is one pass over each.
    let mut first = 0;
    for row in &galley.rows {
        let row_end = row_start + row.glyphs.len();
        while ranges
            .get(first)
            .is_some_and(|range| range.end <= row_start)
        {
            first += 1;
        }
        for range in ranges[first..]
            .iter()
            .take_while(|range| range.start < row_end)
        {
            if typing.is_some_and(|at| range.start <= at && at <= range.end) {
                continue;
            }
            let start = range.start.max(row_start);
            let end = range.end.min(row_end);
            if start >= end {
                continue;
            }
            let glyphs = &row.glyphs[start - row_start..end - row_start];
            let left = origin.x + row.pos.x + glyphs[0].pos.x;
            // Where the caret stands after the word: the next glyph's left edge. egui
            // snaps glyphs to whole pixels, so the last advance can overshoot it.
            let right = origin.x
                + row.pos.x
                + row
                    .glyphs
                    .get(end - row_start)
                    .map_or_else(|| glyphs[glyphs.len() - 1].max_x(), |next| next.pos.x);
            // The lowest baseline in the word, so a raised first letter (superscript)
            // doesn't lift the line into the text.
            let baseline = glyphs
                .iter()
                .map(|glyph| glyph.pos.y)
                .fold(f32::MIN, f32::max);
            segments.push((left, right, origin.y + row.pos.y + baseline + 2.5));
        }
        row_start += row.char_count_including_newline().0;
    }
    segments
}

fn wavy_line(painter: &egui::Painter, left: f32, right: f32, y: f32, color: Color32) {
    let mut points = Vec::new();
    let mut x = left;
    let mut up = true;
    while x < right {
        points.push(pos2(x, if up { y - 1.0 } else { y + 1.0 }));
        up = !up;
        x += 2.0;
    }
    points.push(pos2(right, if up { y - 1.0 } else { y + 1.0 }));
    painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
}

/// Suggestions for the misspelled word that was right-clicked, then "Add to dictionary"
/// and "Ignore all", as in the desktop notebook. Shows nothing unless the click landed on a misspelled
/// word. Suggestions are looked up in the background; the menu shows them once ready.
fn spelling_menu(
    ui: &mut Ui,
    doc: &Document,
    index: usize,
    editor: &mut EditorState,
    target: &TextMenuTarget,
    t: Tokens,
) -> Option<TextMenuCommand> {
    // `show_text_edit` found the word from the char under the pointer when the menu
    // opened; it must still be the word under that char, unchanged.
    let spelling = editor.spell_target.clone().filter(|spelling| {
        target
            .pointer_char
            .is_some_and(|at| spelling.range.contains(&at))
            && doc.blocks[index]
                .text
                .text
                .chars()
                .skip(spelling.range.start)
                .take(spelling.range.len())
                .eq(spelling.word.chars())
    })?;
    editor
        .spell
        .request_suggestions(&spelling.word, spelling.language);
    let note = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).font(theme::ui(12.0)).color(t.faint));
    };
    let mut command = None;
    match editor.spell.suggestions(&spelling.word, spelling.language) {
        None => note(ui, &t!("spell-looking")),
        Some([]) => note(ui, &t!("spell-no-suggestions")),
        Some(list) => {
            for suggestion in list {
                if text_menu_item(ui, icons::ARROW_RIGHT, suggestion, "", true, t).clicked() {
                    command = Some(TextMenuCommand::ReplaceWord {
                        range: spelling.range.clone(),
                        word: suggestion.clone(),
                    });
                }
            }
        }
    }
    if text_menu_item(
        ui,
        icons::BOOK_OPEN,
        &t!("spell-add-to-dictionary"),
        "",
        true,
        t,
    )
    .clicked()
    {
        command = Some(TextMenuCommand::AddToDictionary(spelling.word.clone()));
    }
    if text_menu_item(ui, icons::CHECK, &t!("spell-ignore-all"), "", true, t).clicked() {
        command = Some(TextMenuCommand::IgnoreWord(spelling.word.clone()));
    }
    ui.separator();
    command
}

/// Replaces a char range with `replacement`, which takes the style of the first replaced
/// character, and puts the caret after it.
fn replace_chars(
    doc: &mut Document,
    index: usize,
    range: Range<usize>,
    replacement: &str,
    editor: &mut EditorState,
) {
    let caret = range.start + replacement.chars().count();
    replace_text(doc, index, range, replacement);
    let id = doc.blocks[index].id;
    editor.focus_block(id, caret);
}

/// Replaces a char range of block `index` with `replacement`, which takes the style of the
/// first replaced character (a word fixed by the Spelling pane keeps its formatting).
pub(crate) fn replace_text(
    doc: &mut Document,
    index: usize,
    range: Range<usize>,
    replacement: &str,
) {
    let block = &mut doc.blocks[index];
    let style = block.text.style_for_caret(range.start + 1);
    let chars: Vec<char> = block.text.text.chars().collect();
    let mut text: String = chars[..range.start.min(chars.len())].iter().collect();
    text.push_str(replacement);
    text.extend(&chars[range.end.min(chars.len())..]);
    let caret = range.start + replacement.chars().count();
    block.text.sync_after_edit(&text, style, Some(caret));
    block.text.normalize();
}

/// Typing "* ", "- ", "1. ", "1) ", "a. ", "A. ", "i. ", "I. " or "[] " at the start of a
/// line starts a list, as in the desktop notebook.
fn autoformat_list(
    doc: &mut Document,
    index: usize,
    editor: &mut EditorState,
    caret: Option<usize>,
) {
    let block = &doc.blocks[index];
    if block.kind != BlockKind::Paragraph {
        return;
    }
    let text = &block.text.text;
    // the desktop notebook's "1." and "A.", and Word's "a.", "i." and "I.": the list's style follows.
    const LETTERED: [(&str, ListMarker); 4] = [
        ("a. ", ListMarker::LowerAlpha),
        ("A. ", ListMarker::UpperAlpha),
        ("i. ", ListMarker::LowerRoman),
        ("I. ", ListMarker::UpperRoman),
    ];
    let (prefix_chars, kind, marker) = if text.starts_with("* ") || text.starts_with("- ") {
        (2, BlockKind::Bullet, None)
    } else if text.starts_with("1. ") {
        (3, BlockKind::Numbered, None)
    } else if text.starts_with("1) ") {
        (3, BlockKind::Numbered, Some(ListMarker::DecimalParen))
    } else if let Some((_, marker)) = LETTERED.iter().find(|(prefix, _)| text.starts_with(prefix)) {
        (3, BlockKind::Numbered, Some(*marker))
    } else if text.starts_with("[] ") || text.starts_with("[ ] ") {
        (
            if text.starts_with("[] ") { 3 } else { 4 },
            BlockKind::Todo { checked: false },
            None,
        )
    } else {
        return;
    };
    // Only right after the space is typed, not when pasting or editing later.
    if caret != Some(prefix_chars) {
        return;
    }
    let id = block.id;
    let block = &mut doc.blocks[index];
    block.text.delete_chars(0..prefix_chars);
    set_kind(block, kind);
    block.marker = marker;
    editor.focus_block(id, 0);
}

/// How a paragraph's tags dress its text: a highlight (Remember for later's, Definition's
/// or a custom tag's) and a custom tag's font colour.
#[derive(Clone, Copy, Default)]
struct TagLook {
    background: Option<Color32>,
    color: Option<Rgb>,
}

impl TagLook {
    fn of(block: &Block) -> Self {
        let background = block
            .tags
            .iter()
            .find_map(|tag| tag.background())
            .or_else(|| block.custom_tags.iter().find_map(|tag| tag.highlight));
        Self {
            background: background.map(Rgb::to_color32),
            color: block.custom_tags.iter().find_map(|tag| tag.font_color),
        }
    }
}

fn layout_job(
    text: &str,
    runs: &[StyleRun],
    kind: &BlockKind,
    checked: bool,
    quote: bool,
    look: TagLook,
    halign: Align,
    wrap_width: f32,
    t: Tokens,
    marks: &[(Range<usize>, bool)],
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.halign = halign;
    // Like egui's own editor: typed spaces count, so the caret moves in centered text.
    job.keep_trailing_whitespace = true;
    // Runs can lag the buffer by a frame while typing; snap them to char boundaries so
    // no text is ever dropped from the layout.
    let snap = |mut i: usize| {
        i = i.min(text.len());
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    // Find-on-page marks are char ranges, in order and never overlapping; the text may
    // be a frame ahead of them while typing, so they stop at its end. (This runs every
    // frame for every block on screen: one pass over the text, one over the marks.)
    let marks: Vec<(Range<usize>, bool)> =
        util::byte_ranges(text, marks.iter().map(|(range, _)| range))
            .into_iter()
            .zip(marks.iter().map(|(_, current)| *current))
            .collect();
    let mut boundaries: Vec<usize> = vec![0, text.len()];
    for run in runs {
        boundaries.push(snap(run.start));
        boundaries.push(snap(run.end));
    }
    for (range, _) in &marks {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut next_mark = 0;
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start >= end {
            continue;
        }
        while marks
            .get(next_mark)
            .is_some_and(|(range, _)| range.end <= start)
        {
            next_mark += 1;
        }
        let mut style = style_at_runs(runs, start);
        // A custom tag's font colour, where the text has no colour (or link) of its own.
        if style.color.is_none() && style.link.is_none() {
            style.color = look.color;
        }
        // Find-on-page matches; the current one is stronger.
        let format = match marks
            .get(next_mark)
            .filter(|(range, _)| range.start <= start)
        {
            Some((_, current)) => {
                // The find colour replaces any highlighter (inline or a tag's), so the
                // text keeps the ink it has on the page: dark mode's light ink, not the
                // dark ink it takes on a light highlighter.
                style.highlight = None;
                TextFormat {
                    background: theme::find_highlight(t, *current),
                    ..text_format(&style, kind, checked, quote, None, t)
                }
            }
            None => text_format(&style, kind, checked, quote, look.background, t),
        };
        job.append(&text[start..end], 0.0, format);
    }
    if job.sections.is_empty() {
        job.append(
            "",
            0.0,
            text_format(
                &InlineStyle {
                    color: look.color,
                    ..InlineStyle::default()
                },
                kind,
                checked,
                quote,
                look.background,
                t,
            ),
        );
    }
    job
}

fn style_at_runs(runs: &[StyleRun], start: usize) -> InlineStyle {
    runs.iter()
        .find(|run| run.start <= start && start < run.end)
        .map_or_else(InlineStyle::default, |run| run.style.clone())
}

/// A line's height for text of `px` screen points: single spacing, as Calibri sets it.
fn line_for(px: f32) -> f32 {
    (px * 1.22).round()
}

/// Paragraph Spacing Options' Line Spacing At Least: no line is shorter than `least`
/// screen points (0 for none). Text keeps its own taller lines.
fn lines_at_least(job: &mut LayoutJob, least: f32) {
    if least <= 0.0 {
        return;
    }
    for section in &mut job.sections {
        if let Some(line) = section.format.line_height {
            section.format.line_height = Some(line.max(least));
        }
    }
}

/// On a ruled page each line takes a whole number of rule spacings (`pitch`), so every
/// line of a paragraph, and every paragraph after it, sits on a rule as in the desktop notebook. egui
/// centres text in a line taller than its font.
fn snap_lines(job: &mut LayoutJob, pitch: f32) {
    for section in &mut job.sections {
        if let Some(line) = section.format.line_height {
            section.format.line_height = Some((line / pitch).ceil().max(1.0) * pitch);
        }
    }
}

/// Size and line height (screen points) and face a block kind uses where a run sets none:
/// the desktop notebook's built-in styles. Heading levels are stored one more than their number: level 1
/// is the Page Title style, 2–7 are Heading 1–6 (see `BlockKind::heading`).
fn kind_defaults(kind: &BlockKind) -> (f32, f32, FontFace) {
    let px = match kind {
        BlockKind::Heading { level: 0 | 1 } => pt_to_px(20),
        BlockKind::Heading { level: 2 } => pt_to_px(16),
        BlockKind::Heading { level: 3 } => pt_to_px(14),
        BlockKind::Heading { level: 4 | 5 } => pt_to_px(12),
        BlockKind::Citation => pt_to_px(9),
        _ => BODY_SIZE,
    };
    let face = if matches!(kind, BlockKind::Code) {
        FontFace::Mono
    } else {
        FontFace::Body
    };
    let line = if px == BODY_SIZE {
        BODY_LINE
    } else {
        line_for(px)
    };
    (px, line, face)
}

/// A style's own ink where a run sets none: the desktop notebook's blue headings, and its grey quote
/// and citation.
fn kind_ink(kind: &BlockKind, t: Tokens) -> Option<Color32> {
    let (light, dark): (u32, u32) = match kind {
        BlockKind::Heading { level: 2 } => (0x1E4E79, 0x8AB4E8),
        BlockKind::Heading { level: 3 | 6 | 7 } => (0x2E75B5, 0x6CB4F0),
        BlockKind::Heading { level: 4 } => (0x1F3763, 0xA9C3E8),
        BlockKind::Heading { level: 5 } => (0x2F5496, 0x9AB3E3),
        BlockKind::Quote | BlockKind::Citation => return Some(t.muted),
        _ => return None,
    };
    let hex = if t.dark { dark } else { light };
    Some(Color32::from_rgb(
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
    ))
}

/// Styles set in bold: the desktop notebook's Heading 3 to 6.
fn kind_bold(kind: &BlockKind) -> bool {
    matches!(kind, BlockKind::Heading { level: 4..=7 })
}

/// Styles set in italics: Heading 4 and 6, and Quote.
fn kind_italic(kind: &BlockKind) -> bool {
    matches!(kind, BlockKind::Heading { level: 5 | 7 } | BlockKind::Quote)
}

/// The egui family for a face and weight, and whether egui must slant it because the face
/// has no italic of its own.
fn face_family(face: FontFace, bold: bool, italic: bool) -> (egui::FontFamily, bool) {
    let (family, has_italic) = match face {
        FontFace::Body if bold && italic => (theme::named("body-bold-italic"), true),
        FontFace::Body if bold => (theme::named("body-bold"), false),
        FontFace::Body if italic => (theme::named("body-italic"), true),
        FontFace::Body => (theme::named("body"), false),
        FontFace::Serif if bold => (theme::named("serif-bold"), false),
        FontFace::Serif if italic => (theme::named("serif-italic"), true),
        FontFace::Serif => (theme::named("serif"), false),
        FontFace::Sans if bold => (theme::named("ui-bold"), false),
        FontFace::Sans => (egui::FontFamily::Proportional, false),
        FontFace::Mono => (egui::FontFamily::Monospace, false),
        FontFace::Readable if bold => (theme::named("readable-bold"), false),
        FontFace::Readable if italic => (theme::named("readable-italic"), true),
        FontFace::Readable => (theme::named("readable"), false),
        FontFace::Hand => (theme::named("hand"), false),
    };
    (family, italic && !has_italic)
}

/// Ink that stays readable on `background`: Folio's darkest ink on light colours, white
/// on dark ones (Office's Automatic does the same).
fn ink_on(background: Color32) -> Color32 {
    let [r, g, b, _] = background.to_array();
    if Rgb(r, g, b).wants_light_text() {
        Color32::WHITE
    } else {
        Rgb::INKS[0].to_color32()
    }
}

fn text_format(
    style: &InlineStyle,
    kind: &BlockKind,
    checked: bool,
    quote: bool,
    tag_background: Option<Color32>,
    t: Tokens,
) -> TextFormat {
    let _ = checked; // the desktop notebook neither dims nor strikes a ticked To Do
    let (base_size, base_line, default_face) = kind_defaults(kind);
    let (mut size, line) = match style.size {
        Some(pt) => {
            let px = pt_to_px(pt);
            (px, line_for(px))
        }
        None => (base_size, base_line),
    };
    let (family, slant) = if style.code || matches!(kind, BlockKind::Code) {
        (egui::FontFamily::Monospace, style.italic)
    } else {
        face_family(
            style.font.unwrap_or(default_face),
            style.bold || kind_bold(kind),
            style.italic || (kind_italic(kind) && (quote || !matches!(kind, BlockKind::Quote))),
        )
    };
    let mut line_height = Some(line);
    let mut valign = Align::BOTTOM;
    if style.superscript || style.subscript {
        // Smaller text with no line height of its own sits at the top (or bottom) of the row.
        size = (size * 0.62).round().max(7.0);
        line_height = None;
        valign = if style.superscript {
            Align::TOP
        } else {
            Align::BOTTOM
        };
    }
    // An explicit inline highlight wins over a tag's.
    let background = style
        .highlight
        .map(Rgb::to_color32)
        .or(tag_background)
        .unwrap_or(Color32::TRANSPARENT);
    let color = style.color.map_or_else(
        || {
            let ink = kind_ink(kind, t).unwrap_or(t.ink);
            if background == Color32::TRANSPARENT {
                return ink;
            }
            // Light ink would vanish on the light highlighter colours, and dark ink on the
            // dark ones. The light theme's own ink already reads on a light highlight.
            match ink_on(background) {
                on_light if !t.dark && on_light != Color32::WHITE => ink,
                readable => readable,
            }
        },
        Rgb::to_color32,
    );
    let mut format = TextFormat {
        font_id: egui::FontId::new(size, family),
        color,
        line_height,
        background,
        expand_bg: 1.5,
        underline: if style.underline {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        },
        strikethrough: if style.strike {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        },
        italics: slant,
        valign,
        ..Default::default()
    };
    if style.link.is_some() {
        if style.color.is_none() {
            // On a highlight, the link colour of the theme whose page that highlight is
            // like, light or dark, so a link on Black or on Yellow still reads.
            format.color = match background {
                Color32::TRANSPARENT => t.accent,
                _ if ink_on(background) == Color32::WHITE => Tokens::dark().accent,
                _ => Tokens::light().accent,
            };
        }
        format.underline = Stroke::new(1.0, format.color);
    }
    format
}

/// How a style looks in the Styles gallery and menu: its own font, weight, slant and
/// colour, at most `PREVIEW_MAX` px so every row stays a row.
pub(crate) fn style_preview(kind: &BlockKind, t: Tokens) -> TextFormat {
    let mut format = text_format(&InlineStyle::default(), kind, false, true, None, t);
    format.font_id.size = format.font_id.size.clamp(11.0, PREVIEW_MAX);
    format.line_height = None;
    format.background = Color32::TRANSPARENT;
    format.valign = Align::Center;
    format
}

/// The largest preview in the Styles gallery and menu.
pub(crate) const PREVIEW_MAX: f32 = 15.0;

/// A picture in a note, `width` of the column wide (0.1–1), at its own proportions as
/// turned and mirrored by `meta`.
#[allow(clippy::too_many_arguments)]
fn show_image(
    ui: &mut Ui,
    attachment: &str,
    width: f32,
    drawing: bool,
    _page_id: &str,
    library: &Library,
    textures: &mut TextureCache,
    t: Tokens,
    meta: &crate::model::PictureMeta,
) -> egui::Response {
    let available = ui.available_width();
    let width = (available * width.clamp(0.1, 1.0)).min(available);
    // The cache is checked first, so the database is only asked on a miss.
    let texture = textures.get_with(ui.ctx(), attachment, || {
        let meta = library.attachment(attachment).ok().flatten()?;
        Some(library.attachment_path(&meta.stored_path))
    });
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        // Only a cropped picture's kept part is shown, at its proportions.
        let [kept_width, kept_height] = meta.cropped_size([size.x, size.y]);
        let shown = if meta.quarter_turned() {
            vec2(kept_height, kept_width)
        } else {
            vec2(kept_width, kept_height)
        };
        let height = (width * shown.y / shown.x.max(1.0)).round();
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        // No frame: the desktop notebook shows a picture bare until it is selected.
        widgets::paint_texture_turned(ui.painter(), texture.id(), size, rect, meta);
        return response;
    }
    let (rect, response) = ui.allocate_exact_size(vec2(width, 100.0), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), t.sunken);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        if drawing {
            t!("editor-drawing-missing")
        } else {
            t!("editor-picture-missing")
        },
        theme::ui(12.0),
        t.faint,
    );
    response
}

/// A picture block's right-click menu: Alt Text…, Rotate › and Restore Original Size, as
/// the desktop notebook's picture menu. Page pictures add these rows to their item menu.
pub(crate) fn picture_menu(ui: &mut Ui, t: Tokens) -> Option<crate::canvas::PictureCmd> {
    use crate::canvas::PictureCmd as P;
    let mut picked = None;
    ui.set_min_width(210.0);
    if widgets::menu_item(
        ui,
        Some(icons::IMAGE_ALT_TEXT),
        &t!("picture-alt-text-menu"),
        t,
    )
    .clicked()
    {
        picked = Some(P::AltText);
    }
    ui.menu_button(
        submenu_label(icons::ARROW_ROTATE_CLOCKWISE, &t!("picture-rotate"), t),
        |ui| {
            if let Some(command) = rotate_rows(ui, t) {
                picked = Some(command);
            }
        },
    );
    if widgets::menu_item(ui, Some(icons::ARROW_RESET), &t!("picture-restore-size"), t).clicked() {
        picked = Some(P::RestoreSize);
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// A picture's four turns, as rows: Rotate Right 90°, Rotate Left 90°, Flip Horizontal
/// and Flip Vertical. The picture menu's Rotate › and the Picture tab's Rotate list them.
pub(crate) fn rotate_rows(ui: &mut Ui, t: Tokens) -> Option<crate::canvas::PictureCmd> {
    use crate::canvas::PictureCmd as P;
    ui.set_min_width(210.0);
    let mut picked = None;
    for (glyph, label, command) in [
        (
            icons::ARROW_ROTATE_CLOCKWISE,
            t!("ribbon-rotate-right"),
            P::RotateRight,
        ),
        (
            icons::ARROW_ROTATE_COUNTERCLOCKWISE,
            t!("ribbon-rotate-left"),
            P::RotateLeft,
        ),
        (
            icons::FLIP_HORIZONTAL,
            t!("ribbon-flip-horizontal"),
            P::FlipHorizontal,
        ),
        (
            icons::FLIP_VERTICAL,
            t!("ribbon-flip-vertical"),
            P::FlipVertical,
        ),
    ] {
        if widgets::menu_item(ui, Some(glyph), &label, t).clicked() {
            picked = Some(command);
        }
    }
    picked
}

fn apply_block_actions(
    ctx: &egui::Context,
    doc: &mut Document,
    doc_id: &str,
    editor: &mut EditorState,
    actions: Vec<BlockAction>,
) -> bool {
    let now = 0.0;
    let mut changed = false;
    for action in actions {
        match action {
            BlockAction::ToggleChecked(id) => {
                if let Some(index) = doc.block_index(id) {
                    editor.push_undo(doc, now);
                    let checked = doc.blocks[index].checked();
                    doc.blocks[index].kind = BlockKind::Todo { checked: !checked };
                    changed = true;
                }
            }
            BlockAction::ToggleTagChecked(id, tick) => {
                let Some(index) = doc.block_index(id) else {
                    continue;
                };
                let ticked = match &tick {
                    TagTick::Line(tag) => line_tag_state(&doc.blocks[index], *tag),
                    TagTick::Custom(name) => custom_tag_state(&doc.blocks[index], name),
                };
                let Some(ticked) = ticked else {
                    continue;
                };
                editor.push_undo(doc, now);
                let block = &mut doc.blocks[index];
                match tick {
                    TagTick::Line(tag) => set_line_tag_state(block, tag, Some(!ticked)),
                    TagTick::Custom(name) => {
                        for tag in block.custom_tags.iter_mut().filter(|tag| tag.name == name) {
                            tag.checked = !ticked;
                        }
                    }
                }
                changed = true;
            }
            BlockAction::FinishEquation { block, enter } => {
                changed |= finish_equation(doc, editor, block, enter);
            }
            BlockAction::TableCommand(command) => {
                changed |= table_command(ctx, doc, doc_id, editor, command);
                // Picking from the menu took the keyboard from the cell: give it back,
                // unless the command put the caret somewhere itself.
                editor.restore_focus();
            }
        }
    }
    changed
}

/// Ends the session of the equation block being typed in `doc`, if any, as the caret
/// leaving it does: a blank one is deleted. The canvas calls it before it starts another
/// equation in the note. Returns whether the note changed.
pub(crate) fn end_equation(doc: &mut Document, editor: &mut EditorState) -> bool {
    match editor.equation_block {
        Some(id) => finish_equation(doc, editor, id, false),
        None => false,
    }
}

/// Ends the session typing equation block `id`. A blank one is deleted (the undo step its
/// session began with brings it back). After Enter the caret goes to the start of the
/// next paragraph, an empty one added after the equation when none follows, so typing
/// goes on below it as in the desktop notebook; after Esc or a click elsewhere the keyboard stays
/// where it went. Returns whether the note changed.
fn finish_equation(doc: &mut Document, editor: &mut EditorState, id: u64, enter: bool) -> bool {
    if editor.equation_block == Some(id) {
        editor.equation_block = None;
        editor.focus_equation = false;
    }
    let Some(index) = doc.block_index(id) else {
        return false;
    };
    let blank = matches!(
        &doc.blocks[index].kind,
        BlockKind::Equation { source, .. } if source.trim().is_empty()
    );
    if blank {
        let pending = editor.pending_focus.take();
        let active = editor.active_block;
        delete_block(doc, editor, index);
        if !enter {
            // `delete_block` puts the caret in the block after it; a click took it elsewhere.
            editor.pending_focus = pending;
            if active != Some(id) {
                editor.active_block = active;
            }
        }
        return true;
    }
    if !enter {
        return false;
    }
    let mut changed = false;
    let next = (index + 1..doc.blocks.len()).find(|&next| doc.blocks[next].kind.is_textual());
    let next = match next {
        Some(next) => doc.blocks[next].id,
        None => {
            let mut paragraph = Block::paragraph("");
            paragraph.indent = doc.blocks[index].indent;
            let id = paragraph.id;
            doc.blocks.insert(index + 1, paragraph);
            changed = true;
            id
        }
    };
    editor.active_block = Some(next);
    editor.selection = None;
    editor.focus_block(next, 0);
    changed
}

/// How far below the top of a plain paragraph's first row its text's baseline is, as the
/// note editor lays it out: on lines `pitch` apart on a ruled page, else single spaced.
/// The canvas lines notes up with the rules by it.
pub(crate) fn body_baseline(ctx: &egui::Context, pitch: Option<f32>) -> f32 {
    let format = text_format(
        &InlineStyle::default(),
        &BlockKind::Paragraph,
        false,
        false,
        None,
        crate::theme::Tokens::light(),
    );
    let mut job = LayoutJob::default();
    job.append("Saturn", 0.0, format);
    if let Some(pitch) = pitch {
        snap_lines(&mut job, pitch);
    }
    let galley = ctx.fonts_mut(|fonts| fonts.layout_job(job));
    let row = &galley.rows[0];
    row.pos.y + row.glyphs[0].pos.y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mini_bar_opacity_fades_with_the_pointer() {
        let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(300.0, 70.0));
        // Not drawn yet, or no pointer: faded.
        assert_eq!(mini_bar_opacity(Some(pos2(120.0, 120.0)), None), 0.55);
        assert_eq!(mini_bar_opacity(None, Some(rect)), 0.55);
        // On it, or within 8 px: solid.
        assert_eq!(mini_bar_opacity(Some(rect.center()), Some(rect)), 1.0);
        assert_eq!(mini_bar_opacity(Some(pos2(200.0, 178.0)), Some(rect)), 1.0);
        // Further away, fainter, and gone at MINI_FAR.
        let near = mini_bar_opacity(Some(pos2(200.0, 190.0)), Some(rect));
        assert!(
            (near - 0.55 * (1.0 - 20.0 / MINI_FAR)).abs() < 1e-4,
            "{near}"
        );
        let further = mini_bar_opacity(Some(pos2(200.0, 250.0)), Some(rect));
        assert!(further > 0.0 && further < near, "{further} vs {near}");
        assert_eq!(mini_bar_opacity(Some(pos2(-100.0, 100.0)), Some(rect)), 0.0);
    }

    /// Options › General › Show Mini Toolbar on selection is a setting: a page change
    /// (`reset`) keeps it, and puts away the bar shown on the old page.
    #[test]
    fn a_page_change_keeps_the_mini_toolbar_setting() {
        let mut editor = EditorState::default();
        assert!(editor.mini_on_selection, "on by default");
        editor.mini_on_selection = false;
        editor.mini_bar = Some(MiniBar {
            doc_id: "page:note".into(),
            block: 1,
            at: pos2(10.0, 10.0),
            rect: None,
        });
        editor.reset();
        assert!(!editor.mini_on_selection, "still off on the next page");
        assert!(editor.mini_bar.is_none());
    }

    /// Options › Advanced › Calculate mathematical expressions automatically is a setting
    /// too: turned off, it stays off on the next page.
    #[test]
    fn a_page_change_keeps_the_auto_calc_setting() {
        let mut editor = EditorState::default();
        assert!(
            editor.auto_calc,
            "on by default, as in the desktop notebook"
        );
        editor.auto_calc = false;
        editor.reset();
        assert!(!editor.auto_calc, "still off on the next page");
    }

    #[test]
    fn style_previews_keep_their_look_within_a_row() {
        let t = Tokens::light();
        let h1 = style_preview(&BlockKind::heading(1), t);
        let normal = style_preview(&BlockKind::Paragraph, t);
        assert!(h1.font_id.size <= PREVIEW_MAX && h1.font_id.size > normal.font_id.size);
        assert_ne!(
            h1.color, normal.color,
            "Heading 1 keeps the desktop notebook's blue"
        );
        // The body face has an italic of its own, so a slanted style shows in its family
        // rather than egui's synthetic slant.
        let italic = |format: &TextFormat| {
            format.italics || format.font_id.family == theme::named("body-italic")
        };
        assert!(
            italic(&style_preview(&BlockKind::Citation, t))
                || italic(&style_preview(&BlockKind::Quote, t))
        );
        assert_eq!(
            style_preview(&BlockKind::Code, t).font_id.family,
            egui::FontFamily::Monospace
        );
    }

    #[test]
    fn numbers_follow_notebooks_outline_levels() {
        let label = |number: usize, level: u8| ListMarker::for_level(true, level).text(number);
        assert_eq!(label(3, 0), "3.");
        assert_eq!(label(2, 1), "b.");
        assert_eq!(label(27, 1), "aa.");
        assert_eq!(label(4, 2), "iv.");
        assert_eq!(label(14, 5), "xiv.");
        assert_eq!(label(5, 3), "5.");
    }

    #[test]
    fn ranges_cover_whole_middle_paragraphs_and_cut_their_ends() {
        let doc = Document {
            blocks: vec![
                Block::paragraph("first line"),
                Block::new(BlockKind::Divider),
                Block::paragraph("middle"),
                Block::paragraph("last line"),
            ],
        };
        let at = |index: usize, char: usize| Pos {
            block: doc.blocks[index].id,
            char,
        };
        let editor = EditorState {
            range: Some((at(3, 4), at(0, 6))),
            ..EditorState::default()
        };
        let range = editor.range_in(&doc).expect("in this note");
        assert_eq!(range, (at(0, 6), at(3, 4)), "ordered start, end");
        assert_eq!(
            range_segments(&doc, range),
            [(0, 6..10), (2, 0..6), (3, 0..4)]
        );
        assert_eq!(range_text(&doc, range), "line\nmiddle\nlast");
        let other = Document {
            blocks: vec![Block::paragraph("elsewhere")],
        };
        assert!(editor.range_in(&other).is_none());
    }

    #[test]
    fn deleting_or_typing_over_a_range_joins_its_ends() {
        let mut doc = Document {
            blocks: vec![
                Block::paragraph("Hello there"),
                Block::paragraph("gone"),
                Block::paragraph("General Kenobi"),
            ],
        };
        let at = |doc: &Document, index: usize, char: usize| Pos {
            block: doc.blocks[index].id,
            char,
        };
        let range = (at(&doc, 0, 6), at(&doc, 2, 8));
        let mut editor = EditorState::default();
        replace_range(&mut doc, &mut editor, range, "Mr ", 0.0);
        assert_eq!(doc.plain_text(), "Hello Mr Kenobi");
        assert!(editor.range.is_none());
        assert!(editor.undo(&mut doc));
        assert_eq!(doc.blocks.len(), 3, "one undo step brings it all back");
        let range = (at(&doc, 0, 5), at(&doc, 1, 4));
        let caret = delete_range(&mut doc, range);
        assert_eq!(doc.plain_text(), "Hello\nGeneral Kenobi");
        assert_eq!(caret.char, 5);
    }

    #[test]
    fn formats_kinds_and_levels_apply_to_a_whole_range() {
        let mut doc = Document {
            blocks: vec![
                Block::paragraph("one"),
                Block::paragraph("two"),
                Block::paragraph("three"),
            ],
        };
        let at = |doc: &Document, index: usize, char: usize| Pos {
            block: doc.blocks[index].id,
            char,
        };
        let mut editor = EditorState {
            active_block: Some(doc.blocks[0].id),
            range: Some((at(&doc, 0, 1), at(&doc, 2, 2))),
            ..EditorState::default()
        };
        assert!(toggle_mark(&mut doc, &mut editor, Mark::Bold, 0.0));
        let bold = |doc: &Document, index: usize, char: usize| {
            let text = &doc.blocks[index].text;
            text.style_at(text.char_to_byte(char)).bold
        };
        assert!(!bold(&doc, 0, 0) && bold(&doc, 0, 1) && bold(&doc, 1, 0) && bold(&doc, 2, 1));
        assert!(!bold(&doc, 2, 2), "the range ends before the third char");
        assert!(set_block_kind(
            &mut doc,
            &mut editor,
            0,
            BlockKind::Bullet,
            0.0
        ));
        assert!(doc
            .blocks
            .iter()
            .all(|block| block.kind == BlockKind::Bullet));
        let span = outline_span(&doc, &editor, 0);
        assert_eq!(span, 0..3);
    }

    #[test]
    fn shaded_cells_keep_dark_ink_in_dark_mode() {
        let (light, dark) = (crate::theme::Tokens::light(), crate::theme::Tokens::dark());
        let shade = Some(crate::table::SHADES[2]);
        assert_eq!(
            cell_tokens(dark, shade).ink,
            Rgb::INKS[0].to_color32(),
            "light ink would vanish on the light shade"
        );
        assert_eq!(cell_tokens(dark, None).ink, dark.ink);
        assert_eq!(cell_tokens(light, shade).ink, light.ink);
    }

    #[test]
    fn highlighted_text_keeps_dark_ink_in_dark_mode() {
        let dark = crate::theme::Tokens::dark();
        let plain = BlockKind::Paragraph;
        let tag = LineTag::RememberForLater
            .background()
            .map(Rgb::to_color32)
            .expect("Remember for later has a background");
        let tagged = text_format(
            &InlineStyle::default(),
            &plain,
            false,
            false,
            Some(tag),
            dark,
        );
        assert_eq!(tagged.background, tag);
        assert_eq!(
            tagged.color,
            Rgb::INKS[0].to_color32(),
            "light ink would vanish on the highlighter"
        );
        let marked = InlineStyle {
            highlight: Some(Rgb::HIGHLIGHTERS[1]),
            ..InlineStyle::default()
        };
        let marked = text_format(&marked, &plain, false, false, Some(tag), dark);
        assert_eq!(
            marked.background,
            Rgb::HIGHLIGHTERS[1].to_color32(),
            "an inline highlight wins over the tag's"
        );
        assert_eq!(marked.color, Rgb::INKS[0].to_color32());
        let bare = text_format(&InlineStyle::default(), &plain, false, false, None, dark);
        assert_eq!(
            bare.color, dark.ink,
            "unhighlighted text keeps the theme's ink"
        );
        let light = crate::theme::Tokens::light();
        let tagged = text_format(
            &InlineStyle::default(),
            &plain,
            false,
            false,
            Some(tag),
            light,
        );
        assert_eq!(tagged.color, light.ink);
    }

    #[test]
    fn ink_stays_readable_on_every_highlight() {
        let named = |id: &str| {
            Rgb::TEXT_HIGHLIGHTS
                .iter()
                .find(|(_, name)| *name == id)
                .map(|(color, _)| color.to_color32())
                .unwrap()
        };
        for dark in ["color-black", "color-dark-blue", "color-dark-red"] {
            assert_eq!(ink_on(named(dark)), Color32::WHITE, "{dark}");
        }
        for light in ["color-yellow", "color-bright-green"] {
            assert_eq!(ink_on(named(light)), Rgb::INKS[0].to_color32(), "{light}");
        }
        // Text on a dark highlight turns white in both themes; an explicit colour still wins.
        let navy = InlineStyle {
            highlight: Some(Rgb(0x00, 0x00, 0x80)),
            ..InlineStyle::default()
        };
        let plain = BlockKind::Paragraph;
        for t in [crate::theme::Tokens::light(), crate::theme::Tokens::dark()] {
            let format = text_format(&navy, &plain, false, false, None, t);
            assert_eq!(format.color, Color32::WHITE);
            let red = InlineStyle {
                color: Some(Rgb(0xc0, 0x00, 0x00)),
                ..navy.clone()
            };
            let format = text_format(&red, &plain, false, false, None, t);
            assert_eq!(format.color, Rgb(0xc0, 0x00, 0x00).to_color32());
            // A link reads on a dark highlight and on a light one, in either theme.
            for (highlight, link) in [
                (Rgb(0x00, 0x00, 0x80), crate::theme::Tokens::dark().accent),
                (Rgb(0xff, 0xff, 0x00), crate::theme::Tokens::light().accent),
            ] {
                let linked = InlineStyle {
                    highlight: Some(highlight),
                    link: Some("https://example.com".into()),
                    ..InlineStyle::default()
                };
                let format = text_format(&linked, &plain, false, false, None, t);
                assert_eq!(format.color, link, "{highlight:?}");
            }
        }
    }

    #[test]
    fn a_ticked_to_do_is_neither_struck_through_nor_dimmed() {
        let t = crate::theme::Tokens::light();
        let todo = BlockKind::Todo { checked: true };
        let ticked = text_format(&InlineStyle::default(), &todo, true, false, None, t);
        let open = text_format(
            &InlineStyle::default(),
            &BlockKind::Todo { checked: false },
            false,
            false,
            None,
            t,
        );
        assert_eq!(ticked.strikethrough, Stroke::NONE);
        assert_eq!(ticked.color, t.ink);
        assert_eq!(ticked, open, "ticking changes nothing in the text");
    }

    #[test]
    fn on_a_ruled_page_lines_take_whole_rule_spacings() {
        let t = crate::theme::Tokens::light();
        let mut job = LayoutJob::default();
        let body = text_format(
            &InlineStyle::default(),
            &BlockKind::Paragraph,
            false,
            false,
            None,
            t,
        );
        let big = text_format(
            &InlineStyle::default(),
            &BlockKind::PAGE_TITLE,
            false,
            false,
            None,
            t,
        );
        let sup = InlineStyle {
            superscript: true,
            ..InlineStyle::default()
        };
        let sup = text_format(&sup, &BlockKind::Paragraph, false, false, None, t);
        job.append("body", 0.0, body);
        job.append("title", 0.0, big);
        job.append("2", 0.0, sup);
        snap_lines(&mut job, 27.0);
        let lines: Vec<Option<f32>> = job
            .sections
            .iter()
            .map(|section| section.format.line_height)
            .collect();
        assert_eq!(lines, [Some(27.0), Some(54.0), None]);
    }

    #[test]
    fn nested_numbering_restarts_per_level() {
        let mut blocks = Vec::new();
        for indent in [0u8, 0, 1, 1, 0] {
            let mut block = Block::new(BlockKind::Numbered);
            block.indent = indent;
            blocks.push(block);
        }
        blocks.push(Block::paragraph("break"));
        blocks.push(Block::new(BlockKind::Numbered));
        let doc = Document { blocks };
        assert_eq!(
            crate::model::list_numbers(&doc.blocks),
            [1, 2, 1, 2, 3, 0, 1]
        );
    }

    #[test]
    fn a_note_under_an_item_keeps_the_count_going() {
        let mut note = Block::paragraph("note");
        note.indent = 1;
        let doc = Document {
            blocks: vec![
                Block::new(BlockKind::Numbered),
                note,
                Block::new(BlockKind::Numbered),
            ],
        };
        assert_eq!(crate::model::list_numbers(&doc.blocks), [1, 0, 2]);
    }

    /// Paragraph Spacing Options move the paragraphs: space after one pushes the next one
    /// down, space before pushes the paragraph itself down, and Line Spacing At Least makes
    /// its line taller. On a ruled page the space goes by whole rules. One undo step sets
    /// it on the selected paragraphs.
    #[test]
    fn spacing_pushes_the_next_paragraph_down() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let library = Library::open_in_memory().unwrap();
        let mut textures = TextureCache::default();
        // Each row's top and height, as the note editor lays the paragraphs out.
        let mut rows = |doc: &mut Document, pitch: Option<f32>| -> Vec<(f32, f32)> {
            let mut editor = EditorState {
                line_pitch: pitch,
                ..EditorState::default()
            };
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show_document(
                    ui,
                    doc,
                    "page:1",
                    &mut editor,
                    &library,
                    &mut textures,
                    Tokens::light(),
                    0.0,
                );
            });
            output.textures_delta.clear();
            editor.rows["page:1"]
                .iter()
                .map(|row| (row.rect.top(), row.rect.height()))
                .collect()
        };
        let mut doc = Document {
            blocks: vec![Block::paragraph("First"), Block::paragraph("Second")],
        };
        let plain = rows(&mut doc, None);
        doc.blocks[0].spacing.after = 12.0;
        let after = rows(&mut doc, None);
        assert_eq!(after[0], plain[0]);
        assert!(
            (after[1].0 - plain[1].0 - 16.0).abs() < 0.5,
            "12 pt after is 16 px: {plain:?} {after:?}"
        );
        doc.blocks[0].spacing.before = 6.0;
        let before = rows(&mut doc, None);
        assert!((before[0].0 - plain[0].0 - 8.0).abs() < 0.5, "{before:?}");
        assert!((before[1].0 - plain[1].0 - 24.0).abs() < 0.5, "{before:?}");
        doc.blocks[0].spacing = ParaSpacing {
            line_at_least: 30.0,
            ..ParaSpacing::default()
        };
        let tall = rows(&mut doc, None);
        assert!(tall[0].1 >= 40.0 - 0.5, "30 pt lines: {tall:?}");
        assert!((tall[1].0 - tall[0].0 - tall[0].1).abs() < 0.5, "{tall:?}");
        // On a ruled page, 12 pt after (16 px) takes one whole rule.
        doc.blocks[0].spacing = ParaSpacing {
            after: 12.0,
            ..ParaSpacing::default()
        };
        let ruled_plain = {
            let mut bare = doc.clone();
            bare.blocks[0].spacing = ParaSpacing::default();
            rows(&mut bare, Some(27.0))
        };
        let ruled = rows(&mut doc, Some(27.0));
        assert!(
            (ruled[1].0 - ruled_plain[1].0 - 27.0).abs() < 0.5,
            "{ruled_plain:?} {ruled:?}"
        );

        // The command sets it on the selected paragraphs, in one undo step.
        let mut doc = Document {
            blocks: vec![
                Block::paragraph("One"),
                Block::paragraph("Two"),
                Block::paragraph("Three"),
            ],
        };
        let (first, second) = (doc.blocks[0].id, doc.blocks[1].id);
        let mut editor = EditorState {
            active_block: Some(second),
            range: Some((
                Pos {
                    block: first,
                    char: 1,
                },
                Pos {
                    block: second,
                    char: 1,
                },
            )),
            ..EditorState::default()
        };
        let spacing = ParaSpacing {
            before: 3.0,
            after: 12.0,
            line_at_least: 0.0,
        };
        let result = apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::Spacing(spacing),
        );
        assert!(result.changed);
        let spaced: Vec<ParaSpacing> = doc.blocks.iter().map(|block| block.spacing).collect();
        assert_eq!(spaced, [spacing, spacing, ParaSpacing::default()]);
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        assert!(doc.blocks.iter().all(|block| block.spacing.is_none()));
    }

    #[test]
    fn natural_width_follows_the_text_its_level_marker_and_tags() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let t = Tokens::light();
        let mut widths = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let short = Document {
                blocks: vec![Block::paragraph("Short")],
            };
            let long = Document {
                blocks: vec![Block::paragraph("A much longer line of text")],
            };
            let mut item = Block::bullet("Short");
            item.indent = 1;
            let listed = Document { blocks: vec![item] };
            let mut tagged = Block::paragraph("Short");
            tagged.tags.push(LineTag::Important);
            let tagged = Document {
                blocks: vec![tagged],
            };
            for doc in [&short, &long, &listed, &tagged] {
                widths.push(natural_width(ui.ctx(), doc, t));
            }
        });
        output.textures_delta.clear();
        assert!(widths[1] > widths[0] * 2.0, "{widths:?}");
        assert!(
            (widths[2] - widths[0] - LEVEL_INDENT - MARKER_WIDTH).abs() < 0.5,
            "{widths:?}"
        );
        assert!((widths[3] - widths[0] - TAG_SLOT).abs() < 0.5, "{widths:?}");
    }

    #[test]
    fn tags_reach_left_only_past_their_level() {
        let mut star = Block::paragraph("Star");
        star.tags.push(LineTag::Important);
        let mut todo = Block::todo("Do", false);
        todo.tags.push(LineTag::Question);
        let mut deep = Block::paragraph("Deep");
        deep.tags.push(LineTag::Idea);
        deep.indent = 1;
        let doc = |blocks: Vec<Block>| Document { blocks };
        assert_eq!(tag_lead(&doc(vec![Block::paragraph("Plain")])), 0.0);
        assert_eq!(tag_lead(&doc(vec![star])), TAG_SLOT);
        assert_eq!(tag_lead(&doc(vec![todo])), 2.0 * TAG_SLOT);
        assert_eq!(
            tag_lead(&doc(vec![deep])),
            0.0,
            "one tag fits in a level's indent"
        );
        // A custom tag with a box takes a slot; one with no symbol takes none.
        let mut homework = Block::paragraph("Homework");
        homework.custom_tags.push(CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: false,
        });
        assert_eq!(tag_lead(&doc(vec![homework.clone()])), TAG_SLOT);
        homework.custom_tags[0].symbol = TagSymbol::None;
        assert_eq!(tag_lead(&doc(vec![homework])), 0.0);
    }

    #[test]
    fn every_tag_icon_reads_on_the_dark_page() {
        let (light, dark) = (Tokens::light(), Tokens::dark());
        for tag in LineTag::ALL {
            let color = tag_ink(tag, dark);
            assert!(
                theme::contrast(color, dark.raised) >= 3.0,
                "{tag:?} is {color:?} on the dark page"
            );
            // The light page keeps the desktop notebook's colours.
            assert_eq!(tag_ink(tag, light), tag.color().to_color32());
        }
        // Colours that already read on the dark page are kept.
        let gold = LineTag::Important.color().to_color32();
        assert_eq!(theme::lifted_on(gold, dark.raised, 3.0), gold);
        assert_ne!(
            tag_ink(LineTag::DiscussManager, dark),
            LineTag::DiscussManager.color().to_color32()
        );
    }

    #[test]
    fn a_tag_looks_ticked_only_once_it_is() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let glyphs = |tag: LineTag, checked: bool| -> Vec<String> {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let rect = Rect::from_min_size(Pos2::ZERO, vec2(TAG_SLOT, 24.0));
                paint_tag_icon(ui.painter(), rect, tag, checked, Tokens::light());
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                    _ => None,
                })
                .collect()
        };
        // Fluent's numbered boxes are ticked ones: unticked, the box holds its number.
        assert_eq!(glyphs(LineTag::TodoPriority1, false), ["1"]);
        assert_eq!(glyphs(LineTag::TodoPriority2, false), ["2"]);
        assert_eq!(glyphs(LineTag::TodoPriority2, true), [icons::CHECKBOX_2]);
        // The others carry no tick of their own, and get one when ticked.
        assert_eq!(
            glyphs(LineTag::ClientRequest, false),
            [icons::CLIPBOARD_ERROR]
        );
        assert_eq!(
            glyphs(LineTag::CallBack, true),
            [icons::CALL_FORWARD, icons::CHECK]
        );
    }

    #[test]
    fn the_tag_column_shows_icons_then_custom_symbols_and_a_click_ticks() {
        let mut block = Block::todo("Ring the dentist", false);
        block.tags = vec![LineTag::Important, LineTag::Definition, LineTag::CallBack];
        block.checked_tags = vec![LineTag::CallBack];
        block.custom_tags = vec![
            CustomTag {
                name: "Later".into(),
                symbol: TagSymbol::None,
                font_color: None,
                highlight: None,
                checked: false,
            },
            CustomTag {
                name: "Homework".into(),
                symbol: TagSymbol::Checkbox,
                font_color: None,
                highlight: None,
                checked: false,
            },
        ];
        let column = tag_column(&block);
        let shown: Vec<(&str, bool, bool)> = column
            .iter()
            .map(|slot| (slot.label.as_str(), slot.checked, slot.tick.is_some()))
            .collect();
        // Definition is a highlight, "Later" has no symbol; To Do's box is the fourth slot.
        assert_eq!(
            shown,
            [
                (&*LineTag::Important.label(), false, false),
                (&*LineTag::CallBack.label(), true, true),
                ("Homework", false, true),
            ]
        );
        assert_eq!(tag_slots(&block), 4);
        let id = block.id;
        let mut doc = Document {
            blocks: vec![block],
        };
        let mut editor = EditorState::default();
        let ctx = egui::Context::default();
        let click = |tick: TagTick| vec![BlockAction::ToggleTagChecked(id, tick)];
        assert!(apply_block_actions(
            &ctx,
            &mut doc,
            "n",
            &mut editor,
            click(TagTick::Line(LineTag::CallBack))
        ));
        assert!(doc.blocks[0].checked_tags.is_empty());
        assert_eq!(doc.blocks[0].tags.len(), 3, "unticking keeps the tag");
        assert!(apply_block_actions(
            &ctx,
            &mut doc,
            "n",
            &mut editor,
            click(TagTick::Custom("Homework".into()))
        ));
        assert!(doc.blocks[0].custom_tags[1].checked);
        assert_eq!(editor.undo_stack.len(), 2, "one undo step a click");
        // A tag the paragraph no longer has is left alone.
        assert!(!apply_block_actions(
            &ctx,
            &mut doc,
            "n",
            &mut editor,
            click(TagTick::Line(LineTag::Phone))
        ));
        assert_eq!(doc.blocks[0].tags.len(), 3);
    }

    #[test]
    fn a_custom_tags_colours_reach_its_text_but_a_runs_own_colour_wins() {
        let t = Tokens::light();
        let mut block = Block::paragraph("sun moons");
        block.custom_tags.push(CustomTag {
            name: "Warm".into(),
            symbol: TagSymbol::None,
            font_color: Some(Rgb(0xc0, 0x00, 0x00)),
            highlight: Some(Rgb::HIGHLIGHTERS[1]),
            checked: false,
        });
        let look = TagLook::of(&block);
        let red = InlineStyle {
            color: Some(Rgb(0x00, 0x70, 0xc0)),
            ..InlineStyle::default()
        };
        let runs = [StyleRun {
            start: 4,
            end: 9,
            style: red,
        }];
        let job = layout_job(
            "sun moons",
            &runs,
            &BlockKind::Paragraph,
            false,
            false,
            look,
            Align::LEFT,
            400.0,
            t,
            &[],
        );
        let formats: Vec<(Color32, Color32)> = job
            .sections
            .iter()
            .map(|section| (section.format.color, section.format.background))
            .collect();
        let highlight = Rgb::HIGHLIGHTERS[1].to_color32();
        assert_eq!(
            formats,
            [
                (Rgb(0xc0, 0x00, 0x00).to_color32(), highlight),
                (Rgb(0x00, 0x70, 0xc0).to_color32(), highlight),
            ]
        );
        // A built-in highlight comes before a custom one.
        block.tags.push(LineTag::RememberForLater);
        assert_eq!(
            TagLook::of(&block).background,
            LineTag::RememberForLater.background().map(Rgb::to_color32)
        );
    }

    #[test]
    fn list_items_show_their_own_marker_and_wide_numbers_widen_the_slot() {
        let mut numbered = Block::new(BlockKind::Numbered);
        numbered.indent = 1;
        assert_eq!(numbered.list_marker(), Some(ListMarker::LowerAlpha));
        numbered.marker = Some(ListMarker::ParenDecimal);
        assert_eq!(numbered.list_marker(), Some(ListMarker::ParenDecimal));
        // A bullet style picked for a numbered item is not its marker.
        numbered.marker = Some(ListMarker::Star);
        assert_eq!(numbered.list_marker(), Some(ListMarker::LowerAlpha));
        assert_eq!(Block::paragraph("Plain").list_marker(), None);

        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let t = Tokens::light();
        let mut widths = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut wide = Block::new(BlockKind::Numbered);
            wide.marker = Some(ListMarker::ParenDecimal);
            let mut dash = Block::bullet("Dash");
            dash.marker = Some(ListMarker::Dash);
            let numbered = Block::new(BlockKind::Numbered);
            widths.push(marker_width(ui.ctx(), &Block::bullet("Dot"), 1, t));
            widths.push(marker_width(ui.ctx(), &dash, 1, t));
            widths.push(marker_width(ui.ctx(), &numbered, 9, t));
            widths.push(marker_width(ui.ctx(), &wide, 128, t));
        });
        output.textures_delta.clear();
        assert_eq!(widths[..3], [MARKER_WIDTH; 3], "{widths:?}");
        assert!(widths[3] > MARKER_WIDTH, "{widths:?}");
    }

    /// Text put in while a caret move still waits (More Symbols… stays up between its
    /// inserts, and the caret moves only once it closes) goes in where the caret will be,
    /// so each insert follows the one before.
    #[test]
    fn inserts_follow_a_caret_move_still_waiting() {
        let ctx = egui::Context::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("ab")],
        };
        let id = doc.blocks[0].id;
        let mut editor = EditorState {
            active_block: Some(id),
            ..EditorState::default()
        };
        editor.focus_block(id, 1);
        assert!(insert_text(&ctx, &mut doc, "page:1", &mut editor, "€"));
        assert!(insert_text(&ctx, &mut doc, "page:1", &mut editor, "£"));
        assert_eq!(doc.blocks[0].text.text, "a€£b");
    }

    fn laid_out(job: LayoutJob) -> std::sync::Arc<egui::Galley> {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut galley = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            galley = Some(ui.fonts_mut(|fonts| fonts.layout_job(job.clone())));
        });
        output.textures_delta.clear();
        galley.expect("laid out")
    }

    #[test]
    fn squiggles_follow_wrapped_centred_rows_and_skip_the_word_being_typed() {
        let text = "Teh quick brwn fox jumps over teh lazy dgo and then agian";
        let mut styled = StyledText {
            text: text.to_string(),
            runs: Vec::new(),
        };
        // A raised first letter must not lift the line into the text.
        styled.apply_style_chars(0..1, |style| style.superscript = true);
        let job = layout_job(
            text,
            &styled.runs,
            &BlockKind::Paragraph,
            false,
            false,
            TagLook::default(),
            Align::Center,
            150.0,
            Tokens::light(),
            &[],
        );
        let galley = laid_out(job);
        assert!(galley.rows.len() >= 3, "the paragraph wraps");
        assert!(galley.rect.left() < 0.0, "centred rows start left of 0");
        let at = |word: &str| text.find(word).unwrap()..text.find(word).unwrap() + word.len();
        let ranges = [at("Teh"), at("brwn"), at("dgo"), at("agian")];
        let origin = pos2(300.0, 40.0) - vec2(galley.rect.left(), 0.0);
        let segments = squiggle_segments(&galley, origin, &ranges, None);
        assert_eq!(segments.len(), ranges.len());
        for (range, &(left, right, y)) in ranges.iter().zip(&segments) {
            // Under the word, exactly where the caret would be on either side of it.
            let start = galley.pos_from_cursor(CCursor {
                prefer_next_row: true,
                ..CCursor::new(range.start)
            });
            let end = galley.pos_from_cursor(CCursor::new(range.end));
            assert!((left - (origin.x + start.left())).abs() < 0.5, "{range:?}");
            assert!((right - (origin.x + end.left())).abs() < 0.5, "{range:?}");
            assert!(y > origin.y + start.center().y && y < origin.y + start.bottom() + 1.0);
        }
        let first_row = &galley.rows[0];
        let raised = origin.y + first_row.pos.y + first_row.glyphs[0].pos.y + 2.5;
        assert!(
            segments[0].2 > raised + 1.0,
            "the superscript's baseline is ignored"
        );

        // A range over the whole text gets one line per row.
        let rows = squiggle_segments(
            &galley,
            origin,
            std::slice::from_ref(&(0..text.len())),
            None,
        );
        assert_eq!(rows.len(), galley.rows.len());
        assert!(rows.windows(2).all(|pair| pair[1].2 > pair[0].2));

        // The word being typed (the edit's caret is in it, or right after it) is skipped.
        let typing = squiggle_segments(&galley, origin, &ranges, Some(ranges[1].end));
        assert_eq!(typing, [segments[0], segments[2], segments[3]]);
        let after = squiggle_segments(&galley, origin, &ranges, Some(ranges[1].end + 1));
        assert_eq!(after, segments);
    }

    #[test]
    fn find_marks_paint_backgrounds_and_the_current_match_is_stronger() {
        let t = Tokens::light();
        let job = layout_job(
            "été one été",
            &[],
            &BlockKind::Paragraph,
            false,
            false,
            TagLook::default(),
            Align::LEFT,
            400.0,
            t,
            &[(0..3, false), (8..11, true)],
        );
        let parts: Vec<_> = job
            .sections
            .iter()
            .map(|section| {
                (
                    // egui 0.37 sections carry `ByteIndex` ranges.
                    &job.text[section.byte_range.start.0..section.byte_range.end.0],
                    section.format.background,
                )
            })
            .collect();
        assert_eq!(
            parts,
            vec![
                ("été", theme::find_highlight(t, false)),
                (" one ", Color32::TRANSPARENT),
                ("été", theme::find_highlight(t, true)),
            ]
        );
    }

    #[test]
    fn find_marks_on_highlighted_text_keep_light_ink_in_dark_mode() {
        let dark = Tokens::dark();
        let highlighted = InlineStyle {
            highlight: Some(Rgb::HIGHLIGHTERS[1]),
            ..InlineStyle::default()
        };
        let runs = [StyleRun {
            start: 0,
            end: 9,
            style: highlighted,
        }];
        let tag = LineTag::RememberForLater.background().map(Rgb::to_color32);
        for (runs, tag) in [(&runs[..], None), (&[][..], tag)] {
            let job = layout_job(
                "sun moons",
                runs,
                &BlockKind::Paragraph,
                false,
                false,
                TagLook {
                    background: tag,
                    color: None,
                },
                Align::LEFT,
                400.0,
                dark,
                &[(4..8, true)],
            );
            let parts: Vec<_> = job
                .sections
                .iter()
                .map(|section| {
                    (
                        &job.text[section.byte_range.start.0..section.byte_range.end.0],
                        section.format.background,
                        section.format.color,
                    )
                })
                .collect();
            // Outside the match the highlighter keeps its dark ink; the match sits on
            // the dark find colour, so it takes the page's light ink again.
            assert_eq!(
                parts[1],
                ("moon", theme::find_highlight(dark, true), dark.ink),
                "{parts:?}"
            );
            assert_eq!(parts[0].2, Rgb::INKS[0].to_color32(), "{parts:?}");
            assert_eq!(parts[2].2, Rgb::INKS[0].to_color32(), "{parts:?}");
        }
    }

    #[test]
    fn backspace_after_a_divider_removes_the_divider_not_the_text() {
        let mut editor = EditorState::default();
        let text = Block::paragraph("hello");
        let id = text.id;
        let mut doc = Document {
            blocks: vec![Block::paragraph("a"), Block::new(BlockKind::Divider), text],
        };
        assert!(backspace_start(&mut doc, &mut editor, id));
        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.plain_text(), "a\nhello");
        assert_eq!(editor.active_block, Some(id));
    }

    #[test]
    fn list_buttons_toggle_and_media_blocks_are_left_alone() {
        let mut editor = EditorState::default();
        let mut doc = Document {
            blocks: vec![
                Block::todo("milk", true),
                Block::new(BlockKind::Image {
                    attachment: "x".into(),
                    width: 1.0,
                    meta: crate::model::PictureMeta::default(),
                }),
            ],
        };
        doc.blocks[0].indent = 2;
        assert!(set_block_kind(
            &mut doc,
            &mut editor,
            0,
            BlockKind::Todo { checked: false },
            0.0
        ));
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::Paragraph,
            "same kind toggles off"
        );
        assert_eq!(doc.blocks[0].indent, 2, "the paragraph keeps its level");
        assert!(!set_block_kind(
            &mut doc,
            &mut editor,
            1,
            BlockKind::Bullet,
            0.0
        ));
        assert!(matches!(doc.blocks[1].kind, BlockKind::Image { .. }));
    }

    #[test]
    fn enter_on_an_empty_nested_item_outdents_before_leaving_the_list() {
        let mut editor = EditorState::default();
        let mut item = Block::new(BlockKind::Bullet);
        item.indent = 1;
        let id = item.id;
        let mut doc = Document { blocks: vec![item] };
        split_block(&mut doc, &mut editor, id, 0, 0, 0.0);
        assert_eq!((doc.blocks[0].indent, doc.blocks.len()), (0, 1));
        split_block(&mut doc, &mut editor, id, 0, 0, 0.0);
        assert_eq!(doc.blocks[0].kind, BlockKind::Paragraph);
    }

    #[test]
    fn typing_after_undo_clears_redo() {
        let mut editor = EditorState::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("one")],
        };
        editor.push_undo(&doc, 0.0);
        doc.blocks[0].text.text = "two".into();
        assert!(editor.undo(&mut doc));
        assert!(!editor.redo_stack.is_empty());
        let before = doc.clone();
        doc.blocks[0].text.text = "three".into();
        editor.remember_if_needed(&before, 0.1);
        assert!(editor.redo_stack.is_empty());
    }

    #[test]
    fn font_sizes_step_through_notebooks_list() {
        assert_eq!(step_size(11.0, true), 12);
        assert_eq!(
            step_size(
                px_to_pt(kind_defaults(&BlockKind::Heading { level: 1 }).0),
                true
            ),
            22,
            "from a title's own size"
        );
        assert_eq!(
            step_size(13.0, false),
            12,
            "between entries goes to the next one down"
        );
        assert_eq!(step_size(8.0, false), 8);
        assert_eq!(step_size(72.0, true), 72);
    }

    #[test]
    fn bold_italic_and_scripts_render() {
        let t = Tokens::light();
        let both = InlineStyle {
            bold: true,
            italic: true,
            ..Default::default()
        };
        let format = text_format(&both, &BlockKind::Paragraph, false, false, None, t);
        assert_eq!(format.font_id.family, theme::named("body-bold-italic"));
        assert!(!format.italics, "Carlito has a bold italic of its own");
        let sup = InlineStyle {
            superscript: true,
            ..Default::default()
        };
        let format = text_format(&sup, &BlockKind::Paragraph, false, false, None, t);
        assert!(format.font_id.size < BODY_SIZE);
        assert!(format.line_height.is_none() && format.valign == Align::TOP);
        let big = InlineStyle {
            size: Some(36),
            font: Some(FontFace::Sans),
            ..Default::default()
        };
        let format = text_format(&big, &BlockKind::Paragraph, false, false, None, t);
        assert_eq!(format.font_id.size, 48.0, "36 pt at 96 dpi");
        assert_eq!(format.font_id.family, egui::FontFamily::Proportional);
    }

    #[test]
    fn only_finished_or_pasted_addresses_link_themselves() {
        // A space typed after an address links it, even right before another space.
        let (old, new) = ("www.x.org b", "www.x.org  b");
        let mut text = StyledText::new(new);
        autolink_inserted(&mut text, inserted_chars(old, new, Some(10)), false);
        assert_eq!(text.link_at(0).map(|(range, _)| range), Some(0..9));

        // Two keystrokes in one frame inside an address link nothing yet.
        let (old, new) = ("www.example", "www.example.co");
        let mut text = StyledText::new(new);
        autolink_inserted(&mut text, inserted_chars(old, new, Some(14)), false);
        assert!(text.runs.is_empty(), "{text:#?}");

        // A paste links every address in it, the last one too.
        let (old, new) = ("a ", "a www.x.org and https://y.org");
        let mut text = StyledText::new(new);
        autolink_inserted(&mut text, inserted_chars(old, new, Some(29)), true);
        assert_eq!(text.link_at(2).map(|(range, _)| range), Some(2..11));
        assert_eq!(text.link_at(20).map(|(range, _)| range), Some(16..29));
        assert_eq!(text.link_at(13), None);
        assert_eq!(inserted_chars("abc", "ac", Some(1)), 1..1, "a deletion");
    }

    #[test]
    fn formatting_follows_the_caret_into_a_table_cell() {
        let ctx = egui::Context::default();
        let mut table = crate::model::TableBlock::new(2, 2);
        table.rows[0][1].text = StyledText::new("plan");
        let table = Block::new(BlockKind::Table(table));
        let id = table.id;
        let mut doc = Document {
            blocks: vec![Block::paragraph("intro"), table],
        };
        let mut editor = EditorState {
            active_block: Some(id),
            active_cell: Some((0, 1)),
            selection: Some(0..4),
            ..Default::default()
        };
        assert_eq!(
            editor.caret_widget(&doc, "note"),
            Some(table_cell_id("note", id, 0, 1))
        );
        let cell = |doc: &Document| match &doc.blocks[1].kind {
            BlockKind::Table(table) => table.rows[0][1].clone(),
            kind => panic!("{kind:?}"),
        };

        // Bold and the alignment go to the cell, not to the table block.
        let bold = ToolbarCommand::Toggle(Mark::Bold);
        assert!(apply_command(&ctx, &mut doc, &mut editor, bold).changed);
        assert!(cell(&doc).text.style_at(0).bold, "{:#?}", cell(&doc));
        let center = ToolbarCommand::Align(ParaAlign::Center);
        assert!(apply_command(&ctx, &mut doc, &mut editor, center).changed);
        assert_eq!(cell(&doc).align, ParaAlign::Center);
        assert_eq!(doc.blocks[1].align, ParaAlign::Left);
        let state = text_state(&doc, &editor).expect("the cell is formatted");
        assert!(state.mark(Mark::Bold));
        assert_eq!(state.align, ParaAlign::Center);
        assert!((state.pt - 11.0).abs() < 0.01, "{}", state.pt);

        // The table keeps its kind and takes no tags.
        let bullets = ToolbarCommand::SetKind(BlockKind::Bullet);
        assert!(!apply_command(&ctx, &mut doc, &mut editor, bullets).changed);
        let tag = ToolbarCommand::Tag(TagCommand::Toggle(LineTag::ALL[0]));
        assert!(!apply_command(&ctx, &mut doc, &mut editor, tag).changed);
        assert!(matches!(doc.blocks[1].kind, BlockKind::Table(_)));
        assert!(doc.blocks[1].tags.is_empty());

        // Text inserted at the caret (Insert › Date) goes into the cell.
        editor.selection = None;
        assert!(insert_at_caret(
            &mut doc,
            "note",
            &mut editor,
            "!",
            (4, 4),
            0.0
        ));
        assert_eq!(cell(&doc).text.text, "plan!");
        assert!(matches!(
            editor.pending_focus,
            Some(FocusRequest::Cell { id: block, row: 0, col: 1, ref range }) if block == id && *range == (5..5)
        ));

        // Clear Formatting with no selection clears the whole cell, its alignment too.
        assert!(clear_formatting(&mut doc, &mut editor, 1.0));
        assert!(!cell(&doc).text.style_at(0).bold);
        assert_eq!(cell(&doc).align, ParaAlign::Left);
        assert!(matches!(doc.blocks[1].kind, BlockKind::Table(_)));

        // Undo in a cell gives the keyboard back to that cell.
        assert!(editor.undo(&mut doc));
        apply_pending_doc_focus(&ctx, &doc, "note", &mut editor);
        let widget = table_cell_id("note", id, 0, 1);
        assert!(ctx.memory(|memory| memory.has_focus(widget)));
        assert!(has_keyboard(&ctx, &doc, "note", &editor));
        assert_eq!(editor.active_cell, Some((0, 1)));

        // A paragraph taking the caret leaves the table.
        let intro = doc.blocks[0].id;
        editor.focus_block(intro, 0);
        apply_pending_doc_focus(&ctx, &doc, "note", &mut editor);
        assert_eq!(editor.active_cell, None);
        assert_eq!(
            editor.caret_widget(&doc, "note"),
            Some(block_id("note", intro))
        );
    }

    #[test]
    fn table_commands_edit_the_grid_and_keep_the_caret_in_it() {
        let ctx = egui::Context::default();
        let mut doc = Document::default();
        let mut editor = EditorState::default();
        // A new note has only asked for the caret in its empty first line.
        editor.focus_block(doc.blocks[0].id, 0);
        let id = insert_table(&mut doc, "note", &mut editor, 2, 3, 0.0);
        assert_eq!(
            doc.blocks.len(),
            2,
            "the table takes the empty line's place"
        );
        assert_eq!(doc.blocks[0].id, id);
        assert!(
            doc.blocks[1].kind.is_textual(),
            "a line after it to type on"
        );
        assert_eq!(editor.active_cell, Some((0, 0)));
        let info = table_info(&doc, &editor).expect("the caret is in the table");
        assert!(!info.header && !info.hide_borders);
        let grid = |doc: &Document| match &doc.blocks[0].kind {
            BlockKind::Table(table) => table.clone(),
            kind => panic!("{kind:?}"),
        };
        let run = |doc: &mut Document, editor: &mut EditorState, command| {
            table_command(&ctx, doc, "note", editor, command)
        };

        // Select Rows selects, then the next command acts on what it selected.
        editor.active_cell = Some((1, 1));
        assert!(!run(&mut doc, &mut editor, TableCmd::SelectRows));
        let row = CellRange {
            top: 1,
            left: 0,
            bottom: 1,
            right: 2,
        };
        assert_eq!(editor.cell_selection, Some((id, row)));
        let shade = Some(crate::table::SHADES[0]);
        assert!(run(&mut doc, &mut editor, TableCmd::Shading(shade)));
        assert!(grid(&doc).rows[1].iter().all(|cell| cell.shading == shade));
        assert_eq!(grid(&doc).rows[0][0].shading, None);
        assert_eq!(editor.cell_selection, None);

        // Inserts put the caret in the new column or row, same row or column.
        assert!(run(&mut doc, &mut editor, TableCmd::InsertRight));
        assert_eq!(grid(&doc).cols(), 4);
        assert_eq!(editor.active_cell, Some((1, 2)));
        assert!(run(&mut doc, &mut editor, TableCmd::InsertAbove));
        assert_eq!(grid(&doc).rows.len(), 3);
        assert_eq!(editor.active_cell, Some((1, 2)));
        assert!(run(&mut doc, &mut editor, TableCmd::HeaderRow));
        assert!(table_info(&doc, &editor).is_some_and(|info| info.header));

        // Deleting every row deletes the table; the caret goes to the line after it.
        assert!(!run(&mut doc, &mut editor, TableCmd::SelectTable));
        assert!(run(&mut doc, &mut editor, TableCmd::DeleteRows));
        assert_eq!(doc.blocks.len(), 1);
        assert!(doc.blocks[0].kind.is_textual());
        assert_eq!(editor.active_cell, None);
        assert_eq!(editor.pending_block_focus(), Some(doc.blocks[0].id));
        assert_eq!(editor.active_block, Some(doc.blocks[0].id));
        assert_eq!(table_info(&doc, &editor), None);

        // Each command was one undo step: one undo brings the table back.
        assert!(editor.undo(&mut doc));
        assert!(grid(&doc).header);
        assert_eq!(grid(&doc).rows.len(), 3);
    }

    #[test]
    fn clear_formatting_keeps_links_and_enter_never_carries_one() {
        const URL: &str = "https://docs.rs";
        let mut block = Block::paragraph("docs here");
        block.text.set_bold(0..9, true);
        block.text.set_link(0..4, Some(Arc::from(URL)));
        let id = block.id;
        let mut doc = Document {
            blocks: vec![block],
        };
        let mut editor = EditorState {
            active_block: Some(id),
            selection: Some(0..2),
            ..Default::default()
        };

        // A selection loses its formats (as in the desktop notebook) but keeps its link.
        assert!(clear_formatting(&mut doc, &mut editor, 0.0));
        let text = &doc.blocks[0].text;
        assert!(!text.style_at(0).bold && text.style_at(2).bold, "{text:#?}");
        assert_eq!(text.style_at(0).link.as_deref(), Some(URL));
        assert_eq!(text.link_at(3).map(|(range, _)| range), Some(0..4));

        // With nothing selected the whole paragraph does, and its link survives too.
        editor.selection = None;
        assert!(clear_formatting(&mut doc, &mut editor, 0.1));
        let text = &doc.blocks[0].text;
        assert!(
            !text.style_at(2).bold && !text.style_at(6).bold,
            "{text:#?}"
        );
        assert_eq!(text.link_at(0).map(|(range, _)| range), Some(0..4));

        // Enter right after a bold link carries the bold into the next paragraph, never the link.
        let mut block = Block::paragraph("docs");
        block.text.set_bold(0..4, true);
        block.text.set_link(0..4, Some(Arc::from(URL)));
        let id = block.id;
        let mut doc = Document {
            blocks: vec![block],
        };
        let mut editor = EditorState::default();
        assert!(split_block(&mut doc, &mut editor, id, 4, 4, 0.0));
        assert_eq!(doc.blocks.len(), 2);
        let carried = editor.pending_style.clone().expect("the bold is armed");
        assert!(carried.bold && carried.link.is_none(), "{carried:?}");
        assert!(editor.active_style.link.is_none());
        assert_eq!(
            doc.blocks[0].text.link_at(0).map(|(range, _)| range),
            Some(0..4)
        );
    }

    #[test]
    fn tags_toggle_carry_over_on_enter_and_come_off_with_backspace() {
        let mut editor = EditorState::default();
        let block = Block::paragraph("milk");
        let id = block.id;
        let mut doc = Document {
            blocks: vec![block],
        };
        assert!(toggle_line_tag(
            &mut doc,
            &mut editor,
            0,
            LineTag::Question,
            0.0
        ));
        assert!(toggle_line_tag(
            &mut doc,
            &mut editor,
            0,
            LineTag::Important,
            0.0
        ));
        assert_eq!(
            doc.blocks[0].tags,
            [LineTag::Important, LineTag::Question],
            "menu order"
        );
        split_block(&mut doc, &mut editor, id, 4, 4, 0.0);
        assert_eq!(doc.blocks[1].tags, doc.blocks[0].tags, "Enter carries tags");
        let next = doc.blocks[1].id;
        split_block(&mut doc, &mut editor, next, 0, 0, 0.0);
        assert_eq!(
            doc.blocks.len(),
            2,
            "Enter on an empty tagged line removes the tag instead"
        );
        assert!(doc.blocks[1].tags.is_empty());
        // An empty tagged to-do: one Enter leaves a plain line, box and tags together.
        doc.blocks[1].kind = BlockKind::Todo { checked: false };
        doc.blocks[1].tags = vec![LineTag::Phone];
        split_block(&mut doc, &mut editor, next, 0, 0, 0.0);
        assert!(
            doc.blocks.len() == 2
                && doc.blocks[1].kind == BlockKind::Paragraph
                && doc.blocks[1].tags.is_empty(),
            "{:?}",
            doc.blocks[1]
        );
        doc.blocks[1].tags = vec![LineTag::Idea];
        assert!(backspace_start(&mut doc, &mut editor, next));
        assert!(
            doc.blocks[1].tags.is_empty() && doc.blocks.len() == 2,
            "Backspace takes the tag off first"
        );
        doc.blocks[0].kind = BlockKind::Todo { checked: false };
        assert!(clear_line_tags(&mut doc, &mut editor, 0, 0.0));
        assert!(doc.blocks[0].tags.is_empty() && doc.blocks[0].kind == BlockKind::Paragraph);
        assert!(
            !clear_line_tags(&mut doc, &mut editor, 0, 0.0),
            "nothing left to clear"
        );
    }

    #[test]
    fn a_checkable_tag_cycles_apply_tick_remove() {
        let mut editor = EditorState::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("ring the dentist")],
        };
        let mut steps = vec![];
        for _ in 0..3 {
            assert!(cycle_line_tag(
                &mut doc,
                &mut editor,
                0,
                LineTag::CallBack,
                0.0
            ));
            let block = &doc.blocks[0];
            steps.push((block.tags.clone(), block.checked_tags.clone()));
        }
        assert_eq!(
            steps,
            [
                (vec![LineTag::CallBack], vec![]),
                (vec![LineTag::CallBack], vec![LineTag::CallBack]),
                (vec![], vec![]),
            ]
        );
        assert_eq!(editor.undo_stack.len(), 3, "one undo step each");
        // Taking a ticked tag off with the menu takes its tick too.
        cycle_line_tag(&mut doc, &mut editor, 0, LineTag::CallBack, 0.0);
        cycle_line_tag(&mut doc, &mut editor, 0, LineTag::CallBack, 0.0);
        assert!(toggle_line_tag(
            &mut doc,
            &mut editor,
            0,
            LineTag::CallBack,
            0.0
        ));
        assert!(doc.blocks[0].tags.is_empty() && doc.blocks[0].checked_tags.is_empty());
    }

    #[test]
    fn custom_tags_toggle_by_name_and_clear_with_ctrl_0() {
        use crate::model::TagSymbol;
        let mut editor = EditorState::default();
        let homework = CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: Some(Rgb::HIGHLIGHTERS[1]),
            checked: false,
        };
        editor.tags.custom.push(homework.clone());
        editor.tags.custom.push(CustomTag {
            name: "Later".into(),
            symbol: TagSymbol::None,
            ..homework.clone()
        });
        let block = Block::paragraph("algebra");
        let id = block.id;
        let mut doc = Document {
            blocks: vec![block],
        };
        assert!(apply_tag_command(
            &mut doc,
            &mut editor,
            0,
            TagCommand::Custom(0),
            0.0
        ));
        assert_eq!(doc.blocks[0].custom_tags, std::slice::from_ref(&homework));
        // A box ticks with Ctrl+digit; a tag without one only goes on and off.
        assert!(cycle_or_toggle_custom(&mut doc, &mut editor, 0, 0, 0.0));
        assert!(doc.blocks[0].custom_tags[0].checked);
        assert!(cycle_or_toggle_custom(&mut doc, &mut editor, 0, 1, 0.0));
        assert_eq!(doc.blocks[0].custom_tags.len(), 2);
        assert!(!doc.blocks[0].custom_tags[1].checked);
        // Enter carries custom tags along, unticked.
        split_block(&mut doc, &mut editor, id, 7, 7, 0.0);
        let names = |block: &Block| -> Vec<(String, bool)> {
            block
                .custom_tags
                .iter()
                .map(|tag| (tag.name.clone(), tag.checked))
                .collect()
        };
        assert_eq!(
            names(&doc.blocks[1]),
            [("Homework".into(), false), ("Later".into(), false)]
        );
        // Toggled off by name; Backspace takes off a paragraph's only (custom) tags.
        assert!(toggle_custom_tag(&mut doc, &mut editor, 1, 1, 0.0));
        assert_eq!(names(&doc.blocks[1]), [("Homework".into(), false)]);
        let next = doc.blocks[1].id;
        assert!(backspace_start(&mut doc, &mut editor, next));
        assert!(doc.blocks[1].custom_tags.is_empty() && doc.blocks.len() == 2);
        // Ctrl+0 takes off all three: tags, ticks and custom tags.
        doc.blocks[0].tags = vec![LineTag::CallBack];
        doc.blocks[0].checked_tags = vec![LineTag::CallBack];
        assert!(clear_line_tags(&mut doc, &mut editor, 0, 0.0));
        let block = &doc.blocks[0];
        assert!(
            block.tags.is_empty() && block.checked_tags.is_empty() && block.custom_tags.is_empty()
        );
        assert!(!clear_line_tags(&mut doc, &mut editor, 0, 0.0));
        // Customize Tags… is a request for the app, even from a table cell.
        editor.active_cell = Some((0, 0));
        assert!(!apply_tag_command(
            &mut doc,
            &mut editor,
            0,
            TagCommand::Customize,
            0.0
        ));
        assert!(editor.customize_tags_request);
    }

    #[test]
    fn ribbon_commands_format_the_active_paragraph_and_text_state_follows() {
        let ctx = egui::Context::default();
        let mut editor = EditorState::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("milk"), Block::paragraph("eggs")],
        };
        assert!(
            text_state(&doc, &editor).is_none(),
            "nothing to format before a paragraph is active"
        );
        let result = apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::Align(ParaAlign::Center),
        );
        assert!(result.changed && result.interacted);
        assert_eq!(
            doc.blocks[0].align,
            ParaAlign::Center,
            "the first paragraph"
        );
        let state = text_state(&doc, &editor).expect("a paragraph is active");
        assert_eq!(state.align, ParaAlign::Center);
        assert_eq!(state.kind, BlockKind::Paragraph);
        assert_eq!(state.face, FontFace::Body);
        assert!((state.pt - 11.0).abs() < 0.01, "{}", state.pt);
        assert!(state.can_undo && !state.can_redo);
        assert!(!state.mark(Mark::Bold) && !state.highlight && !state.color);

        apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::Toggle(Mark::Bold),
        );
        let state = text_state(&doc, &editor).unwrap();
        assert!(state.mark(Mark::Bold) && !state.mark(Mark::Italic));
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Size(20));
        assert!((text_state(&doc, &editor).unwrap().pt - 20.0).abs() < 0.01);

        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        let state = text_state(&doc, &editor).unwrap();
        assert_eq!(
            state.align,
            ParaAlign::Left,
            "Undo takes the alignment back"
        );
        assert!(!state.can_undo && state.can_redo);

        let result = apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::Tag(TagCommand::FindTags),
        );
        assert!(editor.find_tags_request);
        assert!(
            !result.changed && result.interacted,
            "Find Tags opens the pane and leaves the text alone"
        );
        assert_eq!(
            editor.active_block(&doc).map(|block| block.id),
            Some(doc.blocks[0].id)
        );
    }

    #[test]
    fn indent_commands_change_the_level() {
        let ctx = egui::Context::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("milk"), Block::paragraph("eggs")],
        };
        let mut editor = EditorState {
            active_block: Some(doc.blocks[1].id),
            ..EditorState::default()
        };
        let result = apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Indent(1));
        assert!(result.changed);
        assert_eq!((doc.blocks[0].indent, doc.blocks[1].indent), (0, 1));
        assert_eq!(text_state(&doc, &editor).map(|state| state.level), Some(1));
        let result = apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Indent(-1));
        assert!(result.changed);
        assert_eq!(doc.blocks[1].indent, 0);
        assert!(
            !apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Indent(-1)).changed,
            "nothing goes left of the first level"
        );
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        assert_eq!(doc.blocks[1].indent, 1, "each step is one undo");
    }

    #[test]
    fn restart_and_continue_numbering_set_the_items_start() {
        let ctx = egui::Context::default();
        let mut doc = Document {
            blocks: vec![
                Block::new(BlockKind::Numbered),
                Block::paragraph("break"),
                Block::new(BlockKind::Numbered),
                Block::new(BlockKind::Numbered),
            ],
        };
        let mut editor = EditorState {
            active_block: Some(doc.blocks[2].id),
            ..EditorState::default()
        };
        let numbers = |doc: &Document| crate::model::list_numbers(&doc.blocks);
        assert_eq!(numbers(&doc), [1, 0, 1, 2]);
        let run = |doc: &mut Document, editor: &mut EditorState, start| {
            apply_command(&ctx, doc, editor, ToolbarCommand::ListStart(start)).changed
        };
        assert!(run(&mut doc, &mut editor, Some(ListStart::Continue)));
        assert_eq!(numbers(&doc), [1, 0, 2, 3]);
        assert!(run(&mut doc, &mut editor, Some(ListStart::At(1))));
        assert_eq!(numbers(&doc), [1, 0, 1, 2]);
        assert!(
            !run(&mut doc, &mut editor, Some(ListStart::At(1))),
            "the same start again changes nothing"
        );
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        assert_eq!(numbers(&doc), [1, 0, 2, 3], "each is one undo step");
        // A plain paragraph has no count.
        editor.active_block = Some(doc.blocks[1].id);
        assert!(!run(&mut doc, &mut editor, Some(ListStart::At(5))));
        assert_eq!(doc.blocks[1].start, None);

        // An item started at 5 that leaves the list, then comes back to it, counts on as a
        // new item does: its old start is gone, as it is once the page is reopened.
        editor.active_block = Some(doc.blocks[3].id);
        assert!(run(&mut doc, &mut editor, Some(ListStart::At(5))));
        assert_eq!(numbers(&doc), [1, 0, 2, 5]);
        let marker = |doc: &mut Document, editor: &mut EditorState, marker| {
            apply_command(&ctx, doc, editor, ToolbarCommand::ListMarker(marker)).changed
        };
        assert!(marker(&mut doc, &mut editor, None));
        assert!(marker(&mut doc, &mut editor, Some(ListMarker::Decimal)));
        assert_eq!(doc.blocks[3].start, None);
        assert_eq!(numbers(&doc), [1, 0, 2, 3]);
    }

    /// Customize Numbering…'s OK sets the style and the start together: one Ctrl+Z takes
    /// both back.
    #[test]
    fn customize_numbering_is_one_undo_step() {
        let ctx = egui::Context::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("milk"), Block::paragraph("eggs")],
        };
        let mut editor = EditorState {
            active_block: Some(doc.blocks[1].id),
            ..EditorState::default()
        };
        let command = ToolbarCommand::Numbering(ListMarker::LowerAlpha, ListStart::At(5));
        assert!(apply_command(&ctx, &mut doc, &mut editor, command.clone()).changed);
        let block = &doc.blocks[1];
        assert_eq!(block.kind, BlockKind::Numbered);
        assert_eq!(block.marker, Some(ListMarker::LowerAlpha));
        assert_eq!(block.start, Some(ListStart::At(5)));
        assert_eq!(crate::model::list_numbers(&doc.blocks), [0, 5]);
        assert!(
            !apply_command(&ctx, &mut doc, &mut editor, command).changed,
            "the same again changes nothing"
        );
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        let block = &doc.blocks[1];
        assert_eq!(
            (&block.kind, block.marker, block.start),
            (&BlockKind::Paragraph, None, None),
            "one undo takes the style and the start back"
        );

        // OK on the number an item shows keeps no start: an item added above it later
        // still moves it on. A number it would not get is kept.
        let mut doc = Document {
            blocks: vec![
                Block::new(BlockKind::Numbered),
                Block::new(BlockKind::Numbered),
                Block::new(BlockKind::Numbered),
            ],
        };
        editor.active_block = Some(doc.blocks[1].id);
        let numbering = |doc: &mut Document, editor: &mut EditorState, marker, at| {
            let command = ToolbarCommand::Numbering(marker, ListStart::At(at));
            apply_command(&ctx, doc, editor, command).changed
        };
        assert!(numbering(&mut doc, &mut editor, ListMarker::Decimal, 2));
        assert_eq!(doc.blocks[1].start, None, "2 is its number anyway");
        doc.blocks.insert(0, Block::new(BlockKind::Numbered));
        assert_eq!(crate::model::list_numbers(&doc.blocks), [1, 2, 3, 4]);
        assert!(numbering(&mut doc, &mut editor, ListMarker::LowerAlpha, 3));
        assert_eq!(
            doc.blocks[2].start,
            Some(ListStart::At(3)),
            "a new style counts from 1"
        );
        assert_eq!(crate::model::list_numbers(&doc.blocks), [1, 2, 3, 1]);
    }

    #[test]
    fn the_numbering_library_sets_the_marker_and_none_clears_the_list() {
        let ctx = egui::Context::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("milk"), Block::paragraph("eggs")],
        };
        doc.blocks[1].indent = 2;
        let mut editor = EditorState {
            active_block: Some(doc.blocks[1].id),
            ..EditorState::default()
        };
        assert_eq!(text_state(&doc, &editor).unwrap().marker, None);
        let result = apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::ListMarker(Some(ListMarker::LowerAlpha)),
        );
        assert!(result.changed);
        let block = &doc.blocks[1];
        assert_eq!(block.kind, BlockKind::Numbered);
        assert_eq!(block.marker, Some(ListMarker::LowerAlpha));
        assert_eq!(block.indent, 2, "the item keeps its level");
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::Paragraph,
            "only the active one"
        );
        let state = text_state(&doc, &editor).unwrap();
        assert_eq!(state.marker, Some(ListMarker::LowerAlpha));
        assert_eq!(state.level, 2);

        apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::ListMarker(Some(ListMarker::Star)),
        );
        assert_eq!(doc.blocks[1].kind, BlockKind::Bullet, "a bullet style");
        assert_eq!(doc.blocks[1].marker, Some(ListMarker::Star));

        let result = apply_command(
            &ctx,
            &mut doc,
            &mut editor,
            ToolbarCommand::ListMarker(None),
        );
        assert!(result.changed);
        assert_eq!(doc.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(doc.blocks[1].marker, None);
        assert_eq!(text_state(&doc, &editor).unwrap().marker, None);

        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        assert_eq!(
            (&doc.blocks[1].kind, doc.blocks[1].marker),
            (&BlockKind::Bullet, Some(ListMarker::Star)),
            "one undo step brings the list back"
        );
    }

    #[test]
    fn cut_takes_the_selected_text() {
        let ctx = egui::Context::default();
        let mut doc = Document {
            blocks: vec![Block::paragraph("Hello world")],
        };
        let mut editor = EditorState {
            active_block: Some(doc.blocks[0].id),
            selection: Some(6..11),
            ..EditorState::default()
        };
        assert!(text_state(&doc, &editor).unwrap().has_selection);
        let result = apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Copy);
        assert!(!result.changed, "Copy leaves the text alone");
        assert_eq!(doc.blocks[0].text.text, "Hello world");

        let result = apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Cut);
        assert!(result.changed);
        assert_eq!(doc.blocks[0].text.text, "Hello ");
        assert!(!text_state(&doc, &editor).unwrap().has_selection);
        let copied: Vec<String> = ctx.output(|output| {
            output
                .commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        });
        assert_eq!(copied, ["world", "world"], "Copy, then Cut");
        assert!(
            !apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Cut).changed,
            "nothing left selected to cut"
        );
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Undo);
        assert_eq!(doc.blocks[0].text.text, "Hello world");
    }

    #[test]
    fn copies_keep_their_formatting_for_a_paste_in_folio() {
        let ctx = egui::Context::default();
        let mut first = Block::paragraph("Hello world");
        first
            .text
            .apply_style_chars(6..11, |style| style.bold = true);
        let second = Block::heading(2, "Second");
        let mut table = TableBlock::new(2, 2);
        for (row, col, text) in [(0, 0, "a"), (0, 1, "b"), (1, 0, "c"), (1, 1, "d")] {
            table.rows[row][col].text = StyledText::new(text);
        }
        let table = Block::new(BlockKind::Table(table));
        let (p, h, t) = (first.id, second.id, table.id);
        let mut doc = Document {
            blocks: vec![first, second, table],
        };
        let clip = |editor: &EditorState| editor.rich_clip.clone().expect("a rich clip");

        // Inside one paragraph: the chars, bold kept, joining the line they paste into.
        let mut editor = EditorState {
            active_block: Some(p),
            selection: Some(6..11),
            ..EditorState::default()
        };
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Copy);
        let copied = clip(&editor);
        assert!(copied.inline && std::mem::take(&mut editor.copied));
        assert_eq!(copied.plain, "world");
        assert_eq!(copied.blocks.len(), 1);
        assert_eq!(copied.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(copied.blocks[0].text.text, "world");
        assert!(copied.blocks[0].text.style_at(0).bold);

        // Across paragraphs: each paragraph, cut at the range's ends, its kind kept.
        editor.range = Some((Pos { block: p, char: 6 }, Pos { block: h, char: 3 }));
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Copy);
        let copied = clip(&editor);
        assert!(!copied.inline);
        assert_eq!(copied.plain, "world\nSec");
        let texts: Vec<_> = copied.blocks.iter().map(|b| b.text.text.as_str()).collect();
        assert_eq!(texts, ["world", "Sec"]);
        assert!(copied.blocks[0].text.style_at(0).bold);
        assert_eq!(copied.blocks[1].kind, doc.blocks[1].kind, "still a heading");
        editor.range = None;

        // A cell's chars paste as a paragraph's.
        let mut editor = EditorState {
            active_block: Some(t),
            active_cell: Some((0, 1)),
            selection: Some(0..1),
            ..EditorState::default()
        };
        apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Copy);
        let copied = clip(&editor);
        assert!(copied.inline);
        assert_eq!(copied.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(copied.blocks[0].text.text, "b");

        // Cells selected across the table: a table of those cells; Cut empties them.
        editor.cell_selection = Some((t, CellRange::spanning((0, 0), (1, 0))));
        assert!(apply_command(&ctx, &mut doc, &mut editor, ToolbarCommand::Cut).changed);
        let copied = clip(&editor);
        assert_eq!(copied.plain, "a\nc");
        let BlockKind::Table(part) = &copied.blocks[0].kind else {
            panic!("{:?}", copied.blocks[0].kind);
        };
        assert_eq!((part.rows.len(), part.cols()), (2, 1));
        let BlockKind::Table(table) = &doc.blocks[2].kind else {
            unreachable!()
        };
        assert_eq!(table.plain_text(), "\tb\n\td");

        // A new page keeps the clip, as a paste there still finds it.
        editor.reset();
        assert_eq!(clip(&editor).plain, "a\nc");
    }

    #[test]
    fn blocks_go_in_at_the_caret_as_notebook_puts_them() {
        const DOC: &str = "page:note";
        let image = || {
            Block::new(BlockKind::Image {
                attachment: "picture".into(),
                width: 1.0,
                meta: crate::model::PictureMeta::default(),
            })
        };
        let is_image = |block: &Block| matches!(block.kind, BlockKind::Image { .. });
        let texts = |doc: &Document| -> Vec<String> {
            doc.blocks
                .iter()
                .map(|block| {
                    if is_image(block) {
                        "[image]".to_string()
                    } else {
                        block.text.text.clone()
                    }
                })
                .collect()
        };
        // A note of one paragraph, typed in with the caret at `caret` (or `selected` chars
        // from its start selected), then a picture inserted there.
        let insert = |kind: BlockKind, text: &str, caret: usize, selected: Option<usize>| {
            let ctx = egui::Context::default();
            let mut paragraph = Block::new(kind);
            paragraph.text = StyledText::new(text);
            paragraph.indent = 2;
            let mut doc = Document {
                blocks: vec![paragraph],
            };
            let original = doc.clone();
            let id = doc.blocks[0].id;
            set_focus(&ctx, block_id(DOC, id), caret, selected);
            let mut editor = EditorState {
                active_block: Some(id),
                ..EditorState::default()
            };
            assert!(insert_blocks_at_caret(
                &ctx,
                &mut doc,
                DOC,
                &mut editor,
                vec![image()]
            ));
            (doc, editor, original)
        };
        let caret_in = |doc: &Document, editor: &EditorState| {
            let id = editor.active_block.expect("a paragraph has the caret");
            assert_eq!(
                editor.pending_block_focus(),
                Some(id),
                "the caret goes there"
            );
            doc.block_index(id).expect("in the note")
        };

        // In the middle: the paragraph splits and the picture goes between its halves.
        let (mut doc, mut editor, original) = insert(BlockKind::Paragraph, "BeforeAfter", 6, None);
        assert_eq!(texts(&doc), ["Before", "[image]", "After"]);
        assert_eq!(
            caret_in(&doc, &editor),
            2,
            "at the start of the second half"
        );
        assert!(
            doc.blocks.iter().all(|block| block.indent == 2),
            "the picture and the second half take the paragraph's level"
        );
        assert!(editor.undo(&mut doc));
        assert_eq!(doc, original, "one undo takes the whole insert back");

        // At the end: the picture goes after it, and an empty paragraph after the picture.
        let (doc, editor, _) = insert(BlockKind::Paragraph, "BeforeAfter", 11, None);
        assert_eq!(texts(&doc), ["BeforeAfter", "[image]", ""]);
        assert_eq!(caret_in(&doc, &editor), 2);
        assert_eq!(doc.blocks[1].indent, 2);
        assert_eq!(doc.blocks[2].indent, 2, "typing goes on at the same level");

        // In an empty paragraph: the picture goes before it, and the caret stays there.
        let (doc, editor, original) = insert(BlockKind::Paragraph, "", 0, None);
        assert_eq!(texts(&doc), ["[image]", ""]);
        assert_eq!(doc.blocks[1].id, original.blocks[0].id);
        assert_eq!(caret_in(&doc, &editor), 1);

        // A selection is replaced, as typing replaces it.
        let (doc, editor, _) = insert(BlockKind::Paragraph, "BeforeAfter", 0, Some(6));
        assert_eq!(texts(&doc), ["[image]", "After"]);
        assert_eq!(caret_in(&doc, &editor), 1);

        // A To Do split in two: its second half starts unchecked.
        let (doc, _, _) = insert(BlockKind::Todo { checked: true }, "BeforeAfter", 6, None);
        assert_eq!(texts(&doc), ["Before", "[image]", "After"]);
        assert_eq!(doc.blocks[0].kind, BlockKind::Todo { checked: true });
        assert_eq!(doc.blocks[2].kind, BlockKind::Todo { checked: false });
    }

    #[test]
    fn an_equation_block_edits_its_source_and_a_blank_one_goes() {
        let equation = |source: &str| {
            Block::new(BlockKind::Equation {
                source: source.into(),
                scale: crate::model::default_equation_size(),
            })
        };
        let set_source = |doc: &mut Document, index: usize, text: &str| {
            if let BlockKind::Equation { source, .. } = &mut doc.blocks[index].kind {
                *source = text.into();
            }
        };
        let doc = Document {
            blocks: vec![Block::paragraph("Area"), equation("x^2")],
        };
        let id = doc.blocks[1].id;

        // It is kept as typed and found by search.
        let (back, lossy) = Document::from_json_lossy(&doc.to_json());
        assert!(!lossy);
        assert_eq!(back, doc, "an equation block round-trips");
        assert_eq!(doc.plain_text(), "Area\nx^2");

        // Its whole edit is one undo step.
        let mut edited = doc.clone();
        let mut editor = EditorState::default();
        editor.begin_equation(&edited, id, 1.0);
        assert_eq!(editor.equation_block, Some(id));
        assert_eq!(editor.active_block, Some(id));
        set_source(&mut edited, 1, "x^");
        set_source(&mut edited, 1, "x^3");
        assert_eq!(
            equation_latex(&edited, &editor),
            Some(math::latex_of("x^3"))
        );
        assert!(editor.undo(&mut edited));
        assert_eq!(edited, doc, "one undo takes the whole edit back");
        assert_eq!(editor.equation_block, None, "and ends it");

        // Enter ends it, and typing goes on in a new paragraph below it.
        let mut edited = doc.clone();
        let mut editor = EditorState::default();
        editor.begin_equation(&edited, id, 1.0);
        assert!(finish_equation(&mut edited, &mut editor, id, true));
        assert_eq!(editor.equation_block, None);
        assert_eq!(edited.blocks.len(), 3);
        assert_eq!(edited.blocks[1], doc.blocks[1], "the equation stays");
        assert_eq!(edited.blocks[2].kind, BlockKind::Paragraph);
        assert_eq!(editor.active_block, Some(edited.blocks[2].id));
        assert_eq!(editor.pending_block_focus(), Some(edited.blocks[2].id));

        // Emptied, it goes, and undo brings it back as it was.
        let mut edited = doc.clone();
        let mut editor = EditorState::default();
        editor.begin_equation(&edited, id, 1.0);
        set_source(&mut edited, 1, "  ");
        assert!(finish_equation(&mut edited, &mut editor, id, false));
        assert_eq!(edited.blocks.len(), 1);
        assert_eq!(edited.blocks[0], doc.blocks[0]);
        assert_eq!(
            editor.pending_block_focus(),
            None,
            "a click elsewhere keeps the keyboard where it went"
        );
        assert!(editor.undo(&mut edited));
        assert_eq!(edited, doc);
    }
}
