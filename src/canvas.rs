#![allow(clippy::too_many_arguments)]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Local, TimeZone};
use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, DragPanButtons, Event, Id,
    Key, LayerId, Layout, Margin, Modifiers, PointerButton, Pos2, Rangef, Rect, RichText, Scene,
    Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, Vec2,
};
use serde::{Deserialize, Serialize};

use crate::audio;
use crate::db::{DbResult, Library};
use crate::editor::{self, AttachmentAction, EditorState, MediaView, ToolbarCommand};
use crate::find::{self, FindState, MatchPlace, MilestoneField};
use crate::i18n::{self, t};
use crate::icons;
use crate::images::{self, TextureCache};
use crate::math;
use crate::model::{
    Block, BlockKind, CanvasDoc, CanvasItem, CanvasKind, Document, InkStroke, Milestone, Page,
    PictureMeta, PrintoutPage, Rgb, ShapeKind, ITEM_TOP, SHAPE_LINE,
};
use crate::pdf;
use crate::ribbon::{Cmd, PasteMode};
use crate::theme::{self, Tokens};
use crate::util;
use crate::widgets;

const TEXT_WIDTH: f32 = 420.0;
pub use crate::model::{PAGE_CONTENT_TOP, PAGE_LEFT, TITLE_TOP};

/// The page header (the desktop notebook's title, its rule and the date), in page units at 100 %.
/// The title is `theme::TITLE_SIZE` (20 pt) tall text in this box.
const TITLE_HEIGHT: f32 = 36.0;
const TITLE_RULE_Y: f32 = TITLE_TOP + TITLE_HEIGHT + 10.0;
/// The rule under the title is at least this wide, and wider under a long title.
pub(crate) const TITLE_RULE_MIN: f32 = 400.0;
const DATE_TOP: f32 = TITLE_RULE_Y + 8.0;
const DATE_SIZE: f32 = 13.0;
/// Between the date and the time, as the desktop notebook spaces them.
const TIME_GAP: f32 = 28.0;
const FRISE_WIDTH: f32 = 760.0;
const FRISE_HEIGHT: f32 = 210.0;
/// The radius of a frise's milestone dots.
const FRISE_DOT: f32 = 7.0;
/// The space between a milestone's date label and its dot.
const FRISE_DOT_GAP: f32 = 6.0;
/// The space between a milestone's dot and its note.
const FRISE_NOTE_GAP: f32 = 9.0;
/// The space between a milestone's title and its date label.
const FRISE_ROW_GAP: f32 = 2.0;
/// The space kept inside a frise's frame.
const FRISE_PAD: f32 = 10.0;
/// The smallest frise: room for a title, a date, the line and a two-line note.
pub(crate) const FRISE_MIN: Vec2 = Vec2::new(240.0, 112.0);
/// Height of the grab bar above a selected or hovered container, in screen points. With
/// its 2 px gap it fits in the `ITEM_TOP` kept above every item.
const BAR_HEIGHT: f32 = ITEM_TOP - 2.0;
/// A note's grab bar: a thin grey strip along the top of its frame, as in the desktop notebook.
const NOTE_BAR: f32 = 10.0;
/// Picture › Crop's handles: how long their bars are, in screen points (their hit area
/// reaches as far past the picture), and the least a crop keeps each way, in page units.
const CROP_HANDLE: f32 = 12.0;
const CROP_MIN: f32 = 8.0;
/// The crop handles, clockwise from the top-left corner, by the sides of the kept part
/// each moves: -1 the left or top, 1 the right or bottom, 0 neither.
const CROP_SIDES: [(i8, i8); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
];
/// Room between a note's frame and its text. The first note's text starts under the
/// title's first letter, `NOTE_PAD_X` right of its frame.
pub(crate) const NOTE_PAD_X: f32 = 10.0;
const NOTE_PAD_TOP: f32 = 4.0;
const NOTE_PAD_BOTTOM: f32 = 6.0;
/// A new, empty note's width; it widens as you type (P4.8).
const NOTE_MIN_WIDTH: f32 = 96.0;
/// Width of the find-on-page bar, in screen points.
const FIND_WIDTH: f32 = 380.0;
/// How far the page zooms out and in.
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.2..=4.0;
/// Pictures are inserted no larger than this, keeping their proportions.
const PICTURE_MAX: Vec2 = Vec2::new(480.0, 360.0);
/// Space around an equation inside its box, and the smallest box (so it stays easy to grab).
const EQUATION_PAD: f32 = 6.0;
const EQUATION_MIN: Vec2 = Vec2::new(64.0, 36.0);
/// Attached files are shown as a card this size.
const FILE_SIZE: Vec2 = Vec2::new(260.0, 56.0);
/// Space between a printout's attachment icon and its pages, and between pages.
const PRINTOUT_GAP: f32 = 12.0;
/// Pages wider than this (posters) are shown scaled down to it.
const PRINTOUT_MAX_WIDTH: f32 = 1632.0;
/// Recordings are shown as a card this size.
const AUDIO_SIZE: Vec2 = Vec2::new(300.0, 64.0);
/// How many canvas undo steps a page keeps.
const UNDO_LIMIT: usize = 80;
/// Arrow-key nudges this close together (in seconds) are one undo step.
const NUDGE_GROUP_SECONDS: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasTool {
    /// Type anywhere, select, and move. The default, like the desktop notebook's text cursor.
    Select,
    /// Drag to move around the page (the desktop notebook's panning hand).
    Pan,
    /// Draw around ink and items to select them.
    Lasso,
    Pen,
    Highlighter,
    Eraser,
    Shape(ShapeKind),
    /// Drag on the page to add or remove space (Insert › Insert Space).
    InsertSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraserSize {
    Small,
    Medium,
    Large,
}

/// What the eraser rubs out: whole strokes, or only the ink under it (the desktop notebook's stroke
/// and point erasers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraserMode {
    Stroke,
    Point(EraserSize),
}

impl EraserMode {
    pub const ALL: [EraserMode; 4] = [
        EraserMode::Stroke,
        EraserMode::Point(EraserSize::Small),
        EraserMode::Point(EraserSize::Medium),
        EraserMode::Point(EraserSize::Large),
    ];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            EraserMode::Stroke => t!("eraser-stroke"),
            EraserMode::Point(EraserSize::Small) => t!("eraser-small"),
            EraserMode::Point(EraserSize::Medium) => t!("eraser-medium"),
            EraserMode::Point(EraserSize::Large) => t!("eraser-large"),
        }
    }

    /// How far from the pointer it reaches, in screen points.
    fn reach(self) -> f32 {
        match self {
            EraserMode::Stroke | EraserMode::Point(EraserSize::Medium) => 10.0,
            EraserMode::Point(EraserSize::Small) => 4.0,
            EraserMode::Point(EraserSize::Large) => 20.0,
        }
    }
}

/// A pen or highlighter in Draw's gallery: what the tool draws with.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PenPreset {
    #[serde(default)]
    pub highlighter: bool,
    pub color: Rgb,
    pub width: f32,
}

impl PenPreset {
    /// The gallery a new library starts with, as the desktop notebook's: black, blue, red and green
    /// pens, then bright yellow and lime highlighters.
    pub fn defaults() -> Vec<PenPreset> {
        let pen = |color| PenPreset {
            highlighter: false,
            color,
            width: 2.0,
        };
        let highlighter = |color| PenPreset {
            highlighter: true,
            color,
            width: 3.0,
        };
        vec![
            pen(Rgb(0x00, 0x00, 0x00)),
            pen(Rgb(0x00, 0x78, 0xd4)),
            pen(Rgb(0xe7, 0x12, 0x24)),
            pen(Rgb(0x00, 0x8c, 0x3a)),
            highlighter(Rgb(0xff, 0xff, 0x00)),
            highlighter(Rgb(0x66, 0xcc, 0x00)),
        ]
    }

    /// The thickness it draws at, in millimetres, as its settings and tooltip name it.
    fn millimetres(&self) -> String {
        let mm = PEN_THICKNESS
            .iter()
            .find(|(width, _)| (width - self.width).abs() < 0.01)
            .map_or(self.width * 0.25, |(_, mm)| *mm);
        // A highlighter draws four times as wide as a pen of the same width.
        mm_text(if self.highlighter { mm * 4.0 } else { mm })
    }
}

/// the desktop notebook's pen colours, with the id of each one's name: Draw's Add Pen and Pen settings
/// grids, and the pens' tooltips.
pub const PEN_COLORS: [(Rgb, &str); 16] = [
    (Rgb(0x00, 0x00, 0x00), "ribbon-color-black"),
    (Rgb(0x7a, 0x7a, 0x7a), "ribbon-color-grey"),
    (Rgb(0xe7, 0x12, 0x24), "ribbon-color-red"),
    (Rgb(0xf7, 0x63, 0x0c), "ribbon-color-orange"),
    (Rgb(0xff, 0xc1, 0x14), "ribbon-color-yellow"),
    (Rgb(0x66, 0xcc, 0x00), "ribbon-color-lime"),
    (Rgb(0x00, 0x8c, 0x3a), "ribbon-color-green"),
    (Rgb(0x33, 0xcc, 0xff), "ribbon-color-light-blue"),
    (Rgb(0x00, 0x78, 0xd4), "ribbon-color-blue"),
    (Rgb(0x1b, 0x3d, 0x8c), "ribbon-color-dark-blue"),
    (Rgb(0x77, 0x19, 0xaa), "ribbon-color-purple"),
    (Rgb(0xe3, 0x00, 0x8c), "ribbon-color-magenta"),
    (Rgb(0xff, 0x66, 0xcc), "ribbon-color-pink"),
    (Rgb(0x8d, 0x55, 0x24), "ribbon-color-brown"),
    (Rgb(0xff, 0xff, 0xff), "ribbon-color-white"),
    (Rgb(0xff, 0xff, 0x00), "ribbon-color-bright-yellow"),
];

/// The pen thicknesses Draw › Pen settings offers, as (width, millimetres).
pub const PEN_THICKNESS: [(f32, f32); 5] =
    [(1.0, 0.25), (1.5, 0.35), (2.0, 0.5), (3.0, 0.7), (4.0, 1.0)];

/// "0.5 mm", "1 mm": a pen's thickness, with no trailing zeros.
fn mm_text(mm: f32) -> String {
    let number = i18n::decimal(f64::from(mm), 2);
    // Two decimals always follow the separator: drop the zeros, then the separator.
    let number = number
        .trim_end_matches('0')
        .trim_end_matches(|ch: char| !ch.is_ascii_digit());
    t!("unit-millimetres", size = number).into_owned()
}

/// A pen's tooltip and accessible name: "Pen: Red, 0.5 mm".
pub(crate) fn pen_label(pen: &PenPreset) -> String {
    let color = PEN_COLORS
        .iter()
        .find(|(color, _)| *color == pen.color)
        .map_or_else(
            || {
                let Rgb(r, g, b) = pen.color;
                format!("#{r:02x}{g:02x}{b:02x}")
            },
            |(_, id)| i18n::tr(id).into_owned(),
        );
    let width = pen.millimetres();
    if pen.highlighter {
        t!("ribbon-highlighter-preset", color = color, width = width)
    } else {
        t!("ribbon-pen-preset", color = color, width = width)
    }
    .into_owned()
}

/// Draw's pens from the setting `pens`; the desktop notebook's defaults when it is absent, unreadable
/// or empty.
pub fn load_pens(library: &Library) -> Vec<PenPreset> {
    library
        .setting("pens")
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<Vec<PenPreset>>(&json).ok())
        .filter(|pens| !pens.is_empty())
        .unwrap_or_else(PenPreset::defaults)
}

/// Stores Draw's pens in the setting `pens`.
pub fn save_pens(library: &mut Library, pens: &[PenPreset]) -> DbResult<()> {
    library.set_setting("pens", &serde_json::to_string(pens)?)
}

/// How many colours Draw › pen ▾ › Recent Colors keeps.
const RECENT_PEN_COLORS: usize = 6;

/// Draw's Recent Colors from the setting `recent_pen_colors` ("#rrggbb,#rrggbb"), kept
/// beside the pens.
pub fn load_recent_colors(library: &Library) -> Vec<Rgb> {
    library
        .setting("recent_pen_colors")
        .ok()
        .flatten()
        .map(|list| {
            list.split(',')
                .filter_map(|hex| Rgb::from_hex(hex.trim()))
                .take(RECENT_PEN_COLORS)
                .collect()
        })
        .unwrap_or_default()
}

/// Stores Draw's Recent Colors in the setting `recent_pen_colors`.
pub fn save_recent_colors(library: &mut Library, colors: &[Rgb]) -> DbResult<()> {
    let list: Vec<String> = colors.iter().map(|color| color.hex()).collect();
    library.set_setting("recent_pen_colors", &list.join(","))
}

/// Puts `color`, just picked for a pen, first among the Recent Colors unless it is one of
/// the desktop notebook's 16, keeping six. Returns whether the list changed.
fn remember_pen_color(recent: &mut Vec<Rgb>, color: Rgb) -> bool {
    if PEN_COLORS.iter().any(|(listed, _)| *listed == color) || recent.first() == Some(&color) {
        return false;
    }
    recent.retain(|kept| *kept != color);
    recent.insert(0, color);
    recent.truncate(RECENT_PEN_COLORS);
    true
}

/// Draw › Rotate: turns the selected ink and shapes a quarter turn, or mirrors them.
/// The 45° turns are for ink, lines and arrows only (`turns_45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    Right,
    Left,
    FlipHorizontal,
    FlipVertical,
    Right45,
    Left45,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeMode {
    /// Text containers: only the width; the height follows the text.
    Width,
    Free,
    /// Pictures keep their proportions.
    Aspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragMode {
    Move,
    Resize(ResizeMode),
}

#[derive(Debug, Clone)]
struct DragState {
    mode: DragMode,
    start_pointer: Pos2,
    /// The moving items and where each began; one entry when resizing.
    items: Vec<(u64, Rect)>,
    /// Selected strokes and their points when the drag began.
    strokes: Vec<(usize, Vec<[f32; 2]>)>,
    /// A note Shift+dragged by its bar: dropped on another note, it merges into it.
    merge: bool,
}

impl DragState {
    fn moves(&self, id: u64) -> bool {
        self.items.iter().any(|(item, _)| *item == id)
    }
}

/// A rubber band being dragged on empty paper.
#[derive(Debug, Clone)]
struct Band {
    start: Pos2,
    end: Pos2,
    /// What was selected before (Shift adds to it).
    base: Selection,
}

/// The loop being drawn with the lasso tool, in scene coordinates.
#[derive(Debug, Clone)]
struct Lasso {
    points: Vec<Pos2>,
    base: Selection,
}

/// An Insert Space drag: where it started, its direction once known, and where everything
/// past the line was when the direction was fixed.
#[derive(Debug, Clone)]
struct SpaceDrag {
    line: Pos2,
    /// `Some(true)` moves things down (a horizontal line), `Some(false)` right.
    down: Option<bool>,
    items: Vec<(u64, Pos2)>,
    strokes: Vec<(usize, Vec<[f32; 2]>)>,
    /// How far things may come back before the first reaches the line.
    room: f32,
}

#[derive(Debug, Clone)]
struct TimelineEditor {
    item_id: u64,
    milestones: Vec<Milestone>,
}

/// An equation whose source is being typed, and the canvas from before, so the whole
/// edit becomes one undo step.
#[derive(Debug, Clone)]
struct EquationEdit {
    id: u64,
    before: CanvasDoc,
    /// Focus the source box on the next frame.
    focus: bool,
    /// A change was made while it was typed (see `push_undo`): measure the typing that
    /// follows from the canvas after that change.
    resnap: bool,
}

enum TimelineOutcome {
    Editing(TimelineEditor),
    Save(TimelineEditor),
    Closed,
}

/// Something an item's menu, double-click, ribbon button or Enter asked for. It is
/// carried out after the items are drawn, where the library is at hand.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ItemAction {
    Open,
    SaveAs,
    /// Play or pause a recording.
    TogglePlay,
    /// Jump to a point of a recording, as a fraction of its length.
    Seek(f32),
    /// A printout page: open the PDF it comes from.
    OpenOriginal,
    /// A printout page: copy its text, or the text of every page of its printout.
    CopyPrintoutText {
        all: bool,
    },
    /// An attached PDF: insert it as a printout under its icon.
    InsertPrintout,
}

/// What the Picture tab and the picture menus do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureCmd {
    Background,
    RestoreSize,
    AltText,
    RotateRight,
    RotateLeft,
    FlipHorizontal,
    FlipVertical,
    /// Picture › Crop: starts cropping the selected page picture, or keeps the crop.
    Crop,
}

/// A picture on the page, or one inside a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureTarget {
    Item(u64),
    Block { note: u64, block: u64 },
}

/// The picture the Picture tab acts on: the selected page picture, else the picture in
/// the note being edited that was clicked last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureInfo {
    pub target: PictureTarget,
    pub background: bool,
    /// A printout page, which keeps its printed size and is never cropped.
    pub printout: bool,
    /// Picture › Crop is under way on it.
    pub cropping: bool,
}

/// Picture › Crop under way on a page picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cropping {
    pub item: u64,
    /// The picture's whole (uncropped) rect on the page, in page units.
    pub full: Rect,
    /// The rect being kept, in page units, inside `full`.
    pub keep: Rect,
}

pub fn picture_info(
    page: &Page,
    state: &CanvasState,
    editor_state: &EditorState,
) -> Option<PictureInfo> {
    if let Some(id) = state.selection.primary() {
        if let Some(CanvasKind::Image { meta, printout, .. }) =
            page.canvas.item(id).map(|item| &item.kind)
        {
            return Some(PictureInfo {
                target: PictureTarget::Item(id),
                background: meta.background,
                printout: printout.is_some(),
                cropping: state.cropping.as_ref().is_some_and(|crop| crop.item == id),
            });
        }
    }
    let note = state.active_text?;
    let doc = page.canvas.item(note)?.doc()?;
    let index = editor_state
        .active_block
        .and_then(|block| doc.block_index(block))?;
    matches!(doc.blocks[index].kind, BlockKind::Image { .. }).then_some(PictureInfo {
        target: PictureTarget::Block {
            note,
            block: doc.blocks[index].id,
        },
        background: false,
        printout: false,
        cropping: false,
    })
}

/// The picture properties of a target, to change them.
fn picture_meta_mut(page: &mut Page, target: PictureTarget) -> Option<&mut PictureMeta> {
    match target {
        PictureTarget::Item(id) => match &mut page.canvas.item_mut(id)?.kind {
            CanvasKind::Image { meta, .. } => Some(meta),
            _ => None,
        },
        PictureTarget::Block { note, block } => {
            let doc = page.canvas.item_mut(note)?.doc_mut()?;
            let index = doc.block_index(block)?;
            match &mut doc.blocks[index].kind {
                BlockKind::Image { meta, .. } => Some(meta),
                _ => None,
            }
        }
    }
}

/// The Alt Text dialog, while it is open.
pub struct AltTextEdit {
    pub target: PictureTarget,
    pub text: String,
    /// Give the text box the caret on the first frame.
    pub focus: bool,
}

/// Runs a picture command. One page undo step; Alt Text opens its dialog, whose OK
/// takes the step.
pub fn run_picture_cmd(
    page: &mut Page,
    state: &mut CanvasState,
    library: &Library,
    target: PictureTarget,
    command: PictureCmd,
) -> bool {
    if command == PictureCmd::AltText {
        if let Some(alt) = picture_meta_mut(page, target).map(|meta| meta.alt.clone()) {
            state.alt_text = Some(AltTextEdit {
                target,
                text: alt,
                focus: true,
            });
        }
        return false;
    }
    // Only a page picture can be the page's background; one in a note flows with its text.
    let on_page = matches!(target, PictureTarget::Item(_));
    if picture_meta_mut(page, target).is_none() || (command == PictureCmd::Background && !on_page) {
        return false;
    }
    state.push_undo(&page.canvas);
    change_picture(page, state, library, target, command);
    // Nothing changed (it was at its original size already): no step to take back.
    if state.undo.back() == Some(&page.canvas) {
        state.undo.pop_back();
        return false;
    }
    true
}

/// What a picture command does to the picture, with no undo step of its own: the Picture
/// tab's and the picture menus' commands, and Draw › Rotate on a picture.
fn change_picture(
    page: &mut Page,
    state: &mut CanvasState,
    library: &Library,
    target: PictureTarget,
    command: PictureCmd,
) {
    let Some(meta) = picture_meta_mut(page, target) else {
        return;
    };
    let turned_before = meta.quarter_turned();
    match command {
        PictureCmd::RotateRight => meta.rotate(true),
        PictureCmd::RotateLeft => meta.rotate(false),
        PictureCmd::FlipHorizontal => meta.flip_shown(false),
        PictureCmd::FlipVertical => meta.flip_shown(true),
        PictureCmd::Background => meta.background = !meta.background,
        // Crop is handled before, in `run_ribbon_cmd`.
        PictureCmd::RestoreSize | PictureCmd::AltText | PictureCmd::Crop => {}
    }
    let turned_after = meta.quarter_turned();
    let background = meta.background;
    let shown = meta.clone();
    match target {
        PictureTarget::Item(id) => {
            let attachment = page
                .canvas
                .item(id)
                .and_then(|item| item.kind.attachment().map(str::to_owned));
            // A printout page's own size is its printed size, as it was inserted.
            let printout = page
                .canvas
                .item(id)
                .and_then(|item| item.kind.printout())
                .map(|printout| printout.size);
            let Some(item) = page.canvas.item_mut(id) else {
                return;
            };
            if turned_before != turned_after {
                // A quarter turn swaps the picture's width and height around its centre,
                // and what would end up off the page's top or left edge is brought back
                // onto it, as Draw › Rotate does.
                let center = item.rect().center();
                std::mem::swap(&mut item.width, &mut item.height);
                item.x = (center.x - item.width / 2.0).round().max(0.0);
                item.y = (center.y - item.height / 2.0).round().max(ITEM_TOP);
            }
            if command == PictureCmd::RestoreSize {
                let size = match printout {
                    Some(size) => size.map(|[width, height]| vec2(width, height)),
                    None => {
                        // A cropped picture's original size is its kept part's.
                        let natural = attachment
                            .and_then(|id| library.attachment(&id).ok().flatten())
                            .and_then(|file| {
                                images::upright_size(&library.attachment_path(&file.stored_path))
                            })
                            .map(|natural| shown.cropped_size(natural));
                        Some(picture_size(natural))
                    }
                };
                if let Some(size) = size {
                    let size = if turned_after {
                        vec2(size.y, size.x)
                    } else {
                        size
                    };
                    item.width = size.x;
                    item.height = size.y;
                }
            }
            if command == PictureCmd::Background && background {
                state.selection.clear();
            }
        }
        PictureTarget::Block { note, block } => {
            if command == PictureCmd::RestoreSize {
                if let Some(doc) = page.canvas.item_mut(note).and_then(CanvasItem::doc_mut) {
                    if let Some(index) = doc.block_index(block) {
                        if let BlockKind::Image { width, .. } = &mut doc.blocks[index].kind {
                            *width = 1.0;
                        }
                    }
                }
            }
        }
    }
}

/// Picture › Crop on a page picture: its whole picture is laid out around what it shows,
/// to crop again from. A printout page or a background picture is never cropped.
fn start_crop(page: &Page, state: &mut CanvasState, id: u64) -> bool {
    let Some(item) = page.canvas.item(id) else {
        return false;
    };
    let CanvasKind::Image {
        meta,
        printout: None,
        ..
    } = &item.kind
    else {
        return false;
    };
    let shown = item.rect();
    // A picture with no area has no part to keep (and its crop would not be a number).
    if meta.background || !(shown.width() > 0.0 && shown.height() > 0.0) {
        return false;
    }
    let [left, top, right, bottom] = meta.shown_crop();
    let size = vec2(
        shown.width() / (right - left),
        shown.height() / (bottom - top),
    );
    let full = Rect::from_min_size(
        pos2(shown.min.x - left * size.x, shown.min.y - top * size.y),
        size,
    );
    state.cropping = Some(Cropping {
        item: id,
        full,
        keep: shown,
    });
    true
}

/// The part of `full` that `keep` is, as fractions: left, top, right, bottom.
fn crop_fractions(full: Rect, keep: Rect) -> [f32; 4] {
    [
        (keep.min.x - full.min.x) / full.width(),
        (keep.min.y - full.min.y) / full.height(),
        (keep.max.x - full.min.x) / full.width(),
        (keep.max.y - full.min.y) / full.height(),
    ]
}

/// `meta` showing only the `shown` part (fractions of the whole picture as shown), or the
/// whole picture when that is nearly all of it.
fn cropped_meta(meta: &PictureMeta, shown: [f32; 4]) -> PictureMeta {
    let whole = PictureMeta {
        crop: None,
        ..meta.clone()
    };
    let all = shown[2] - shown[0] >= 0.999 && shown[3] - shown[1] >= 0.999;
    PictureMeta {
        crop: (!all).then(|| whole.source_rect(shown)),
        ..whole
    }
}

/// Ends Picture › Crop keeping the crop: the picture shows the part kept, where it was
/// kept. One undo step; nothing when the crop was left as it was.
fn commit_crop(page: &mut Page, state: &mut CanvasState) -> bool {
    let Some(Cropping {
        item: id,
        full,
        keep,
    }) = state.cropping.take()
    else {
        return false;
    };
    if page.canvas.item(id).is_none_or(|item| item.rect() == keep) {
        return false;
    }
    state.push_undo(&page.canvas);
    let Some(item) = page.canvas.item_mut(id) else {
        return false;
    };
    if let CanvasKind::Image { meta, .. } = &mut item.kind {
        *meta = cropped_meta(meta, crop_fractions(full, keep));
    }
    item.x = keep.min.x;
    item.y = keep.min.y;
    item.width = keep.width();
    item.height = keep.height();
    true
}

/// Files named by pasted text, waiting for the user to say whether to attach them. A
/// paste arrives as plain text, so Folio can't tell a file manager's copy from text
/// copied off a web page or a chat: nothing on the computer joins the notebook unasked.
#[derive(Debug, Clone, PartialEq)]
struct PendingFiles {
    /// Each file and its size in bytes.
    files: Vec<(PathBuf, u64)>,
    /// Where the first one goes: where a menu's Paste was asked for, else the middle of
    /// the view.
    at: Option<Pos2>,
    /// What was pasted, for "Paste as text".
    text: String,
}

/// The answer to "Insert files from your computer?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilesChoice {
    Waiting,
    Attach,
    PasteText,
    Cancel,
}

/// Items and ink copied from a page. The clipboard survives page changes.
#[derive(Debug, Clone, Default, PartialEq)]
struct Clip {
    items: Vec<CanvasItem>,
    ink: Vec<InkStroke>,
}

/// What is selected on the page: whole items and ink strokes. Strokes are indices into
/// `CanvasDoc::ink`; they are dropped whenever the ink list may have changed under them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub items: Vec<u64>,
    pub strokes: Vec<usize>,
}

impl Selection {
    pub fn one(id: u64) -> Self {
        Self {
            items: vec![id],
            strokes: Vec::new(),
        }
    }

    /// The item when exactly one item (and no ink) is selected: what the ribbon, Enter,
    /// resizing and table editing act on.
    pub fn primary(&self) -> Option<u64> {
        match (self.items.as_slice(), self.strokes.is_empty()) {
            ([id], true) => Some(*id),
            _ => None,
        }
    }

    pub fn contains(&self, id: u64) -> bool {
        self.items.contains(&id)
    }

    pub fn set_one(&mut self, id: u64) {
        *self = Self::one(id);
    }

    /// Adds `id`, or takes it out if it was already selected (Shift/Ctrl+click).
    pub fn toggle(&mut self, id: u64) {
        if let Some(index) = self.items.iter().position(|item| *item == id) {
            self.items.remove(index);
        } else {
            self.items.push(id);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.strokes.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.strokes.is_empty()
    }

    /// Items and strokes together.
    pub fn len(&self) -> usize {
        self.items.len() + self.strokes.len()
    }

    /// Several things (or any ink): they move and act together, inside a dashed frame.
    fn is_group(&self) -> bool {
        self.len() > 1 || !self.strokes.is_empty()
    }

    fn remove(&mut self, id: u64) {
        self.items.retain(|item| *item != id);
    }
}

pub struct CanvasState {
    pub tool: CanvasTool,
    pub selection: Selection,
    /// The note whose text is being edited. It stays set while the ribbon is used, so
    /// formatting buttons apply to it, and clears when the page is clicked elsewhere.
    pub active_text: Option<u64>,
    pub focus_title: bool,
    /// The page title is where the caret is: set while it has the keyboard and kept while
    /// the ribbon or a dialog holds it, so Insert › Symbol goes in at its caret. A note
    /// taking the caret, an item picked, or anything else on the page taking the keyboard
    /// ends it.
    pub title_active: bool,
    /// Give the title the keyboard back, its caret where it was (a dialog that typed into
    /// it has closed).
    pub refocus_title: bool,
    /// Set by the app while a dialog, search, or the reader covers the page, so page
    /// shortcuts (Delete, paste, Escape) don't act on items behind it.
    pub covered: bool,
    pub scene_rect: Option<Rect>,
    /// A place to bring into view on the next frame (Find Tags, find on page).
    pub reveal: Option<Reveal>,
    /// The ribbon's Find tags button was pressed.
    pub open_find_tags: bool,
    pub ink_color: Rgb,
    pub ink_width: f32,
    pub shape_kind: ShapeKind,
    pub shape_fill: Option<Rgb>,
    pub eraser: EraserMode,
    /// Ink to Shape: pen strokes that look like shapes become shapes.
    pub ink_to_shape: bool,
    /// Draw's pen and highlighter gallery, as stored in the setting `pens`.
    pub pens: Vec<PenPreset>,
    /// The preset last picked: Draw › Pen settings and the colour and width commands
    /// change it while its tool is on.
    pub pen: Option<usize>,
    /// Draw › pen ▾ › Recent Colors: the last colours picked that are not among
    /// the desktop notebook's 16, newest first, as stored in the setting `recent_pen_colors`.
    pub recent_colors: Vec<Rgb>,
    /// Lock Drawing Mode: the shape tool stays on after each shape, until Esc.
    pub lock_drawing: bool,
    /// Options › Display › Move vertical scroll bar to the left, set by the app.
    pub scrollbar_left: bool,
    drag: Option<DragState>,
    /// The rubber band being dragged on empty paper.
    band: Option<Band>,
    /// The loop being drawn with the lasso tool.
    lasso: Option<Lasso>,
    /// The Insert Space drag in progress.
    space: Option<SpaceDrag>,
    stroke: Option<InkStroke>,
    /// The current eraser drag has recorded its undo step.
    erasing: bool,
    /// Where the eraser was last frame, so a fast swipe erases everything it crossed.
    erase_from: Option<Pos2>,
    shape_draft: Option<(Pos2, Pos2)>,
    timeline_editor: Option<TimelineEditor>,
    editing_label: Option<u64>,
    equation_edit: Option<EquationEdit>,
    /// The item whose corner drag ended last frame (an equation then takes its new size).
    resized: Option<u64>,
    focus_label: bool,
    /// The canvas as it was when a note started being edited (and that note's id), so
    /// the whole edit becomes one canvas undo step.
    text_snapshot: Option<(u64, CanvasDoc)>,
    /// The shape label being typed in whose edit is already one undo step.
    typing: Option<u64>,
    /// When the last arrow-key nudge happened and what it moved, so a burst of them on
    /// the same selection is one undo step.
    last_nudge: Option<(f64, Selection)>,
    clipboard: Option<Clip>,
    /// The text `copy_selection` put on the system clipboard with `clipboard`: a paste of
    /// exactly that text is the page clipboard, whatever language was on when it was copied.
    clipboard_text: Option<String>,
    /// See ItemAction.
    item_action: Option<(u64, ItemAction)>,
    /// A picture command from a picture's menu (on the page or in a note), run once the
    /// items are drawn.
    pub(crate) picture_request: Option<(PictureTarget, PictureCmd)>,
    /// The Alt Text dialog, while it is open.
    pub alt_text: Option<AltTextEdit>,
    /// Picture › Crop, while it is under way.
    pub cropping: Option<Cropping>,
    /// See PendingFiles.
    pending_files: Option<PendingFiles>,
    /// See PrintoutRun. It survives a page change only to be cancelled cleanly there.
    printout: Option<PrintoutRun>,
    /// See PendingPdf.
    pending_pdfs: VecDeque<PendingPdf>,
    /// "Insert Printout" on an attached PDF: the item its pages go under (the file card,
    /// or the note holding the PDF's File block) and the PDF's attachment. It is handled
    /// where the library is writable.
    printout_from_card: Option<(u64, String)>,
    /// The recording in progress. It carries on across pages until it is stopped.
    recording: Option<audio::Recorder>,
    /// The recording being played on this page.
    player: Option<audio::Player>,
    /// Lengths of recordings whose item doesn't know its length, read from the file.
    audio_lengths: HashMap<String, u64>,
    /// Set by the View ribbon's Versions button; the app then opens the page versions.
    pub open_versions: bool,
    /// A ribbon command for this page, run by `show` this frame (the ribbon is drawn
    /// above the panels, before the page).
    pub ribbon_cmd: Option<crate::ribbon::Cmd>,
    /// Where empty paper (or an item) was right-clicked, for "Text box here" and friends.
    menu_at: Option<Pos2>,
    /// A right-click menu's Paste asked the platform for the clipboard: the point it was
    /// asked at, and when. The paste event that answers it (next frame) lands there.
    paste_at: Option<(Pos2, f64)>,
    /// A note container's menu asked for what a paragraph handle does (Select › Level n),
    /// for that note: run where its handles' actions run, next frame.
    note_request: Option<(u64, editor::HandleAction)>,
    /// The insertion point: where empty paper (or the inside of the selection's frame)
    /// was last clicked on this page. Something inserted with no note being typed in
    /// goes into a new note there (`insert_point`).
    last_click: Option<Pos2>,
    /// A note whose text a command changed outside its editor (Insert › Date…): a note
    /// that sizes itself to its text is measured again on its next frame.
    remeasure: Option<u64>,
    /// The part of the scene visible last frame, in scene coordinates.
    visible: Option<Rect>,
    /// The layer Scene draws the page in (last frame's), to tell the page from what is
    /// drawn over it.
    scene_layer: Option<LayerId>,
    /// Where the scene is drawn on screen.
    viewport: Rect,
    /// Each page's last view this session (its scroll and zoom, with the page area it was
    /// shown in), so going back to a page shows it as you left it.
    views: HashMap<String, (Rect, Rect)>,
    zoom: f32,
    pan: Vec2,
    undo: VecDeque<CanvasDoc>,
    redo: Vec<CanvasDoc>,
    page_id: String,
    /// The find-on-page bar (Ctrl+F), while open.
    pub find: Option<FindState>,
    /// Width of the title text last frame, for the title field and its rule.
    title_width: f32,
    /// The header's date and time, formatted once per page date and language.
    header: HeaderText,
    /// Where the header is on the page this frame (`None` while it is hidden): a click on
    /// empty paper there puts the caret in the title instead of starting a note.
    header_rect: Option<Rect>,
    /// The month the page-date calendar shows while it is open.
    calendar: Option<(i32, u32)>,
    /// How far each note's tags hang left of its text (`editor::tag_lead`), as last drawn.
    /// When it changes, the note's frame moves so its text stays where it was.
    note_leads: HashMap<u64, f32>,
    /// The page's far corner (`content_far`) and the item and stroke counts it was
    /// measured with: `cached_far`.
    far: Option<(usize, usize, Pos2)>,
}

#[derive(Debug, Clone, Default)]
struct HeaderText {
    created_at: i64,
    lang: Option<i18n::Lang>,
    date: String,
    time: String,
    /// The tooltips (and accessible names) of the date and the time.
    date_tip: String,
    time_tip: String,
}

impl HeaderText {
    fn new(created_at: i64, lang: i18n::Lang) -> Self {
        let local = Local.timestamp_millis_opt(created_at).single();
        Self {
            created_at,
            lang: Some(lang),
            date: local.map(util::long_date).unwrap_or_default(),
            time: local.map(util::clock_time).unwrap_or_default(),
            date_tip: t!("canvas-date-tip").into_owned(),
            time_tip: t!("canvas-time-tip").into_owned(),
        }
    }
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            tool: CanvasTool::Select,
            selection: Selection::default(),
            active_text: None,
            focus_title: false,
            title_active: false,
            refocus_title: false,
            covered: false,
            scene_rect: None,
            reveal: None,
            open_find_tags: false,
            ink_color: Rgb(0x10, 0x10, 0x14),
            ink_width: 2.0,
            shape_kind: ShapeKind::Rect,
            shape_fill: Some(Rgb(0xe8, 0xe5, 0xfb)),
            eraser: EraserMode::Stroke,
            ink_to_shape: false,
            pens: PenPreset::defaults(),
            pen: None,
            recent_colors: Vec::new(),
            lock_drawing: false,
            scrollbar_left: false,
            drag: None,
            band: None,
            lasso: None,
            space: None,
            stroke: None,
            erasing: false,
            erase_from: None,
            shape_draft: None,
            timeline_editor: None,
            editing_label: None,
            equation_edit: None,
            resized: None,
            focus_label: false,
            text_snapshot: None,
            typing: None,
            last_nudge: None,
            clipboard: None,
            clipboard_text: None,
            item_action: None,
            picture_request: None,
            alt_text: None,
            cropping: None,
            pending_files: None,
            printout: None,
            pending_pdfs: VecDeque::new(),
            printout_from_card: None,
            recording: None,
            player: None,
            audio_lengths: HashMap::new(),
            open_versions: false,
            ribbon_cmd: None,
            menu_at: None,
            paste_at: None,
            note_request: None,
            last_click: None,
            remeasure: None,
            visible: None,
            scene_layer: None,
            viewport: Rect::NOTHING,
            views: HashMap::new(),
            zoom: 1.0,
            pan: Vec2::ZERO,
            undo: VecDeque::new(),
            redo: Vec::new(),
            page_id: String::new(),
            find: None,
            title_width: 0.0,
            header: HeaderText::default(),
            header_rect: None,
            calendar: None,
            note_leads: HashMap::new(),
            far: None,
        }
    }
}

impl CanvasState {
    pub fn reset(&mut self, page_id: &str) {
        // The item clipboard, the eraser / Ink to Shape choices, the pens and Lock Drawing
        // Mode, and a recording in progress survive page changes (the desktop notebook keeps recording
        // while you move around), and so does where each page was scrolled to.
        let clipboard = self.clipboard.take();
        let clipboard_text = self.clipboard_text.take();
        let (eraser, ink_to_shape) = (self.eraser, self.ink_to_shape);
        let pens = std::mem::take(&mut self.pens);
        let recent_colors = std::mem::take(&mut self.recent_colors);
        let (pen, ink_color, ink_width, lock_drawing) =
            (self.pen, self.ink_color, self.ink_width, self.lock_drawing);
        let recording = self.recording.take();
        let printout = self.printout.take();
        let mut views = std::mem::take(&mut self.views);
        if let Some(view) = self.scene_rect.filter(|_| !self.page_id.is_empty()) {
            views.insert(self.page_id.clone(), (view, self.viewport));
        }
        let (scene_rect, viewport) = views
            .get(page_id)
            .map_or((None, Rect::NOTHING), |(view, area)| (Some(*view), *area));
        *self = Self {
            page_id: page_id.to_string(),
            clipboard,
            clipboard_text,
            eraser,
            ink_to_shape,
            pens,
            recent_colors,
            pen,
            ink_color,
            ink_width,
            lock_drawing,
            recording,
            printout,
            views,
            scene_rect,
            viewport,
            ..Self::default()
        };
    }

    /// A printout is being inserted, or a dropped PDF is waiting for its question.
    pub fn printout_busy(&self) -> bool {
        self.printout.is_some() || !self.pending_pdfs.is_empty()
    }

    /// Stops a printout being inserted and removes the PDF and pages it stored, which
    /// nothing on a page uses yet (Folio is closing).
    pub fn abandon_printout(&mut self, library: &mut Library) {
        if let Some(run) = self.printout.take() {
            run.job.cancel();
            discard_printout(library, &run);
        }
        self.pending_pdfs.clear();
    }

    #[cfg(test)]
    pub fn printout_asks_password(&self) -> bool {
        self.printout
            .as_ref()
            .is_some_and(|run| run.password.is_some())
    }

    pub fn ensure_page(&mut self, page_id: &str) {
        if self.page_id != page_id {
            self.reset(page_id);
        }
    }

    /// How long the recording in progress has run (pauses not counted) and whether it is
    /// paused; `None` when nothing is being recorded.
    pub fn recording_time(&self) -> Option<(u64, bool)> {
        self.recording
            .as_ref()
            .map(|recorder| (recorder.elapsed().as_millis() as u64, recorder.is_paused()))
    }

    pub fn set_recording_paused(&mut self, paused: bool) {
        if let Some(recorder) = self.recording.as_mut() {
            recorder.set_paused(paused);
        }
    }

    /// The length read from the file of a clip that doesn't know its own.
    #[cfg(test)]
    pub fn audio_length(&self, attachment: &str) -> Option<u64> {
        self.audio_lengths.get(attachment).copied()
    }

    /// Makes the recording in progress fail as an unplugged microphone does.
    #[cfg(test)]
    pub fn fail_recording(&self, error: &str) {
        if let Some(recorder) = self.recording.as_ref() {
            recorder.fail(error);
        }
    }

    /// The clip being played (or paused).
    #[cfg(test)]
    pub fn player_attachment(&self) -> Option<&str> {
        self.player
            .as_ref()
            .map(|player| player.attachment.as_str())
    }

    /// The page the recording in progress is on.
    pub fn recording_page(&self) -> Option<&str> {
        self.recording
            .as_ref()
            .map(|recorder| recorder.page_id.as_str())
    }

    /// A problem that stopped the recording in progress (the microphone was unplugged…).
    pub fn take_recording_error(&self) -> Option<String> {
        self.recording.as_ref()?.take_error()
    }

    /// Stops a recording in progress: completes its file, records the attachment's
    /// size, and gives its clip its length wherever it is: in `page` when that is the page
    /// it was made on (and in the undo history kept for it), else in that page as saved
    /// (the recording carried on while you moved to another page).
    pub fn finish_recording(
        &mut self,
        page: Option<&mut Page>,
        editor: &mut EditorState,
        library: &mut Library,
    ) -> Result<bool, String> {
        let Some(recorder) = self.recording.take() else {
            return Ok(false);
        };
        let recorded = recorder.stop()?;
        let _ = library.set_attachment_size(&recorded.attachment, recorded.bytes);
        self.audio_lengths.remove(&recorded.attachment);
        let (clip, length) = (recorded.attachment.as_str(), recorded.duration_ms);
        // What was typed or done while it recorded holds the clip without its length: undo
        // must not bring that back.
        editor.set_clip_length(clip, length);
        if self.page_id == recorded.page_id {
            let snapshots = self
                .undo
                .iter_mut()
                .chain(self.redo.iter_mut())
                .chain(self.text_snapshot.iter_mut().map(|(_, doc)| doc))
                .chain(self.equation_edit.iter_mut().map(|edit| &mut edit.before));
            for snapshot in snapshots {
                snapshot.set_clip_length(clip, length);
            }
        }
        let Some(page) = page.filter(|page| page.id == recorded.page_id) else {
            if let Ok(Some(mut stored)) = library.get_page(&recorded.page_id) {
                if stored.canvas.set_clip_length(clip, length) {
                    let _ = library.save_page(&stored);
                }
            }
            return Ok(false);
        };
        if page.canvas.set_clip_length(clip, length) {
            return Ok(true);
        }
        // Undo, a restored version or a delete took the clip off the page while it
        // recorded: put it back, so a recording is never left without its clip.
        self.push_undo(&page.canvas);
        let at = self.view_center() - AUDIO_SIZE / 2.0;
        page.canvas.add(CanvasItem::new(
            CanvasKind::Audio {
                attachment: recorded.attachment.clone(),
                duration_ms: length,
            },
            at.x.round(),
            at.y.round(),
            AUDIO_SIZE.x,
            AUDIO_SIZE.y,
        ));
        Ok(true)
    }

    /// Records `doc`, the canvas just before a change, as an undo step. Typing in the
    /// active note is committed first as its own step, so undo takes back the change and
    /// then the typing, in that order.
    fn push_undo(&mut self, doc: &CanvasDoc) {
        self.typing = None;
        self.last_nudge = None;
        if let Some((owner, snapshot)) = self.text_snapshot.take() {
            if self.typing_needs_step(&snapshot, owner, doc) {
                self.push_snapshot(&snapshot);
            }
            // A change that leaves the note focused (Insert → Audio while typing) must
            // not be measured as typing: `canvas_contents` takes the note's next
            // snapshot from the canvas after the change, so undoing the later typing
            // never takes the change back with it.
        }
        // The same for the equation being typed.
        if let Some(edit) = self.equation_edit.as_ref() {
            if self.equation_is_new(edit) && equation_blank(doc, edit.id) {
                // Inserted and not typed in yet: the change's step leaves it out, so undo
                // goes straight back to before it was inserted (usually that is the
                // creation step itself, and nothing new is recorded).
                let mut without = doc.clone();
                without.remove(edit.id);
                self.push_snapshot(&without);
                return;
            }
            if self.equation_needs_step(edit, doc) {
                let before = edit.before.clone();
                self.push_snapshot(&before);
            }
            if let Some(edit) = self.equation_edit.as_mut() {
                edit.before = doc.clone();
                edit.resnap = true;
            }
        }
        self.push_snapshot(doc);
    }

    /// Whether the equation being typed was inserted in this session: it was blank when
    /// the session began and its creation is the last undo step, which takes the typing
    /// with it.
    fn equation_is_new(&self, edit: &EquationEdit) -> bool {
        equation_blank(&edit.before, edit.id)
            && self
                .undo
                .back()
                .is_some_and(|last| last.item(edit.id).is_none())
    }

    /// Whether typing in the equation since its session began needs an undo step of its
    /// own now that the canvas is `now`.
    fn equation_needs_step(&self, edit: &EquationEdit, now: &CanvasDoc) -> bool {
        edit.before != *now && !self.equation_is_new(edit)
    }

    /// Note `id` has the caret this frame. The canvas is remembered when its edit starts
    /// (or after a change made while it is edited, once a drag making it has ended), so
    /// the typing becomes one undo step.
    fn typing_in(&mut self, canvas: &CanvasDoc, id: u64) {
        if self.drag.is_none()
            && self
                .text_snapshot
                .as_ref()
                .is_none_or(|(owner, _)| *owner != id)
        {
            self.text_snapshot = Some((id, canvas.clone()));
        }
        self.active_text = Some(id);
        self.selection.set_one(id);
    }

    /// Whether typing in note `owner` since `snapshot` (the canvas when its edit began)
    /// needs an undo step of its own now that the canvas is `now`. A note created blank
    /// needs none while its creation is the last step: undoing that takes the typing too.
    fn typing_needs_step(&self, snapshot: &CanvasDoc, owner: u64, now: &CanvasDoc) -> bool {
        if snapshot == now {
            return false;
        }
        let just_created = self
            .undo
            .back()
            .is_none_or(|last| last.item(owner).is_none());
        !(just_created && note_was_blank(snapshot, owner))
    }

    fn push_snapshot(&mut self, doc: &CanvasDoc) {
        if self.undo.back() == Some(doc) {
            return;
        }
        self.undo.push_back(doc.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    fn undo(&mut self, page: &mut Page) -> bool {
        self.typing = None;
        self.last_nudge = None;
        // Undo on the page right after typing in a note takes back the typing first.
        if let Some((owner, snapshot)) = self.text_snapshot.take() {
            if self.typing_needs_step(&snapshot, owner, &page.canvas) {
                self.push_snapshot(&snapshot);
            }
        }
        // So does undo while an equation is typed (the ribbon's Undo), which also ends
        // its session. One inserted in this session goes with its creation step.
        if let Some(edit) = self.equation_edit.take() {
            if self.equation_needs_step(&edit, &page.canvas) {
                self.push_snapshot(&edit.before);
            }
        }
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        self.redo.push(page.canvas.clone());
        page.canvas = previous;
        // A crop under way was measured on the picture as it was.
        self.cropping = None;
        self.repair_selection(page);
        true
    }

    fn redo(&mut self, page: &mut Page) -> bool {
        self.typing = None;
        self.last_nudge = None;
        if self.redo.is_empty() {
            return false;
        }
        // An equation being typed ends its session first; its typing is a new change,
        // which (like any other) leaves nothing to redo.
        if let Some(edit) = self.equation_edit.take() {
            if self.equation_needs_step(&edit, &page.canvas) {
                self.push_snapshot(&edit.before);
            }
        }
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.text_snapshot = None;
        self.undo.push_back(page.canvas.clone());
        page.canvas = next;
        self.cropping = None;
        self.repair_selection(page);
        true
    }

    fn repair_selection(&mut self, page: &Page) {
        // Undo and redo bring back a note's place and its tags together: nothing must move
        // it again for its tags.
        self.note_leads.clear();
        self.far = None;
        let canvas = &page.canvas;
        // A redo can set a picture selected since as the background again: it drops out.
        self.selection
            .items
            .retain(|id| canvas.item(*id).is_some_and(|item| !item.is_background()));
        // Undo and redo can reorder or drop strokes, so their indices no longer mean anything.
        self.selection.strokes.clear();
        if self
            .active_text
            .is_some_and(|id| page.canvas.item(id).and_then(|item| item.doc()).is_none())
        {
            self.active_text = None;
            self.text_snapshot = None;
        }
        self.drag = None;
        self.editing_label = None;
        if self
            .equation_edit
            .as_ref()
            .is_some_and(|edit| page.canvas.item(edit.id).is_none())
        {
            self.equation_edit = None;
        }
    }

    /// The middle of what is on screen, for placing inserted items.
    fn view_center(&self) -> Pos2 {
        self.visible
            .or(self.scene_rect)
            .map_or(pos2(320.0, 220.0), |rect| rect.center())
    }

    /// Where the scene was drawn on screen last frame (the page's canvas area).
    // First user: the test harness's `empty_paper_point` (P2.17); delete this allow then.
    #[allow(dead_code)]
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// The zoom last frame was drawn at.
    #[cfg(test)]
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// How many canvas undo steps there are.
    #[cfg(test)]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Converts a scene position to a screen position, using last frame's view.
    #[cfg(test)]
    pub fn scene_to_screen(&self, pos: Pos2) -> Pos2 {
        let visible = self.visible.unwrap_or(Rect::ZERO);
        self.viewport.min + (pos - visible.min) * self.zoom
    }

    /// The part of the page shown last frame, in page units.
    #[cfg(test)]
    pub fn visible(&self) -> Option<Rect> {
        self.visible
    }

    /// The date and the time the page header shows.
    #[cfg(test)]
    pub fn header_text(&self) -> (&str, &str) {
        (&self.header.date, &self.header.time)
    }

    /// Opens find on page (Ctrl+F), or gives its field the caret again, searching for
    /// `prefill` (the words selected in a note) when given.
    pub fn open_find(&mut self, prefill: Option<String>) {
        let find = self.find.get_or_insert_with(FindState::default);
        if let Some(text) = prefill.filter(|_| !find.field_focused) {
            find.query = text;
            find.current = 0;
        }
        find.from_search = false;
        find.focus = true;
        find.dirty = true;
        find.reveal = true;
    }

    /// Opens the page's find bar for a global search hit, so the words searched for
    /// stay highlighted (it closes itself when only the title or a tag matched).
    pub fn open_find_from_search(&mut self, query: &str) {
        // A search finds pages holding every word; the page marks the first one.
        let first = crate::db::search_terms(query)
            .into_iter()
            .next()
            .unwrap_or_default();
        self.find = Some(FindState {
            query: first,
            from_search: true,
            dirty: true,
            reveal: true,
            ..FindState::default()
        });
    }

    fn select(&mut self, id: u64) {
        self.selection.set_one(id);
        if self.active_text != Some(id) {
            self.active_text = None;
        }
    }
}

/// A place on the page to bring into view: a canvas item and, for notes, a paragraph
/// to put the caret in. Set by the app (Find Tags, find on page); consumed by `show`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reveal {
    pub item: u64,
    pub block: Option<u64>,
    /// Select the item and put the caret in the paragraph. When false the target is only
    /// panned into view: the selection, the note being edited and keyboard focus stay.
    pub focus: bool,
}

pub struct CanvasResult {
    pub changed: bool,
    pub toast: Option<(String, bool)>,
}

/// Where the pointer is and how the scene is scaled this frame.
#[derive(Clone, Copy)]
struct Viewport {
    zoom: f32,
    /// The pointer in scene coordinates, when it is over the canvas itself rather than
    /// a menu or dialog drawn on top of it.
    hover: Option<Pos2>,
}

impl Viewport {
    /// Converts a size in screen points to scene units.
    fn px(self, points: f32) -> f32 {
        points / self.zoom
    }

    fn hovering(self, rect: Rect) -> bool {
        self.hover.is_some_and(|pos| rect.contains(pos))
    }
}

pub fn show(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &mut Library,
    textures: &mut TextureCache,
    t: Tokens,
) -> CanvasResult {
    state.ensure_page(&page.id);
    let ctx = ui.ctx().clone();
    let mut changed = false;
    let mut toast = None;
    let was_active = state.active_text;
    // Picture › Crop ends, keeping the crop, once its picture is no longer selected.
    if state
        .cropping
        .as_ref()
        .is_some_and(|crop| !state.selection.contains(crop.item))
    {
        changed |= commit_crop(page, state);
    }

    find_bar(&ctx, page, state, editor_state, t);
    reveal_find_match(state);
    changed |= canvas_keys(&ctx, page, state, editor_state, library, &mut toast);
    if let Some(cmd) = state.ribbon_cmd.take() {
        changed |= run_ribbon_cmd(ui, cmd, page, state, editor_state, library, &mut toast);
    }
    reveal_pending(&ctx, page, state, editor_state);
    // The recording and playing clips, for the recording cards in notes, and the lengths
    // of the clips whose blocks do not know theirs (stopped while another page was open).
    let recording = state.recording.as_ref().map(|r| r.attachment.clone());
    let mut lengths = HashMap::new();
    // And the PDFs in notes already printed out on the page, for their menus.
    let mut printed = HashSet::new();
    for doc in page.canvas.items.iter().filter_map(CanvasItem::doc) {
        for block in &doc.blocks {
            match &block.kind {
                BlockKind::Audio {
                    attachment,
                    duration_ms: 0,
                } => {
                    if recording.as_ref() != Some(attachment) {
                        let length = clip_length(&mut state.audio_lengths, library, attachment);
                        lengths.insert(attachment.clone(), length);
                    }
                }
                BlockKind::File {
                    attachment, name, ..
                } if is_pdf_name(name) && has_printout(page, attachment) => {
                    printed.insert(attachment.clone());
                }
                _ => {}
            }
        }
    }
    editor_state.media = MediaView {
        recording,
        playing: state.player.as_ref().map(|p| {
            (
                p.attachment.clone(),
                p.position_ms() as f32 / p.duration_ms().max(1) as f32,
                p.is_playing(),
            )
        }),
        lengths,
        printed,
    };

    let available = ui.available_size();
    let mut scene_rect = state
        .scene_rect
        .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, available));
    // What the page holds decides how far it reaches; it is measured once before the page
    // is drawn and once after (drawing may move or add things), and remembered meanwhile.
    if changed {
        state.far = None;
    }
    let far = cached_far(state, page);
    scene_rect = clamp_view(scene_rect, page_extent(far, scene_rect.size()));
    let mut page_changed = false;
    let parent_layer = ui.layer_id();
    let page_id = page.id.clone();
    ui.push_id(("folio-canvas", page_id.as_str()), |ui| {
        let viewport = Rect::from_min_size(ui.cursor().min, ui.available_size_before_wrap());
        if let Some(view) = resized_view(state.viewport, viewport.size(), scene_rect) {
            scene_rect = view;
        }
        state.viewport = viewport;
        keep_wheel_on_page(ui.ctx(), viewport, scene_rect, far, state, parent_layer);
        Scene::new()
            .zoom_range(ZOOM_RANGE)
            .max_inner_size(page_extent(far, scene_rect.size()).size())
            .drag_pan_buttons(DragPanButtons::MIDDLE | DragPanButtons::SECONDARY)
            .show(ui, &mut scene_rect, |ui| {
                page_changed |= canvas_contents(
                    ui,
                    page,
                    state,
                    editor_state,
                    library,
                    textures,
                    t,
                    &mut toast,
                    parent_layer,
                    far,
                );
            });
    });
    // Scene only writes the rect back when it pans or zooms itself. Keep the stored
    // rect equal to what is really visible, so a resized window does not leave the
    // stored view out of proportion, then apply drag-panning on empty paper.
    if let Some(visible) = state.visible {
        let aspect = |rect: Rect| rect.width() / rect.height().max(1.0);
        if (aspect(visible) - aspect(scene_rect)).abs() > 0.01 {
            scene_rect = visible;
        }
    }
    // Keep the line being typed on screen as the note grows past the bottom.
    if page_changed {
        if let (Some(id), Some(visible)) = (state.active_text, state.visible) {
            let doc_id = format!("{}:{}", page.id, id);
            let line = editor_state
                .active_block
                .and_then(|block| ctx.read_response(editor::block_id(&doc_id, block)))
                .map(|response| response.rect);
            if let Some(line) = line {
                let overflow = line.bottom() + 24.0 / state.zoom - visible.bottom();
                if overflow > 0.0 && line.top() < visible.bottom() + visible.height() {
                    state.pan.y -= overflow;
                }
            }
        }
    }
    if state.pan != Vec2::ZERO {
        scene_rect = scene_rect.translate(-state.pan);
        state.pan = Vec2::ZERO;
    }
    // The view stays on the page, whatever moved it (the wheel, a drag, the caret).
    let moving = state.drag.is_some()
        || state.space.is_some()
        || state.stroke.is_some()
        || state.lasso.is_some();
    if changed || page_changed || moving {
        state.far = None;
    }
    scene_rect = clamp_view(
        scene_rect,
        page_extent(cached_far(state, page), scene_rect.size()),
    );
    state.scene_rect = Some(scene_rect);
    changed |= page_changed;
    if let Some(id) = was_active.filter(|id| state.active_text != Some(*id)) {
        changed |= finish_text_edit(page, state, id);
    }
    // The equation being typed is finished once its source box loses focus: Enter, Esc,
    // Tab, or a press anywhere else. A press waits for its release, so what the click does
    // (the ribbon's Undo, Insert ▸ Table…) still sees the session open.
    let source_focused = state.equation_edit.as_ref().is_none_or(|edit| {
        edit.focus || ctx.memory(|memory| memory.has_focus(equation_source_id(&page.id, edit.id)))
    });
    if !source_focused && !ctx.input(|input| input.pointer.any_down()) {
        let id = state.equation_edit.as_ref().map(|edit| edit.id);
        let clicked = ctx.input(|input| input.pointer.any_released());
        changed |= finish_equation_edit(page, state);
        // Enter leaves the equation selected, like Esc does; a click leaves the selection
        // to what was clicked.
        if let Some(id) = id.filter(|id| !clicked && page.canvas.item(*id).is_some()) {
            if state.selection.is_empty() {
                state.selection.set_one(id);
            }
        }
    }

    if let Some(editor) = state.timeline_editor.take() {
        match timeline_dialog(&ctx, page, editor, t) {
            TimelineOutcome::Editing(editor) => state.timeline_editor = Some(editor),
            TimelineOutcome::Save(updated) => {
                state.push_undo(&page.canvas);
                if let Some(item) = page.canvas.item_mut(updated.item_id) {
                    if let CanvasKind::Timeline { milestones, .. } = &mut item.kind {
                        *milestones = updated.milestones;
                        changed = true;
                    }
                }
            }
            TimelineOutcome::Closed => {}
        }
    }
    if let Some(pending) = state.pending_files.take() {
        match pasted_files_dialog(&ctx, &page.id, &pending, t) {
            FilesChoice::Waiting => state.pending_files = Some(pending),
            FilesChoice::Attach => {
                let mut at = pending.at.unwrap_or_else(|| state.view_center());
                for (path, _) in &pending.files {
                    changed |= insert_path(page, state, library, path, at, &mut toast);
                    at += vec2(24.0, 24.0);
                }
            }
            FilesChoice::PasteText => {
                changed |= paste_text(page, state, editor_state, &pending.text, pending.at);
            }
            FilesChoice::Cancel => {}
        }
    }
    changed |= poll_printout(&ctx, page, state, library, t, &mut toast);
    if let Some(mut edit) = state.alt_text.take() {
        match alt_text_dialog(&ctx, &page.id, &mut edit, t) {
            Some(true) => {
                if picture_meta_mut(page, edit.target).is_some() {
                    state.push_undo(&page.canvas);
                    if let Some(meta) = picture_meta_mut(page, edit.target) {
                        meta.alt = edit.text.trim().to_string();
                    }
                    // OK with the text unchanged takes no step.
                    if state.undo.back() == Some(&page.canvas) {
                        state.undo.pop_back();
                    } else {
                        forget_picture_note_history(page, editor_state, edit.target);
                        changed = true;
                    }
                }
            }
            Some(false) => {}
            None => state.alt_text = Some(edit),
        }
    }

    if changed {
        if let Some(find) = state.find.as_mut() {
            find.dirty = true;
        }
    }
    refresh_find(&ctx, page, state, editor_state);
    // A clip that is recording or playing redraws its clock.
    let playing = state
        .player
        .as_ref()
        .is_some_and(|player| player.is_playing());
    if state.recording.is_some() || playing {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    if let Some(error) = state.player.as_ref().and_then(|player| player.take_error()) {
        toast = Some((
            t!("canvas-error-play-recording", error = error.to_string()).into_owned(),
            true,
        ));
        state.player = None;
    }
    // Stop playing a clip that was deleted.
    let orphaned = state
        .player
        .as_ref()
        .is_some_and(|player| !page_has_attachment(page, &player.attachment));
    if orphaned {
        state.player = None;
    }
    if changed {
        page.word_count = page.word_count();
    }
    CanvasResult { changed, toast }
}

/// Whether `attachment` is still on the page: an item's, or a block's in any note.
fn page_has_attachment(page: &Page, attachment: &str) -> bool {
    page.canvas.items.iter().any(|item| {
        item.kind.attachment() == Some(attachment)
            || item.doc().is_some_and(|doc| {
                doc.blocks
                    .iter()
                    .any(|block| block.kind.attachment() == Some(attachment))
            })
    })
}

/// The far corner of what is on the page (its header, items and ink), in page units.
fn content_far(page: &Page) -> Pos2 {
    let mut far = pos2(PAGE_LEFT + TITLE_RULE_MIN, PAGE_CONTENT_TOP);
    for item in &page.canvas.items {
        far = far.max(item.rect().max);
    }
    for stroke in &page.canvas.ink {
        for point in &stroke.points {
            far = far.max(pos2(point[0], point[1]));
        }
    }
    far
}

/// `content_far`, remembered while the page's item and stroke counts stay the same. A
/// change that moves things without changing the counts (a drag, an edit, a note growing
/// with its text) clears it.
fn cached_far(state: &mut CanvasState, page: &Page) -> Pos2 {
    let counts = (page.canvas.items.len(), page.canvas.ink.len());
    match state.far {
        Some((items, strokes, far)) if (items, strokes) == counts => far,
        _ => {
            let far = content_far(page);
            state.far = Some((counts.0, counts.1, far));
            far
        }
    }
}

/// How far the page reaches, in page units: from its corner to its content (whose far
/// corner is `far`, see `content_far`) plus one view of room to the right and below, and
/// never less than the view. the desktop notebook's page grows as you add to it; there is no fixed size.
fn page_extent(far: Pos2, view_size: Vec2) -> Rect {
    Rect::from_min_max(Pos2::ZERO, (far + view_size).max(view_size.to_pos2()))
}

/// Scene pans with the wheel (and zooms with Ctrl+wheel, around the pointer) and draws the
/// page at its new place in the same frame, before `show` clamps the view: at the page's
/// edges each wheel frame would be drawn off the page, then snap back. Hand Scene a wheel
/// pan that keeps the view on the page instead (after a zoom, the pan that brings the
/// zoomed view back onto it). `outer` is where Scene draws on screen and `view` the part
/// of the page it shows, as Scene will be given them.
fn keep_wheel_on_page(
    ctx: &egui::Context,
    outer: Rect,
    view: Rect,
    far: Pos2,
    state: &CanvasState,
    parent_layer: LayerId,
) {
    let Some(pointer) = ctx.input(|input| input.pointer.latest_pos()) else {
        return;
    };
    // Only where Scene takes the wheel: over the page, not a menu or dialog above it.
    let above = ctx.layer_id_at(pointer);
    if !outer.contains(pointer)
        || above.is_some_and(|layer| layer != parent_layer && Some(layer) != state.scene_layer)
    {
        return;
    }
    let (zoom, pan) = ctx.input(|input| (input.zoom_delta(), input.smooth_scroll_delta));
    if (zoom == 1.0 && pan == Vec2::ZERO) || !view.is_positive() || !outer.is_positive() {
        return;
    }
    // Scene's transform from the page to the screen (`fit_to_rect_in_scene`), then its
    // zoom around the pointer, clamped to the zoom range.
    let (low, high) = (*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
    let scale = (outer.size() / view.size()).min_elem().clamp(low, high);
    let translation = outer.center().to_vec2() - scale * view.center().to_vec2();
    let zoom = zoom.clamp(low / scale, high / scale);
    let scale = scale * zoom;
    let translation = translation + (pointer.to_vec2() - translation) * (1.0 - zoom);
    // Where that pan would put the view, and where the page lets it go.
    let size = outer.size() / scale;
    let min = (outer.min.to_vec2() - translation - pan) / scale;
    let allowed = clamp_view(
        Rect::from_min_size(min.to_pos2(), size),
        page_extent(far, size),
    );
    let kept = outer.min.to_vec2() - scale * allowed.min.to_vec2() - translation;
    if (kept - pan).length() > 0.01 {
        ctx.input_mut(|input| input.smooth_scroll_delta = kept);
    }
}

/// `view` moved, never resized, to stay on the page: not above or left of its corner, and
/// not past its far edges. Where the view is larger than the page it sits at the corner.
fn clamp_view(view: Rect, extent: Rect) -> Rect {
    if !view.is_finite() || !view.is_positive() {
        return view;
    }
    let mut min = view.min;
    for axis in 0..2 {
        let last = extent.max[axis] - view.size()[axis];
        min[axis] = if last > extent.min[axis] {
            min[axis].clamp(extent.min[axis], last)
        } else {
            extent.min[axis]
        };
    }
    Rect::from_min_size(min, view.size())
}

/// The view for a page area that changed size (a panel opened or closed beside it, or the
/// window was resized): the same zoom and top-left corner, showing more or less of the
/// page. Scene would otherwise fit the old view into the new area and change the zoom.
/// `None` when the size is unchanged, or when `view` is not a whole view of the old area
/// (fit-to-page asks Scene for a rectangle of any shape).
fn resized_view(old: Rect, new: Vec2, view: Rect) -> Option<Rect> {
    if !old.is_positive() || new.x < 1.0 || new.y < 1.0 || (old.size() - new).length() < 0.5 {
        return None;
    }
    let aspect = |size: Vec2| size.x / size.y.max(1.0);
    if !view.is_positive() || (aspect(old.size()) - aspect(view.size())).abs() > 0.01 {
        return None;
    }
    let zoom = old.width() / view.width();
    Some(Rect::from_min_size(view.min, new / zoom))
}

/// Brings a requested item (or one of its paragraphs) into view once the page has been
/// drawn, keeping the zoom, and (when asked) puts the caret in the paragraph.
fn reveal_pending(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) {
    let (Some(reveal), Some(visible)) = (state.reveal, state.visible) else {
        return;
    };
    state.reveal = None;
    let Some(item) = page.canvas.item(reveal.item) else {
        return;
    };
    let doc_id = format!("{}:{}", page.id, item.id);
    let block = reveal.block.filter(|block| {
        item.doc()
            .is_some_and(|doc| doc.block_index(*block).is_some())
    });
    let target = block
        .and_then(|block| ctx.read_response(editor::block_id(&doc_id, block)))
        .map_or(item.rect(), |response| response.rect);
    let margin = 40.0 / state.zoom.max(0.01);
    if !visible.shrink(margin).contains_rect(target) {
        let current = state.scene_rect.unwrap_or(visible);
        state.scene_rect = Some(current.translate(target.center() - visible.center()));
    }
    if !reveal.focus {
        return;
    }
    match block {
        Some(block) => {
            state.active_text = Some(item.id);
            state.selection.set_one(item.id);
            editor_state.focus_block(block, 0);
        }
        None => state.select(item.id),
    }
}

/// the desktop notebook's page header, drawn on the page so it scrolls and zooms with the notes: the
/// title (typed in place), a rule as wide as the title, and the date and time the page was
/// created, which open pickers to change them (P3.8). Returns whether the page changed.
fn page_header(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    vp: Viewport,
    t: Tokens,
) -> bool {
    state.header_rect = None;
    if page.title_hidden {
        editor_state.title_focused = false;
        state.title_active = false;
        return false;
    }
    let ctx = ui.ctx().clone();
    let id = editor::title_id(page);
    let mut changed = false;

    // A new page, Duplicate, a click on the header: the caret goes to the end of the
    // title, which comes into view. (Not behind a dialog: egui would drop the request.)
    let behind_dialog = ctx.memory(|memory| memory.top_modal_layer().is_some());
    if !behind_dialog {
        let request = editor_state
            .take_title_focus()
            .or_else(|| std::mem::take(&mut state.focus_title).then_some(false));
        if std::mem::take(&mut state.refocus_title) {
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
        if let Some(select_all) = request {
            let chars = page.title.chars().count();
            editor::set_focus(&ctx, id, chars, select_all.then_some(chars));
            // Only a caret out of view moves the view, and only as far as it must.
            if let Some(view) = state.visible {
                let x = PAGE_LEFT + (state.title_width - 24.0).max(0.0);
                let caret =
                    Rect::from_min_max(pos2(x, TITLE_TOP), pos2(x + 1.0, TITLE_TOP + TITLE_HEIGHT));
                let margin = 16.0 / state.zoom.max(0.01);
                state.pan -= scroll_into(view.shrink(margin), caret);
            }
        }
    }
    if ctx.memory(|memory| memory.has_focus(id))
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
    {
        changed |= focus_first_text(page, state, editor_state);
    }

    let font = theme::light(theme::TITLE_SIZE);
    let width = state.title_width.max(TITLE_RULE_MIN);
    let title_rect = Rect::from_min_size(pos2(PAGE_LEFT, TITLE_TOP), vec2(width, TITLE_HEIGHT));
    let output = ui
        .scope_builder(UiBuilder::new().max_rect(title_rect), |ui| {
            TextEdit::singleline(&mut page.title)
                .id(id)
                .frame(egui::Frame::NONE)
                .margin(Margin::ZERO)
                .desired_width(width)
                .font(font.clone())
                .text_color(t.text2)
                .hint_text(
                    RichText::new(t!("common-untitled"))
                        .font(font.clone())
                        .color(t.faint),
                )
                .show(ui)
        })
        .inner;
    state.title_width = output.galley.size().x + 24.0;
    changed |= output.response.changed();
    editor_state.title_focused = output.response.has_focus();
    if output.response.has_focus() {
        editor_state.pending_style = None;
        state.active_text = None;
        state.selection.clear();
        state.title_active = true;
    } else if state.active_text.is_some()
        || !state.selection.is_empty()
        || (!behind_dialog
            && ctx.memory(|memory| memory.focused().is_some_and(|focused| focused != id)))
    {
        state.title_active = false;
    }
    ui.painter().hline(
        PAGE_LEFT..=PAGE_LEFT + width,
        TITLE_RULE_Y,
        Stroke::new(vp.px(1.0), t.line),
    );

    let lang = i18n::lang();
    if state.header.created_at != page.created_at || state.header.lang != Some(lang) {
        state.header = HeaderText::new(page.created_at, lang);
    }
    let font = theme::ui(DATE_SIZE);
    let (date, time) = ui.fonts_mut(|fonts| {
        (
            fonts.layout_no_wrap(state.header.date.clone(), font.clone(), t.faint),
            fonts.layout_no_wrap(state.header.time.clone(), font, t.faint),
        )
    });
    let date_rect = Rect::from_min_size(pos2(PAGE_LEFT, DATE_TOP), date.size());
    let time_rect = Rect::from_min_size(pos2(date_rect.right() + TIME_GAP, DATE_TOP), time.size());
    let date_response = header_stamp(
        ui,
        page,
        date_rect,
        date,
        "page-date",
        &state.header.date_tip,
        vp,
        t,
    );
    let time_response = header_stamp(
        ui,
        page,
        time_rect,
        time,
        "page-time",
        &state.header.time_tip,
        vp,
        t,
    );
    changed |= date_picker(page, state, &date_response, t);
    changed |= time_picker(page, &time_response, t);
    state.header_rect = Some(Rect::from_min_max(
        pos2(PAGE_LEFT - 8.0, TITLE_TOP),
        pos2(PAGE_LEFT + width, date_rect.bottom()).max(time_rect.right_bottom()),
    ));
    changed
}

/// The date or the time under the title: grey text you can click to change it.
fn header_stamp(
    ui: &mut Ui,
    page: &Page,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    salt: &str,
    tip: &str,
    vp: Viewport,
    t: Tokens,
) -> egui::Response {
    let response = ui.interact(
        rect.expand(vp.px(3.0)),
        Id::unique((salt, page.id.as_str())),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.expand(vp.px(3.0)), CornerRadius::same(2), t.hover);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter().galley(rect.min, galley, t.faint);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    bounds_on_screen(ui, &response);
    response.on_hover_text(tip)
}

/// egui tells accessibility where a widget is in its own layer, here the zoomed and panned
/// page: give the widget's place on screen instead, like every other widget's. Call it
/// after `widget_info`, which sets the in-layer place.
fn bounds_on_screen(ui: &Ui, response: &egui::Response) {
    if let Some(to_screen) = ui.ctx().layer_transform_to_global(ui.layer_id()) {
        let on_screen = to_screen * response.rect;
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_bounds(egui::accesskit::Rect {
                x0: on_screen.min.x.into(),
                y0: on_screen.min.y.into(),
                x1: on_screen.max.x.into(),
                y1: on_screen.max.y.into(),
            });
        });
    }
}

/// The page date's picker, the desktop notebook's "change the page date": a month calendar with the
/// page's day marked and today ringed.
fn date_picker(
    page: &mut Page,
    state: &mut CanvasState,
    response: &egui::Response,
    t: Tokens,
) -> bool {
    use chrono::{Datelike, NaiveDate};
    let Some(created) = Local.timestamp_millis_opt(page.created_at).single() else {
        return false;
    };
    let mut changed = false;
    let shown = egui::Popup::from_toggle_button_response(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let (mut year, mut month) = *state
                .calendar
                .get_or_insert((created.year(), created.month()));
            // Seven 28 px days and their gaps; without a set width the header row's
            // right-to-left part would stretch the popup across the page.
            ui.set_width(7.0 * 30.0);
            ui.horizontal(|ui| {
                if widgets::icon_button_sized(
                    ui,
                    icons::CARET_LEFT,
                    &t!("canvas-calendar-previous"),
                    24.0,
                    13.0,
                    t,
                )
                .clicked()
                {
                    (year, month) = if month == 1 {
                        (year - 1, 12)
                    } else {
                        (year, month - 1)
                    };
                }
                ui.label(
                    RichText::new(util::month_year(year, month))
                        .font(theme::semibold(13.0))
                        .color(t.ink),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button_sized(
                        ui,
                        icons::CARET_RIGHT,
                        &t!("canvas-calendar-next"),
                        24.0,
                        13.0,
                        t,
                    )
                    .clicked()
                    {
                        (year, month) = if month == 12 {
                            (year + 1, 1)
                        } else {
                            (year, month + 1)
                        };
                    }
                });
            });
            state.calendar = Some((year, month));
            let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
                return;
            };
            let offset = if util::week_starts_monday() {
                first.weekday().num_days_from_monday()
            } else {
                first.weekday().num_days_from_sunday()
            };
            let today = Local::now().date_naive();
            // Days are 28 × 24 cells: Folio's theme makes buttons and grid cells at least
            // 30 × 28, which would push the grid past the header.
            ui.spacing_mut().interact_size = vec2(28.0, 24.0);
            egui::Grid::new(("page-calendar", page.id.as_str()))
                .spacing(vec2(2.0, 2.0))
                .show(ui, |ui| {
                    for initial in util::weekday_initials() {
                        ui.add_sized(
                            vec2(28.0, 18.0),
                            egui::Label::new(
                                RichText::new(initial).font(theme::ui(11.0)).color(t.muted),
                            ),
                        );
                    }
                    ui.end_row();
                    for cell in 0..42u32 {
                        let day = first + chrono::Days::new(u64::from(cell))
                            - chrono::Days::new(u64::from(offset));
                        if cell > 0 && cell % 7 == 0 {
                            if day.month() != month {
                                break;
                            }
                            ui.end_row();
                        }
                        let picked = day == created.date_naive();
                        let label = RichText::new(day.day().to_string())
                            .font(theme::ui(12.0))
                            .color(if day.month() == month { t.ink } else { t.faint });
                        let button = ui.add_sized(
                            vec2(28.0, 24.0),
                            egui::Button::selectable(picked, label).stroke(if day == today {
                                Stroke::new(1.0, t.accent)
                            } else {
                                Stroke::NONE
                            }),
                        );
                        // A day's spoken name is its date in full, so tests and screen readers
                        // can tell the 15ths of two months apart.
                        let name = Local
                            .from_local_datetime(&day.and_time(created.time()))
                            .earliest()
                            .map(util::long_date)
                            .unwrap_or_default();
                        button.widget_info(|| {
                            egui::WidgetInfo::selected(egui::Role::Button, true, picked, &name)
                        });
                        if button.clicked() {
                            if let Some(ms) = util::with_date(page.created_at, day) {
                                page.created_at = ms;
                                changed = true;
                            }
                            ui.close();
                        }
                    }
                });
            ui.add_space(4.0);
            if widgets::text_button(ui, None, &t!("canvas-calendar-today"), t).clicked() {
                if let Some(ms) = util::with_date(page.created_at, today) {
                    page.created_at = ms;
                    changed = true;
                }
                ui.close();
            }
        });
    if shown.is_none() {
        state.calendar = None;
    }
    changed
}

/// The page time's picker: hour and minute, and Now.
fn time_picker(page: &mut Page, response: &egui::Response, t: Tokens) -> bool {
    use chrono::Timelike;
    let Some(created) = Local.timestamp_millis_opt(page.created_at).single() else {
        return false;
    };
    let mut changed = false;
    egui::Popup::from_toggle_button_response(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let (mut hour, mut minute) = (created.hour(), created.minute());
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("canvas-time-hour")).font(theme::ui(12.0)));
                let hours = ui.add(egui::DragValue::new(&mut hour).range(0..=23));
                ui.label(RichText::new(t!("canvas-time-minute")).font(theme::ui(12.0)));
                let minutes = ui.add(egui::DragValue::new(&mut minute).range(0..=59));
                if hours.changed() || minutes.changed() {
                    if let Some(ms) = util::with_time(page.created_at, hour, minute) {
                        page.created_at = ms;
                        changed = true;
                    }
                }
            });
            if widgets::text_button(ui, None, &t!("canvas-time-now"), t).clicked() {
                let now = Local::now();
                if let Some(ms) = util::with_time(page.created_at, now.hour(), now.minute()) {
                    page.created_at = ms;
                    changed = true;
                }
                ui.close();
            }
        });
    changed
}

/// Keyboard and clipboard handling for the page itself (not text inside a note).
fn canvas_keys(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let mut changed = false;
    if state.timeline_editor.is_some()
        || state.pending_files.is_some()
        || state.printout_busy()
        || state.alt_text.is_some()
        || state.covered
    {
        return false;
    }

    // Pictures on the clipboard (screenshots, copied images) paste onto the page even
    // while typing, as in the desktop notebook.
    let pasted_images: Vec<_> = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::PasteImage(image) => Some(image.clone()),
                _ => None,
            })
            .collect()
    });
    for image in pasted_images {
        match png_bytes(&image) {
            Some(bytes) => {
                let at = state.view_center();
                changed |= insert_picture_bytes(
                    page,
                    state,
                    library,
                    &bytes,
                    &format!("{}.png", t!("canvas-pasted-image-file")),
                    at,
                    toast,
                );
            }
            None => *toast = Some((t!("canvas-error-pasted-image").into_owned(), true)),
        }
    }

    // Escape steps out one level: from typing to the selected note, then to nothing.
    // (egui may already have dropped text focus on Escape, so don't rely on it.)
    // An open menu (a note's right-click menu) takes the first Escape itself.
    let menu_open = egui::Popup::is_any_open(ctx);
    // Picture › Crop: Enter keeps the crop, Escape leaves the picture as it was.
    if state.cropping.is_some() && !menu_open {
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
            return changed | commit_crop(page, state);
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            state.cropping = None;
            return changed;
        }
        // Any other key keeps the crop first, as any other command does.
        let key = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, Event::Key { pressed: true, .. }))
        });
        if key {
            changed |= commit_crop(page, state);
        }
    }
    // Escape puts the Format Painter away first, even while a note has the keyboard; the
    // caret stays where it is (egui dropped the text focus on Escape: it comes back).
    if editor_state.painter.is_some()
        && !menu_open
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        editor_state.painter = None;
        if state.active_text.is_some() {
            editor_state.restore_focus();
        }
        return changed;
    }
    // Escape first drops a selection across paragraphs, leaving the caret at its end, as
    // the desktop notebook cancels an outline selection; the next Escape leaves the note.
    if editor_state.range.is_some()
        && !menu_open
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        if let Some((_, focus)) = editor_state.range.take() {
            editor_state.focus_block(focus.block, focus.char);
        }
        return changed;
    }
    let leaving_text = state.active_text.is_some()
        || state.editing_label.is_some()
        || state.equation_edit.is_some();
    if leaving_text
        && !menu_open
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        if let Some(focused) = ctx.memory(|memory| memory.focused()) {
            ctx.memory_mut(|memory| memory.surrender_focus(focused));
        }
        // An equation's session ends in `show` once its source box has lost focus.
        if let Some(id) = state
            .active_text
            .take()
            .or(state.editing_label.take())
            .or(state.equation_edit.as_ref().map(|edit| edit.id))
        {
            state.selection.set_one(id);
        }
        editor_state.pending_style = None;
        editor_state.clear_pending_focus();
        // The caret left the note: the Paste Options button under its last paste goes.
        editor_state.paste_session = None;
    }
    if !menu_open {
        changed |= page_keys(ctx, page, state, editor_state);
    }
    if ctx.egui_wants_keyboard_input() {
        return changed;
    }

    let redo_shortcut = egui::KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    let undo_shortcut = egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
    let ctrl_y = egui::KeyboardShortcut::new(Modifiers::CTRL, Key::Y);
    if ctx.input_mut(|input| input.consume_shortcut(&redo_shortcut))
        || (cfg!(not(target_os = "macos"))
            && ctx.input_mut(|input| input.consume_shortcut(&ctrl_y)))
    {
        changed |= state.redo(page);
    } else if ctx.input_mut(|input| input.consume_shortcut(&undo_shortcut)) {
        changed |= state.undo(page);
    }

    // Ctrl+A selects everything on the page (inside a note it selects the note's text).
    if ctx.input_mut(|input| {
        input.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::A))
    }) {
        select_all(page, state);
    }
    // the desktop notebook's Ctrl++ / Ctrl+- zoom the page. No text has the keyboard here: in a note
    // Ctrl+- stays strikethrough and Ctrl+= subscript, as Folio's notes have them.
    if ctx.input_mut(|input| {
        input.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus))
    }) {
        zoom_scene(state, 1.0 / 0.85);
    } else if ctx.input_mut(|input| {
        input.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus))
            || input.consume_shortcut(&egui::KeyboardShortcut::new(
                Modifiers::COMMAND,
                Key::Equals,
            ))
    }) {
        zoom_scene(state, 0.85);
    }
    // the desktop notebook's Alt+= inserts an equation. The key may also arrive as typed text, which
    // the new equation's source box (focused this frame) must not receive. With no note
    // being typed in, a new note holding it goes where the pointer is on the page, else
    // at the insertion point.
    if ctx.input_mut(|input| {
        input.consume_shortcut(&egui::KeyboardShortcut::new(Modifiers::ALT, Key::Equals))
    }) {
        ctx.input_mut(|input| {
            input
                .events
                .retain(|event| !matches!(event, Event::Text(text) if text == "=" || text == "≠"));
        });
        if let Some(pointer) = page_pointer(ctx, state) {
            state.last_click = Some(pointer);
        }
        changed |= insert_equation(ctx, page, state, editor_state);
    }
    // A picture clicked in a note leaves no text focus: Delete or Backspace removes it,
    // as the desktop notebook does.
    if let Some(PictureInfo {
        target: PictureTarget::Block { note, block },
        ..
    }) = picture_info(page, state, editor_state)
    {
        if ctx.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Delete)
                || input.consume_key(Modifiers::NONE, Key::Backspace)
        }) {
            state.push_undo(&page.canvas);
            if let Some(doc) = page.canvas.item_mut(note).and_then(CanvasItem::doc_mut) {
                if let Some(index) = doc.block_index(block) {
                    doc.blocks.remove(index);
                    if doc.blocks.is_empty() {
                        doc.blocks.push(Block::paragraph(""));
                    }
                }
            }
            forget_picture_note_history(page, editor_state, PictureTarget::Block { note, block });
            editor_state.active_block = None;
            return true;
        }
    }
    // Only a selection made on purpose (not a note still being edited after a stray
    // click elsewhere) is deleted, cut, or nudged by the keyboard.
    let acting = !state.selection.is_empty()
        && !state
            .active_text
            .is_some_and(|id| state.selection.contains(id));
    if acting
        && ctx.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Delete)
                || input.consume_key(Modifiers::NONE, Key::Backspace)
        })
    {
        changed |= delete_selection(page, state);
    }
    let primary = state
        .selection
        .primary()
        .filter(|id| acting && page.canvas.item(*id).is_some());
    if let Some(id) = primary {
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
            changed |= open_item(page, state, editor_state, id);
        }
    }
    if acting {
        let step = if ctx.input(|input| input.modifiers.shift) {
            10.0
        } else {
            1.0
        };
        let mut nudge = Vec2::ZERO;
        for (key, direction) in [
            (Key::ArrowLeft, vec2(-1.0, 0.0)),
            (Key::ArrowRight, vec2(1.0, 0.0)),
            (Key::ArrowUp, vec2(0.0, -1.0)),
            (Key::ArrowDown, vec2(0.0, 1.0)),
        ] {
            if ctx.input_mut(|input| {
                input.consume_key(Modifiers::NONE, key) || input.consume_key(Modifiers::SHIFT, key)
            }) {
                nudge += direction * step;
            }
        }
        if nudge != Vec2::ZERO {
            let now = ctx.input(|input| input.time);
            changed |= nudge_selection(page, state, nudge, now);
        }
    }
    if (!state.selection.is_empty() || state.tool != CanvasTool::Select)
        && !menu_open
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        state.selection.clear();
        state.drag = None;
        state.band = None;
        state.lasso = None;
        state.shape_draft = None;
        state.stroke = None;
        // An Insert Space drag in progress puts everything back where it was.
        if let Some(space) = state.space.take().filter(|space| space.down.is_some()) {
            shift_past_line(page, &space, 0.0);
            if state.undo.back() == Some(&page.canvas) {
                state.undo.pop_back();
            }
        }
        state.tool = CanvasTool::Select;
    }

    // Copy, cut and paste whole items. Text pasted onto the page becomes a new note.
    // The events are taken out of the queue so a note focused below doesn't see them too.
    let events: Vec<Event> = ctx.input_mut(|input| {
        let (clipboard, rest): (Vec<Event>, Vec<Event>) = std::mem::take(&mut input.events)
            .into_iter()
            .partition(|event| matches!(event, Event::Copy | Event::Cut | Event::Paste(_)));
        input.events = rest;
        clipboard
    });
    let (now, shift) = ctx.input(|input| (input.time, input.modifiers.shift));
    for event in events {
        match event {
            Event::Copy | Event::Cut => {
                if !acting || !copy_selection(ctx, page, state) {
                    continue;
                }
                // The last copy wins: a paste in a note no longer takes the text copied
                // before.
                editor_state.rich_clip = None;
                if matches!(event, Event::Cut) {
                    changed |= delete_selection(page, state);
                }
            }
            Event::Paste(text) => {
                // A right-click menu's Paste puts what it pastes where the menu was opened;
                // Ctrl+V and the ribbon's, in the middle of the view.
                let at = state
                    .paste_at
                    .take()
                    .filter(|(_, asked)| now - asked < 1.0)
                    .map(|(at, _)| at);
                let internal = state
                    .clipboard
                    .clone()
                    .filter(|_| state.clipboard_text.as_deref() == Some(text.as_str()));
                // Files copied in a file manager arrive as a list of file:// URIs. They
                // are attached only once the user confirms (see PendingFiles).
                if let Some(paths) = pasted_paths(&text) {
                    let files = paths
                        .into_iter()
                        .map(|path| {
                            let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
                            (path, size)
                        })
                        .collect();
                    state.pending_files = Some(PendingFiles { files, at, text });
                    // The rest of this frame's clipboard events wait for the answer.
                    break;
                } else if let Some(clip) = internal {
                    changed |= paste_clipboard(page, state, library, clip, at);
                } else if let Some((mode, (blocks, inline))) =
                    // Text copied with its formatting (in Folio, or HTML from another app)
                    // keeps it in its new note, unless Keep Text Only was asked for.
                    Some(editor_state.take_paste_mode(now, shift))
                            .filter(|mode| *mode != PasteMode::TextOnly)
                            .and_then(|mode| {
                                Some((mode, editor::rich_source(editor_state, &text)?))
                            })
                {
                    changed |= paste_rich_note(page, state, editor_state, blocks, inline, mode, at);
                } else if !text.trim().is_empty() {
                    changed |= paste_text(page, state, editor_state, &text, at);
                }
            }
            _ => {}
        }
    }
    changed
}

/// the desktop notebook's page keys:
/// - Ctrl+A with the whole note already selected selects everything on the page;
/// - PgUp / PgDn scroll the page by most of a view, and the caret of the note being typed
///   in moves with it;
/// - Ctrl+Home / Ctrl+End go to the start of the page's first note or the end of its
///   last one, and the view follows. With nothing focused they move the view to the
///   page's top or the end of what is on it.
///
/// Keys typed in the page title stay the title's, except PgUp / PgDn. True when the page
/// changed (a blank note left behind goes, as any note left does).
fn page_keys(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) -> bool {
    let typing = editor_state.text_has_focus(ctx) && active_note(page, state).is_some();
    let in_title = ctx.memory(|memory| memory.has_focus(editor::title_id(page)));
    let free = !ctx.egui_wants_keyboard_input();
    if !(typing || free || in_title) {
        return false;
    }
    let pressed = |modifiers, key| util::consume_exact(ctx, modifiers, key);
    // Looked at only on a Ctrl+A press: it walks the note's paragraphs.
    let ctrl_a = typing
        && ctx.input(|input| {
            input.events.iter().any(|event| {
                matches!(event, Event::Key { key: Key::A, pressed: true, modifiers, .. }
                    if modifiers.matches_exact(Modifiers::COMMAND))
            })
        });
    let whole_note = ctrl_a
        && active_note(page, state)
            .and_then(|id| page.canvas.item(id))
            .and_then(CanvasItem::doc)
            .is_some_and(|doc| editor::whole_note_selected(doc, editor_state));
    if whole_note && pressed(Modifiers::COMMAND, Key::A) {
        if let Some(focused) = ctx.memory(|memory| memory.focused()) {
            ctx.memory_mut(|memory| memory.surrender_focus(focused));
        }
        editor_state.range = None;
        editor_state.selection = None;
        select_all(page, state);
        return false;
    }
    for (key, down) in [(Key::PageUp, false), (Key::PageDown, true)] {
        if pressed(Modifiers::NONE, key) {
            scroll_by_page(ctx, page, state, editor_state, down, typing);
            return false;
        }
    }
    if typing || free {
        for (key, end) in [(Key::Home, false), (Key::End, true)] {
            if pressed(Modifiers::COMMAND, key) {
                return page_end(page, state, editor_state, end, typing);
            }
        }
    }
    false
}

/// PgUp / PgDn: the view moves by 90 % of its height, as far as the page goes; the caret
/// of the note being typed in goes as far (a whole step at the page's top or bottom).
fn scroll_by_page(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    down: bool,
    typing: bool,
) {
    let Some(view) = state.scene_rect else {
        return;
    };
    let step = state.visible.unwrap_or(view).height() * 0.9 * if down { 1.0 } else { -1.0 };
    let moved = clamp_view(
        view.translate(vec2(0.0, step)),
        page_extent(content_far(page), view.size()),
    );
    state.scene_rect = Some(moved);
    let Some(id) = active_note(page, state).filter(|_| typing) else {
        return;
    };
    let Some(doc) = page.canvas.item(id).and_then(CanvasItem::doc) else {
        return;
    };
    let shift = moved.top() - view.top();
    let dy = if shift.abs() > 0.5 { shift } else { step };
    editor::caret_by(ctx, doc, &format!("{}:{}", page.id, id), editor_state, dy);
}

/// Ctrl+Home / Ctrl+End. While typing, the caret goes to the start of the topmost note
/// (then the leftmost) or to the end of the lowest one, which becomes the note being typed
/// in; the view follows (`reveal_pending`). Otherwise the view goes to the page's top, or
/// to the end of what is on it. True when the note left behind was blank and went.
fn page_end(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    end: bool,
    typing: bool,
) -> bool {
    if !typing {
        if let Some(view) = state.scene_rect {
            let far = content_far(page);
            let top = if end {
                (far.y + 40.0 - view.height()).max(0.0)
            } else {
                0.0
            };
            let moved = view.translate(vec2(0.0, top - view.top()));
            state.scene_rect = Some(clamp_view(moved, page_extent(far, view.size())));
        }
        return false;
    }
    let order = |item: &CanvasItem| {
        let rect = item.rect();
        (if end { rect.bottom() } else { rect.top() }, rect.left())
    };
    let notes = page.canvas.items.iter().filter(|item| item.doc().is_some());
    let compare = |a: &&CanvasItem, b: &&CanvasItem| {
        order(a)
            .partial_cmp(&order(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    let target = if end {
        notes.max_by(compare)
    } else {
        notes.min_by(compare)
    };
    let Some((note, block, char)) = target.and_then(|item| {
        let doc = item.doc()?;
        let hidden = doc.hidden_blocks();
        let mut texts =
            (0..doc.blocks.len()).filter(|i| doc.blocks[*i].kind.is_textual() && !hidden[*i]);
        let index = if end { texts.next_back() } else { texts.next() }?;
        let block = &doc.blocks[index];
        Some((
            item.id,
            block.id,
            if end { block.text.char_count() } else { 0 },
        ))
    }) else {
        return false;
    };
    let mut changed = false;
    if let Some(left) = state.active_text.filter(|id| *id != note) {
        // As `show` does for a note left: its typing becomes one step, a blank one goes.
        changed = finish_text_edit(page, state, left);
        state.active_text = Some(note);
        state.selection.set_one(note);
    }
    editor_state.range = None;
    editor_state.selection = None;
    editor_state.pending_style = None;
    editor_state.focus_block(block, char);
    state.reveal = Some(Reveal {
        item: note,
        block: Some(block),
        focus: false,
    });
    changed
}

/// The text selected in the note being typed in, if it is short enough to search for.
pub fn selected_text(
    page: &Page,
    state: &CanvasState,
    editor_state: &EditorState,
) -> Option<String> {
    let item = page.canvas.item(state.active_text?)?;
    let block = item
        .doc()?
        .blocks
        .iter()
        .find(|block| Some(block.id) == editor_state.active_block)?;
    let range = editor_state.selection.clone()?;
    let text: String = block
        .text
        .text
        .chars()
        .skip(range.start)
        .take(range.end.saturating_sub(range.start))
        .collect();
    let text = text.trim();
    (!text.is_empty() && !text.contains('\n') && text.chars().count() <= 80)
        .then(|| text.to_string())
}

/// The find-on-page bar (Ctrl+F), floating at the top right of the page like the desktop notebook's
/// page search. It runs before the page's own keys, so Enter, F3 and Escape reach it
/// first.
fn find_bar(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    t: Tokens,
) {
    if state.covered
        || state.timeline_editor.is_some()
        || state.pending_files.is_some()
        || state.printout_busy()
        || state.alt_text.is_some()
    {
        return;
    }
    let Some(typing_here) = state.find.as_ref().map(|find| find.field_focused) else {
        return;
    };
    let field_id = Id::unique("folio-find-field");
    // Remember whether the keyboard was last in the field or elsewhere (a note, a label,
    // a cell): pressing the Close button takes their focus before its click lands.
    if ctx
        .memory(|memory| memory.focused())
        .is_some_and(|id| id != field_id)
    {
        if let Some(find) = state.find.as_mut() {
            find.typing_on_page = true;
        }
    }
    // egui has already dropped the field's focus on Escape, so last frame's focus
    // decides. From the page, Escape closes the bar only when nothing else uses it,
    // and an open menu takes the first Escape.
    let menu_open = egui::Popup::is_any_open(ctx);
    let idle = state.active_text.is_none()
        && state.editing_label.is_none()
        && state.selection.is_empty()
        && state.tool == CanvasTool::Select
        && !menu_open
        && !ctx.egui_wants_keyboard_input();
    if ((typing_here && !menu_open) || idle)
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        if typing_here {
            close_find(page, state, editor_state, true);
        } else {
            state.find = None;
        }
        return;
    }
    // Enter would make the one-line field drop its focus, so take it first. Shift
    // variants first: key patterns match logically.
    let mut step = ctx.input_mut(|input| {
        if input.consume_key(Modifiers::SHIFT, Key::F3)
            || (typing_here && input.consume_key(Modifiers::SHIFT, Key::Enter))
        {
            Some(false)
        } else if input.consume_key(Modifiers::NONE, Key::F3)
            || (typing_here && input.consume_key(Modifiers::NONE, Key::Enter))
        {
            Some(true)
        } else {
            None
        }
    });
    let viewport = state.viewport;
    let Some(find) = state.find.as_mut() else {
        return;
    };
    let mut close = false;
    let mut refocus = false;
    if viewport.is_positive() {
        egui::Area::new(Id::unique("folio-find-bar"))
            .order(egui::Order::Foreground)
            // Beside the Full Page View button in the page card's corner.
            .fixed_pos(pos2(
                viewport.right() - FIND_WIDTH - 44.0,
                viewport.top() + 10.0,
            ))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.raised)
                    .stroke(Stroke::new(1.0, t.line_strong))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(FIND_WIDTH - 16.0);
                        // A new Area spends its first frame measuring itself, disabled,
                        // and a disabled field drops the caret: give it once it can keep it.
                        if find.focus && ui.is_enabled() {
                            find.focus = false;
                            editor::set_focus(
                                ui.ctx(),
                                field_id,
                                0,
                                Some(find.query.chars().count()),
                            );
                        }
                        // The bar holds the magnifier, the field, the status and three 24 px
                        // buttons with the item spacing between them: the field takes what the
                        // status leaves, so a longer status never runs under the buttons.
                        let status = find.status();
                        let ctx = ui.ctx().clone();
                        let spacing = ui.spacing().item_spacing.x;
                        let field_width = (ui.available_width()
                            - widgets::text_width(
                                &ctx,
                                icons::MAGNIFYING_GLASS,
                                theme::icon(13.0),
                            )
                            - widgets::text_width(&ctx, &status, theme::ui(11.5))
                            - 3.0 * 24.0
                            - 5.0 * spacing)
                            .clamp(90.0, 170.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(icons::MAGNIFYING_GLASS)
                                    .font(theme::icon(13.0))
                                    .color(t.muted),
                            );
                            let response = ui.add(
                                TextEdit::singleline(&mut find.query)
                                    .id(field_id)
                                    .frame(egui::Frame::NONE)
                                    .margin(Margin::ZERO)
                                    .desired_width(field_width)
                                    .font(theme::ui(13.0))
                                    .text_color(t.ink)
                                    .hint_text(
                                        RichText::new(t!("canvas-find-hint"))
                                            .font(theme::ui(13.0))
                                            .color(t.faint),
                                    ),
                            );
                            if response.changed() {
                                find.current = 0;
                                find.dirty = true;
                                find.reveal = true;
                            }
                            find.field_focused = response.has_focus();
                            if find.field_focused {
                                find.typing_on_page = false;
                            }
                            ui.label(RichText::new(status).font(theme::ui(11.5)).color(t.faint));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::icon_button_sized(
                                    ui,
                                    icons::X,
                                    &t!("canvas-find-close"),
                                    24.0,
                                    12.0,
                                    t,
                                )
                                .clicked()
                                {
                                    close = true;
                                }
                                if widgets::icon_button_sized(
                                    ui,
                                    icons::CARET_DOWN,
                                    &t!("canvas-find-next"),
                                    24.0,
                                    12.0,
                                    t,
                                )
                                .clicked()
                                {
                                    step = Some(true);
                                    refocus = true;
                                }
                                if widgets::icon_button_sized(
                                    ui,
                                    icons::CARET_UP,
                                    &t!("canvas-find-previous"),
                                    24.0,
                                    12.0,
                                    t,
                                )
                                .clicked()
                                {
                                    step = Some(false);
                                    refocus = true;
                                }
                            });
                        });
                    });
            });
    }
    if let Some(forward) = step {
        find.step(forward);
    }
    if refocus {
        // The press on the button took the caret; keep typing in the field.
        ctx.memory_mut(|memory| memory.request_focus(field_id));
    }
    if close {
        let on_match = state.find.as_ref().is_some_and(|find| !find.typing_on_page);
        close_find(page, state, editor_state, on_match);
    }
}

/// Closes the find bar from its field or its Close button. When `on_match`, like
/// the desktop notebook, Word and browsers, the caret is left on the current match (selected), which
/// is what the page shows: the note typed in before may be out of view by now. With no
/// match, or when the user was typing in a note instead, that note gets its caret back.
fn close_find(
    page: &Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    on_match: bool,
) {
    let found = state
        .find
        .take()
        .and_then(|find| find.current_match().cloned())
        .filter(|found| on_match && page.canvas.item(found.item).is_some());
    let Some(found) = found else {
        if let Some(item) = state.active_text {
            // Stepping may have moved the view away from it.
            state.reveal = Some(Reveal {
                item,
                block: editor_state.active_block,
                focus: false,
            });
            editor_state.restore_focus();
        }
        return;
    };
    let block = found.place.block();
    state.reveal = Some(Reveal {
        item: found.item,
        block,
        focus: false,
    });
    // A match in a file's name or an equation's source has no caret to take: their note
    // is selected, as a match on an item selects the item. So is a table's, for a match
    // in one of its cells.
    let textual = |block: u64| {
        page.canvas
            .item(found.item)
            .and_then(CanvasItem::doc)
            .and_then(|doc| doc.block_index(block).map(|index| &doc.blocks[index]))
            .is_some_and(|block| block.kind.is_textual())
    };
    match block.filter(|block| textual(*block)) {
        Some(block) => {
            state.active_text = Some(found.item);
            state.selection.set_one(found.item);
            editor_state.focus_selection(block, found.range);
        }
        None => state.select(found.item),
    }
}

/// Recomputes find-on-page matches when the query or the page changed, and hands the
/// note highlights to the editor (rebuilding them if something reset the editor).
fn refresh_find(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) {
    let Some(find) = state.find.as_mut() else {
        if !editor_state.find_marks.is_empty() {
            editor_state.find_marks.clear();
            ctx.request_repaint();
        }
        return;
    };
    let lost = editor_state.find_marks.is_empty()
        && find
            .matches
            .iter()
            .any(|found| found.place.block().is_some());
    if !find.dirty && !lost {
        return;
    }
    if find.dirty {
        find.dirty = false;
        find.matches = find::collect(&page.canvas, &find.query);
        if find.from_search && find.matches.is_empty() {
            state.find = None;
            editor_state.find_marks.clear();
            ctx.request_repaint();
            return;
        }
        find.from_search = false;
        find.current = find.current.min(find.matches.len().saturating_sub(1));
    }
    editor_state.find_marks.clear();
    for (index, found) in find.matches.iter().enumerate() {
        let doc_id = format!("{}:{}", page.id, found.item);
        let key = match found.place {
            MatchPlace::Cell { block, row, col } => {
                Some(editor::table_cell_id(&doc_id, block, row, col))
            }
            MatchPlace::Block(block) => Some(editor::block_id(&doc_id, block)),
            _ => None,
        };
        if let Some(key) = key {
            editor_state
                .find_marks
                .entry(key)
                .or_default()
                .push((found.range.clone(), index == find.current));
        }
    }
    ctx.request_repaint();
}

/// Hands the current find match to the page to bring into view once matches are up to
/// date, leaving the caret in the find field.
fn reveal_find_match(state: &mut CanvasState) {
    if let Some(find) = state
        .find
        .as_mut()
        .filter(|find| find.reveal && !find.dirty)
    {
        find.reveal = false;
        if let Some(found) = find.current_match() {
            state.reveal = Some(Reveal {
                item: found.item,
                block: found.place.block(),
                focus: false,
            });
        }
    }
}

/// The selected items (in stacking order) and strokes.
fn selection_clip(page: &Page, state: &CanvasState) -> Clip {
    let mut items: Vec<CanvasItem> = page
        .canvas
        .items
        .iter()
        .filter(|item| state.selection.contains(item.id))
        .cloned()
        .collect();
    items.sort_by_key(|item| (item.z, item.id));
    let ink = state
        .selection
        .strokes
        .iter()
        .filter_map(|index| page.canvas.ink.get(*index))
        .cloned()
        .collect();
    Clip { items, ink }
}

/// What goes on the system clipboard: the copied text in reading order, or what was copied.
fn clip_text(clip: &Clip) -> String {
    let mut items: Vec<&CanvasItem> = clip.items.iter().collect();
    items.sort_by(|a, b| {
        (a.y, a.x)
            .partial_cmp(&(b.y, b.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let text = items
        .iter()
        .map(|item| item.kind.plain_text())
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        return text;
    }
    let count = (clip.items.len() + clip.ink.len()) as i64;
    match (clip.items.as_slice(), clip.ink.len()) {
        ([item], 0) => format!("[{}]", item.kind.label()),
        ([], 1) => t!("canvas-clip-ink").into_owned(),
        _ => t!("canvas-clip-items", count = count).into_owned(),
    }
}

fn clip_bounds(clip: &Clip) -> Rect {
    let mut bounds = Rect::NOTHING;
    for item in &clip.items {
        bounds = bounds.union(item.rect());
    }
    for stroke in &clip.ink {
        for point in &stroke.points {
            bounds.extend_with(pos2(point[0], point[1]));
        }
    }
    bounds
}

/// Copies the selection to the system clipboard (as text) and the page clipboard.
fn copy_selection(ctx: &egui::Context, page: &Page, state: &mut CanvasState) -> bool {
    let clip = selection_clip(page, state);
    if clip.items.is_empty() && clip.ink.is_empty() {
        return false;
    }
    let text = clip_text(&clip);
    ctx.copy_text(text.clone());
    state.clipboard_text = Some(text);
    state.clipboard = Some(clip);
    true
}

/// Pictures, files and recordings copied from another page get their own attachment on
/// this one, in notes too. A printout's PDF is copied once, for its icon and its pages
/// alike, so they still go together (its icon offers no second Insert Printout).
fn adopt_attachments(page_id: &str, library: &mut Library, clip: &mut Clip) {
    let mut copies: HashMap<String, String> = HashMap::new();
    let mut adopt = |attachment: &mut String| {
        if let Some(copy) = copies.get(attachment.as_str()) {
            *attachment = copy.clone();
            return;
        }
        if let Ok(Some(meta)) = library.attachment(attachment) {
            if meta.page_id != page_id {
                if let Ok(Some(copy)) = library.copy_attachment(page_id, &meta.id) {
                    copies.insert(std::mem::replace(attachment, copy.id.clone()), copy.id);
                }
            }
        }
    };
    for item in &mut clip.items {
        if let Some(attachment) = item.kind.attachment_mut() {
            adopt(attachment);
        } else if let Some(doc) = item.doc_mut() {
            for block in &mut doc.blocks {
                if let Some(attachment) = block.kind.attachment_mut() {
                    adopt(attachment);
                }
            }
        }
        if let Some(printout) = item.kind.printout_mut() {
            adopt(&mut printout.source);
        }
    }
}

/// Adds a copy of `clip` with fresh ids, keeping its layout, and selects it: with its
/// top-left corner `at` (Paste in a right-click menu, at the point clicked), else in the
/// middle of the view, or 24 points down-right of an identical copy already on the page
/// (Ctrl+V). Returns the copy as placed.
fn paste_clip(
    page: &mut Page,
    state: &mut CanvasState,
    mut clip: Clip,
    at: Option<Pos2>,
) -> Option<Clip> {
    if clip.items.is_empty() && clip.ink.is_empty() {
        return None;
    }
    state.push_undo(&page.canvas);
    let bounds = clip_bounds(&clip);
    let original_here = clip.items.iter().any(|item| {
        page.canvas
            .items
            .iter()
            .any(|other| other.rect() == item.rect())
    }) || clip
        .ink
        .iter()
        .any(|stroke| page.canvas.ink.contains(stroke));
    let offset = match at {
        Some(at) => at - bounds.min,
        None if original_here => vec2(24.0, 24.0),
        None => state.view_center() - bounds.center(),
    };
    // Pasted things land on the page, never above or left of its corner.
    let offset = limit_move(
        offset,
        clip.items.iter().map(CanvasItem::rect),
        clip.ink.iter().flat_map(|stroke| stroke.points.iter()),
    );
    let mut ids = Vec::new();
    for item in &mut clip.items {
        item.id = crate::model::next_block_id();
        if let Some(doc) = item.doc_mut() {
            for block in &mut doc.blocks {
                block.id = crate::model::next_block_id();
            }
        }
        item.x = (item.x + offset.x).round();
        item.y = (item.y + offset.y).round();
        ids.push(page.canvas.add(item.clone()));
    }
    let first_stroke = page.canvas.ink.len();
    for stroke in &mut clip.ink {
        for point in &mut stroke.points {
            point[0] += offset.x;
            point[1] += offset.y;
        }
        page.canvas.ink.push(stroke.clone());
    }
    state.selection = Selection {
        items: ids,
        strokes: (first_stroke..page.canvas.ink.len()).collect(),
    };
    state.active_text = None;
    Some(clip)
}

/// Pastes the page clipboard and remembers the pasted copy, so pasting again cascades.
fn paste_clipboard(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    mut clip: Clip,
    at: Option<Pos2>,
) -> bool {
    adopt_attachments(&page.id, library, &mut clip);
    match paste_clip(page, state, clip, at) {
        Some(pasted) => {
            state.clipboard = Some(pasted);
            true
        }
        None => false,
    }
}

/// A right-click menu's Paste: asks the platform for the clipboard, whose answer (a paste
/// event next frame) lands where the menu was opened (`menu_at`).
fn request_paste_at(ctx: &egui::Context, state: &mut CanvasState) {
    let now = ctx.input(|input| input.time);
    state.paste_at = state.menu_at.map(|at| (at, now));
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
}

/// Where a note made by a paste goes: its top-left corner at the point a right-click
/// menu's Paste was asked at, else around the middle of the view.
fn pasted_note_origin(state: &CanvasState, at: Option<Pos2>) -> Pos2 {
    match at {
        Some(at) => at.round(),
        None => {
            let center = state.view_center();
            pos2(
                (center.x - TEXT_WIDTH / 2.0).round(),
                (center.y - 40.0).round(),
            )
        }
    }
}

fn paste_text(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    text: &str,
    at: Option<Pos2>,
) -> bool {
    state.push_undo(&page.canvas);
    let blocks: Vec<Block> = text
        .lines()
        .map(|line| {
            let mut block = Block::paragraph(line.trim_end());
            // Pasted web addresses are links at once, as when pasting into a note.
            block.text.autolink_all();
            block
        })
        .collect();
    let doc = Document {
        blocks: if blocks.is_empty() {
            vec![Block::paragraph("")]
        } else {
            blocks
        },
    };
    let last = doc
        .blocks
        .last()
        .map(|block| (block.id, block.text.char_count()));
    let origin = pasted_note_origin(state, at);
    let item = CanvasItem::new(
        CanvasKind::Text { doc },
        origin.x,
        origin.y,
        TEXT_WIDTH,
        60.0,
    );
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = Some(id);
    state.tool = CanvasTool::Select;
    editor_state.reset();
    if let Some((block, end)) = last {
        editor_state.focus_block(block, end);
    }
    true
}

/// Rich text pasted onto the page (no note being typed in): a new note where `paste_text`
/// puts one, holding the blocks as they were copied, with Paste Options on it.
fn paste_rich_note(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    blocks: Vec<Block>,
    inline: bool,
    mode: PasteMode,
    at: Option<Pos2>,
) -> bool {
    // The page's undo step covers the whole paste; the new note has no history of its own.
    state.push_undo(&page.canvas);
    let empty = Document::default();
    let first = empty.blocks[0].id;
    let origin = pasted_note_origin(state, at);
    let item = CanvasItem::new(
        CanvasKind::Text { doc: empty },
        origin.x,
        origin.y,
        TEXT_WIDTH,
        60.0,
    );
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = Some(id);
    state.tool = CanvasTool::Select;
    editor_state.reset();
    let doc_id = format!("{}:{}", page.id, id);
    let Some(doc) = page.canvas.item_mut(id).and_then(CanvasItem::doc_mut) else {
        return true;
    };
    let at = crate::paste::PasteAt {
        block: first,
        cell: None,
        range: 0..0,
    };
    editor::paste_with_options(
        doc,
        &doc_id,
        editor_state,
        at,
        blocks,
        inline,
        mode,
        crate::model::InlineStyle::default(),
    );
    true
}

/// Hands a file to the system's own app for it.
#[cfg(not(test))]
fn open_with_system(path: &Path) -> std::io::Result<()> {
    open::that(path)
}

/// Tests never launch another app: they read what would have been opened.
#[cfg(test)]
fn open_with_system(path: &Path) -> std::io::Result<()> {
    OPENED.lock().unwrap().push(path.to_path_buf());
    Ok(())
}

#[cfg(test)]
pub(crate) static OPENED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Carries out what an item's menu, double-click, ribbon button or Enter asked for.
fn handle_item_action(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    library: &Library,
    toast: &mut Option<(String, bool)>,
) {
    let Some((id, action)) = state.item_action.take() else {
        return;
    };
    let Some(item) = page.canvas.item(id) else {
        return;
    };
    match action {
        ItemAction::CopyPrintoutText { all } => {
            let text = match item.kind.printout() {
                Some(printout) if all => printout_text(page, &printout.source),
                Some(printout) => printout.text.trim_end().to_string(),
                None => String::new(),
            };
            if text.trim().is_empty() {
                *toast = Some((t!("printout-toast-no-text").into_owned(), false));
            } else {
                ctx.copy_text(text);
            }
            return;
        }
        ItemAction::InsertPrintout => {
            if let Some(attachment) = item.kind.attachment() {
                state.printout_from_card = Some((id, attachment.to_string()));
            }
            return;
        }
        _ => {}
    }
    let attachment = match action {
        ItemAction::OpenOriginal => item
            .kind
            .printout()
            .map(|printout| printout.source.as_str()),
        _ => item.kind.attachment(),
    };
    let Some(attachment) = attachment else {
        return;
    };
    run_attachment_action(state, library, attachment, action, toast);
}

/// The note whose File block holds `attachment`.
fn note_with_file(page: &Page, attachment: &str) -> Option<u64> {
    page.canvas
        .items
        .iter()
        .find(|item| {
            item.doc().is_some_and(|doc| {
                doc.blocks.iter().any(|block| {
                    matches!(&block.kind, BlockKind::File { attachment: file, .. } if file == attachment)
                })
            })
        })
        .map(|item| item.id)
}

/// The text of every page of the printout made from `source`, in page order.
fn printout_text(page: &Page, source: &str) -> String {
    let mut pages: Vec<&PrintoutPage> = page
        .canvas
        .items
        .iter()
        .filter_map(|item| item.kind.printout())
        .filter(|printout| printout.source == source)
        .collect();
    pages.sort_by_key(|printout| printout.page);
    pages
        .iter()
        .map(|printout| printout.text.trim_end())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Whether a printout of the PDF `source` is on the page: its icon then offers no second
/// Insert Printout, which would stack new pages over the ones there.
fn has_printout(page: &Page, source: &str) -> bool {
    page.canvas
        .items
        .iter()
        .filter_map(|item| item.kind.printout())
        .any(|printout| printout.source == source)
}

/// Opens, saves or plays an attachment, for an item or a block in a note.
fn run_attachment_action(
    state: &mut CanvasState,
    library: &Library,
    attachment: &str,
    action: ItemAction,
    toast: &mut Option<(String, bool)>,
) {
    let recording = state
        .recording
        .as_ref()
        .is_some_and(|recorder| recorder.attachment == attachment);
    match action {
        // A clip still being recorded can't be opened, saved or played yet.
        _ if recording => {
            *toast = Some((t!("canvas-toast-stop-first").into_owned(), false));
        }
        ItemAction::Open | ItemAction::OpenOriginal => {
            let opened = library
                .attachment_open_copy(attachment)
                .map_err(|error| error.to_string())
                .and_then(|copy| {
                    open_with_system(&copy.path).map_err(|error| error.to_string())?;
                    Ok(copy)
                });
            *toast = Some(match opened {
                Ok(copy) if copy.edited => {
                    (t!("canvas-toast-opened-edited-copy").into_owned(), false)
                }
                Ok(_) => (t!("canvas-toast-opened-copy").into_owned(), false),
                Err(error) => (
                    t!("canvas-error-open-file", error = error.to_string()).into_owned(),
                    true,
                ),
            });
        }
        ItemAction::SaveAs => {
            let Ok(Some(meta)) = library.attachment(attachment) else {
                *toast = Some((t!("canvas-error-file-missing").into_owned(), true));
                return;
            };
            let Some(target) = rfd::FileDialog::new()
                .set_file_name(crate::db::attachment_file_name(&meta))
                .save_file()
            else {
                return;
            };
            match std::fs::copy(library.attachment_path(&meta.stored_path), &target) {
                Ok(_) => {
                    *toast = Some((
                        t!("canvas-toast-saved", path = target.display().to_string()).into_owned(),
                        false,
                    ))
                }
                Err(error) => {
                    *toast = Some((
                        t!("canvas-error-save-file", error = error.to_string()).into_owned(),
                        true,
                    ))
                }
            }
        }
        ItemAction::TogglePlay | ItemAction::Seek(_) => {
            if state
                .player
                .as_ref()
                .is_none_or(|player| player.attachment != attachment)
            {
                let Some(path) = library
                    .attachment(attachment)
                    .ok()
                    .flatten()
                    .map(|meta| library.attachment_path(&meta.stored_path))
                else {
                    *toast = Some((t!("canvas-error-recording-missing").into_owned(), true));
                    return;
                };
                match audio::Player::start(path, attachment) {
                    Ok(player) => state.player = Some(player),
                    Err(error) => {
                        *toast = Some((
                            t!("canvas-error-play-recording", error = error.to_string())
                                .into_owned(),
                            true,
                        ));
                        return;
                    }
                }
                // A new player is already playing from the start.
                if action == ItemAction::TogglePlay {
                    return;
                }
            }
            if let Some(player) = state.player.as_ref() {
                match action {
                    ItemAction::Seek(fraction) => player.seek(fraction),
                    _ => player.toggle(),
                }
            }
        }
        // Carried out by `handle_item_action`, before it gets here.
        ItemAction::CopyPrintoutText { .. } | ItemAction::InsertPrintout => {}
    }
}

fn delete_item(page: &mut Page, state: &mut CanvasState, id: u64) {
    state.push_undo(&page.canvas);
    page.canvas.remove(id);
    state.selection.remove(id);
    if state.active_text == Some(id) {
        state.active_text = None;
        state.text_snapshot = None;
    }
    if state.drag.as_ref().is_some_and(|drag| drag.moves(id)) {
        state.drag = None;
    }
    forget_deleted_equation(page, state);
}

/// Ends the session of an equation that was just deleted while it was typed. One inserted
/// in this session and never typed in also takes its creation step, which would now
/// restore nothing (`push_undo` left it out of the delete's own step).
fn forget_deleted_equation(page: &Page, state: &mut CanvasState) {
    if state
        .equation_edit
        .as_ref()
        .is_some_and(|edit| page.canvas.item(edit.id).is_none())
    {
        state.equation_edit = None;
        if state.undo.back() == Some(&page.canvas) {
            state.undo.pop_back();
        }
    }
}

fn delete_selection(page: &mut Page, state: &mut CanvasState) -> bool {
    if state.selection.is_empty() {
        return false;
    }
    state.push_undo(&page.canvas);
    for id in std::mem::take(&mut state.selection.items) {
        page.canvas.remove(id);
        if state.active_text == Some(id) {
            state.active_text = None;
            state.text_snapshot = None;
        }
    }
    let mut strokes = std::mem::take(&mut state.selection.strokes);
    strokes.sort_unstable();
    strokes.dedup();
    for index in strokes.into_iter().rev() {
        if index < page.canvas.ink.len() {
            page.canvas.ink.remove(index);
        }
    }
    state.drag = None;
    forget_deleted_equation(page, state);
    true
}

/// Brings the selected items to the front (or sends them to the back), keeping their
/// order among themselves.
fn restack_selection(page: &mut Page, state: &mut CanvasState, front: bool) -> bool {
    let mut chosen: Vec<(i32, u64)> = page
        .canvas
        .items
        .iter()
        .filter(|item| state.selection.contains(item.id))
        .map(|item| (item.z, item.id))
        .collect();
    if chosen.is_empty() {
        return false;
    }
    chosen.sort_unstable();
    let zs = page.canvas.items.iter().map(|item| item.z);
    let base = if front {
        zs.max().unwrap_or(0) + 1
    } else {
        zs.min().unwrap_or(0) - chosen.len() as i32
    };
    state.push_undo(&page.canvas);
    for (offset, (_, id)) in chosen.into_iter().enumerate() {
        if let Some(item) = page.canvas.item_mut(id) {
            item.z = base + offset as i32;
        }
    }
    true
}

/// Draw › Arrange › Bring Forward (`up`) or Send Backward: each selected item moves one
/// place up or down the stacking order, past one unselected item, keeping the order of
/// the selected items among themselves.
fn restack_step(page: &mut Page, state: &mut CanvasState, up: bool) -> bool {
    // Bottom to top, as the page paints them.
    let mut order: Vec<(i32, u64)> = page
        .canvas
        .items
        .iter()
        .map(|item| (item.z, item.id))
        .collect();
    order.sort_unstable();
    let mut ids: Vec<(u64, bool)> = order
        .iter()
        .map(|(_, id)| (*id, state.selection.contains(*id)))
        .collect();
    let mut moved = false;
    if up {
        // From the top down, so a run of selected items rises together.
        for index in (0..ids.len().saturating_sub(1)).rev() {
            if ids[index].1 && !ids[index + 1].1 {
                ids.swap(index, index + 1);
                moved = true;
            }
        }
    } else {
        for index in 1..ids.len() {
            if ids[index].1 && !ids[index - 1].1 {
                ids.swap(index - 1, index);
                moved = true;
            }
        }
    }
    if !moved {
        return false;
    }
    state.push_undo(&page.canvas);
    for (z, (id, _)) in ids.into_iter().enumerate() {
        if let Some(item) = page.canvas.item_mut(id) {
            item.z = z as i32 + 1;
        }
    }
    true
}

/// Review › Spelling's Change: the chars `range` of paragraph `block` in note `item`,
/// which hold `expected` (the misspelled word the pane shows), become `word`, keeping the
/// word's formatting. One undo step on the page; typing in the note is committed first as
/// its own. Returns false and changes nothing when the paragraph no longer holds
/// `expected` there (the note was edited since the pane found it).
pub fn replace_in_note(
    page: &mut Page,
    state: &mut CanvasState,
    item: u64,
    block: u64,
    range: Range<usize>,
    expected: &str,
    word: &str,
) -> bool {
    let Some((index, paragraph)) =
        page.canvas
            .item(item)
            .and_then(CanvasItem::doc)
            .and_then(|doc| {
                doc.block_index(block)
                    .map(|index| (index, &doc.blocks[index]))
            })
    else {
        return false;
    };
    let text = &paragraph.text.text;
    let holds_expected = paragraph.kind.is_textual()
        && !range.is_empty()
        && range.end <= text.chars().count()
        && text
            .chars()
            .skip(range.start)
            .take(range.len())
            .eq(expected.chars());
    if !holds_expected || expected == word {
        return false;
    }
    state.push_undo(&page.canvas);
    if let Some(doc) = page.canvas.item_mut(item).and_then(CanvasItem::doc_mut) {
        editor::replace_text(doc, index, range, word);
    }
    // A note that sizes itself to its text is measured again.
    state.remeasure = Some(item);
    true
}

/// Draw › Rotate: turns the selected ink and shapes a quarter turn, or mirrors them,
/// around the centre of their bounds, in one undo step. Other selected items stay where
/// they are (pictures turn on the Picture tab). What would end up off the page's top or
/// left edge is brought back onto it.
/// The selection holds ink or a shape, which `rotate_selection` turns.
fn turns_ink_or_shapes(page: &Page, state: &CanvasState) -> bool {
    state
        .selection
        .strokes
        .iter()
        .any(|index| *index < page.canvas.ink.len())
        || page.canvas.items.iter().any(|item| {
            state.selection.contains(item.id) && matches!(item.kind, CanvasKind::Shape { .. })
        })
}

/// Whether Rotate › 45° applies: something is selected, and it is all ink, lines and
/// arrows (a picture or a boxed shape has no angle to keep).
fn turns_45(page: &Page, state: &CanvasState) -> bool {
    turns_ink_or_shapes(page, state)
        && state.selection.items.iter().all(|id| {
            page.canvas.item(*id).is_some_and(|item| {
                matches!(
                    item.kind,
                    CanvasKind::Shape {
                        shape: ShapeKind::Line | ShapeKind::Arrow,
                        ..
                    }
                )
            })
        })
}

/// The selected page pictures.
fn selected_pictures(page: &Page, state: &CanvasState) -> Vec<u64> {
    page.canvas
        .items
        .iter()
        .filter(|item| state.selection.contains(item.id) && !item.is_background())
        .filter(|item| matches!(item.kind, CanvasKind::Image { .. }))
        .map(|item| item.id)
        .collect()
}

/// Draw › Rotate, as the desktop notebook's: the selected ink and shapes turn together
/// (`rotate_selection`), and each selected picture turns in its box as the Picture tab
/// turns it, all in one undo step. With no ink, shape or picture selected, the picture
/// clicked in the note being edited turns.
fn rotate_with_pictures(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &Library,
    rotation: Rotation,
) -> bool {
    let command = match rotation {
        Rotation::Right => PictureCmd::RotateRight,
        Rotation::Left => PictureCmd::RotateLeft,
        Rotation::FlipHorizontal => PictureCmd::FlipHorizontal,
        Rotation::FlipVertical => PictureCmd::FlipVertical,
        // A picture or a boxed shape has no angle: a 45° turn needs ink, lines and arrows
        // only, as the menu offers it.
        Rotation::Right45 | Rotation::Left45 => {
            return turns_45(page, state) && rotate_selection(page, state, rotation);
        }
    };
    let mut pictures: Vec<PictureTarget> = selected_pictures(page, state)
        .into_iter()
        .map(PictureTarget::Item)
        .collect();
    if pictures.is_empty() && !turns_ink_or_shapes(page, state) {
        if let Some(PictureInfo {
            target: target @ PictureTarget::Block { .. },
            ..
        }) = picture_info(page, state, editor_state)
        {
            pictures.push(target);
        }
    }
    if pictures.is_empty() {
        return rotate_selection(page, state, rotation);
    }
    // The command's one step, taken first: `rotate_selection` finds the page unchanged
    // since and takes no other.
    state.push_undo(&page.canvas);
    rotate_selection(page, state, rotation);
    for target in pictures {
        change_picture(page, state, library, target, command);
        forget_picture_note_history(page, editor_state, target);
    }
    true
}

fn rotate_selection(page: &mut Page, state: &mut CanvasState, rotation: Rotation) -> bool {
    let mut strokes: Vec<usize> = state
        .selection
        .strokes
        .iter()
        .copied()
        .filter(|index| *index < page.canvas.ink.len())
        .collect();
    // Each stroke turns once, however often the selection names it.
    strokes.sort_unstable();
    strokes.dedup();
    let shapes: Vec<u64> = page
        .canvas
        .items
        .iter()
        .filter(|item| state.selection.contains(item.id))
        .filter(|item| matches!(item.kind, CanvasKind::Shape { .. }))
        .map(|item| item.id)
        .collect();
    if strokes.is_empty() && shapes.is_empty() {
        return false;
    }
    let turning = Selection {
        items: shapes.clone(),
        strokes: strokes.clone(),
    };
    let pivot = selection_bounds(page, &turning).center();
    let rotate =
        |d: Vec2, a: f32| vec2(d.x * a.cos() - d.y * a.sin(), d.x * a.sin() + d.y * a.cos());
    let turn = |p: Pos2| -> Pos2 {
        let d = p - pivot;
        match rotation {
            // y grows downwards: a clockwise turn takes right to down.
            Rotation::Right => pivot + vec2(-d.y, d.x),
            Rotation::Left => pivot + vec2(d.y, -d.x),
            Rotation::FlipHorizontal => pos2(pivot.x - d.x, p.y),
            Rotation::FlipVertical => pos2(p.x, pivot.y - d.y),
            Rotation::Right45 => pivot + rotate(d, std::f32::consts::FRAC_PI_4),
            Rotation::Left45 => pivot + rotate(d, -std::f32::consts::FRAC_PI_4),
        }
    };
    state.push_undo(&page.canvas);
    for index in &strokes {
        for point in &mut page.canvas.ink[*index].points {
            let p = turn(pos2(point[0], point[1]));
            *point = [p.x, p.y];
        }
    }
    for id in &shapes {
        let Some(item) = page.canvas.item_mut(*id) else {
            continue;
        };
        let rect = item.rect();
        let CanvasKind::Shape {
            shape,
            flip_x,
            flip_y,
            ..
        } = &mut item.kind
        else {
            continue;
        };
        let turned = match (rotation, *shape) {
            // A line or arrow: its two ends turn, and its box and direction follow them,
            // as when it was drawn.
            (
                Rotation::Right | Rotation::Left | Rotation::Right45 | Rotation::Left45,
                ShapeKind::Line | ShapeKind::Arrow,
            ) => {
                let (start, end) = connector_ends(rect, *flip_x, *flip_y);
                let (start, end) = (turn(start), turn(end));
                (*flip_x, *flip_y) = (end.x < start.x, end.y < start.y);
                Rect::from_two_pos(start, end)
            }
            // Other shapes keep their middle and swap their width and height.
            (Rotation::Right | Rotation::Left, _) => {
                Rect::from_center_size(turn(rect.center()), vec2(rect.height(), rect.width()))
            }
            // A boxed shape has no angle: the menu never offers a 45° turn for it.
            (Rotation::Right45 | Rotation::Left45, _) => rect,
            // A mirror flips a line's direction or points a triangle the other way.
            (Rotation::FlipHorizontal, _) => {
                *flip_x = !*flip_x;
                Rect::from_center_size(turn(rect.center()), rect.size())
            }
            (Rotation::FlipVertical, _) => {
                *flip_y = !*flip_y;
                Rect::from_center_size(turn(rect.center()), rect.size())
            }
        };
        item.x = turned.left();
        item.y = turned.top();
        item.width = turned.width();
        item.height = turned.height();
    }
    // Keep it all on the page, as moving things does.
    let mut room = Vec2::splat(f32::INFINITY);
    for item in page
        .canvas
        .items
        .iter()
        .filter(|item| shapes.contains(&item.id))
    {
        room = room.min(vec2(item.x, item.y - ITEM_TOP));
    }
    for index in &strokes {
        for point in &page.canvas.ink[*index].points {
            room = room.min(vec2(point[0], point[1]));
        }
    }
    let shift = vec2((-room.x).max(0.0), (-room.y).max(0.0));
    if shift != Vec2::ZERO {
        for item in page
            .canvas
            .items
            .iter_mut()
            .filter(|item| shapes.contains(&item.id))
        {
            item.x += shift.x;
            item.y += shift.y;
        }
        for index in &strokes {
            for point in &mut page.canvas.ink[*index].points {
                point[0] += shift.x;
                point[1] += shift.y;
            }
        }
    }
    true
}

fn select_all(page: &Page, state: &mut CanvasState) {
    state.selection = Selection {
        // A background picture stays out of it, as it does from the band and the lasso.
        items: page
            .canvas
            .items
            .iter()
            .filter(|item| !item.is_background())
            .map(|item| item.id)
            .collect(),
        strokes: (0..page.canvas.ink.len()).collect(),
    };
    state.active_text = None;
}

/// How far `delta` may move items (at `items`) and ink (through `points`) before any of it
/// would leave the page: nothing goes left of its edge or above its top, and an item keeps
/// `ITEM_TOP` above it for its grab bar. What already sits past a limit is not pulled
/// further out.
fn limit_move<'a>(
    delta: Vec2,
    items: impl IntoIterator<Item = Rect>,
    points: impl IntoIterator<Item = &'a [f32; 2]>,
) -> Vec2 {
    let mut room = Vec2::splat(f32::INFINITY);
    for rect in items {
        room = room.min(vec2(rect.left(), rect.top() - ITEM_TOP));
    }
    for point in points {
        room = room.min(vec2(point[0], point[1]));
    }
    vec2(delta.x.max(-room.x.max(0.0)), delta.y.max(-room.y.max(0.0)))
}

/// Moves the selection by `delta`, never off the page's top or left edge. A burst of arrow
/// presses is one undo step.
fn nudge_selection(page: &mut Page, state: &mut CanvasState, delta: Vec2, now: f64) -> bool {
    if state.selection.is_empty() {
        return false;
    }
    let delta = limit_move(
        delta,
        page.canvas
            .items
            .iter()
            .filter(|item| state.selection.contains(item.id))
            .map(CanvasItem::rect),
        state
            .selection
            .strokes
            .iter()
            .filter_map(|index| page.canvas.ink.get(*index))
            .flat_map(|stroke| stroke.points.iter()),
    );
    if delta == Vec2::ZERO {
        return false;
    }
    let burst = state
        .last_nudge
        .as_ref()
        .is_some_and(|(at, nudged)| now - at <= NUDGE_GROUP_SECONDS && *nudged == state.selection);
    if !burst {
        state.push_undo(&page.canvas);
    }
    state.last_nudge = Some((now, state.selection.clone()));
    for item in page
        .canvas
        .items
        .iter_mut()
        .filter(|item| state.selection.contains(item.id))
    {
        item.x += delta.x;
        item.y += delta.y;
    }
    for index in &state.selection.strokes {
        if let Some(stroke) = page.canvas.ink.get_mut(*index) {
            for point in &mut stroke.points {
                point[0] += delta.x;
                point[1] += delta.y;
            }
        }
    }
    true
}

/// Recolours a shape's fill or a frise.
fn set_fill(item: &mut CanvasItem, color: Rgb) {
    match &mut item.kind {
        CanvasKind::Shape { fill, .. } => *fill = Some(color),
        CanvasKind::Timeline { color: c, .. } => *c = color,
        _ => {}
    }
}

/// Gives the selected shapes' outlines and the selected ink `color`.
fn recolor_selection(page: &mut Page, state: &mut CanvasState, color: Rgb) -> bool {
    let shapes: Vec<u64> = page
        .canvas
        .items
        .iter()
        .filter(|item| state.selection.contains(item.id))
        .filter(|item| matches!(&item.kind, CanvasKind::Shape { stroke, .. } if *stroke != color))
        .map(|item| item.id)
        .collect();
    let strokes: Vec<usize> = state
        .selection
        .strokes
        .iter()
        .copied()
        .filter(|index| {
            page.canvas
                .ink
                .get(*index)
                .is_some_and(|s| s.color != color)
        })
        .collect();
    if shapes.is_empty() && strokes.is_empty() {
        return false;
    }
    state.push_undo(&page.canvas);
    for id in shapes {
        if let Some(CanvasKind::Shape { stroke, .. }) =
            page.canvas.item_mut(id).map(|item| &mut item.kind)
        {
            *stroke = color;
        }
    }
    for index in strokes {
        page.canvas.ink[index].color = color;
    }
    true
}

/// Enter on a selected item: type in a note, edit a shape's label, or edit a frise.
fn open_item(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    id: u64,
) -> bool {
    let Some(item) = page.canvas.item(id) else {
        return false;
    };
    if let Some(block) = item.doc().and_then(|doc| doc.blocks.last()) {
        let end = block.text.char_count();
        state.active_text = Some(id);
        editor_state.reset();
        editor_state.focus_block(block.id, end);
        false
    } else {
        edit_item(page, state, id)
    }
}

/// Starts editing a shape's label or an equation, or opens the frise editor. Returns
/// whether that changed the page (see `begin_equation_edit`).
fn edit_item(page: &mut Page, state: &mut CanvasState, id: u64) -> bool {
    let Some(item) = page.canvas.item(id) else {
        return false;
    };
    match &item.kind {
        CanvasKind::Shape { .. } if !item.is_connector() => {
            state.editing_label = Some(id);
            state.focus_label = true;
        }
        CanvasKind::Timeline { milestones, .. } => {
            state.timeline_editor = Some(TimelineEditor {
                item_id: id,
                milestones: milestones.clone(),
            });
        }
        CanvasKind::File { .. } => state.item_action = Some((id, ItemAction::Open)),
        CanvasKind::Audio { .. } => state.item_action = Some((id, ItemAction::TogglePlay)),
        CanvasKind::Equation { .. } => return begin_equation_edit(page, state, id),
        _ => {}
    }
    false
}

fn focus_first_text(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) -> bool {
    let target = page
        .canvas
        .items
        .iter()
        .filter(|item| item.doc().is_some())
        .min_by(|a, b| {
            (a.y, a.x)
                .partial_cmp(&(b.y, b.x))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|item| {
            item.doc()
                .and_then(|doc| doc.blocks.first())
                .map(|block| (item.id, block.id))
        });
    if let Some((id, block)) = target {
        state.active_text = Some(id);
        state.selection.set_one(id);
        state.tool = CanvasTool::Select;
        editor_state.reset();
        editor_state.focus_block(block, 0);
        return false;
    }
    // The first note's text starts under the title's first letter, its frame to the left,
    // as in the desktop notebook.
    add_note(
        page,
        state,
        editor_state,
        pos2(PAGE_LEFT - NOTE_PAD_X, PAGE_CONTENT_TOP),
    );
    true
}

/// What the ribbon shows for the page: the tool and pen, the selection, undo and the
/// paper. (Recording is the app's to report: it carries on across pages.)
#[derive(Debug, Clone)]
pub struct RibbonSnapshot {
    pub tool: CanvasTool,
    pub eraser: EraserMode,
    pub ink_to_shape: bool,
    pub ink_color: Rgb,
    pub ink_width: f32,
    pub selection_count: usize,
    /// The kind of the only selected item, when exactly one is selected.
    pub single_kind: Option<SingleKind>,
    /// The selection holds a shape or frise (Fill applies).
    pub fillable: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The pen preset last picked, an index into `pens`.
    pub pen: Option<usize>,
    pub pens: Vec<PenPreset>,
    /// Draw › pen ▾ › Recent Colors.
    pub recent_colors: Vec<Rgb>,
    pub lock_drawing: bool,
    /// The selection holds ink, a shape or a picture (Rotate applies).
    pub rotatable: bool,
    /// The selection holds only ink, lines and arrows (Rotate › 45° applies).
    pub turns_45: bool,
    /// The page's zoom (1.0 is 100 %), for View › Zoom Level.
    pub zoom: f32,
}

/// The selected item's kind, as far as the ribbon cares: the contextual tabs and the
/// commands that act on one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleKind {
    Equation,
    Audio,
    File,
    Picture,
    Other,
}

/// The ribbon's view of the page this frame.
pub fn ribbon_snapshot(page: &Page, state: &CanvasState) -> RibbonSnapshot {
    let single_kind = state
        .selection
        .primary()
        .and_then(|id| page.canvas.item(id))
        .map(|item| match item.kind {
            CanvasKind::Equation { .. } => SingleKind::Equation,
            CanvasKind::Audio { .. } => SingleKind::Audio,
            CanvasKind::File { .. } => SingleKind::File,
            CanvasKind::Image { .. } => SingleKind::Picture,
            _ => SingleKind::Other,
        });
    // As `CanvasState::undo` sees it: typing in a note or an equation that has not yet
    // made its own step is undone first. Those comparisons only run on an empty stack.
    let can_undo = !state.undo.is_empty()
        || state
            .text_snapshot
            .as_ref()
            .is_some_and(|(owner, snapshot)| {
                state.typing_needs_step(snapshot, *owner, &page.canvas)
            })
        || state
            .equation_edit
            .as_ref()
            .is_some_and(|edit| state.equation_needs_step(edit, &page.canvas));
    RibbonSnapshot {
        tool: state.tool,
        eraser: state.eraser,
        ink_to_shape: state.ink_to_shape,
        ink_color: state.ink_color,
        ink_width: state.ink_width,
        selection_count: state.selection.len(),
        single_kind,
        fillable: !colorable_selection(page, state).is_empty(),
        can_undo,
        can_redo: !state.redo.is_empty(),
        pen: state.pen,
        pens: state.pens.clone(),
        recent_colors: state.recent_colors.clone(),
        lock_drawing: state.lock_drawing,
        rotatable: turns_ink_or_shapes(page, state) || !selected_pictures(page, state).is_empty(),
        turns_45: turns_45(page, state),
        zoom: state.zoom,
    }
}

/// The note being typed in, if it is still on the page: what the ribbon's text
/// commands format.
fn active_note(page: &Page, state: &CanvasState) -> Option<u64> {
    state.active_text.filter(|id| {
        page.canvas
            .item(*id)
            .is_some_and(|item| item.doc().is_some())
    })
}

/// Whether the caret of the note being typed in is in its text, not on a picture picked
/// in it (nor nowhere, once that picture was deleted).
fn caret_in_note_text(page: &Page, state: &CanvasState, editor_state: &EditorState) -> bool {
    active_note(page, state)
        .and_then(|id| page.canvas.item(id))
        .and_then(CanvasItem::doc)
        .and_then(|doc| {
            editor_state
                .active_index(doc)
                .map(|index| &doc.blocks[index].kind)
        })
        .is_some_and(|kind| !matches!(kind, BlockKind::Image { .. }))
}

/// Runs a text command on the note being typed in, as the ribbon's formatting buttons
/// do. Returns whether its text changed.
fn run_text_command(
    ctx: &egui::Context,
    page: &mut Page,
    state: &CanvasState,
    editor_state: &mut EditorState,
    command: ToolbarCommand,
) -> bool {
    let Some(doc) = active_note(page, state)
        .and_then(|id| page.canvas.item_mut(id))
        .and_then(CanvasItem::doc_mut)
    else {
        return false;
    };
    let result = editor::apply_command(ctx, doc, editor_state, command);
    if result.interacted {
        // The click took focus from the text; give it back with the caret in place.
        editor_state.restore_focus();
    }
    result.changed
}

/// Where something inserted with no note being typed in goes: the last place empty
/// paper was clicked, while it is in view, else near the middle of the view.
fn insert_point(state: &CanvasState) -> Pos2 {
    state
        .last_click
        .filter(|at| state.visible.is_some_and(|view| view.contains(*at)))
        .unwrap_or_else(|| state.view_center() - vec2(TEXT_WIDTH / 2.0, 40.0))
}

/// The note something is inserted into: the one being typed in, else a new one at the
/// insertion point (one page undo step) with the caret in its empty paragraph.
fn note_to_insert_into(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) -> Option<u64> {
    if let Some(id) = active_note(page, state) {
        return Some(id);
    }
    add_text(page, state, editor_state, insert_point(state));
    let id = state.active_text?;
    // add_text reset the editor; its one empty paragraph is where the caret is.
    editor_state.active_block = page.canvas.item(id)?.doc()?.blocks.first().map(|b| b.id);
    Some(id)
}

/// Puts `blocks` into the note being typed in at its caret, or into a new note at the
/// insertion point. Returns the note's id.
fn insert_into_note(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    blocks: Vec<Block>,
) -> Option<u64> {
    let id = note_to_insert_into(page, state, editor_state)?;
    let doc_id = format!("{}:{}", page.id, id);
    let doc = page.canvas.item_mut(id)?.doc_mut()?;
    let inserted = editor::insert_blocks_at_caret(ctx, doc, &doc_id, editor_state, blocks);
    state.remeasure = Some(id);
    inserted.then_some(id)
}

/// Types `text` (a date, a time, a symbol) at the caret of the note being typed in, or
/// into a new note at the insertion point. Returns whether the page changed.
fn type_into_note(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    text: &str,
) -> bool {
    let typing = active_note(page, state).is_some();
    let Some(id) = note_to_insert_into(page, state, editor_state) else {
        return false;
    };
    let doc_id = format!("{}:{}", page.id, id);
    let typed = page
        .canvas
        .item_mut(id)
        .and_then(CanvasItem::doc_mut)
        .is_some_and(|doc| editor::insert_text(ctx, doc, &doc_id, editor_state, text));
    state.remeasure = Some(id);
    // A new note changed the page even if nothing could be typed into it.
    typed || !typing
}

/// Insert › Equation and Alt+=: an empty equation block at the caret, or in a new note
/// at the insertion point, with its source field taking the keyboard.
fn insert_equation(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) -> bool {
    insert_equation_with(ctx, page, state, editor_state, String::new())
}

/// Inserts an equation of `source` (blank for Insert New Equation, or one of Office's
/// ready-made ones) at the caret, and starts editing it, as the desktop notebook selects a new one.
fn insert_equation_with(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    source: String,
) -> bool {
    // The equation being typed in the note ends first, as when the caret leaves it, so a
    // blank one goes rather than stay behind the new one.
    let ended = active_note(page, state)
        .and_then(|id| page.canvas.item_mut(id))
        .and_then(CanvasItem::doc_mut)
        .is_some_and(|doc| editor::end_equation(doc, editor_state));
    let block = Block::new(BlockKind::Equation {
        source,
        scale: crate::model::default_equation_size(),
    });
    let id = block.id;
    if insert_into_note(ctx, page, state, editor_state, vec![block]).is_none() {
        return ended;
    }
    editor_state.start_equation(id);
    true
}

/// Widens note `id` round something `width` wide put into it (a picture, a file or
/// recording card) and its inner padding, as the desktop notebook's container grows round a picture.
/// The note keeps that width from then on: its text alone no longer sizes it.
fn widen_note(page: &mut Page, id: u64, width: f32) {
    if let Some(item) = page.canvas.item_mut(id) {
        item.width = item.width.max((width + 2.0 * NOTE_PAD_X).ceil());
        item.auto_width = false;
    }
}

/// The pointer on the page, in page units, while it is over the page as last drawn
/// (`canvas_keys` runs before the page is drawn this frame).
fn page_pointer(ctx: &egui::Context, state: &CanvasState) -> Option<Pos2> {
    let global = ctx.input(|input| input.pointer.hover_pos())?;
    if !state.viewport.contains(global) {
        return None;
    }
    let to_page = ctx.layer_transform_from_global(state.scene_layer?)?;
    let pos = to_page * global;
    state.visible.filter(|view| view.contains(pos)).map(|_| pos)
}

/// The only selected item, when it is still on the page.
fn selected_item(page: &Page, state: &CanvasState) -> Option<u64> {
    state
        .selection
        .primary()
        .filter(|id| page.canvas.item(*id).is_some())
}

/// Runs a ribbon command on this page: the bodies of the old band buttons, unchanged.
/// Returns whether the page changed; commands that do not apply here do nothing.
#[allow(clippy::too_many_arguments)]
pub fn run_ribbon_cmd(
    ui: &mut Ui,
    cmd: Cmd,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let mut changed = false;
    // Any other command keeps a crop under way first, as a click elsewhere does.
    if !matches!(cmd, Cmd::PictureEdit(PictureCmd::Crop)) {
        changed |= commit_crop(page, state);
    }
    let typing = active_note(page, state).is_some();
    // A picture picked in the note (or just deleted from it) leaves its text without the
    // caret: Ctrl+Z is then the page's, and so is the ribbon's Undo. The picture's own
    // commands are page steps.
    let on_picture = typing && !caret_in_note_text(page, state, editor_state);
    match cmd {
        Cmd::Text(command) => {
            changed |= run_text_command(ui.ctx(), page, state, editor_state, command);
        }
        Cmd::Undo if on_picture => changed |= state.undo(page),
        Cmd::Redo if on_picture => changed |= state.redo(page),
        // While typing in a note these act on its text (its own undo, the paragraph);
        // otherwise Undo, Redo and Delete act on the page.
        Cmd::Undo | Cmd::Redo | Cmd::Delete if typing => {
            let command = match cmd {
                Cmd::Undo => ToolbarCommand::Undo,
                Cmd::Redo => ToolbarCommand::Redo,
                _ => ToolbarCommand::DeleteBlock,
            };
            changed |= run_text_command(ui.ctx(), page, state, editor_state, command);
        }
        Cmd::Undo => changed |= state.undo(page),
        Cmd::Redo => changed |= state.redo(page),
        Cmd::Delete => changed |= delete_selection(page, state),
        // Home › Clipboard: the text's while typing in a note, else the page's items.
        Cmd::Paste(mode) => {
            // The platform answers with a paste event next frame, which goes the way asked
            // for: into the note (its text edit gets the focus back), else onto the page,
            // as Ctrl+V does.
            editor_state.paste_mode = Some(mode);
            editor_state.paste_mode_at = ui.input(|input| input.time);
            if typing {
                changed |=
                    run_text_command(ui.ctx(), page, state, editor_state, ToolbarCommand::Paste);
            } else {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        }
        Cmd::Cut | Cmd::Copy => {
            let cut = cmd == Cmd::Cut;
            if typing {
                let command = if cut {
                    ToolbarCommand::Cut
                } else {
                    ToolbarCommand::Copy
                };
                changed |= run_text_command(ui.ctx(), page, state, editor_state, command);
                if std::mem::take(&mut editor_state.copied) {
                    // Note text copied: the page forgets its item copy.
                    state.clipboard_text = None;
                }
            } else if copy_selection(ui.ctx(), page, state) {
                editor_state.rich_clip = None;
                if cut {
                    changed |= delete_selection(page, state);
                }
            }
        }
        // One click picks up the formatting at the caret, a double-click keeps the painter
        // on; a click while it is on puts it away. The caret stays in the note.
        Cmd::FormatPainter => {
            let double = ui.input(|input| {
                input
                    .pointer
                    .button_double_clicked(egui::PointerButton::Primary)
            });
            if let Some(doc) = active_note(page, state)
                .and_then(|id| page.canvas.item(id))
                .and_then(CanvasItem::doc)
            {
                editor::format_painter(doc, editor_state, double);
            } else if editor_state.painter.is_some() && !double {
                editor_state.painter = None;
            }
            editor_state.restore_focus();
        }
        // Insert: at the caret of the note being typed in, else in a new note at the
        // insertion point (the desktop notebook's container you are typing in).
        Cmd::Picture => {
            let Some(path) = pick_file(true) else {
                return false;
            };
            match library.import_image(&page.id, &path) {
                Ok(attachment) => {
                    let size = picture_size(images::upright_size(&path));
                    let block = Block::new(BlockKind::Image {
                        attachment: attachment.id,
                        width: 1.0,
                        meta: PictureMeta::default(),
                    });
                    let note = insert_into_note(ui.ctx(), page, state, editor_state, vec![block]);
                    if let Some(id) = note {
                        widen_note(page, id, size.x);
                    }
                    changed = true;
                }
                Err(error) => {
                    *toast = Some((
                        t!("canvas-error-insert-picture", error = error.to_string()).into_owned(),
                        true,
                    ));
                }
            }
        }
        // Each page of a PDF as a picture under its icon, where the next insert goes. The
        // pages land once the worker has drawn them all (`poll_printout`).
        Cmd::FilePrintout => {
            insert_printout_dialog(ui.ctx(), page, state, library, toast);
        }
        Cmd::Attachment => {
            let Some(path) = pick_file(false) else {
                return false;
            };
            match library.import_file(&page.id, &path) {
                Ok(attachment) => {
                    let block = Block::new(BlockKind::File {
                        attachment: attachment.id,
                        name: attachment.file_name,
                        size: attachment.byte_size,
                    });
                    let note = insert_into_note(ui.ctx(), page, state, editor_state, vec![block]);
                    if let Some(id) = note {
                        widen_note(page, id, FILE_SIZE.x);
                    }
                    changed = true;
                }
                Err(error) => {
                    *toast = Some((
                        t!("canvas-error-attach-file", error = error.to_string()).into_owned(),
                        true,
                    ));
                }
            }
        }
        Cmd::RecordAudio => {
            if state.recording.is_some() {
                match state.finish_recording(Some(&mut *page), editor_state, library) {
                    Ok(done) => changed |= done,
                    Err(error) => {
                        *toast = Some((
                            t!("error-finish-recording", error = error.to_string()).into_owned(),
                            true,
                        ));
                    }
                }
            } else {
                changed |= start_recording(ui.ctx(), page, state, editor_state, library, toast);
            }
            if state.active_text.is_some() {
                editor_state.restore_focus();
            }
        }
        Cmd::Timeline => {
            add_frise(page, state);
            changed = true;
        }
        // The ribbon stays on its tab: the Table tab shows beside it without taking it.
        Cmd::InsertTable { rows, cols } => {
            changed |= insert_table_here(
                ui,
                page,
                state,
                editor_state,
                usize::from(rows),
                usize::from(cols),
            );
        }
        Cmd::Equation => changed |= insert_equation(ui.ctx(), page, state, editor_state),
        Cmd::ReadyEquation(index) => {
            if let Some((_, source)) = math::READY_EQUATIONS.get(usize::from(index)) {
                let source = (*source).to_string();
                changed |= insert_equation_with(ui.ctx(), page, state, editor_state, source);
            }
        }
        Cmd::Date | Cmd::Time | Cmd::DateTime | Cmd::Symbol(_) => {
            let text = match cmd {
                Cmd::Time => editor::now_time(),
                Cmd::DateTime => editor::now_date_time(),
                Cmd::Symbol(symbol) => symbol.to_string(),
                _ => editor::today(),
            };
            // With the caret in the title, it goes in there, as in the desktop notebook.
            changed |= if state.title_active && !page.title_hidden {
                editor::type_into_title(ui.ctx(), page, &text)
            } else {
                type_into_note(ui.ctx(), page, state, editor_state, &text)
            };
        }
        Cmd::Link => {
            let opened = active_note(page, state).is_some_and(|id| {
                let doc_id = format!("{}:{}", page.id, id);
                page.canvas
                    .item(id)
                    .and_then(CanvasItem::doc)
                    .is_some_and(|doc| editor::begin_link(ui.ctx(), doc, &doc_id, editor_state))
            });
            if !opened {
                *toast = Some((t!("canvas-toast-link-where").into_owned(), false));
            }
        }
        Cmd::Tool(tool) => {
            surrender_text_focus(ui);
            state.active_text = None;
            state.tool = tool;
        }
        Cmd::Eraser(mode) => {
            surrender_text_focus(ui);
            state.eraser = mode;
            state.tool = CanvasTool::Eraser;
            state.active_text = None;
        }
        Cmd::InkToShape => {
            state.ink_to_shape = !state.ink_to_shape;
            if state.ink_to_shape {
                surrender_text_focus(ui);
                state.active_text = None;
                state.tool = CanvasTool::Pen;
            }
        }
        // A colour or width picked while drawing with a pen preset retunes that preset.
        Cmd::Ink(color) => {
            state.ink_color = color;
            changed |= recolor_selection(page, state, color);
            if retune_pen(state, |pen| pen.color = color) {
                store_pens(library, &state.pens, toast);
            }
            if remember_pen_color(&mut state.recent_colors, color) {
                if let Err(error) = save_recent_colors(library, &state.recent_colors) {
                    *toast = Some((
                        t!("error-save-setting", error = error.to_string()).into_owned(),
                        true,
                    ));
                }
            }
        }
        Cmd::PenWidth(width) => {
            state.ink_width = width;
            if retune_pen(state, |pen| pen.width = width) {
                store_pens(library, &state.pens, toast);
            }
        }
        // Draw's pen gallery: picking a pen takes up its tool, colour and width.
        Cmd::PenPreset(index) => {
            if index < state.pens.len() {
                surrender_text_focus(ui);
                select_pen(state, index);
            }
        }
        Cmd::AddPen { highlighter, color } => {
            surrender_text_focus(ui);
            state.pens.push(PenPreset {
                highlighter,
                color,
                width: if highlighter { 3.0 } else { 2.0 },
            });
            select_pen(state, state.pens.len() - 1);
            store_pens(library, &state.pens, toast);
        }
        Cmd::RemovePen => {
            // The gallery always keeps one pen.
            let Some(index) = state
                .pen
                .filter(|index| *index < state.pens.len() && state.pens.len() > 1)
            else {
                return false;
            };
            surrender_text_focus(ui);
            state.pens.remove(index);
            select_pen(state, index.saturating_sub(1));
            store_pens(library, &state.pens, toast);
        }
        Cmd::LockDrawing => {
            state.lock_drawing = !state.lock_drawing;
            // A setting for the next shapes: typing in the note goes on.
            if typing {
                editor_state.restore_focus();
            }
        }
        Cmd::Shape(shape) => {
            state.shape_kind = shape;
            state.tool = CanvasTool::Shape(shape);
            state.active_text = None;
        }
        Cmd::Fill(color) => {
            // One undo step for every selected shape and frise.
            let colorable = colorable_selection(page, state);
            if colorable.is_empty() {
                return false;
            }
            state.push_undo(&page.canvas);
            for id in &colorable {
                if let Some(item) = page.canvas.item_mut(*id) {
                    set_fill(item, color);
                }
            }
            changed = true;
        }
        Cmd::BringToFront | Cmd::SendToBack | Cmd::BringForward | Cmd::SendBackward => {
            changed |= match cmd {
                Cmd::BringToFront => restack_selection(page, state, true),
                Cmd::SendToBack => restack_selection(page, state, false),
                _ => restack_step(page, state, cmd == Cmd::BringForward),
            };
            // The note being typed in moved up or down the stack; its caret stays.
            if active_note(page, state).is_some() {
                editor_state.restore_focus();
            }
        }
        Cmd::Rotate(rotation) => {
            changed |= rotate_with_pictures(page, state, editor_state, library, rotation);
        }
        // Crop again keeps the crop; on a page picture, it starts cropping it.
        Cmd::PictureEdit(PictureCmd::Crop) => {
            if state.cropping.is_some() {
                changed |= commit_crop(page, state);
            } else {
                match picture_info(page, state, editor_state) {
                    Some(PictureInfo {
                        target: PictureTarget::Item(id),
                        ..
                    }) => {
                        if !start_crop(page, state, id) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
        }
        Cmd::PictureEdit(command) => match picture_info(page, state, editor_state) {
            Some(info) => {
                if run_picture_cmd(page, state, library, info.target, command) {
                    forget_picture_note_history(page, editor_state, info.target);
                    changed = true;
                }
            }
            None => return false,
        },
        // The Table tab: on the table holding the caret in the note being typed in.
        Cmd::TableEdit(command) => {
            let Some(id) = active_note(page, state) else {
                return false;
            };
            let doc_id = format!("{}:{}", page.id, id);
            let Some(doc) = page.canvas.item_mut(id).and_then(CanvasItem::doc_mut) else {
                return false;
            };
            changed |= editor::table_command(ui.ctx(), doc, &doc_id, editor_state, command);
            // The click took focus from the cell; give it back, or to the cell the command
            // put the caret in.
            editor_state.restore_focus();
        }
        Cmd::EditEquation => {
            let Some(id) = selected_item(page, state).filter(|id| {
                page.canvas
                    .item(*id)
                    .is_some_and(|item| matches!(item.kind, CanvasKind::Equation { .. }))
            }) else {
                return false;
            };
            // While it is being typed, the click only ends the session (the source box
            // loses focus); restarting it here would drop its undo snapshot.
            let editing = state
                .equation_edit
                .as_ref()
                .is_some_and(|edit| edit.id == id);
            if !editing {
                changed |= begin_equation_edit(page, state, id);
            }
        }
        Cmd::CopyLatex => {
            // The equation block being typed (in the active note; block ids are unique
            // across notes), else the selected equation item.
            let latex = if editor_state.equation_block.is_some() {
                active_note(page, state)
                    .and_then(|id| page.canvas.item(id))
                    .into_iter()
                    .chain(&page.canvas.items)
                    .filter_map(CanvasItem::doc)
                    .find_map(|doc| editor::equation_latex(doc, editor_state))
            } else {
                selected_item(page, state)
                    .and_then(|id| page.canvas.item(id))
                    .and_then(|item| match &item.kind {
                        CanvasKind::Equation { source, .. } if !source.trim().is_empty() => {
                            Some(math::latex_of(source))
                        }
                        _ => None,
                    })
            };
            if let Some(latex) = latex {
                ui.ctx().copy_text(latex);
            }
        }
        Cmd::PlayPause | Cmd::SaveAs => {
            // Play for a selected recording, Save as for a recording or an attached
            // file.
            let id = selected_item(page, state).filter(|id| {
                page.canvas.item(*id).is_some_and(|item| match item.kind {
                    CanvasKind::Audio { .. } => true,
                    CanvasKind::File { .. } => cmd == Cmd::SaveAs,
                    _ => false,
                })
            });
            if let Some(id) = id {
                let action = if cmd == Cmd::PlayPause {
                    ItemAction::TogglePlay
                } else {
                    ItemAction::SaveAs
                };
                state.item_action = Some((id, action));
            }
        }
        Cmd::ZoomOut => zoom_scene(state, 1.0 / 0.85),
        Cmd::ZoomIn => zoom_scene(state, 0.85),
        Cmd::Zoom100 => zoom_scene(state, state.zoom),
        Cmd::Zoom(percent) => zoom_to(state, f32::from(percent) / 100.0),
        Cmd::PageWidth => page_width(page, state),
        // The app's commands (recording, views, dialogs, the ribbon itself).
        _ => {}
    }
    changed
}

/// Takes up pen preset `index`: its tool (pen or highlighter), colour and width. Typing
/// stops, as with any drawing tool.
fn select_pen(state: &mut CanvasState, index: usize) {
    let Some(pen) = state.pens.get(index).copied() else {
        return;
    };
    state.active_text = None;
    state.tool = if pen.highlighter {
        CanvasTool::Highlighter
    } else {
        CanvasTool::Pen
    };
    state.ink_color = pen.color;
    state.ink_width = pen.width;
    state.pen = Some(index);
}

/// Applies `change` to the pen preset being drawn with, if any (a preset is current and
/// its tool is on). Returns whether the preset changed.
fn retune_pen(state: &mut CanvasState, change: impl FnOnce(&mut PenPreset)) -> bool {
    if !matches!(state.tool, CanvasTool::Pen | CanvasTool::Highlighter) {
        return false;
    }
    let Some(pen) = state.pen.and_then(|index| state.pens.get_mut(index)) else {
        return false;
    };
    let before = *pen;
    change(pen);
    *pen != before
}

/// Saves Draw's pens, reporting a failure in the toast.
fn store_pens(library: &mut Library, pens: &[PenPreset], toast: &mut Option<(String, bool)>) {
    if let Err(error) = save_pens(library, pens) {
        *toast = Some((
            t!("error-save-setting", error = error.to_string()).into_owned(),
            true,
        ));
    }
}

/// The erasers as menu entries, the current one marked. Returns the one clicked.
pub(crate) fn eraser_list(ui: &mut Ui, current: EraserMode) -> Option<EraserMode> {
    ui.set_min_width(150.0);
    let mut picked = None;
    for mode in EraserMode::ALL {
        let item = egui::Button::selectable(
            mode == current,
            RichText::new(mode.label()).font(theme::ui(12.0)),
        );
        if ui.add(item).clicked() {
            picked = Some(mode);
            ui.close();
        }
    }
    picked
}

/// A swatch in the ribbon's Fill menu.
const FILL_MENU_SWATCH: f32 = 20.0;

/// The shapes as menu entries, the current one marked. Returns the one clicked.
pub(crate) fn shape_list(ui: &mut Ui, current: ShapeKind) -> Option<ShapeKind> {
    let mut picked = None;
    for shape in ShapeKind::ALL {
        if ui
            .add(
                egui::Button::new(RichText::new(shape.label()).font(theme::ui(12.0)))
                    .frame(false)
                    .selected(shape == current),
            )
            .clicked()
        {
            picked = Some(shape);
            ui.close();
        }
    }
    picked
}

/// A section heading in Draw's pen menus ("Thickness", "Colours"), bold as the desktop notebook's.
fn pen_menu_heading(ui: &mut Ui, text: &str, t: Tokens) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).font(theme::semibold(12.0)).color(t.ink));
    ui.add_space(2.0);
}

/// the desktop notebook's pen colours as a 4 × 4 grid of swatches, `current` framed. Returns the one
/// clicked.
pub(crate) fn pen_color_grid(ui: &mut Ui, current: Option<Rgb>, t: Tokens) -> Option<Rgb> {
    pen_swatches(ui, &PEN_COLORS.map(|(color, _)| color), current, t)
}

/// Colours as the pen menus show them: 28 px swatches, four to a row, `current` framed.
/// Returns the one clicked.
fn pen_swatches(ui: &mut Ui, colors: &[Rgb], current: Option<Rgb>, t: Tokens) -> Option<Rgb> {
    const SWATCH: f32 = 28.0;
    let mut picked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for row in colors.chunks(4) {
            ui.horizontal(|ui| {
                for &color in row {
                    let selected = current == Some(color);
                    if editor::color_swatch(ui, color, SWATCH, selected, t).clicked() {
                        picked = Some(color);
                    }
                }
            });
        }
    });
    picked
}

/// One of Pen settings' thickness buttons: a dot as wide as the line it draws, ringed
/// when it is the current width.
fn thickness_dot(ui: &mut Ui, width: f32, current: bool, label: &str, t: Tokens) -> egui::Response {
    const SIZE: f32 = 24.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    painter.circle_filled(rect.center(), 1.0 + width * 1.1, t.ink);
    if current {
        painter.circle_stroke(rect.center(), SIZE / 2.0 - 2.0, Stroke::new(1.5, t.ink));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, label));
    response.on_hover_text(label)
}

/// Pen settings' "+" after the thickness dots, as the desktop notebook's: it shows or hides the custom
/// thickness slider (`open`), and is ringed while the pen draws at a width no dot has.
fn custom_thickness_button(
    ui: &mut Ui,
    open: bool,
    custom: bool,
    label: &str,
    t: Tokens,
) -> egui::Response {
    const SIZE: f32 = 24.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::click());
    let painter = ui.painter();
    if open {
        painter.rect_filled(rect, CornerRadius::same(4), t.nav_selected);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    let center = rect.center();
    let stroke = Stroke::new(1.5, t.ink);
    painter.line_segment([center - vec2(6.0, 0.0), center + vec2(6.0, 0.0)], stroke);
    painter.line_segment([center - vec2(0.0, 6.0), center + vec2(0.0, 6.0)], stroke);
    if custom {
        painter.circle_stroke(center, SIZE / 2.0 - 2.0, stroke);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, label));
    response.on_hover_text(label)
}

/// Draw › the pens gallery's ▾ (the desktop notebook's pen settings): the thickness and colour of the
/// pen being drawn with, then Remove Pen.
pub(crate) fn pen_settings(ui: &mut Ui, snap: &RibbonSnapshot, t: Tokens) -> Option<Cmd> {
    let mut picked = None;
    let highlighter = snap
        .pen
        .and_then(|index| snap.pens.get(index))
        .map_or(snap.tool == CanvasTool::Highlighter, |pen| pen.highlighter);
    pen_menu_heading(ui, &t!("ribbon-thickness"), t);
    let custom_id = Id::unique("folio-pen-custom");
    let mut custom_open = ui.data(|data| data.get_temp::<bool>(custom_id)) == Some(true);
    let preset = PEN_THICKNESS
        .iter()
        .find(|(width, _)| (snap.ink_width - width).abs() < 0.01);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (width, _) in PEN_THICKNESS {
            let label = PenPreset {
                highlighter,
                color: snap.ink_color,
                width,
            }
            .millimetres();
            let current = (snap.ink_width - width).abs() < 0.01;
            if thickness_dot(ui, width, current, &label, t).clicked() {
                picked = Some(Cmd::PenWidth(width));
            }
        }
        let label = t!("ribbon-custom-thickness");
        if custom_thickness_button(ui, custom_open, preset.is_none(), &label, t).clicked() {
            custom_open = !custom_open;
            ui.data_mut(|data| data.insert_temp(custom_id, custom_open));
        }
    });
    if custom_open {
        // In millimetres, as the dots' tooltips name them: a highlighter's are four times
        // a pen's of the same width.
        let scale = if highlighter { 4.0 } else { 1.0 };
        let shown = preset.map_or(snap.ink_width * 0.25, |(_, mm)| *mm) * scale;
        // The value being dragged or typed, until the slider lets go of it.
        let value_id = custom_id.with("mm");
        let mut mm = ui
            .data(|data| data.get_temp::<f32>(value_id))
            .unwrap_or(shown);
        let slider = ui
            .scope(|ui| {
                // egui paints the track in the idle widgets' fill, the menu's own colour:
                // the theme's strong line shows it in both themes.
                ui.visuals_mut().widgets.inactive.bg_fill = t.line_strong;
                ui.add(
                    egui::Slider::new(&mut mm, 0.1..=5.0)
                        .step_by(0.05)
                        // Only what is dragged or typed is kept in range and on its steps:
                        // a width between them, merely shown, would count as a pick.
                        .clamping(egui::SliderClamping::Edits)
                        .suffix("\u{a0}mm")
                        // In the interface's decimal style ("0,5" in French), typed either way.
                        .custom_formatter(|value, _| i18n::decimal(value, 2))
                        .custom_parser(|text| text.trim().replace(',', ".").parse().ok())
                        .update_while_editing(false),
                )
            })
            .inner;
        if slider.dragged() || slider.has_focus() {
            ui.data_mut(|data| data.insert_temp(value_id, mm));
        } else {
            ui.data_mut(|data| data.remove::<f32>(value_id));
        }
        // A pick closes the menu, so a drag picks once it ends.
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            picked = Some(Cmd::PenWidth(mm / scale / 0.25));
        }
    }
    ui.add_space(4.0);
    // the desktop notebook's Recent Colors: the colours picked that are not in its 16.
    if !snap.recent_colors.is_empty() {
        pen_menu_heading(ui, &t!("ribbon-recent-colors"), t);
        if let Some(color) = pen_swatches(ui, &snap.recent_colors, Some(snap.ink_color), t) {
            picked = Some(Cmd::Ink(color));
        }
        ui.add_space(4.0);
        ui.separator();
    }
    pen_menu_heading(ui, &t!("ribbon-colors"), t);
    if let Some(color) = pen_color_grid(ui, Some(snap.ink_color), t) {
        picked = Some(Cmd::Ink(color));
    }
    ui.add_space(4.0);
    ui.separator();
    if widgets::menu_item(ui, Some(icons::PALETTE), &t!("ribbon-more-colors"), t).clicked() {
        picked = Some(Cmd::MorePenColors);
    }
    ui.separator();
    // The gallery keeps at least one pen.
    let removable = snap.pen.is_some() && snap.pens.len() > 1;
    ui.add_enabled_ui(removable, |ui| {
        if widgets::menu_item(ui, Some(icons::TRASH), &t!("ribbon-remove-pen"), t).clicked() {
            picked = Some(Cmd::RemovePen);
        }
    });
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Draw › Add Pen ▾: a pen or a highlighter in any of the desktop notebook's colours.
pub(crate) fn add_pen_menu(ui: &mut Ui, t: Tokens) -> Option<Cmd> {
    let mut picked = None;
    pen_menu_heading(ui, &t!("ribbon-pen"), t);
    if let Some(color) = pen_color_grid(ui, None, t) {
        picked = Some(Cmd::AddPen {
            highlighter: false,
            color,
        });
    }
    ui.add_space(4.0);
    pen_menu_heading(ui, &t!("ribbon-highlighter"), t);
    if let Some(color) = pen_color_grid(ui, None, t) {
        picked = Some(Cmd::AddPen {
            highlighter: true,
            color,
        });
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Draw › Rotate ▾, in the desktop notebook's order: 45° turns (only for ink, lines and arrows, as
/// `turns_45` says), quarter turns and mirrors for the selected ink and shapes.
pub(crate) fn rotate_list(ui: &mut Ui, turns_45: bool, t: Tokens) -> Option<Cmd> {
    ui.set_min_width(180.0);
    let mut picked = None;
    let eighths = [
        (
            icons::ARROW_ROTATE_CLOCKWISE,
            t!("ribbon-rotate-right-45"),
            Rotation::Right45,
        ),
        (
            icons::ARROW_ROTATE_COUNTERCLOCKWISE,
            t!("ribbon-rotate-left-45"),
            Rotation::Left45,
        ),
    ];
    ui.add_enabled_ui(turns_45, |ui| {
        for (glyph, label, rotation) in eighths {
            if widgets::menu_item(ui, Some(glyph), &label, t).clicked() {
                picked = Some(Cmd::Rotate(rotation));
                ui.close();
            }
        }
    });
    let rows = [
        (
            icons::ARROW_ROTATE_CLOCKWISE,
            t!("ribbon-rotate-right"),
            Rotation::Right,
        ),
        (
            icons::ARROW_ROTATE_COUNTERCLOCKWISE,
            t!("ribbon-rotate-left"),
            Rotation::Left,
        ),
        (
            icons::FLIP_HORIZONTAL,
            t!("ribbon-flip-horizontal"),
            Rotation::FlipHorizontal,
        ),
        (
            icons::FLIP_VERTICAL,
            t!("ribbon-flip-vertical"),
            Rotation::FlipVertical,
        ),
    ];
    for (glyph, label, rotation) in rows {
        if widgets::menu_item(ui, Some(glyph), &label, t).clicked() {
            picked = Some(Cmd::Rotate(rotation));
            ui.close();
        }
    }
    picked
}

/// The selected shapes and frises (they get fill swatches).
fn colorable_selection(page: &Page, state: &CanvasState) -> Vec<u64> {
    page.canvas
        .items
        .iter()
        .filter(|item| state.selection.contains(item.id))
        .filter(|item| {
            matches!(
                item.kind,
                CanvasKind::Shape { .. } | CanvasKind::Timeline { .. }
            )
        })
        .map(|item| item.id)
        .collect()
}

/// The fill colours (the highlighters) as a row of swatches, for the ribbon's Fill
/// menu. Returns the one clicked.
pub(crate) fn fill_list(ui: &mut Ui, t: Tokens) -> Option<Rgb> {
    let mut picked = None;
    ui.horizontal(|ui| {
        for color in Rgb::HIGHLIGHTERS {
            if color_swatch(ui, color, FILL_MENU_SWATCH, t).clicked() {
                picked = Some(color);
                ui.close();
            }
        }
    });
    picked
}

fn color_swatch(ui: &mut Ui, color: Rgb, size: f32, t: Tokens) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    ui.painter()
        .circle_filled(rect.center(), size * 0.35, color.to_color32());
    if response.hovered() {
        ui.painter()
            .circle_stroke(rect.center(), size * 0.5, Stroke::new(1.0, t.accent));
    }
    let label = t!("canvas-colour");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.as_ref()));
    response.on_hover_text(label.as_ref())
}

/// The pointer in scene coordinates, if nothing (a menu, a dialog) covers the canvas.
fn canvas_hover(ui: &Ui, parent_layer: LayerId, view: Rect) -> Option<Pos2> {
    let ctx = ui.ctx();
    let global = ctx.input(|input| input.pointer.hover_pos())?;
    let above = ctx.layer_id_at(global);
    if above.is_some_and(|layer| layer != parent_layer && layer != ui.layer_id()) {
        return None;
    }
    let to_scene = ctx.layer_transform_from_global(ui.layer_id())?;
    let pos = to_scene * global;
    view.contains(pos).then_some(pos)
}

fn canvas_contents(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &mut Library,
    textures: &mut TextureCache,
    t: Tokens,
    toast: &mut Option<(String, bool)>,
    parent_layer: LayerId,
    far: Pos2,
) -> bool {
    let mut changed = false;
    // A band left over from a drag that ended without its stop (the pointer left the
    // window, a dialog opened) is dropped once the button is up.
    if state.band.is_some() && !ui.input(|input| input.pointer.primary_down()) {
        state.band = None;
    }
    let view = ui.clip_rect();
    state.visible = Some(view);
    state.scene_layer = Some(ui.layer_id());
    let zoom = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .map(|transform| transform.scaling)
        // A zero-sized first frame can produce a degenerate transform.
        .filter(|zoom| zoom.is_finite() && *zoom > 0.01)
        .unwrap_or(1.0);
    state.zoom = zoom;
    let vp = Viewport {
        zoom,
        hover: canvas_hover(ui, parent_layer, view),
    };
    // Empty paper takes clicks wherever the page reaches, and wherever the view shows.
    let extent = page_extent(far, view.size());
    let canvas_rect = extent.union(view);
    let bars = scrollbar_layout(view, extent, far, vp, state.scrollbar_left);
    let background = ui.interact(
        canvas_rect,
        Id::unique(("canvas-background", page.id.as_str())),
        Sense::click_and_drag(),
    );
    let pointer = scene_pointer(ui);
    let drawing_tool = matches!(
        state.tool,
        CanvasTool::Pen | CanvasTool::Highlighter | CanvasTool::Eraser
    );
    // Pressing anywhere on the page outside the note being typed in finishes that edit,
    // except on a scrollbar: the caret stays, as in the desktop notebook, and comes back once egui has
    // taken the keyboard from the text for the press.
    let mut keep_caret = false;
    if ui.input(|input| input.pointer.any_pressed()) {
        let on_bar = vp
            .hover
            .is_some_and(|pos| bars.iter().any(|bar| bar.track.contains(pos)));
        keep_caret = on_bar && state.active_text.is_some();
        if let Some(pos) = vp.hover.filter(|_| !on_bar) {
            if let Some(id) = state.active_text {
                let inside = page
                    .canvas
                    .item(id)
                    .is_some_and(|item| chrome_rect(item.rect(), vp).contains(pos));
                if !inside {
                    state.active_text = None;
                }
            }
            if let Some(id) = state.editing_label {
                if page
                    .canvas
                    .item(id)
                    .is_none_or(|item| !item.rect().contains(pos))
                {
                    state.editing_label = None;
                }
            }
        }
    }

    // A press on the page outside the picture being cropped keeps the crop, as the desktop notebook's
    // click elsewhere does. Its handles reach a little past the picture.
    if let Some(full) = state.cropping.as_ref().map(|crop| crop.full) {
        if ui.input(|input| input.pointer.any_pressed())
            && vp
                .hover
                .is_some_and(|pos| !full.expand(vp.px(CROP_HANDLE)).contains(pos))
        {
            changed |= commit_crop(page, state);
        }
    }

    // Several selected things (or any ink) move together by dragging inside their frame.
    let group_area = (state.tool == CanvasTool::Select && state.selection.is_group())
        .then(|| selection_bounds(page, &state.selection).expand(vp.px(8.0)))
        .filter(|area| area.is_finite());
    if let Some(area) = group_area {
        let frame = ui.interact(
            area,
            Id::unique(("canvas-group", page.id.as_str())),
            Sense::click_and_drag(),
        );
        // Middle- or right-drag pans here too, as on the paper around the frame.
        let panning = pan_by_drag(ui, &frame, state);
        if frame.hovered() && !panning {
            ui.ctx().set_cursor_icon(CursorIcon::Move);
        }
        if frame.drag_started_by(PointerButton::Primary) {
            begin_group_drag(ui, page, state);
        }
        if frame.clicked() {
            // Like a click on paper: start a note there (which also ends the selection).
            if let Some(pos) = pointer {
                state.last_click = Some(pos);
                add_text(page, state, editor_state, pos);
                changed = true;
            }
        }
        // Right-clicking inside the frame offers the group's actions.
        changed |= item_menu(&frame, page, state, None, t);
    }

    draw_page_background(ui, page, view, vp, t);
    // Pictures set as the page's background: under everything, never picked, so typing,
    // ink and the rubber band all work on top of them (the desktop notebook's Set Picture as Background).
    for item in page.canvas.items.iter().filter(|item| item.is_background()) {
        if let CanvasKind::Image {
            attachment,
            meta,
            printout,
        } = &item.kind
        {
            paint_picture(
                ui,
                library,
                textures,
                t,
                attachment,
                item.rect(),
                meta,
                vp,
                printout.is_some(),
            );
        }
    }
    changed |= page_header(ui, page, state, editor_state, vp, t);

    // Middle- and right-drag pan the page with every tool; only the left button draws a
    // band, a shape, a loop or ink.
    let panning = pan_by_drag(ui, &background, state);
    if !drawing_tool {
        match state.tool {
            CanvasTool::Select => {
                if background.clicked() {
                    if let Some(pos) = pointer {
                        if state.header_rect.is_some_and(|header| header.contains(pos)) {
                            // Under the title, a click goes to the title, as in the desktop notebook.
                            editor_state.focus_title(false);
                        } else {
                            // Type anywhere: a click on empty paper starts a note there.
                            state.last_click = Some(pos);
                            add_text(page, state, editor_state, pos);
                            changed = true;
                        }
                    }
                }
                // Dragging empty paper selects what the band covers (Shift adds), as in the desktop notebook.
                if background.drag_started_by(PointerButton::Primary) {
                    if let Some(start) = press_origin_scene(ui).or(pointer) {
                        let base = if adding_to_selection(ui) {
                            state.selection.clone()
                        } else {
                            Selection::default()
                        };
                        state.band = Some(Band {
                            start,
                            end: start,
                            base,
                        });
                    }
                }
                if background.dragged_by(PointerButton::Primary) {
                    if let (Some(pos), Some(band)) = (pointer, state.band.as_mut()) {
                        band.end = pos;
                    }
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                } else if background.hovered() && !panning {
                    ui.ctx().set_cursor_icon(CursorIcon::Text);
                }
                if let Some(band) = &state.band {
                    state.selection = band_selection(page, band);
                }
                if background.drag_stopped() {
                    state.band = None;
                }
                if background.secondary_clicked() {
                    state.menu_at = pointer;
                    state.selection.clear();
                }
                background.context_menu(|ui| {
                    // Over a background picture (the topmost, as `items` is in creation
                    // order), the desktop notebook's checked "Set Picture as Background" takes it back.
                    let under = state.menu_at.and_then(|at| {
                        page.canvas
                            .items
                            .iter()
                            .rev()
                            .find(|item| item.is_background() && item.rect().contains(at))
                            .map(|item| item.id)
                    });
                    let (paste, paste_hint) = (
                        t!("common-paste"),
                        util::menu_shortcut(Modifiers::COMMAND, Key::V),
                    );
                    let background_label = t!("picture-background");
                    let creation = [
                        (icons::TEXT_AA, t!("canvas-menu-text-here")),
                        (icons::MATH_OPERATIONS, t!("canvas-menu-equation-here")),
                        (icons::IMAGE, t!("canvas-menu-picture")),
                        (icons::PAPERCLIP, t!("canvas-menu-file")),
                    ];
                    let background_row = under
                        .map(|_| (icons::CHECK, background_label.as_ref(), ""))
                        .into_iter();
                    // A printout page in the background keeps its printout rows, so a
                    // worksheet's text can still be copied and its PDF opened.
                    let printout_rows: Vec<_> = under
                        .filter(|id| {
                            page.canvas
                                .item(*id)
                                .is_some_and(|item| item.kind.printout().is_some())
                        })
                        .map(|_| {
                            [
                                ItemRow::CopyPrintoutPage,
                                ItemRow::CopyPrintoutAll,
                                ItemRow::OpenOriginal,
                            ]
                            .map(|row| (row, row.text(None)))
                        })
                        .into_iter()
                        .flatten()
                        .collect();
                    ui.set_width(menu_width(
                        ui,
                        [(icons::CLIPBOARD_TEXT, paste.as_ref(), paste_hint.as_str())]
                            .into_iter()
                            .chain(
                                printout_rows
                                    .iter()
                                    .map(|(_, (glyph, label, _))| (*glyph, label.as_ref(), "")),
                            )
                            .chain(background_row)
                            .chain(
                                creation
                                    .iter()
                                    .map(|(glyph, label)| (*glyph, label.as_ref(), "")),
                            ),
                    ));
                    // the desktop notebook's Paste comes first. The platform answers with a paste event,
                    // which `canvas_keys` takes as it takes Ctrl+V's, at the point clicked.
                    if widgets::menu_row(
                        ui,
                        Some(icons::CLIPBOARD_TEXT),
                        &paste,
                        &paste_hint,
                        true,
                        t,
                    )
                    .clicked()
                    {
                        request_paste_at(ui.ctx(), state);
                        ui.close();
                    }
                    ui.separator();
                    if let Some(id) = under {
                        for (row, (glyph, label, _)) in &printout_rows {
                            if widgets::menu_item(ui, Some(*glyph), label, t).clicked() {
                                let action = match row {
                                    ItemRow::CopyPrintoutPage => {
                                        ItemAction::CopyPrintoutText { all: false }
                                    }
                                    ItemRow::CopyPrintoutAll => {
                                        ItemAction::CopyPrintoutText { all: true }
                                    }
                                    _ => ItemAction::OpenOriginal,
                                };
                                state.item_action = Some((id, action));
                                ui.close();
                            }
                        }
                        if !printout_rows.is_empty() {
                            ui.separator();
                        }
                        if widgets::menu_item(ui, Some(icons::CHECK), &background_label, t)
                            .clicked()
                        {
                            state.picture_request =
                                Some((PictureTarget::Item(id), PictureCmd::Background));
                            ui.close();
                        }
                        ui.separator();
                    }
                    let at = state.menu_at.unwrap_or_else(|| state.view_center());
                    let [text_here, equation_here, picture, file] = &creation;
                    if widgets::menu_item(ui, Some(text_here.0), &text_here.1, t).clicked() {
                        add_text(page, state, editor_state, at);
                        changed = true;
                        ui.close();
                    }
                    if widgets::menu_item(ui, Some(equation_here.0), &equation_here.1, t).clicked()
                    {
                        // A new note holding an equation block, at the point clicked.
                        state.last_click = Some(at);
                        changed |= insert_equation(ui.ctx(), page, state, editor_state);
                        ui.close();
                    }
                    if widgets::menu_item(ui, Some(picture.0), &picture.1, t).clicked() {
                        ui.close();
                        changed |= add_image_dialog(page, state, library, toast);
                    }
                    if widgets::menu_item(ui, Some(file.0), &file.1, t).clicked() {
                        ui.close();
                        changed |= add_file_dialog(page, state, library, toast);
                    }
                });
            }
            CanvasTool::Shape(shape) => {
                if background.hovered() && !panning {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                if background.drag_started_by(PointerButton::Primary) {
                    if let Some(pos) = pointer {
                        state.shape_draft = Some((pos, pos));
                    }
                }
                if background.dragged() {
                    if let (Some(pos), Some((_, end))) = (pointer, &mut state.shape_draft) {
                        *end = pos;
                    }
                }
                if background.drag_stopped() {
                    if let Some((start, end)) = state.shape_draft.take() {
                        add_shape(page, state, shape, start, end);
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }

    let mut order: Vec<(i32, u64, usize)> = page
        .canvas
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.z, item.id, index))
        .collect();
    order.sort_unstable();
    let mut focused_text = None;
    for (_, id, hint) in order {
        // Deleting an item mid-loop (its close button) shifts the ones after it.
        let index = match page.canvas.items.get(hint) {
            Some(item) if item.id == id => hint,
            _ => match page.canvas.items.iter().position(|item| item.id == id) {
                Some(index) => index,
                None => continue,
            },
        };
        // Painted under everything above, and never picked up.
        if page.canvas.items[index].is_background() {
            continue;
        }
        let before = page.canvas.items[index].rect();
        if page.canvas.items[index].doc().is_some() {
            let (text_changed, focused) = text_item(
                ui,
                page,
                state,
                editor_state,
                library,
                textures,
                t,
                index,
                vp,
            );
            changed |= text_changed;
            if focused {
                focused_text = Some(id);
            }
        } else {
            changed |= object_item(ui, page, state, library, textures, t, index, vp);
        }
        // A note's size follows its text, an equation's its formula, without a change to
        // the page: its far corner is measured again (`cached_far`).
        let resized = page
            .canvas
            .items
            .get(index)
            .is_some_and(|item| item.id == id && item.rect() != before);
        if resized {
            state.far = None;
        }
    }
    handle_item_action(ui.ctx(), page, state, library, toast);
    if let Some((target, command)) = state.picture_request.take() {
        if run_picture_cmd(page, state, library, target, command) {
            forget_picture_note_history(page, editor_state, target);
            changed = true;
        }
    }
    if let Some((attachment, action)) = editor_state.attachment_action.take() {
        let action = match action {
            AttachmentAction::Open => Some(ItemAction::Open),
            AttachmentAction::SaveAs => Some(ItemAction::SaveAs),
            AttachmentAction::TogglePlay => Some(ItemAction::TogglePlay),
            // A PDF's File block: its pages go under the note that holds it.
            AttachmentAction::InsertPrintout => {
                if let Some(note) = note_with_file(page, &attachment) {
                    state.printout_from_card = Some((note, attachment.clone()));
                }
                None
            }
        };
        if let Some(action) = action {
            run_attachment_action(state, library, &attachment, action, toast);
        }
    }
    // The items have seen the corner drag that ended last frame.
    state.resized = None;
    changed |= update_drag(ui, page, state, editor_state);
    if matches!(state.tool, CanvasTool::Select | CanvasTool::Lasso) && state.selection.is_group() {
        let area = selection_bounds(page, &state.selection).expand(vp.px(8.0));
        if area.is_finite() {
            let corners = [
                area.left_top(),
                area.right_top(),
                area.right_bottom(),
                area.left_bottom(),
                area.left_top(),
            ];
            ui.painter().extend(egui::Shape::dashed_line(
                &corners,
                Stroke::new(vp.px(1.0), t.accent),
                vp.px(5.0),
                vp.px(4.0),
            ));
        }
    }
    if let Some(band) = &state.band {
        ui.painter().rect(
            Rect::from_two_pos(band.start, band.end),
            CornerRadius::ZERO,
            theme::with_alpha(t.accent, 24),
            Stroke::new(vp.px(1.0), theme::with_alpha(t.accent, 200)),
            StrokeKind::Inside,
        );
    }

    if let Some(id) = focused_text {
        state.typing_in(&page.canvas, id);
    }
    // A focus request for a note that no longer exists would block page shortcuts.
    if let Some(block) = editor_state.pending_block_focus() {
        let exists = page.canvas.items.iter().any(|item| {
            item.doc()
                .is_some_and(|doc| doc.block_index(block).is_some())
        });
        if !exists {
            editor_state.clear_pending_focus();
        }
    }

    if let Some((start, end)) = state.shape_draft {
        if let CanvasTool::Shape(shape) = state.tool {
            let connector = matches!(shape, ShapeKind::Line | ShapeKind::Arrow);
            paint_shape(
                ui,
                shape,
                Rect::from_two_pos(start, end),
                (connector && end.x < start.x, connector && end.y < start.y),
                Stroke::new(SHAPE_LINE, theme::with_alpha(t.accent, 200)),
                None,
            );
        }
    }

    paint_ink(ui, page, state, view, t);
    changed |= equation_editor(ui, page, state, t);
    if drawing_tool {
        changed |= ink_tool(ui, page, state, editor_state, canvas_rect, pointer, vp, t);
    }
    if state.tool == CanvasTool::Lasso {
        lasso_tool(ui, page, state, canvas_rect, pointer, vp);
    }
    // A loop left over from a drag that ended without its stop is dropped once the button
    // is up. This runs after the tool, which needs the loop on the frame of the release.
    if state.lasso.is_some() && !ui.input(|input| input.pointer.primary_down()) {
        state.lasso = None;
    }
    if let Some(lasso) = &state.lasso {
        let mut path = lasso.points.clone();
        path.extend(lasso.points.first().copied());
        ui.painter().extend(egui::Shape::dashed_line(
            &path,
            Stroke::new(vp.px(1.2), t.accent),
            vp.px(6.0),
            vp.px(4.0),
        ));
    }

    if state.tool == CanvasTool::InsertSpace {
        changed |= insert_space_tool(ui, page, state, canvas_rect, pointer, vp, t);
    }

    // The Pan tool, or Space held with no text caret, drags the page around with any tool.
    let space_pan =
        ui.input(|input| input.key_down(Key::Space)) && !ui.ctx().egui_wants_keyboard_input();
    if state.tool == CanvasTool::Pan || space_pan {
        let grab = ui.interact(
            canvas_rect,
            Id::unique(("canvas-pan", page.id.as_str())),
            Sense::click_and_drag(),
        );
        if grab.dragged() {
            state.pan += grab.drag_delta();
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if grab.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
    }

    changed |= dropped_files(ui, page, state, library, toast, pointer, view, t);
    page_scrollbars(ui, &page.id, state, &bars, vp, t);
    if keep_caret {
        editor_state.restore_focus();
    }
    changed
}

/// How thick the page's scrollbars are, in screen points.
const SCROLLBAR: f32 = 10.0;

/// One of the page's scrollbars, in page units: its track along the view's edge, the thumb
/// in it, and how thumb travel maps to page travel.
struct Scrollbar {
    vertical: bool,
    track: Rect,
    thumb: Rect,
    /// How much of the page the view shows along the bar, and how far it can scroll.
    shown: f32,
    room: f32,
    /// How far the thumb can move in its track.
    travel: f32,
}

/// Where the page's scrollbars go this frame. Only an edge with somewhere to go has a bar:
/// content past the view on that axis, or a view scrolled away from the page's start. A
/// page that fits in the window has none, as in the desktop notebook. The vertical bar runs down the
/// right edge, or the left one with `left` (Options › Display).
fn scrollbar_layout(
    view: Rect,
    extent: Rect,
    far: Pos2,
    vp: Viewport,
    left: bool,
) -> Vec<Scrollbar> {
    let needed = |axis: usize| {
        extent.size()[axis] > view.size()[axis] + 0.5
            && (far[axis] > view.max[axis] + 0.5 || view.min[axis] > extent.min[axis] + 0.5)
    };
    let (horizontal, vertical) = (needed(0), needed(1));
    let thick = vp.px(SCROLLBAR);
    // Where both show, each stops short of the corner the other runs into.
    let corner = |other: bool| if other { thick } else { 0.0 };
    let mut bars = Vec::new();
    for (shows, is_vertical) in [(vertical, true), (horizontal, false)] {
        if !shows {
            continue;
        }
        let axis = usize::from(is_vertical);
        let (shown, total) = (view.size()[axis], extent.size()[axis]);
        let track = if is_vertical {
            let x = if left {
                view.left()
            } else {
                view.right() - thick
            };
            Rect::from_min_max(
                pos2(x, view.top()),
                pos2(x + thick, view.bottom() - corner(horizontal)),
            )
        } else if left {
            Rect::from_min_max(
                pos2(view.left() + corner(vertical), view.bottom() - thick),
                pos2(view.right(), view.bottom()),
            )
        } else {
            Rect::from_min_max(
                pos2(view.left(), view.bottom() - thick),
                pos2(view.right() - corner(vertical), view.bottom()),
            )
        };
        let length = track.size()[axis];
        let thumb_length = (length * shown / total).max(vp.px(24.0)).min(length);
        let travel = (length - thumb_length).max(1.0);
        let room = total - shown;
        let along = ((view.min[axis] - extent.min[axis]) / room).clamp(0.0, 1.0) * travel;
        let mut thumb = track;
        thumb.min[axis] = track.min[axis] + along;
        thumb.max[axis] = thumb.min[axis] + thumb_length;
        bars.push(Scrollbar {
            vertical: is_vertical,
            track,
            thumb,
            shown,
            room,
            travel,
        });
    }
    bars
}

/// the desktop notebook's scrollbars along the page's right (or left) and bottom edges (see
/// `scrollbar_layout`), drawn over the notes. The thumb shows which part of the page is in view; dragging it
/// scrolls, and a click on the track moves almost a screen at a time.
fn page_scrollbars(
    ui: &mut Ui,
    page_id: &str,
    state: &mut CanvasState,
    bars: &[Scrollbar],
    vp: Viewport,
    t: Tokens,
) {
    for bar in bars {
        let (vertical, track, thumb) = (bar.vertical, bar.track, bar.thumb);
        let axis = usize::from(vertical);
        let id = Id::unique(("page-scrollbar", page_id, vertical));
        let on_track = ui.interact(track, id.with("track"), Sense::click());
        let on_thumb = ui.interact(thumb, id, Sense::drag());
        on_thumb.widget_info(|| {
            let name = if vertical {
                t!("canvas-scrollbar-vertical")
            } else {
                t!("canvas-scrollbar-horizontal")
            };
            egui::WidgetInfo::labeled(egui::Role::ScrollBar, true, &name)
        });
        bounds_on_screen(ui, &on_thumb);
        // Screen points to page units, then thumb travel to page travel.
        let mut moved = 0.0;
        if on_thumb.dragged() {
            let screen = ui.input(|input| input.pointer.delta())[axis];
            moved = screen / vp.zoom * bar.room / bar.travel;
        } else if on_track.clicked() {
            if let Some(pos) = scene_pointer(ui) {
                moved = if pos[axis] < thumb.min[axis] {
                    -bar.shown
                } else {
                    bar.shown
                } * 0.9;
            }
        }
        state.pan[axis] -= moved;

        let hot = on_thumb.hovered() || on_thumb.dragged() || on_track.hovered();
        let thick = track.size()[1 - axis];
        let width = thick * if hot { 0.7 } else { 0.4 };
        // The thin thumb hugs the page's outer edge, on whichever side the bar is.
        let mut shape = thumb.shrink(vp.px(1.0));
        if vertical && state.scrollbar_left {
            shape.max.x = shape.min.x + width;
        } else if vertical {
            shape.min.x = shape.max.x - width;
        } else {
            shape.min.y = shape.max.y - width;
        }
        ui.painter().rect_filled(
            shape,
            CornerRadius::same(4),
            if hot { t.faint } else { t.line_strong },
        );
    }
}

/// Pans the page while `response` is dragged with the middle or right button (the Scene
/// does this itself only where nothing else takes the drag). Returns whether it panned.
fn pan_by_drag(ui: &Ui, response: &egui::Response, state: &mut CanvasState) -> bool {
    if response.dragged_by(PointerButton::Middle) || response.dragged_by(PointerButton::Secondary) {
        state.pan += response.drag_delta();
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        return true;
    }
    false
}

/// Called when a note stops being edited. The edit becomes one canvas undo step, and
/// an empty text note is removed (the desktop notebook does the same).
fn finish_text_edit(page: &mut Page, state: &mut CanvasState, id: u64) -> bool {
    let snapshot = state
        .text_snapshot
        .take()
        .filter(|(owner, _)| *owner == id)
        .map(|(_, snapshot)| snapshot);
    // A brand-new note's creation step already covers this edit.
    if let Some(snapshot) = snapshot {
        if state.typing_needs_step(&snapshot, id, &page.canvas) {
            state.push_snapshot(&snapshot);
        }
    }
    let empty_note = page.canvas.item(id).is_some_and(|item| match &item.kind {
        CanvasKind::Text { doc } => doc_is_blank(doc),
        _ => false,
    });
    if !empty_note {
        return false;
    }
    page.canvas.remove(id);
    state.selection.remove(id);
    // A note created and left without typing: drop the creation step that would now
    // restore nothing. A note that had text keeps its step, so undo brings it back.
    if state.undo.back() == Some(&page.canvas) {
        state.undo.pop_back();
    }
    true
}

/// Called when the source box loses focus. The edit becomes one undo step; an empty
/// equation is removed (undo brings back what it said before; one inserted and left
/// empty leaves no step at all).
fn finish_equation_edit(page: &mut Page, state: &mut CanvasState) -> bool {
    let Some(edit) = state.equation_edit.take() else {
        return false;
    };
    // A brand-new equation's creation step already covers the typing.
    if state.equation_needs_step(&edit, &page.canvas) {
        state.push_snapshot(&edit.before);
    }
    if page.canvas.item(edit.id).is_none() || !equation_blank(&page.canvas, edit.id) {
        return false;
    }
    page.canvas.remove(edit.id);
    // Like `delete_item`: only this equation leaves the selection.
    state.selection.remove(edit.id);
    // A note whose edit began while this was typed (the click that ended it) measures its
    // typing from a page without the empty equation, so undo never brings that back.
    if let Some((_, snapshot)) = state.text_snapshot.as_mut() {
        snapshot.remove(edit.id);
    }
    // Inserted and left empty: drop the creation step that would now restore nothing.
    if state.undo.back() == Some(&page.canvas) {
        state.undo.pop_back();
    }
    true
}

/// Ends the equation being typed on `page`, before the page is saved and left (opening
/// another page, search…): an equation item, or an equation block in the note typed in.
/// A crop under way is kept, as a click elsewhere keeps it. Returns whether the page
/// changed: an empty equation is removed, and so is a note it leaves empty (one Alt+= made
/// to hold it), as when the caret leaves them.
pub fn finish_editing(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) -> bool {
    if state.page_id != page.id {
        return false;
    }
    let mut changed = commit_crop(page, state);
    changed |= finish_equation_edit(page, state);
    if let Some(id) = active_note(page, state) {
        let ended = page
            .canvas
            .item_mut(id)
            .and_then(CanvasItem::doc_mut)
            .is_some_and(|doc| editor::end_equation(doc, editor_state));
        changed |= ended;
        let blank = page
            .canvas
            .item(id)
            .and_then(CanvasItem::doc)
            .is_some_and(doc_is_blank);
        if ended && blank {
            finish_text_edit(page, state, id);
            state.active_text = None;
        }
    }
    changed
}

/// Whether equation `id` is blank (or absent) in `canvas`.
fn equation_blank(canvas: &CanvasDoc, id: u64) -> bool {
    match canvas.item(id).map(|item| &item.kind) {
        Some(CanvasKind::Equation { source, .. }) => source.trim().is_empty(),
        _ => true,
    }
}

/// Whether note `id` was empty in `canvas` (or absent from it).
fn note_was_blank(canvas: &CanvasDoc, id: u64) -> bool {
    canvas
        .item(id)
        .and_then(CanvasItem::doc)
        .is_none_or(doc_is_blank)
}

fn doc_is_blank(doc: &Document) -> bool {
    doc.blocks.iter().all(|block| match &block.kind {
        BlockKind::Paragraph => block.text.text.trim().is_empty(),
        // An equation block left blank goes when its editing ends (Insert › Equation, Alt+=).
        BlockKind::Equation { source, .. } => source.trim().is_empty(),
        _ => false,
    })
}

/// The rule line that the first line of text of the default first note sits on, just
/// under its baseline: the note's 4 px top padding (`NOTE_PAD_TOP`; paragraphs have none),
/// then 14 px down the 18 px row of 14.67 px Carlito (`editor::body_baseline`). Every rule
/// line is a whole number of spacings from it, so typed text sits on the lines, as in
/// the desktop notebook.
pub const FIRST_RULE: f32 = PAGE_CONTENT_TOP + NOTE_PAD_TOP + 15.0;
/// Rule lines start below this, under the header's date (at the top of the page when the
/// title is hidden); and the desktop notebook's red margin line.
const RULE_TOP: f32 = PAGE_CONTENT_TOP - 16.0;
pub const RULE_MARGIN: f32 = 28.0;

/// The first rule line at or below `y`, for lines `step` apart.
fn rule_at_or_below(y: f32, step: f32) -> f32 {
    FIRST_RULE + ((y - FIRST_RULE) / step).ceil() * step
}

/// The spacing of a page's rule lines, which note text snaps to (not a grid's).
fn ruled_pitch(page: &Page) -> Option<f32> {
    match page.rule {
        crate::model::RuleLines::Lines(_) => page.rule.spacing(),
        _ => None,
    }
}

/// How far to move a note's text (whose top would be at `text_top`) so that on lines
/// `pitch` apart its first baseline sits on a rule as the default note's does: on the
/// rule at or below where an unruled line's would be. Rows taller than the text centre
/// it, so this is mostly a small step up; a note between two rules goes down to the next.
fn rule_phase(ctx: &egui::Context, text_top: f32, pitch: f32) -> f32 {
    // The default note's text top is `PAGE_CONTENT_TOP + NOTE_PAD_TOP`, and FIRST_RULE is
    // this far below it: a hair under its baseline.
    let under = FIRST_RULE - PAGE_CONTENT_TOP - NOTE_PAD_TOP;
    let plain = editor::body_baseline(ctx, None);
    let ruled = editor::body_baseline(ctx, Some(pitch));
    // Half a pixel of slack, so a note already on a rule never drops to the next.
    let rule = rule_at_or_below(text_top + under - 0.5, pitch);
    rule - (under - plain) - ruled - text_top
}

/// The blue of rule lines and grids, and the red of the ruled margin line, on the page and
/// in View ▸ Rule Lines' thumbnails.
pub fn rule_colors(t: Tokens) -> (Color32, Color32) {
    if t.dark {
        (
            Color32::from_rgb(0x3b, 0x55, 0x72),
            Color32::from_rgb(0x7a, 0x3b, 0x3b),
        )
    } else {
        (
            Color32::from_rgb(0xa9, 0xc8, 0xe8),
            Color32::from_rgb(0xe8, 0xa0, 0xa0),
        )
    }
}

/// The page behind the notes, as the desktop notebook paints it: white or the page colour, then blue
/// rule lines with a red margin line, or a blue grid. Lines start under the date (at the
/// top when the title is hidden), fall where typed text sits (`FIRST_RULE`), and thin out
/// when zoomed far out instead of drawing thousands of them.
fn draw_page_background(ui: &mut Ui, page: &Page, view: Rect, vp: Viewport, t: Tokens) {
    use crate::model::RuleLines;
    let fill = match page.color {
        Some(color) if t.dark => color.dark().to_color32(),
        Some(color) => color.light().to_color32(),
        None => t.raised,
    };
    ui.painter().rect_filled(view, CornerRadius::ZERO, fill);
    let Some(spacing) = page.rule.spacing() else {
        return;
    };
    if !view.is_finite() || view.area() > 1.0e9 {
        return;
    }
    let (line, margin) = rule_colors(t);
    // Rule Line Color: the page's own, in either theme.
    let line = page.rule_color.map_or(line, Rgb::to_color32);
    let stroke = Stroke::new(vp.px(1.0), line);
    let mut step = spacing;
    while step * vp.zoom < 6.0 {
        step *= 2.0;
    }
    let floor = if page.title_hidden { 0.0 } else { RULE_TOP };
    let top = view.top().max(rule_at_or_below(floor, step));
    let mut y = rule_at_or_below(top, step);
    while y < view.bottom() {
        ui.painter().hline(view.x_range(), y, stroke);
        y += step;
    }
    match page.rule {
        RuleLines::Grid(_) => {
            let mut x = (view.left().max(0.0) / step).floor() * step;
            while x < view.right() {
                ui.painter()
                    .vline(x, Rangef::new(top, view.bottom()), stroke);
                x += step;
            }
        }
        RuleLines::Lines(_) => {
            ui.painter()
                .vline(RULE_MARGIN, view.y_range(), Stroke::new(vp.px(1.0), margin));
        }
        RuleLines::None => {}
    }
}

fn surrender_text_focus(ui: &Ui) {
    if let Some(id) = ui.memory(|memory| memory.focused()) {
        ui.memory_mut(|memory| memory.surrender_focus(id));
    }
}

/// A note's frame with its grab bar, and the margin on its left where paragraph handles
/// show: the area that keeps its chrome up and its edit going.
fn chrome_rect(rect: Rect, vp: Viewport) -> Rect {
    Rect::from_min_max(
        pos2(
            rect.left() - vp.px(editor::HANDLE_GUTTER),
            rect.top() - vp.px(NOTE_BAR),
        ),
        rect.max,
    )
    .expand(vp.px(6.0))
}

fn text_item(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &Library,
    textures: &mut TextureCache,
    t: Tokens,
    index: usize,
    vp: Viewport,
) -> (bool, bool) {
    let id = page.canvas.items[index].id;
    // the desktop notebook hangs a paragraph's tags in the margin. When tags come or go (or a tagged
    // paragraph's level changes) the frame grows or shrinks on its left and the text stays
    // where it was. A note seen for the first time only records what it hangs. The frame
    // never goes past the page's left edge, as no move takes it there (`limit_move`): a
    // note near the edge stops at it, and its text makes the rest of the room.
    let lead = page.canvas.items[index].doc().map_or(0.0, editor::tag_lead);
    let shifted = match state.note_leads.insert(id, lead) {
        Some(before) if (lead - before).abs() > 0.5 => {
            let item = &mut page.canvas.items[index];
            let x = (item.x - (lead - before)).max(item.x.min(0.0));
            let moved = item.x - x;
            item.x = x;
            if !item.auto_width {
                item.width = (item.width + moved).max(NOTE_MIN_WIDTH);
            }
            if moved != 0.0 {
                ui.ctx().request_discard("a note's tags moved its frame");
            }
            moved != 0.0
        }
        _ => false,
    };
    let rect = page.canvas.items[index].rect();
    let doc_id = format!("{}:{}", page.id, id);
    let selected = state.selection.contains(id);
    let dragging = state.drag.as_ref().is_some_and(|drag| drag.moves(id));
    // Clicking the note's empty space (below or beside the text) puts the caret at its
    // end, instead of falling through to the paper and starting another note.
    let body = ui.interact(
        rect,
        Id::unique(("text-body", page.id.as_str(), id)),
        Sense::click(),
    );
    if body.clicked() && state.tool == CanvasTool::Select {
        let pointer = scene_pointer(ui);
        if adding_to_selection(ui) {
            surrender_text_focus(ui);
            state.active_text = None;
            state.selection.toggle(id);
        } else if let Some((block, pos)) = page.canvas.items[index]
            .doc()
            .and_then(|doc| nearest_line(ui.ctx(), doc, &doc_id, pointer))
        {
            state.active_text = Some(id);
            editor_state.focus_block(block, pos);
        }
    }
    if body.hovered() && state.tool == CanvasTool::Select {
        ui.ctx().set_cursor_icon(CursorIcon::Text);
    }

    let mut changed = shifted;
    // On a ruled page a note's lines are a rule spacing apart and its text is moved onto
    // the rules.
    let pitch = ruled_pitch(page);
    let text_top = rect.top() + NOTE_PAD_TOP;
    let text_top = text_top + pitch.map_or(0.0, |pitch| rule_phase(ui.ctx(), text_top, pitch));
    let content_rect = Rect::from_min_size(
        pos2(rect.left() + NOTE_PAD_X, text_top),
        vec2((rect.width() - NOTE_PAD_X * 2.0).max(40.0), 3000.0),
    );
    let item = &mut page.canvas.items[index];
    let auto_width = item.auto_width;
    let Some(doc) = item.doc_mut() else {
        return (false, false);
    };
    let doc_has_table = doc
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Table(_)));
    let used = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(content_rect)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_width(content_rect.width());
                editor_state.line_pitch = pitch;
                changed |= editor::show_document(
                    ui,
                    doc,
                    &doc_id,
                    editor_state,
                    library,
                    textures,
                    t,
                    0.0,
                );
                editor_state.line_pitch = None;
                ui.min_rect().size()
            },
        )
        .inner;
    // Note text copied: the page forgets its item copy, so the last copy is what pastes.
    if std::mem::take(&mut editor_state.copied) {
        state.clipboard_text = None;
    }
    // A picture in this note was clicked: the note is the one being edited, so the
    // Picture tab and Delete act on that picture.
    if editor_state.picture_clicked.take().is_some() {
        state.active_text = Some(id);
    }
    if let Some((block, command)) = editor_state.picture_request.take() {
        state.active_text = Some(id);
        state.picture_request = Some((PictureTarget::Block { note: id, block }, command));
    }
    // the desktop notebook's containers widen as you type, up to the edge of the page in view, then
    // wrap; they narrow again when text goes. Measured only when the text changed (here,
    // or by a command: Insert › Date…).
    let remeasure = state.remeasure == Some(id);
    if remeasure {
        state.remeasure = None;
    }
    let natural =
        ((changed || remeasure) && auto_width).then(|| editor::natural_width(ui.ctx(), doc, t));
    let focused = editor::has_keyboard(ui.ctx(), doc, &doc_id, editor_state);
    let min_height = NOTE_PAD_TOP + editor::BODY_LINE + NOTE_PAD_BOTTOM;
    item.height = (text_top - rect.top() + used.y + NOTE_PAD_BOTTOM)
        .max(min_height)
        .ceil();
    // A table never sticks out of its note: the desktop notebook widens the container to fit it. The
    // note keeps that width, as round a picture (`widen_note`): its text alone, all that
    // `natural_width` measures, would narrow it again under the next keystroke.
    let needed = used.x + NOTE_PAD_X * 2.0;
    if doc_has_table && needed > item.width + 0.5 {
        item.width = needed.ceil();
        item.auto_width = false;
        ui.ctx().request_discard("a note widened to its table");
    }
    if let Some(natural) = natural.filter(|_| item.auto_width) {
        let cap = state
            .visible
            .map_or(2400.0, |view| view.right() - item.x - vp.px(24.0))
            .clamp(NOTE_MIN_WIDTH, 2400.0);
        let width = (natural + NOTE_PAD_X * 2.0 + 2.0)
            .clamp(NOTE_MIN_WIDTH, cap)
            .ceil();
        if (width - item.width).abs() >= 1.0 {
            item.width = width;
            // This pass laid the text out at the old width; show it at the new one at once
            // instead of wrapped for a frame.
            ui.ctx().request_discard("a note resized to its text");
        }
    }

    let active = focused || state.active_text == Some(id);
    // A press on the grab bar keeps the frame up until the drag starts, even when the
    // pointer leaves it first (a quick drag up, onto a note above to merge with it).
    let pressed = ui.input(|input| input.pointer.primary_down())
        && press_origin_scene(ui).is_some_and(|origin| chrome_rect(rect, vp).contains(origin));
    let hot = selected || active || dragging || pressed || vp.hovering(chrome_rect(rect, vp));
    // the desktop notebook's paragraph handles, in the margin left of the frame.
    let action = page.canvas.items[index].doc().and_then(|doc| {
        editor::outline_handles(
            ui,
            doc,
            &doc_id,
            editor_state,
            rect.left(),
            rect.right(),
            vp.zoom,
            hot && state.tool == CanvasTool::Select,
            t,
        )
    });
    // The note's own menu (Select › Level n) asks for what its handles do.
    let action = action.or_else(|| {
        state
            .note_request
            .take_if(|(note, _)| *note == id)
            .map(|(_, action)| action)
    });
    if let Some(action) = action {
        let select = matches!(action, editor::HandleAction::Select(_));
        if select {
            state.active_text = Some(id);
            state.selection.set_one(id);
        }
        // In the note being typed in, the editor's history records it; elsewhere, the page's.
        let page_step = !select && !focused;
        if page_step {
            state.push_undo(&page.canvas);
        }
        let now = ui.input(|input| input.time);
        let applied = page.canvas.items[index].doc_mut().is_some_and(|doc| {
            editor::apply_handle_action(ui.ctx(), doc, &doc_id, editor_state, action, now)
        });
        // Nothing changed: drop the step just recorded (only if it was recorded).
        if page_step && !applied && state.undo.back() == Some(&page.canvas) {
            state.undo.pop_back();
        }
        // The press on the handle took the keyboard from the text; in the note being typed
        // in, the caret comes back where it was (unless a fold moved it already).
        if !select && state.active_text == Some(id) && editor_state.pending_block_focus().is_none()
        {
            editor_state.restore_focus();
        }
        changed |= applied;
    }
    if hot {
        // the desktop notebook's frame: a thin grey line round the note and its grab bar, selected or
        // not (a selected note shows it by its darker bar).
        let frame = rect.with_min_y(rect.top() - vp.px(NOTE_BAR));
        let outline = Stroke::new(vp.px(1.0), t.container_border);
        ui.painter()
            .rect_stroke(frame, CornerRadius::ZERO, outline, StrokeKind::Outside);
        changed |= item_chrome(ui, page, state, id, rect, vp, t, ResizeMode::Width);
    }
    // The note a Shift+dragged note would merge into.
    let merge_target = vp.hovering(rect)
        && state
            .drag
            .as_ref()
            .is_some_and(|drag| drag.merge && !drag.moves(id));
    if merge_target {
        ui.painter().rect_stroke(
            rect.with_min_y(rect.top() - vp.px(NOTE_BAR)),
            CornerRadius::ZERO,
            Stroke::new(vp.px(2.0), t.accent),
            StrokeKind::Outside,
        );
    }
    // A change in the note that is the page's step (a picture's corner drag, a column
    // border dragged in a note not being typed in): recorded from the note as it was just
    // before, after the typing in it, and the note's own history goes, as for any change
    // the page makes to a note (a later undo in its text would cross it).
    if let Some((_, before)) = editor_state
        .page_step
        .take()
        .filter(|(owner, _)| *owner == doc_id)
    {
        let mut canvas = page.canvas.clone();
        if let Some(doc) = canvas.item_mut(id).and_then(CanvasItem::doc_mut) {
            *doc = before;
            state.push_undo(&canvas);
            editor_state.forget_history_of(&[&doc_id]);
        }
    }
    let exists = page.canvas.item(id).is_some();
    (changed, focused && exists)
}

/// A picture in a note changed as a page step: that note's own history goes, as for any
/// change the page makes to a note, so a later undo in its text never brings the picture
/// back as it was.
fn forget_picture_note_history(page: &Page, editor_state: &mut EditorState, target: PictureTarget) {
    if let PictureTarget::Block { note, .. } = target {
        editor_state.forget_history_of(&[&format!("{}:{}", page.id, note)]);
    }
}

/// What an item's right-click menu is for, which decides its rows (`MenuKind::rows`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    /// A note container (the menu of its grab bar).
    Note,
    /// A picture; `printout` when it is a page of a printout.
    Picture {
        printout: bool,
    },
    /// An attached file; `pdf` for a PDF that can still become a printout (none of it is
    /// on the page yet).
    File {
        pdf: bool,
    },
    Recording,
    /// A shape, frise, equation or old canvas table: `edit` when it has an Edit row (a
    /// connector and a table have none), `latex` for an equation with a formula.
    Object {
        edit: bool,
        latex: bool,
    },
    /// Several items, ink, or a right-click inside the selection's frame.
    Several,
}

/// A row of an item's right-click menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemRow {
    Cut,
    Copy,
    Paste,
    Delete,
    /// Order ›: Bring Forward, Bring to Front, Send Backward, Send to Back.
    Order,
    /// A note's Select ›: Level 1 to 5, All Levels (`editor::level_rows`).
    Select,
    Open,
    SaveAs,
    PlayPause,
    /// Edit label, Edit frise or Edit equation.
    Edit,
    CopyLatex,
    /// A picture's Rotate ›.
    Rotate,
    RestoreSize,
    Background,
    AltText,
    /// A printout page's "Copy Text from This Page of the Printout".
    CopyPrintoutPage,
    /// A printout page's "Copy Text from All Pages of the Printout".
    CopyPrintoutAll,
    /// A printout page's "Open Original": the PDF it comes from.
    OpenOriginal,
    /// An attached PDF's "Insert Printout".
    InsertPrintout,
    Separator,
}

impl MenuKind {
    /// The menu for a right-click on item `id` (`None`: inside the selection's frame), once
    /// the click has selected it: one item's own menu while it is all that is selected,
    /// else the menu of several items.
    fn of(page: &Page, state: &CanvasState, id: Option<u64>) -> Self {
        let Some(item) = id
            .filter(|id| state.selection.primary() == Some(*id))
            .and_then(|id| page.canvas.item(id))
        else {
            return MenuKind::Several;
        };
        match &item.kind {
            CanvasKind::Text { .. } => MenuKind::Note,
            CanvasKind::Image { printout, .. } => MenuKind::Picture {
                printout: printout.is_some(),
            },
            CanvasKind::File {
                attachment, name, ..
            } => MenuKind::File {
                pdf: is_pdf_name(name) && !has_printout(page, attachment),
            },
            CanvasKind::Audio { .. } => MenuKind::Recording,
            CanvasKind::Shape { .. } => MenuKind::Object {
                edit: !item.is_connector(),
                latex: false,
            },
            CanvasKind::Timeline { .. } => MenuKind::Object {
                edit: true,
                latex: false,
            },
            CanvasKind::Equation { source, .. } => MenuKind::Object {
                edit: true,
                latex: !source.trim().is_empty(),
            },
            CanvasKind::Table { .. } => MenuKind::Object {
                edit: false,
                latex: false,
            },
        }
    }

    /// The menu's rows in the desktop notebook's order (reference §5), with a separator between groups.
    fn rows(self) -> Vec<ItemRow> {
        use ItemRow as R;
        match self {
            MenuKind::Note => vec![
                R::Cut,
                R::Copy,
                R::Paste,
                R::Delete,
                R::Separator,
                R::Select,
                R::Order,
            ],
            MenuKind::Picture { printout } => {
                let mut rows = vec![R::Cut, R::Copy];
                // Reference §5: a printout page's text rows and View Original Printout
                // come right after Copy.
                if printout {
                    rows.extend([R::CopyPrintoutPage, R::CopyPrintoutAll, R::OpenOriginal]);
                }
                rows.extend([
                    R::Paste,
                    R::Separator,
                    R::SaveAs,
                    R::Separator,
                    R::Order,
                    R::Separator,
                    R::Rotate,
                    R::RestoreSize,
                    R::Background,
                    R::AltText,
                    R::Separator,
                    R::Delete,
                ]);
                rows
            }
            MenuKind::File { pdf } => {
                let mut rows = vec![R::Open, R::SaveAs, R::Separator];
                // Reference §5: Insert Printout opens the attachment's printout group.
                if pdf {
                    rows.extend([R::InsertPrintout, R::Separator]);
                }
                rows.extend([R::Cut, R::Copy, R::Separator, R::Delete]);
                rows
            }
            MenuKind::Recording => vec![
                R::PlayPause,
                R::SaveAs,
                R::Separator,
                R::Cut,
                R::Copy,
                R::Separator,
                R::Delete,
            ],
            MenuKind::Object { edit, latex } => {
                let mut rows = vec![R::Cut, R::Copy, R::Paste, R::Separator];
                if edit {
                    rows.push(R::Edit);
                }
                if latex {
                    rows.push(R::CopyLatex);
                }
                if edit || latex {
                    rows.push(R::Separator);
                }
                rows.extend([R::Order, R::Separator, R::Delete]);
                rows
            }
            MenuKind::Several => vec![R::Cut, R::Copy, R::Delete, R::Separator, R::Order],
        }
    }
}

impl ItemRow {
    /// The row's glyph, label and shortcut hint. `kind` is the item's, for Edit's label.
    fn text(self, kind: Option<&CanvasKind>) -> (&'static str, Cow<'static, str>, String) {
        let none = String::new;
        match self {
            ItemRow::Cut => (
                icons::SCISSORS,
                t!("common-cut"),
                util::menu_shortcut(Modifiers::COMMAND, Key::X),
            ),
            ItemRow::Copy => (
                icons::COPY,
                t!("common-copy"),
                util::menu_shortcut(Modifiers::COMMAND, Key::C),
            ),
            ItemRow::Paste => (
                icons::CLIPBOARD_TEXT,
                t!("common-paste"),
                util::menu_shortcut(Modifiers::COMMAND, Key::V),
            ),
            ItemRow::Delete => (
                icons::TRASH,
                t!("common-delete"),
                util::menu_shortcut(Modifiers::NONE, Key::Delete),
            ),
            ItemRow::Order => (icons::LAYER, t!("canvas-menu-order"), none()),
            ItemRow::Select => (icons::SELECTION_ALL, t!("editor-select-levels"), none()),
            ItemRow::Open => (icons::ARROW_SQUARE_OUT, t!("common-open"), none()),
            ItemRow::SaveAs => (icons::FLOPPY_DISK, t!("canvas-menu-save-as"), none()),
            ItemRow::PlayPause => (icons::PLAY, t!("canvas-menu-play-pause"), none()),
            ItemRow::Edit => {
                let label = match kind {
                    Some(CanvasKind::Timeline { .. }) => t!("canvas-edit-frise"),
                    Some(CanvasKind::Equation { .. }) => t!("canvas-menu-edit-equation"),
                    _ => t!("canvas-menu-edit-label"),
                };
                (icons::RENAME, label, none())
            }
            ItemRow::CopyLatex => (icons::COPY, t!("canvas-menu-copy-latex"), none()),
            ItemRow::Rotate => (icons::ARROW_ROTATE_CLOCKWISE, t!("picture-rotate"), none()),
            ItemRow::RestoreSize => (icons::ARROW_RESET, t!("picture-restore-size"), none()),
            ItemRow::Background => (icons::IMAGE_ARROW_BACK, t!("picture-background"), none()),
            ItemRow::AltText => (icons::IMAGE_ALT_TEXT, t!("picture-alt-text-menu"), none()),
            ItemRow::CopyPrintoutPage => (icons::COPY, t!("printout-menu-copy-page"), none()),
            ItemRow::CopyPrintoutAll => (icons::COPY, t!("printout-menu-copy-all"), none()),
            ItemRow::OpenOriginal => (
                icons::ARROW_SQUARE_OUT,
                t!("printout-menu-open-original"),
                none(),
            ),
            ItemRow::InsertPrintout => (icons::FILE_PDF, t!("printout-menu-insert"), none()),
            ItemRow::Separator => ("", Cow::Borrowed(""), none()),
        }
    }
}

/// The width of a right-click menu of these rows (glyph, label, shortcut hint): its widest
/// label and widest hint with the room `widgets::menu_row` leaves around them, and 32 px
/// for a submenu's arrow.
pub(crate) fn menu_width<'a>(
    ui: &Ui,
    rows: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> f32 {
    let (mut label, mut hint) = (0.0_f32, 0.0_f32);
    for (glyph, text, shortcut) in rows {
        let row = format!("{glyph}    {text}");
        label = label.max(widgets::text_width(ui.ctx(), &row, theme::ui(12.5)));
        hint = hint.max(widgets::text_width(ui.ctx(), shortcut, theme::ui(11.5)));
    }
    let gap = if hint > 0.0 { 24.0 } else { 0.0 };
    (label + 2.0 * ui.spacing().button_padding.x + 32.0 + hint + gap).max(170.0)
}

/// A submenu's row text, laid out as `widgets::menu_row` lays out its glyph and label.
pub(crate) fn submenu_text(glyph: &str, label: &str, t: Tokens) -> RichText {
    RichText::new(format!("{glyph}    {label}"))
        .font(theme::ui(12.5))
        .color(t.ink)
}

/// Order ›'s four rows, as the desktop notebook lists them: Bring Forward, Bring to Front, Send
/// Backward, Send to Back. They restack the whole selection.
fn order_menu(ui: &mut Ui, page: &mut Page, state: &mut CanvasState, t: Tokens) -> bool {
    // (glyph, label, one step rather than all the way, up)
    let rows = [
        (
            icons::POSITION_FORWARD,
            t!("ribbon-bring-forward"),
            true,
            true,
        ),
        (
            icons::POSITION_TO_FRONT,
            t!("picture-bring-to-front"),
            false,
            true,
        ),
        (
            icons::POSITION_BACKWARD,
            t!("ribbon-send-backward"),
            true,
            false,
        ),
        (
            icons::POSITION_TO_BACK,
            t!("picture-send-to-back"),
            false,
            false,
        ),
    ];
    ui.set_width(menu_width(
        ui,
        rows.iter()
            .map(|(glyph, label, ..)| (*glyph, label.as_ref(), "")),
    ));
    let mut changed = false;
    for (glyph, label, step, up) in rows {
        if widgets::menu_row(ui, Some(glyph), &label, "", true, t).clicked() {
            changed |= if step {
                restack_step(page, state, up)
            } else {
                restack_selection(page, state, up)
            };
            ui.close();
        }
    }
    changed
}

/// The right-click menu of an item (and a note's grab bar), or of a group (`id` is `None`:
/// a right-click inside the selection's frame), with the desktop notebook's rows for the kind of thing
/// it is (`MenuKind`). Its actions apply to the whole selection.
fn item_menu(
    response: &egui::Response,
    page: &mut Page,
    state: &mut CanvasState,
    id: Option<u64>,
    t: Tokens,
) -> bool {
    if response.secondary_clicked() {
        // Where the menu was asked for, as the paper's own menu keeps it.
        state.menu_at = response.interact_pointer_pos();
    }
    let mut changed = false;
    response.context_menu(|ui| {
        let kind = MenuKind::of(page, state, id);
        // The one item the menu is for, unless it is for several.
        let single = id.filter(|_| kind != MenuKind::Several);
        let item_kind = single
            .and_then(|id| page.canvas.item(id))
            .map(|item| &item.kind);
        let latex = match item_kind {
            Some(CanvasKind::Equation { source, .. }) => Some(math::latex_of(source)),
            _ => None,
        };
        let rows: Vec<_> = kind
            .rows()
            .into_iter()
            .map(|row| {
                let (glyph, label, hint) = row.text(item_kind);
                (row, glyph, label, hint)
            })
            .collect();
        ui.set_width(menu_width(
            ui,
            rows.iter()
                .filter(|(row, ..)| *row != ItemRow::Separator)
                .map(|(_, glyph, label, hint)| (*glyph, label.as_ref(), hint.as_str())),
        ));
        for (row, glyph, label, hint) in rows {
            match row {
                ItemRow::Separator => {
                    ui.separator();
                    continue;
                }
                ItemRow::Order => {
                    // Ink alone has no place in the stacking order.
                    let items = !state.selection.items.is_empty();
                    ui.add_enabled_ui(items, |ui| {
                        ui.menu_button(submenu_text(glyph, &label, t), |ui| {
                            changed |= order_menu(ui, page, state, t);
                        });
                    });
                    continue;
                }
                ItemRow::Select => {
                    ui.menu_button(submenu_text(glyph, &label, t), |ui| {
                        if let (Some(levels), Some(id)) = (editor::level_rows(ui, t), single) {
                            state.note_request =
                                Some((id, editor::HandleAction::ShowLevels(levels)));
                        }
                    });
                    continue;
                }
                ItemRow::Rotate => {
                    ui.menu_button(submenu_text(glyph, &label, t), |ui| {
                        let command = editor::rotate_rows(ui, t);
                        if let (Some(command), Some(id)) = (command, single) {
                            state.picture_request = Some((PictureTarget::Item(id), command));
                            ui.close();
                        }
                    });
                    continue;
                }
                _ => {}
            }
            if !widgets::menu_row(ui, Some(glyph), &label, &hint, true, t).clicked() {
                continue;
            }
            match (row, single) {
                (ItemRow::Cut, _) => {
                    if copy_selection(ui.ctx(), page, state) {
                        changed |= delete_selection(page, state);
                    }
                }
                (ItemRow::Copy, _) => {
                    copy_selection(ui.ctx(), page, state);
                }
                // The platform answers with a paste event, which `canvas_keys` takes as
                // it takes Ctrl+V's, at the point clicked.
                (ItemRow::Paste, _) => request_paste_at(ui.ctx(), state),
                (ItemRow::Delete, _) => changed |= delete_selection(page, state),
                (ItemRow::Open, Some(id)) => state.item_action = Some((id, ItemAction::Open)),
                (ItemRow::SaveAs, Some(id)) => {
                    state.item_action = Some((id, ItemAction::SaveAs));
                }
                (ItemRow::PlayPause, Some(id)) => {
                    state.item_action = Some((id, ItemAction::TogglePlay));
                }
                (ItemRow::Edit, Some(id)) => {
                    state.selection.set_one(id);
                    changed |= edit_item(page, state, id);
                }
                (ItemRow::CopyLatex, _) => {
                    if let Some(latex) = latex.clone() {
                        ui.ctx().copy_text(latex);
                    }
                }
                (ItemRow::RestoreSize, Some(id)) => {
                    state.picture_request =
                        Some((PictureTarget::Item(id), PictureCmd::RestoreSize));
                }
                (ItemRow::Background, Some(id)) => {
                    state.picture_request = Some((PictureTarget::Item(id), PictureCmd::Background));
                }
                (ItemRow::AltText, Some(id)) => {
                    state.picture_request = Some((PictureTarget::Item(id), PictureCmd::AltText));
                }
                (ItemRow::CopyPrintoutPage, Some(id)) => {
                    state.item_action = Some((id, ItemAction::CopyPrintoutText { all: false }));
                }
                (ItemRow::CopyPrintoutAll, Some(id)) => {
                    state.item_action = Some((id, ItemAction::CopyPrintoutText { all: true }));
                }
                (ItemRow::OpenOriginal, Some(id)) => {
                    state.item_action = Some((id, ItemAction::OpenOriginal));
                }
                (ItemRow::InsertPrintout, Some(id)) => {
                    state.item_action = Some((id, ItemAction::InsertPrintout));
                }
                _ => {}
            }
            ui.close();
        }
    });
    changed
}

/// For a click in a note's padding: the text block closest to the pointer, and whether
/// the caret goes at its start (click above or left of it) or its end.
fn nearest_line(
    ctx: &egui::Context,
    doc: &Document,
    doc_id: &str,
    pointer: Option<Pos2>,
) -> Option<(u64, usize)> {
    let fallback = doc
        .blocks
        .iter()
        .rev()
        .find(|block| block.kind.is_textual())
        .map(|block| (block.id, block.text.char_count()));
    let Some(pointer) = pointer else {
        return fallback;
    };
    doc.blocks
        .iter()
        .filter(|block| block.kind.is_textual())
        .filter_map(|block| {
            let rect = ctx.read_response(editor::block_id(doc_id, block.id))?.rect;
            let distance = if pointer.y < rect.top() {
                rect.top() - pointer.y
            } else {
                (pointer.y - rect.bottom()).max(0.0)
            };
            let at_start = pointer.y < rect.top() || pointer.x < rect.left();
            let pos = if at_start { 0 } else { block.text.char_count() };
            Some((distance, block.id, pos))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id, pos)| (id, pos))
        .or(fallback)
}

/// Pictures, shapes, frises and tables.
fn object_item(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    library: &Library,
    textures: &mut TextureCache,
    t: Tokens,
    index: usize,
    vp: Viewport,
) -> bool {
    let item = &page.canvas.items[index];
    let id = item.id;
    let rect = item.rect();
    let selected = state.selection.contains(id);
    let primary = state.selection.primary() == Some(id);
    let mut changed = false;
    let resize = match &item.kind {
        CanvasKind::Image {
            attachment,
            meta,
            printout,
        } => {
            let (attachment, meta, printout) =
                (attachment.clone(), meta.clone(), printout.is_some());
            if let Some(cropping) = state.cropping.filter(|crop| crop.item == id) {
                // While it is cropped it is neither moved nor resized: only its crop
                // handles act.
                let paint = |ui: &mut Ui, textures: &mut TextureCache, rect, meta: &PictureMeta| {
                    paint_picture(ui, library, textures, t, &attachment, rect, meta, vp, false);
                };
                let keep = crop_picture(ui, textures, &meta, cropping, vp, paint);
                if let Some(crop) = state.cropping.as_mut() {
                    crop.keep = keep;
                }
                return false;
            }
            paint_picture(
                ui,
                library,
                textures,
                t,
                &attachment,
                rect,
                &meta,
                vp,
                printout,
            );
            ResizeMode::Aspect
        }
        CanvasKind::Shape {
            shape,
            stroke,
            fill,
            label,
            flip_x,
            flip_y,
            thickness,
        } => {
            let fill = fill
                .map(|fill| theme::with_alpha(fill.to_color32(), if t.dark { 100 } else { 170 }));
            paint_shape(
                ui,
                *shape,
                rect,
                (*flip_x, *flip_y),
                Stroke::new(thickness.unwrap_or(SHAPE_LINE), ink_on_paper(*stroke, t)),
                fill,
            );
            let (shape, label) = (*shape, label.clone());
            changed |= shape_label(ui, page, state, t, index, shape, &label, rect);
            ResizeMode::Free
        }
        CanvasKind::Timeline { color, milestones } => {
            paint_timeline(ui, t, *color, milestones, rect, &item_marks(state, id));
            ResizeMode::Free
        }
        CanvasKind::Table { .. } => ResizeMode::Free,
        CanvasKind::File { name, size, .. } => {
            let marks = place_marks(&item_marks(state, id), MatchPlace::Label);
            paint_card(
                ui,
                t,
                file_glyph(name),
                t.accent,
                name,
                &util::file_size(*size),
                rect,
                &marks,
            );
            ResizeMode::Free
        }
        CanvasKind::Audio {
            attachment,
            duration_ms,
        } => {
            let (attachment, duration_ms) = (attachment.clone(), *duration_ms);
            paint_audio(ui, state, library, t, &attachment, duration_ms, rect);
            ResizeMode::Free
        }
        CanvasKind::Equation { .. } => {
            changed |= equation_item(ui, page, state, t, index, rect);
            ResizeMode::Aspect
        }
        CanvasKind::Text { .. } => return false,
    };

    let Some(item) = page.canvas.item(id) else {
        return true;
    };
    let connector = item.is_connector();
    let dragging = state.drag.as_ref().is_some_and(|drag| drag.moves(id));
    // A line is only "under" the pointer near the line itself, not across its box. So is
    // an empty outline (Ink to Shape draws those around notes): clicks inside it reach
    // what it surrounds, as in the desktop notebook. Once picked up, it moves from anywhere inside.
    let outline = match &item.kind {
        CanvasKind::Shape {
            shape,
            fill,
            label,
            flip_x,
            flip_y,
            thickness,
            ..
        } if connector || (fill.is_none() && label.is_empty() && !selected) => Some((
            *shape,
            (*flip_x, *flip_y),
            thickness.unwrap_or(SHAPE_LINE) / 2.0,
        )),
        _ => None,
    };
    let near = vp.hover.is_some_and(|pos| match outline {
        Some((shape, flips, half_width)) => {
            outline_distance(shape, rect, flips, pos) < vp.px(8.0) + half_width
        }
        None => rect.contains(pos),
    });
    // Only the Type tool picks items up; with a shape or pen tool, drags start new
    // shapes or strokes even on top of existing items.
    let picking = state.tool == CanvasTool::Select;
    if picking && (near || selected || dragging) && state.editing_label != Some(id) {
        let hit = match outline {
            Some((_, _, half_width)) => rect.expand(vp.px(8.0) + half_width),
            None => rect,
        };
        let response = ui.interact(
            hit,
            Id::unique(("canvas-item", page.id.as_str(), id)),
            Sense::click_and_drag(),
        );
        if state.tool == CanvasTool::Select {
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if response.clicked() {
                if adding_to_selection(ui) {
                    state.selection.toggle(id);
                    state.active_text = None;
                } else {
                    state.select(id);
                }
            }
            if response.double_clicked() {
                let mut equation = false;
                match &item.kind {
                    CanvasKind::Timeline { milestones, .. } => {
                        state.timeline_editor = Some(TimelineEditor {
                            item_id: id,
                            milestones: milestones.clone(),
                        });
                    }
                    CanvasKind::Shape { .. } if !connector => {
                        state.editing_label = Some(id);
                        state.focus_label = true;
                    }
                    CanvasKind::File { .. } => state.item_action = Some((id, ItemAction::Open)),
                    CanvasKind::Audio { .. } => {
                        state.item_action = Some((id, ItemAction::TogglePlay))
                    }
                    CanvasKind::Equation { .. } => equation = true,
                    _ => {}
                }
                if equation {
                    changed |= begin_equation_edit(page, state, id);
                }
            }
            if response.drag_started() {
                if !state.selection.contains(id) {
                    state.select(id);
                }
                begin_drag(ui, page, state, id, DragMode::Move);
            }
        }
        if response.secondary_clicked() && !state.selection.contains(id) {
            state.select(id);
        }
        changed |= item_menu(&response, page, state, Some(id), t);
        if page.canvas.item(id).is_none() {
            return true;
        }
    }
    // Like picking items up, the play button and seek track belong to the Type tool: a
    // shape or pan drag that starts on a clip draws or pans instead.
    if picking
        && matches!(
            page.canvas.item(id).map(|item| &item.kind),
            Some(CanvasKind::Audio { .. })
        )
    {
        audio_controls(ui, &page.id, state, id, rect);
    }
    if selected {
        ui.painter().rect_stroke(
            if connector {
                rect.expand(vp.px(4.0))
            } else {
                rect
            },
            CornerRadius::same(3),
            Stroke::new(vp.px(1.5), t.accent),
            StrokeKind::Outside,
        );
    }
    if primary || dragging {
        changed |= item_chrome(ui, page, state, id, rect, vp, t, resize);
    }
    changed
}

/// Shift or Ctrl (Cmd on macOS) held: clicks add to or take from the selection.
fn adding_to_selection(ui: &Ui) -> bool {
    ui.input(|input| input.modifiers.shift || input.modifiers.command)
}

/// Where the primary button went down, in scene coordinates. Drags measure from there
/// so nothing lags the pointer by egui's drag threshold.
fn press_origin_scene(ui: &Ui) -> Option<Pos2> {
    let origin = ui.ctx().input(|input| input.pointer.press_origin())?;
    let to_scene = ui.ctx().layer_transform_from_global(ui.layer_id())?;
    Some(to_scene * origin)
}

fn selected_strokes(page: &Page, state: &CanvasState) -> Vec<(usize, Vec<[f32; 2]>)> {
    state
        .selection
        .strokes
        .iter()
        .filter_map(|index| {
            page.canvas
                .ink
                .get(*index)
                .map(|s| (*index, s.points.clone()))
        })
        .collect()
}

/// The area the selected items and strokes cover, in scene coordinates.
fn selection_bounds(page: &Page, selection: &Selection) -> Rect {
    let mut bounds = Rect::NOTHING;
    for item in page
        .canvas
        .items
        .iter()
        .filter(|item| selection.contains(item.id))
    {
        bounds = bounds.union(item.rect());
    }
    for stroke in selection
        .strokes
        .iter()
        .filter_map(|index| page.canvas.ink.get(*index))
    {
        bounds = bounds.union(stroke_bounds(stroke));
    }
    bounds
}

/// Adds to `selection` the items whose middle is `inside`, and the strokes with most of
/// their points inside.
fn pick_inside(page: &Page, selection: &mut Selection, inside: impl Fn(Pos2) -> bool) {
    for item in page
        .canvas
        .items
        .iter()
        .filter(|item| !item.is_background())
    {
        if inside(item.rect().center()) && !selection.contains(item.id) {
            selection.items.push(item.id);
        }
    }
    for (index, stroke) in page.canvas.ink.iter().enumerate() {
        let hits = stroke
            .points
            .iter()
            .filter(|point| inside(pos2(point[0], point[1])))
            .count();
        if hits * 2 > stroke.points.len() && !selection.strokes.contains(&index) {
            selection.strokes.push(index);
        }
    }
}

fn band_selection(page: &Page, band: &Band) -> Selection {
    let area = Rect::from_two_pos(band.start, band.end);
    let mut selection = band.base.clone();
    pick_inside(page, &mut selection, |pos| area.contains(pos));
    selection
}

/// Whether `point` is inside `polygon` (even-odd rule).
fn point_in_polygon(point: Pos2, polygon: &[Pos2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if (current.y > point.y) != (previous.y > point.y)
            && point.x
                < (previous.x - current.x) * (point.y - current.y) / (previous.y - current.y)
                    + current.x
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// The lasso: draw around ink and items to select them. A drag that starts inside the
/// current selection moves it instead, as in the desktop notebook.
fn lasso_tool(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    canvas_rect: Rect,
    pointer: Option<Pos2>,
    vp: Viewport,
) {
    let overlay = ui.interact(
        canvas_rect,
        Id::unique(("canvas-lasso", page.id.as_str())),
        Sense::click_and_drag(),
    );
    let has_selection = !state.selection.is_empty();
    let area = selection_bounds(page, &state.selection).expand(vp.px(8.0));
    let over_selection = move |pos: Pos2| has_selection && area.is_finite() && area.contains(pos);
    let panning = pan_by_drag(ui, &overlay, state);
    if overlay.hovered() && !panning {
        let moving = pointer.is_some_and(over_selection);
        ui.ctx().set_cursor_icon(if moving {
            CursorIcon::Move
        } else {
            CursorIcon::Crosshair
        });
    }
    if overlay.clicked() {
        state.selection.clear();
        state.active_text = None;
    }
    if overlay.drag_started_by(PointerButton::Primary) {
        let start = press_origin_scene(ui).or(pointer);
        if start.is_some_and(over_selection) {
            begin_group_drag(ui, page, state);
        } else if let Some(start) = start {
            let base = if adding_to_selection(ui) {
                state.selection.clone()
            } else {
                Selection::default()
            };
            state.lasso = Some(Lasso {
                points: vec![start],
                base,
            });
        }
    }
    if overlay.dragged_by(PointerButton::Primary) {
        if let (Some(pos), Some(lasso)) = (pointer, state.lasso.as_mut()) {
            if lasso
                .points
                .last()
                .is_none_or(|last| last.distance(pos) > vp.px(3.0))
            {
                lasso.points.push(pos);
            }
        }
    }
    if overlay.drag_stopped() {
        if let Some(lasso) = state.lasso.take() {
            let mut selection = lasso.base;
            pick_inside(page, &mut selection, |pos| {
                point_in_polygon(pos, &lasso.points)
            });
            state.selection = selection;
            state.active_text = None;
        }
    }
}

/// Insert › Insert Space, as in the desktop notebook: a drag on the page moves everything past the line
/// where it started down (or right) by the drag, opening space there; dragging back up (or
/// left) closes space until the first thing moved reaches the line. The whole drag is one
/// undo step, and the tool goes back to Select after it. Returns whether the page changed.
fn insert_space_tool(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    area: Rect,
    pointer: Option<Pos2>,
    vp: Viewport,
    t: Tokens,
) -> bool {
    // On top of the items, so a drag that starts on one inserts space too.
    let response = ui.interact(
        area,
        Id::unique(("canvas-insert-space", page.id.as_str())),
        Sense::drag(),
    );
    // Middle- and right-drag still pan the page.
    let panning = pan_by_drag(ui, &response, state);
    if response.drag_started_by(PointerButton::Primary) {
        if let Some(line) = press_origin_scene(ui).or(pointer) {
            state.space = Some(SpaceDrag {
                line,
                down: None,
                items: Vec::new(),
                strokes: Vec::new(),
                room: 0.0,
            });
        }
    }
    if response.dragged_by(PointerButton::Primary) {
        if let (Some(pos), Some(mut space)) = (pointer, state.space.take()) {
            let delta = pos - space.line;
            if space.down.is_none() && delta.length() > vp.px(6.0) {
                let down = delta.y.abs() >= delta.x.abs();
                space.down = Some(down);
                state.push_undo(&page.canvas);
                collect_past_line(page, &mut space, down);
            }
            if let Some(down) = space.down {
                let along = if down { delta.y } else { delta.x };
                shift_past_line(page, &space, along.round().max(-space.room));
            }
            state.space = Some(space);
        }
    }
    // A drag that ended without its stop (the pointer left the window) ends once the
    // button is up.
    let stopped = state.space.is_some()
        && (response.drag_stopped()
            || (!response.dragged() && !ui.input(|input| input.pointer.primary_down())));
    if stopped {
        let mut changed = state.space.take().is_some_and(|space| space.down.is_some());
        // Dragged back to where it started: nothing moved, so no undo step either.
        if changed && state.undo.back() == Some(&page.canvas) {
            state.undo.pop_back();
            changed = false;
        }
        // One use, as the desktop notebook's Insert Space.
        state.tool = CanvasTool::Select;
        return changed;
    }
    if panning {
        return false;
    }

    // The guide: a dashed line across the page in view, where space goes in.
    let vertical = state
        .space
        .as_ref()
        .is_some_and(|space| space.down == Some(false));
    let at = state.space.as_ref().map(|space| space.line).or(vp.hover);
    if let Some(at) = at {
        let span = area.intersect(ui.clip_rect());
        let ends = if vertical {
            [pos2(at.x, span.top()), pos2(at.x, span.bottom())]
        } else {
            [pos2(span.left(), at.y), pos2(span.right(), at.y)]
        };
        ui.painter().extend(egui::Shape::dashed_line(
            &ends,
            Stroke::new(vp.px(1.0), t.accent),
            vp.px(6.0),
            vp.px(4.0),
        ));
        ui.ctx().set_cursor_icon(if vertical {
            CursorIcon::ResizeHorizontal
        } else {
            CursorIcon::ResizeVertical
        });
    }
    false
}

/// Records what an Insert Space drag moves: the items whose top (or left) is at or past its
/// line, the strokes whose bounds start there, where each is now, and how far they may come
/// back before the first one reaches the line.
fn collect_past_line(page: &Page, space: &mut SpaceDrag, down: bool) {
    let axis = usize::from(down);
    let line = space.line[axis];
    // Items also stay below the room kept above every item for its grab bar.
    let item_floor = if down {
        line.max(ITEM_TOP)
    } else {
        line.max(0.0)
    };
    let mut room = f32::INFINITY;
    space.items = page
        .canvas
        .items
        .iter()
        .filter(|item| item.rect().min[axis] >= line)
        .map(|item| {
            room = room.min(item.rect().min[axis] - item_floor);
            (item.id, pos2(item.x, item.y))
        })
        .collect();
    space.strokes = page
        .canvas
        .ink
        .iter()
        .enumerate()
        .filter_map(|(index, stroke)| {
            let start = stroke_bounds(stroke).min[axis];
            (start >= line).then(|| {
                room = room.min(start - line.max(0.0));
                (index, stroke.points.clone())
            })
        })
        .collect();
    space.room = if room.is_finite() { room.max(0.0) } else { 0.0 };
}

/// Puts everything an Insert Space drag moves `offset` past where it was when the drag's
/// direction was fixed.
fn shift_past_line(page: &mut Page, space: &SpaceDrag, offset: f32) {
    let axis = usize::from(space.down == Some(true));
    for (id, origin) in &space.items {
        if let Some(item) = page.canvas.item_mut(*id) {
            let mut at = *origin;
            at[axis] += offset;
            item.x = at.x;
            item.y = at.y;
        }
    }
    for (index, points) in &space.strokes {
        if let Some(stroke) = page.canvas.ink.get_mut(*index) {
            stroke.points = points
                .iter()
                .map(|point| {
                    let mut point = *point;
                    point[axis] += offset;
                    point
                })
                .collect();
        }
    }
}

fn begin_drag(ui: &Ui, page: &mut Page, state: &mut CanvasState, id: u64, mode: DragMode) {
    // Moving an item that is part of a group moves the whole group.
    let group =
        mode == DragMode::Move && state.selection.contains(id) && state.selection.is_group();
    let (ids, strokes) = if group {
        (state.selection.items.clone(), selected_strokes(page, state))
    } else {
        (vec![id], Vec::new())
    };
    start_drag(ui, page, state, mode, &ids, strokes);
}

/// Starts moving the whole selection (from its frame, or the lasso tool).
fn begin_group_drag(ui: &Ui, page: &mut Page, state: &mut CanvasState) {
    let ids = state.selection.items.clone();
    let strokes = selected_strokes(page, state);
    start_drag(ui, page, state, DragMode::Move, &ids, strokes);
}

fn start_drag(
    ui: &Ui,
    page: &mut Page,
    state: &mut CanvasState,
    mode: DragMode,
    ids: &[u64],
    strokes: Vec<(usize, Vec<[f32; 2]>)>,
) {
    let items: Vec<(u64, Rect)> = ids
        .iter()
        .filter_map(|id| page.canvas.item(*id).map(|item| (*id, item.rect())))
        .collect();
    // Only the primary button moves things (a middle- or right-drag would leave behind an
    // empty undo step, since `update_drag` drops a drag once the primary button is up).
    if (items.is_empty() && strokes.is_empty()) || !ui.input(|input| input.pointer.primary_down()) {
        return;
    }
    let Some(start_pointer) = press_origin_scene(ui).or_else(|| scene_pointer(ui)) else {
        return;
    };
    state.push_undo(&page.canvas);
    state.drag = Some(DragState {
        mode,
        start_pointer,
        items,
        strokes,
        merge: false,
    });
}

/// Moves or resizes what is being dragged. Item positions snap to whole points so text
/// stays crisp at 100%.
fn update_drag(
    ui: &Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
) -> bool {
    let Some(drag) = state.drag.take() else {
        return false;
    };
    if !ui.input(|input| input.pointer.primary_down()) {
        if matches!(drag.mode, DragMode::Resize(_)) {
            state.resized = drag.items.first().map(|(id, _)| *id);
        }
        if drag.merge {
            return merge_dropped_note(ui, page, state, editor_state, &drag);
        }
        if matches!(drag.mode, DragMode::Move) && drag.strokes.is_empty() {
            return snap_note_to_rules(page, &drag.items);
        }
        return false;
    }
    let Some(pointer) = scene_pointer(ui) else {
        state.drag = Some(drag);
        return false;
    };
    let delta = pointer - drag.start_pointer;
    let mut changed = false;
    match drag.mode {
        DragMode::Move => {
            let delta = limit_move(
                delta,
                drag.items.iter().map(|(_, start)| *start),
                drag.strokes.iter().flat_map(|(_, start)| start.iter()),
            );
            for (id, start) in &drag.items {
                let Some(item) = page.canvas.item_mut(*id) else {
                    continue;
                };
                let before = item.rect();
                item.x = (start.left() + delta.x).round();
                item.y = (start.top() + delta.y).round();
                changed |= item.rect() != before;
            }
            for (index, start) in &drag.strokes {
                let Some(stroke) = page.canvas.ink.get_mut(*index) else {
                    continue;
                };
                for (point, from) in stroke.points.iter_mut().zip(start) {
                    let moved = [from[0] + delta.x, from[1] + delta.y];
                    changed |= *point != moved;
                    *point = moved;
                }
            }
        }
        DragMode::Resize(mode) => {
            if let Some(item) = drag
                .items
                .first()
                .and_then(|(id, _)| page.canvas.item_mut(*id))
            {
                let start = drag.items[0].1;
                let before = item.rect();
                let min = if item.is_connector() {
                    Vec2::ZERO
                } else if matches!(item.kind, CanvasKind::Timeline { .. }) {
                    FRISE_MIN
                } else {
                    Vec2::splat(24.0)
                };
                match mode {
                    ResizeMode::Width => {
                        // A dragged width stays, as in the desktop notebook.
                        item.auto_width = false;
                        item.width = (start.width() + delta.x).clamp(80.0, 2400.0).round();
                    }
                    ResizeMode::Free => {
                        item.width = (start.width() + delta.x).clamp(min.x, 2400.0).round();
                        item.height = (start.height() + delta.y).max(min.y).round();
                    }
                    ResizeMode::Aspect => {
                        let ratio = start.height() / start.width().max(1.0);
                        let width = (start.width() + delta.x).clamp(40.0, 2400.0);
                        item.width = width.round();
                        item.height = (width * ratio).round();
                    }
                }
                changed = item.rect() != before;
            }
        }
    }
    state.drag = Some(drag);
    changed
}

/// A note dragged on its own on a ruled page lands with its text on the nearest rule, as
/// in the desktop notebook, instead of a few pixels off it (which would move its text to the next rule
/// down, leaving a gap above it). Part of the drag's undo step.
fn snap_note_to_rules(page: &mut Page, items: &[(u64, Rect)]) -> bool {
    let (Some(pitch), [(id, _)]) = (ruled_pitch(page), items) else {
        return false;
    };
    let Some(item) = page
        .canvas
        .item_mut(*id)
        .filter(|item| matches!(item.kind, CanvasKind::Text { .. }))
    else {
        return false;
    };
    // Where the default note's first rule is, relative to its frame's top.
    let first = FIRST_RULE - PAGE_CONTENT_TOP;
    let nearest = rule_at_or_below(item.y + first - pitch / 2.0, pitch);
    let mut y = nearest - first;
    if y < ITEM_TOP {
        y += pitch;
    }
    let changed = item.y != y;
    item.y = y;
    changed
}

/// Ends a Shift+drag of a note: dropped on another note, its paragraphs join that one (at
/// the top when dropped on its upper half, else at the end) and it goes away. The drag's
/// undo step covers the whole merge.
fn merge_dropped_note(
    ui: &Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    drag: &DragState,
) -> bool {
    let Some(&(dragged, _)) = drag.items.first() else {
        return false;
    };
    let Some(pointer) = scene_pointer(ui) else {
        return false;
    };
    let target = page
        .canvas
        .items
        .iter()
        .filter(|item| {
            item.id != dragged
                && matches!(item.kind, CanvasKind::Text { .. })
                && item.rect().contains(pointer)
        })
        .max_by_key(|item| item.z)
        .map(|item| (item.id, pointer.y < item.rect().center().y));
    let Some((target, at_top)) = target else {
        return false;
    };
    // Only plain notes merge (stickies and the rest stay where they were dropped).
    let is_note = page
        .canvas
        .item(dragged)
        .is_some_and(|item| matches!(item.kind, CanvasKind::Text { .. }));
    if !is_note {
        return false;
    }
    let Some(CanvasItem {
        kind: CanvasKind::Text { doc: moved },
        ..
    }) = page.canvas.remove(dragged)
    else {
        return false;
    };
    if let Some(doc) = page.canvas.item_mut(target).and_then(CanvasItem::doc_mut) {
        if at_top {
            doc.blocks.splice(0..0, moved.blocks);
        } else {
            doc.blocks.extend(moved.blocks);
        }
    }
    // Only the page's history records the merge: the editor's undo for either note (the
    // one last typed in) must not reach back past it.
    editor_state.forget_history_of(&[
        &format!("{}:{}", page.id, target),
        &format!("{}:{}", page.id, dragged),
    ]);
    if state.active_text == Some(dragged) {
        state.active_text = None;
        surrender_text_focus(ui);
    }
    state.selection.set_one(target);
    true
}

/// The grab bar of a hovered or selected item (drag it to move; click it to select the
/// whole item), its delete button (not on notes: Delete, the menu and Cut remove them, as
/// in the desktop notebook), and its resize handle.
fn item_chrome(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    id: u64,
    rect: Rect,
    vp: Viewport,
    t: Tokens,
    resize: ResizeMode,
) -> bool {
    let selected = state.selection.contains(id);
    // A note's bar is the desktop notebook's: flush with the top of its frame, light grey, a dotted grip
    // in the middle and the ◂▸ width handle at its right end, and no delete button.
    // Pictures, shapes and the rest keep a bar above them with one.
    let note = resize == ResizeMode::Width;
    let (bar_height, gap) = if note {
        (NOTE_BAR, 0.0)
    } else {
        (BAR_HEIGHT, 2.0)
    };
    let bar = Rect::from_min_max(
        pos2(rect.left(), rect.top() - vp.px(bar_height + gap)),
        pos2(
            rect.right().max(rect.left() + vp.px(60.0)),
            rect.top() - vp.px(gap),
        ),
    );
    let close = (!note).then(|| {
        Rect::from_min_max(
            pos2(bar.right() - vp.px(BAR_HEIGHT + 4.0), bar.top()),
            bar.max,
        )
    });
    let bar_response = ui
        .interact(
            bar,
            Id::unique(("canvas-bar", page.id.as_str(), id)),
            Sense::click_and_drag(),
        )
        .on_hover_text(t!("canvas-bar-tip").as_ref());
    let close_response = close.map(|close| {
        ui.interact(
            close,
            Id::unique(("canvas-close", page.id.as_str(), id)),
            Sense::click(),
        )
        .on_hover_text(t!("common-delete").as_ref())
    });
    let dragging = state.drag.as_ref().is_some_and(|drag| drag.moves(id));
    // The note being typed in is selected too, but its bar stays the desktop notebook's grey: only a
    // note selected as a whole (Escape, a click on its bar) shows the desktop notebook's darker bar.
    // The accent is for pictures, shapes and ink.
    let typing = state.active_text == Some(id);
    let fill = if note {
        if bar_response.hovered() || dragging {
            t.container_border
        } else if selected && !typing {
            t.line_strong
        } else {
            t.grab_bar
        }
    } else if bar_response.hovered() || dragging {
        theme::with_alpha(t.accent, 90)
    } else if selected {
        theme::with_alpha(t.accent, 55)
    } else {
        theme::with_alpha(t.ink, 22)
    };
    // A note's bar is square, inside its square frame.
    let radius = if note {
        0
    } else {
        (3.0 * vp.zoom.min(1.0)).round() as u8
    };
    ui.painter().rect_filled(
        bar,
        CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        },
        fill,
    );
    if note {
        let grip = if selected || bar_response.hovered() {
            t.ink
        } else {
            t.muted
        };
        for col in 0..4 {
            ui.painter().circle_filled(
                pos2(
                    bar.center().x + (col as f32 - 1.5) * vp.px(3.5),
                    bar.center().y,
                ),
                vp.px(0.9),
                grip,
            );
        }
    } else {
        let grip = if selected || bar_response.hovered() {
            t.accent
        } else {
            t.muted
        };
        for row in 0..2 {
            for col in 0..3 {
                ui.painter().circle_filled(
                    pos2(
                        bar.center().x + (col as f32 - 1.0) * vp.px(5.0),
                        bar.center().y + (row as f32 - 0.5) * vp.px(4.0),
                    ),
                    vp.px(1.1),
                    grip,
                );
            }
        }
    }
    if let (Some(close), Some(close_response)) = (close, &close_response) {
        ui.painter().text(
            close.center(),
            Align2::CENTER_CENTER,
            icons::X,
            theme::icon(vp.px(10.0)),
            if close_response.hovered() {
                t.danger
            } else {
                t.muted
            },
        );
        if close_response.clicked() {
            surrender_text_focus(ui);
            delete_item(page, state, id);
            return true;
        }
    }
    if bar_response.hovered() {
        ui.ctx().set_cursor_icon(if dragging {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        });
    }
    if bar_response.clicked() || bar_response.secondary_clicked() {
        surrender_text_focus(ui);
        state.active_text = None;
        if bar_response.clicked() && adding_to_selection(ui) {
            state.selection.toggle(id);
        } else if bar_response.clicked() || !state.selection.contains(id) {
            state.selection.set_one(id);
        }
    }
    if item_menu(&bar_response, page, state, Some(id), t) {
        return true;
    }
    if bar_response.drag_started() {
        if !state.selection.contains(id) {
            state.selection.set_one(id);
        }
        begin_drag(ui, page, state, id, DragMode::Move);
        // Shift+drag a note onto another to merge them, as in the desktop notebook.
        let text_note = page
            .canvas
            .item(id)
            .is_some_and(|item| matches!(item.kind, CanvasKind::Text { .. }));
        if text_note && ui.input(|input| input.modifiers.shift) && !state.selection.is_group() {
            if let Some(drag) = state.drag.as_mut() {
                drag.merge = true;
            }
        }
    }

    // Items in a group move together; only a lone item can be resized.
    let resizable = !(state.selection.is_group() && state.selection.contains(id));
    if resizable {
        let handle = match resize {
            // the desktop notebook's ◂▸ at the right end of the bar.
            ResizeMode::Width => {
                Rect::from_min_max(pos2(bar.right() - vp.px(16.0), bar.top()), bar.max)
            }
            ResizeMode::Free | ResizeMode::Aspect => {
                Rect::from_center_size(rect.right_bottom(), Vec2::splat(vp.px(14.0)))
            }
        };
        let handle_response = ui
            .interact(
                handle,
                Id::unique(("canvas-resize", page.id.as_str(), id)),
                Sense::drag(),
            )
            .on_hover_text(t!("canvas-resize-tip").as_ref());
        match resize {
            ResizeMode::Width => {
                let color = if handle_response.hovered() || handle_response.dragged() {
                    t.ink
                } else {
                    t.muted
                };
                let center = handle.center();
                for dir in [-1.0f32, 1.0] {
                    let tip = center + vec2(dir * vp.px(5.5), 0.0);
                    let base = tip - vec2(dir * vp.px(3.0), 0.0);
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            tip,
                            base + vec2(0.0, -vp.px(3.0)),
                            base + vec2(0.0, vp.px(3.0)),
                        ],
                        color,
                        Stroke::NONE,
                    ));
                }
            }
            _ => {
                let handle_color = if handle_response.hovered() || handle_response.dragged() {
                    t.accent
                } else {
                    theme::with_alpha(t.accent, 190)
                };
                ui.painter()
                    .circle_filled(handle.center(), vp.px(4.5), handle_color);
                ui.painter().circle_stroke(
                    handle.center(),
                    vp.px(4.5),
                    Stroke::new(vp.px(1.0), t.raised),
                );
            }
        }
        if handle_response.hovered() || handle_response.dragged() {
            ui.ctx().set_cursor_icon(match resize {
                ResizeMode::Width => CursorIcon::ResizeHorizontal,
                _ => CursorIcon::ResizeNwSe,
            });
        }
        if handle_response.drag_started() {
            state.selection.set_one(id);
            begin_drag(ui, page, state, id, DragMode::Resize(resize));
        }
        // the desktop notebook also lets you drag a note's right edge to set its width.
        if note {
            let edge = Rect::from_center_size(
                pos2(rect.right(), rect.center().y),
                vec2(vp.px(6.0), rect.height()),
            );
            let edge_response = ui.interact(
                edge,
                Id::unique(("canvas-resize-edge", page.id.as_str(), id)),
                Sense::drag(),
            );
            if edge_response.hovered() || edge_response.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if edge_response.drag_started() {
                state.selection.set_one(id);
                begin_drag(ui, page, state, id, DragMode::Resize(ResizeMode::Width));
            }
        }
    }
    false
}

/// A page picture, fitted in its box and turned or mirrored as `meta` says. It is drawn
/// only when in view, from a texture sized for how big it shows on screen.
#[allow(clippy::too_many_arguments)]
fn paint_picture(
    ui: &mut Ui,
    library: &Library,
    textures: &mut TextureCache,
    t: Tokens,
    attachment: &str,
    rect: Rect,
    meta: &PictureMeta,
    vp: Viewport,
    printout: bool,
) {
    // Off-screen pictures cost nothing: a long printout only loads the pages in view.
    if !rect.intersects(ui.clip_rect()) {
        return;
    }
    // Turning a picture doesn't change its longest side, so the tier holds either way. A
    // cropped one shows a part of it: loaded as big as the whole would show.
    let [left, top, right, bottom] = meta.crop_box();
    let side = rect.width().max(rect.height()) / (right - left).min(bottom - top)
        * vp.zoom
        * ui.ctx().pixels_per_point();
    let lookup = textures.get_sized(ui.ctx(), attachment, side, || {
        let meta = library.attachment(attachment).ok().flatten()?;
        Some(library.attachment_path(&meta.stored_path))
    });
    match lookup {
        images::Lookup::Ready(texture) => {
            widgets::paint_texture_turned(
                ui.painter(),
                texture.id(),
                texture.size_vec2(),
                rect,
                meta,
            );
        }
        // Its turn to load comes in a frame or two: a blank sheet meanwhile.
        images::Lookup::Loading => {
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, Color32::WHITE);
        }
        images::Lookup::Missing => {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(3), t.sunken);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                t!("canvas-picture-missing"),
                theme::ui(11.0),
                t.faint,
            );
        }
    }
    // A printout page is white paper on a white page: a hairline shows its edge.
    if printout {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(vp.px(1.0), t.line_strong),
            StrokeKind::Inside,
        );
    }
}

/// Picture › Crop on the page: the whole picture dimmed, the part kept bright over it, and
/// Office's black crop handles on the kept part's edges and corners. Returns the part
/// kept, once this frame's handle drag has moved it.
fn crop_picture(
    ui: &mut Ui,
    textures: &mut TextureCache,
    meta: &PictureMeta,
    cropping: Cropping,
    vp: Viewport,
    mut paint: impl FnMut(&mut Ui, &mut TextureCache, Rect, &PictureMeta),
) -> Rect {
    let Cropping {
        item: id,
        full,
        mut keep,
    } = cropping;
    // Presses on the picture stay with it: only one outside it keeps the crop.
    ui.interact(
        full,
        Id::unique(("folio-crop", id, CROP_SIDES.len())),
        Sense::click_and_drag(),
    );
    for (index, sides) in CROP_SIDES.into_iter().enumerate() {
        let hit = Rect::from_center_size(
            crop_handle_point(keep, sides),
            Vec2::splat(vp.px(CROP_HANDLE * 2.0)),
        );
        let handle = Id::unique(("folio-crop", id, index));
        let response = ui.interact(hit, handle, Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(match sides {
                (0, _) => CursorIcon::ResizeVertical,
                (_, 0) => CursorIcon::ResizeHorizontal,
                (-1, -1) | (1, 1) => CursorIcon::ResizeNwSe,
                _ => CursorIcon::ResizeNeSw,
            });
        }
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(handle, keep));
        }
        if response.dragged() {
            let start = ui
                .data(|data| data.get_temp::<Rect>(handle))
                .unwrap_or(keep);
            if let (Some(origin), Some(pointer)) = (press_origin_scene(ui), scene_pointer(ui)) {
                keep = moved_crop(start, full, sides, pointer - origin);
            }
        }
    }
    let whole = PictureMeta {
        crop: None,
        ..meta.clone()
    };
    paint(ui, textures, full, &whole);
    ui.painter()
        .rect_filled(full, CornerRadius::ZERO, Color32::from_black_alpha(110));
    paint(
        ui,
        textures,
        keep,
        &cropped_meta(meta, crop_fractions(full, keep)),
    );
    for sides in CROP_SIDES {
        paint_crop_handle(ui.painter(), crop_handle_point(keep, sides), sides, vp);
    }
    keep
}

/// Where a crop handle sits on the kept part: a corner, or the middle of an edge.
fn crop_handle_point(keep: Rect, (x, y): (i8, i8)) -> Pos2 {
    let along = |side: i8, min: f32, max: f32| match side {
        -1 => min,
        1 => max,
        _ => (min + max) / 2.0,
    };
    pos2(
        along(x, keep.min.x, keep.max.x),
        along(y, keep.min.y, keep.max.y),
    )
}

/// `keep` with the sides a crop handle moves moved by `delta`, inside `full`, and never
/// smaller than `CROP_MIN` each way, nor than the 2% of the picture a crop keeps at least
/// (`PictureMeta::crop_box`), which would otherwise show more than the handles hold.
fn moved_crop(keep: Rect, full: Rect, (x, y): (i8, i8), delta: Vec2) -> Rect {
    let least = (full.size() * 0.02).max(Vec2::splat(CROP_MIN));
    let mut moved = keep;
    match x {
        -1 => {
            moved.min.x = (keep.min.x + delta.x)
                .min(keep.max.x - least.x)
                .max(full.min.x)
        }
        1 => {
            moved.max.x = (keep.max.x + delta.x)
                .max(keep.min.x + least.x)
                .min(full.max.x)
        }
        _ => {}
    }
    match y {
        -1 => {
            moved.min.y = (keep.min.y + delta.y)
                .min(keep.max.y - least.y)
                .max(full.min.y)
        }
        1 => {
            moved.max.y = (keep.max.y + delta.y)
                .max(keep.min.y + least.y)
                .min(full.max.y)
        }
        _ => {}
    }
    moved
}

/// One of Office's crop handles at `point`: black bars 3 points thick lying inside the
/// kept part, an L at a corner, outlined in white so they show on any picture.
fn paint_crop_handle(painter: &egui::Painter, point: Pos2, (x, y): (i8, i8), vp: Viewport) {
    let (long, thick) = (vp.px(CROP_HANDLE), vp.px(3.0));
    // Into the kept part, from each side.
    let (inward_x, inward_y) = (-f32::from(x), -f32::from(y));
    let mut bars = Vec::new();
    if y != 0 {
        let (from, to) = if x == 0 {
            (point.x - long / 2.0, point.x + long / 2.0)
        } else {
            (point.x, point.x + inward_x * long)
        };
        bars.push(Rect::from_two_pos(
            pos2(from, point.y),
            pos2(to, point.y + inward_y * thick),
        ));
    }
    if x != 0 {
        let (from, to) = if y == 0 {
            (point.y - long / 2.0, point.y + long / 2.0)
        } else {
            (point.y, point.y + inward_y * long)
        };
        bars.push(Rect::from_two_pos(
            pos2(point.x, from),
            pos2(point.x + inward_x * thick, to),
        ));
    }
    for bar in &bars {
        painter.rect_filled(bar.expand(vp.px(1.0)), CornerRadius::ZERO, Color32::WHITE);
    }
    for bar in bars {
        painter.rect_filled(bar, CornerRadius::ZERO, Color32::BLACK);
    }
}

/// The Fluent icon for a file, by its extension.
pub(crate) fn file_glyph(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "pdf" => icons::FILE_PDF,
        "doc" | "docx" | "odt" | "rtf" | "pages" => icons::FILE_DOC,
        "xls" | "xlsx" | "ods" | "numbers" => icons::FILE_XLS,
        "csv" => icons::FILE_CSV,
        "ppt" | "pptx" | "odp" | "key" => icons::FILE_PPT,
        "zip" | "7z" | "rar" | "tar" | "gz" => icons::FILE_ZIP,
        "mp3" | "wav" | "ogg" | "m4a" | "flac" | "opus" => icons::FILE_AUDIO,
        "mp4" | "mov" | "mkv" | "webm" | "avi" => icons::FILE_VIDEO,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" => icons::FILE_IMAGE,
        "txt" | "md" => icons::FILE_TEXT,
        _ => icons::FILE,
    }
}

/// The card a file or recording is shown as: an icon, a title and a detail line.
/// `marks` are find-on-page matches in the title (char ranges, in order).
pub(crate) fn paint_card(
    ui: &mut Ui,
    t: Tokens,
    glyph: &str,
    glyph_color: Color32,
    title: &str,
    detail: &str,
    rect: Rect,
    marks: &[(Range<usize>, bool)],
) {
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(6),
        t.raised,
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        theme::icon(22.0),
        glyph_color,
    );
    let left = rect.left() + 48.0;
    paint_marked_text(
        ui,
        pos2(left, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        title,
        rect.right() - left - 10.0,
        theme::ui(13.0),
        t.ink,
        marks,
        t,
    );
    ui.painter().text(
        pos2(left, rect.center().y + 9.0),
        Align2::LEFT_CENTER,
        detail,
        theme::ui(11.0),
        t.faint,
    );
}

/// The seek track along the bottom of a recording's card.
fn audio_track(rect: Rect) -> Rect {
    Rect::from_min_max(
        pos2(rect.left() + 48.0, rect.bottom() - 11.0),
        pos2(rect.right() - 14.0, rect.bottom() - 7.0),
    )
}

/// A recording's play button and seek track, made after the card's own drag area so
/// they get the clicks.
fn audio_controls(ui: &mut Ui, page_id: &str, state: &mut CanvasState, id: u64, rect: Rect) {
    let button =
        Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), Vec2::splat(34.0));
    let play = ui.interact(
        button,
        Id::unique(("audio-play", page_id, id)),
        Sense::click(),
    );
    let label = t!("canvas-play-recording");
    play.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.as_ref()));
    if play.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if play.clicked() {
        state.item_action = Some((id, ItemAction::TogglePlay));
    }
    let track = audio_track(rect);
    let seek = ui.interact(
        track.expand2(vec2(0.0, 6.0)),
        Id::unique(("audio-seek", page_id, id)),
        Sense::click_and_drag(),
    );
    if seek.clicked() || seek.dragged() {
        if let Some(pos) = scene_pointer(ui) {
            let fraction = ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
            state.item_action = Some((id, ItemAction::Seek(fraction)));
        }
    }
}

/// The length of clip `attachment` read from its file (0 when it cannot be read), once:
/// `lengths` keeps it.
fn clip_length(lengths: &mut HashMap<String, u64>, library: &Library, attachment: &str) -> u64 {
    *lengths.entry(attachment.to_string()).or_insert_with(|| {
        library
            .attachment(attachment)
            .ok()
            .flatten()
            .and_then(|meta| audio::duration_ms(&library.attachment_path(&meta.stored_path)))
            .unwrap_or(0)
    })
}

/// A recording's card: a round play button, its title and time, and a progress track.
fn paint_audio(
    ui: &mut Ui,
    state: &mut CanvasState,
    library: &Library,
    t: Tokens,
    attachment: &str,
    duration_ms: u64,
    rect: Rect,
) {
    let recording_ms = state
        .recording
        .as_ref()
        .filter(|recorder| recorder.attachment == attachment)
        .map(|recorder| recorder.elapsed().as_millis() as u64);
    let total = match recording_ms {
        Some(_) => 0,
        None if duration_ms > 0 => duration_ms,
        None => clip_length(&mut state.audio_lengths, library, attachment),
    };
    let player = state
        .player
        .as_ref()
        .filter(|player| player.attachment == attachment);
    let (glyph, color, title, detail, progress) = match (recording_ms, player) {
        (Some(elapsed), _) => (
            icons::RECORD,
            t.danger,
            t!("canvas-audio-recording-now"),
            audio::format_time(elapsed),
            None,
        ),
        (None, Some(player)) => (
            if player.is_playing() {
                icons::PAUSE
            } else {
                icons::PLAY
            },
            t.accent,
            t!("canvas-audio-title"),
            format!(
                "{} / {}",
                audio::format_time(player.position_ms()),
                audio::format_time(player.duration_ms())
            ),
            Some(player.position_ms() as f32 / player.duration_ms().max(1) as f32),
        ),
        (None, None) => (
            icons::PLAY,
            t.accent,
            t!("canvas-audio-title"),
            audio::format_time(total),
            Some(0.0),
        ),
    };
    paint_card(ui, t, glyph, color, &title, &detail, rect, &[]);
    ui.painter().circle_stroke(
        pos2(rect.left() + 26.0, rect.center().y),
        16.0,
        Stroke::new(1.5, color),
    );
    if let Some(progress) = progress {
        let track = audio_track(rect);
        let y = track.center().y;
        ui.painter().line_segment(
            [pos2(track.left(), y), pos2(track.right(), y)],
            Stroke::new(3.0, t.line),
        );
        ui.painter().line_segment(
            [
                pos2(track.left(), y),
                pos2(track.left() + track.width() * progress.clamp(0.0, 1.0), y),
            ],
            Stroke::new(3.0, t.accent),
        );
    }
}

fn connector_ends(rect: Rect, flip_x: bool, flip_y: bool) -> (Pos2, Pos2) {
    let start = pos2(
        if flip_x { rect.right() } else { rect.left() },
        if flip_y { rect.bottom() } else { rect.top() },
    );
    let end = pos2(
        if flip_x { rect.left() } else { rect.right() },
        if flip_y { rect.top() } else { rect.bottom() },
    );
    (start, end)
}

/// A diamond's or triangle's corners in its box. A flipped triangle points down.
fn polygon_corners(shape: ShapeKind, rect: Rect, flip_y: bool) -> Vec<Pos2> {
    if shape == ShapeKind::Diamond {
        vec![
            rect.center_top(),
            rect.right_center(),
            rect.center_bottom(),
            rect.left_center(),
        ]
    } else if flip_y {
        vec![rect.center_bottom(), rect.right_top(), rect.left_top()]
    } else {
        vec![rect.center_top(), rect.right_bottom(), rect.left_bottom()]
    }
}

/// How far `pos` is from a shape's outline (its line, for lines and arrows).
fn outline_distance(
    shape: ShapeKind,
    rect: Rect,
    (flip_x, flip_y): (bool, bool),
    pos: Pos2,
) -> f32 {
    match shape {
        ShapeKind::Rect | ShapeKind::RoundRect => {
            if rect.contains(pos) {
                (pos.x - rect.left())
                    .min(rect.right() - pos.x)
                    .min(pos.y - rect.top())
                    .min(rect.bottom() - pos.y)
            } else {
                rect.distance_to_pos(pos)
            }
        }
        ShapeKind::Ellipse => {
            // Along the line from the centre: close enough for picking it up.
            let radius = (rect.size() / 2.0).max(vec2(0.5, 0.5));
            let offset = pos - rect.center();
            let scaled = vec2(offset.x / radius.x, offset.y / radius.y).length();
            if scaled <= f32::EPSILON {
                radius.min_elem()
            } else {
                (offset - offset / scaled).length()
            }
        }
        ShapeKind::Diamond | ShapeKind::Triangle => {
            let corners = polygon_corners(shape, rect, flip_y);
            (0..corners.len())
                .map(|i| distance_to_segment(pos, corners[i], corners[(i + 1) % corners.len()]))
                .fold(f32::INFINITY, f32::min)
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let (start, end) = connector_ends(rect, flip_x, flip_y);
            distance_to_segment(pos, start, end)
        }
    }
}

fn paint_shape(
    ui: &mut Ui,
    shape: ShapeKind,
    rect: Rect,
    (flip_x, flip_y): (bool, bool),
    stroke: Stroke,
    fill: Option<Color32>,
) {
    match shape {
        ShapeKind::Rect | ShapeKind::RoundRect => {
            let radius = if shape == ShapeKind::RoundRect {
                CornerRadius::same(8)
            } else {
                CornerRadius::same(2)
            };
            if let Some(fill) = fill {
                ui.painter().rect_filled(rect, radius, fill);
            }
            ui.painter()
                .rect_stroke(rect, radius, stroke, StrokeKind::Inside);
        }
        ShapeKind::Ellipse => {
            let radius = rect.size() / 2.0;
            if let Some(fill) = fill {
                ui.painter()
                    .add(egui::Shape::ellipse_filled(rect.center(), radius, fill));
            }
            ui.painter()
                .add(egui::Shape::ellipse_stroke(rect.center(), radius, stroke));
        }
        ShapeKind::Diamond | ShapeKind::Triangle => {
            let points = polygon_corners(shape, rect, flip_y);
            if let Some(fill) = fill {
                ui.painter().add(egui::Shape::convex_polygon(
                    points.clone(),
                    fill,
                    Stroke::NONE,
                ));
            }
            ui.painter().add(egui::Shape::closed_line(points, stroke));
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let (start, end) = connector_ends(rect, flip_x, flip_y);
            ui.painter().line_segment([start, end], stroke);
            if shape == ShapeKind::Arrow && start != end {
                let angle = (end.y - start.y).atan2(end.x - start.x);
                let head = 12.0;
                for spread in [2.6f32, -2.6] {
                    let a = angle + spread;
                    ui.painter()
                        .line_segment([end, end + vec2(a.cos() * head, a.sin() * head)], stroke);
                }
            }
        }
    }
}

/// Whether item `id` is having its corner dragged right now.
fn resizing(state: &CanvasState, id: u64) -> bool {
    state
        .drag
        .as_ref()
        .is_some_and(|drag| drag.moves(id) && matches!(drag.mode, DragMode::Resize(_)))
}

/// Paints an equation and keeps its box fitted to the formula. Dragging the corner
/// scales the drawing; when the drag ends, the new scale becomes the equation's size.
fn equation_item(
    ui: &mut Ui,
    page: &mut Page,
    state: &CanvasState,
    t: Tokens,
    index: usize,
    rect: Rect,
) -> bool {
    let item = &page.canvas.items[index];
    let id = item.id;
    let CanvasKind::Equation { source, size } = &item.kind else {
        return false;
    };
    let (source, mut size) = (source.clone(), *size);
    let editing = state
        .equation_edit
        .as_ref()
        .is_some_and(|edit| edit.id == id);
    let fit = |layout: &math::MathLayout| {
        (layout.size + Vec2::splat(2.0 * EQUATION_PAD)).max(EQUATION_MIN)
    };
    let mut layout = math::layout_source(ui.ctx(), &source, size);
    if resizing(state, id) {
        let scale = (rect.height() / fit(&layout).y).max(0.1);
        paint_equation(ui, &layout, rect, scale, t, editing);
        return false;
    }
    let mut changed = false;
    let mut target = fit(&layout);
    // Only a corner drag that just ended changes the size. Otherwise the box simply
    // follows the formula: glyph sizes vary a little with the display's scale, and that
    // must not rewrite the page.
    let resized = state.resized == Some(id);
    if resized && !layout.is_empty() && (rect.height() - target.y).abs() > 1.0 {
        size = (size * rect.height() / target.y).clamp(8.0, 160.0).round();
        layout = math::layout_source(ui.ctx(), &source, size);
        target = fit(&layout);
        if let CanvasKind::Equation { size: stored, .. } = &mut page.canvas.items[index].kind {
            *stored = size;
        }
        changed = true;
    }
    let item = &mut page.canvas.items[index];
    let (width, height) = (target.x.round(), target.y.round());
    if item.width != width || item.height != height {
        item.width = width;
        item.height = height;
        // Typing and resizing change the page; a refit for the display does not.
        changed |= editing || resized;
    }
    let rect = item.rect();
    let marks = place_marks(&item_marks(state, id), MatchPlace::Label);
    if !marks.is_empty() {
        let current = marks.iter().any(|(_, current)| *current);
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(3),
            theme::find_highlight(t, current),
        );
    }
    paint_equation(ui, &layout, rect, 1.0, t, editing);
    changed
}

fn paint_equation(
    ui: &Ui,
    layout: &math::MathLayout,
    rect: Rect,
    scale: f32,
    t: Tokens,
    editing: bool,
) {
    if editing {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(3),
            Stroke::new(1.0, theme::with_alpha(t.accent, 120)),
            StrokeKind::Inside,
        );
    }
    if layout.is_empty() {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            t!("ribbon-equation"),
            theme::serif_italic(15.0),
            t.faint,
        );
        return;
    }
    let color = if layout.error.is_some() {
        t.danger
    } else {
        t.ink
    };
    math::paint(
        ui.painter(),
        layout,
        rect.center() - layout.size * scale / 2.0,
        scale,
        color,
    );
}

/// The source box under the equation being edited, with a hint and any parse error. It
/// is sized in page units, like the text in notes, so it zooms with the page.
fn equation_editor(ui: &mut Ui, page: &mut Page, state: &mut CanvasState, t: Tokens) -> bool {
    let Some(edit) = state.equation_edit.as_mut() else {
        return false;
    };
    let id = edit.id;
    let Some(index) = page.canvas.items.iter().position(|item| item.id == id) else {
        return false;
    };
    if edit.resnap {
        edit.resnap = false;
        edit.before = page.canvas.clone();
    }
    let edit_id = equation_source_id(&page.id, id);
    let starting = edit.focus;
    if edit.focus {
        edit.focus = false;
        let end = match &page.canvas.items[index].kind {
            CanvasKind::Equation { source, .. } => source.chars().count(),
            _ => 0,
        };
        editor::set_focus(ui.ctx(), edit_id, end, None);
    }
    let rect = page.canvas.items[index].rect();
    let field = Rect::from_min_size(
        pos2(rect.left(), rect.bottom() + 8.0),
        vec2(rect.width().max(360.0), 30.0),
    );
    ui.painter().rect(
        field,
        CornerRadius::same(4),
        t.raised,
        Stroke::new(1.0, t.line_strong),
        StrokeKind::Inside,
    );
    let CanvasKind::Equation { source, .. } = &mut page.canvas.items[index].kind else {
        return false;
    };
    let font = theme::mono(13.0);
    let response = ui.put(
        field.shrink2(vec2(9.0, 6.0)),
        TextEdit::singleline(source)
            .id(edit_id)
            .frame(egui::Frame::NONE)
            .margin(Margin::ZERO)
            .font(font.clone())
            .text_color(t.ink)
            .char_limit(400)
            .hint_text(
                RichText::new("x^2 + 1/2   \\frac{a}{b}   \\sqrt{x}   \\alpha")
                    .font(font)
                    .color(t.faint),
            ),
    );
    let mut shown = rect.union(field);
    if let Err(error) = math::parse(source) {
        let line = ui.painter().text(
            pos2(field.left() + 2.0, field.bottom() + 4.0),
            Align2::LEFT_TOP,
            error.message,
            theme::ui(11.0),
            t.danger,
        );
        shown = shown.union(line);
    }
    // Keep the box in view as it opens and while it is typed in (an equation near the
    // bottom of the window has its box below the edge).
    if starting || response.changed() {
        if let Some(visible) = state.visible {
            let margin = 16.0 / state.zoom.max(0.01);
            state.pan -= scroll_into(visible.shrink(margin), shown);
        }
    }
    response.changed()
}

/// How far to move the view so `wanted` comes into `visible`: as much of it as fits,
/// keeping its top-left corner in view.
fn scroll_into(visible: Rect, wanted: Rect) -> Vec2 {
    let axis = |low: f32, high: f32, min: f32, max: f32| {
        if high > max {
            (high - max).min((low - min).max(0.0))
        } else if low < min {
            low - min
        } else {
            0.0
        }
    };
    vec2(
        axis(
            wanted.left(),
            wanted.right(),
            visible.left(),
            visible.right(),
        ),
        axis(
            wanted.top(),
            wanted.bottom(),
            visible.top(),
            visible.bottom(),
        ),
    )
}

fn shape_label(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    t: Tokens,
    index: usize,
    shape: ShapeKind,
    label: &str,
    rect: Rect,
) -> bool {
    if matches!(shape, ShapeKind::Line | ShapeKind::Arrow) {
        return false;
    }
    let id = page.canvas.items[index].id;
    let inner_width = (rect.width() - 16.0).max(20.0);
    if state.editing_label != Some(id) {
        // The editing session ended without the field noticing (Esc, a click elsewhere).
        if state.typing == Some(id) {
            state.typing = None;
        }
        if !label.is_empty() {
            let marks = place_marks(&item_marks(state, id), MatchPlace::Label);
            // Every line centred, as in the editor below.
            let mut job = marked_job(label, theme::ui(13.0), t.ink, inner_width, &marks, t);
            job.halign = Align::Center;
            let galley = ui.painter().layout_job(job);
            let pos = rect.center() - galley.rect.center().to_vec2();
            ui.painter().galley(pos, galley, t.ink);
        }
        return false;
    }
    let edit_id = Id::unique(("shape-label", page.id.as_str(), id));
    if state.focus_label {
        ui.memory_mut(|memory| memory.request_focus(edit_id));
        state.focus_label = false;
    }
    let mut changed = false;
    if let CanvasKind::Shape { label, .. } = &mut page.canvas.items[index].kind {
        let before = label.clone();
        // Enter starts a new line, as in an Office shape; Esc or a click elsewhere ends.
        let height = ui
            .painter()
            .layout(before.clone(), theme::ui(13.0), t.ink, inner_width)
            .size()
            .y
            .max(22.0);
        let response = ui.put(
            Rect::from_center_size(rect.center(), vec2(inner_width, height)),
            TextEdit::multiline(label)
                .id(edit_id)
                .desired_width(inner_width)
                .desired_rows(1)
                .frame(egui::Frame::NONE)
                .margin(Margin::ZERO)
                .align(Align2::CENTER_CENTER)
                .font(theme::ui(13.0))
                .text_color(t.ink)
                .hint_text(t!("ribbon-label").as_ref()),
        );
        if response.changed() {
            // One undo step per editing session, like typing in a note.
            if state.typing != Some(id) {
                let mut snapshot = page.canvas.clone();
                if let Some(CanvasKind::Shape { label, .. }) =
                    snapshot.item_mut(id).map(|item| &mut item.kind)
                {
                    *label = before;
                }
                state.push_undo(&snapshot);
                state.typing = Some(id);
            }
            changed = true;
        }
        if response.lost_focus() {
            state.editing_label = None;
            state.typing = None;
        }
    }
    changed
}

/// Find-on-page matches inside one item: where, which chars, and whether current.
fn item_marks(state: &CanvasState, id: u64) -> Vec<(MatchPlace, Range<usize>, bool)> {
    let Some(find) = &state.find else {
        return Vec::new();
    };
    find.matches
        .iter()
        .enumerate()
        .filter(|(_, found)| found.item == id)
        .map(|(index, found)| (found.place, found.range.clone(), index == find.current))
        .collect()
}

fn place_marks(
    marks: &[(MatchPlace, Range<usize>, bool)],
    place: MatchPlace,
) -> Vec<(Range<usize>, bool)> {
    marks
        .iter()
        .filter(|(at, _, _)| *at == place)
        .map(|(_, range, current)| (range.clone(), *current))
        .collect()
}

/// Text painted straight onto the page, with find-on-page matches highlighted.
/// `marks` are char ranges of `text`, in order.
fn marked_job(
    text: &str,
    font: egui::FontId,
    color: Color32,
    wrap_width: f32,
    marks: &[(Range<usize>, bool)],
    t: Tokens,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let plain = TextFormat::simple(font, color);
    let ranges = util::byte_ranges(text, marks.iter().map(|(range, _)| range));
    let mut at = 0;
    for (range, (_, current)) in ranges.into_iter().zip(marks) {
        let (start, end) = (range.start.max(at), range.end);
        if start >= end {
            continue;
        }
        if at < start {
            job.append(&text[at..start], 0.0, plain.clone());
        }
        job.append(
            &text[start..end],
            0.0,
            TextFormat {
                background: theme::find_highlight(t, *current),
                ..plain.clone()
            },
        );
        at = end;
    }
    if at < text.len() || job.sections.is_empty() {
        job.append(&text[at..], 0.0, plain);
    }
    job
}

/// `Painter::text` on one line, cut with "…" where it would run past `max_width`, with
/// find-on-page matches highlighted.
fn paint_marked_text(
    ui: &Ui,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    max_width: f32,
    font: egui::FontId,
    color: Color32,
    marks: &[(Range<usize>, bool)],
    t: Tokens,
) {
    let mut job = marked_job(text, font, color, f32::INFINITY, marks, t);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    let galley = ui.painter().layout_job(job);
    let rect = anchor.anchor_size(pos, galley.size());
    ui.painter().galley(rect.min, galley, color);
}

/// Where a frise's rows go, top to bottom: each milestone's title (a year, say), its date
/// label, a gap, the line with the dots, then its note. Page points.
#[derive(Debug, Clone, Copy)]
struct FriseRows {
    title_bottom: f32,
    date_bottom: f32,
    line: f32,
    note_top: f32,
    /// How tall the notes can be and still end inside the frise.
    note_room: f32,
}

/// The rows of a frise filling `rect`, for rows of these heights. They sit in its middle; in
/// a frise too short for them, the titles still start inside it and the notes give way.
fn frise_rows(rect: Rect, title_height: f32, date_height: f32, note_height: f32) -> FriseRows {
    let above = FRISE_DOT + FRISE_DOT_GAP + date_height + FRISE_ROW_GAP + title_height;
    let below = FRISE_DOT + FRISE_NOTE_GAP + note_height;
    let line = (rect.center().y + (above - below) / 2.0).max(rect.top() + FRISE_PAD + above);
    let date_bottom = line - FRISE_DOT - FRISE_DOT_GAP;
    let note_top = line + FRISE_DOT + FRISE_NOTE_GAP;
    FriseRows {
        title_bottom: date_bottom - date_height - FRISE_ROW_GAP,
        date_bottom,
        line,
        note_top,
        note_room: rect.bottom() - FRISE_PAD - note_top,
    }
}

/// A frise's colour as painted: lighter on dark paper, so its dates stay readable there.
pub(crate) fn frise_accent(color: Rgb, t: Tokens) -> Color32 {
    if t.dark {
        theme::pastel(color.to_color32(), Color32::WHITE, 0.4)
    } else {
        color.to_color32()
    }
}

fn paint_timeline(
    ui: &mut Ui,
    t: Tokens,
    color: Rgb,
    milestones: &[Milestone],
    rect: Rect,
    marks: &[(MatchPlace, Range<usize>, bool)],
) {
    let accent = frise_accent(color, t);
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(4),
        if t.dark { t.sunken } else { t.raised },
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    let count = milestones.len().max(1);
    let slot = ((rect.width() - 104.0) / count as f32).max(1.0);
    let font = |field: MilestoneField| match field {
        MilestoneField::Title => theme::semibold(12.0),
        MilestoneField::Date => theme::ui(11.5),
        MilestoneField::Note => theme::ui(11.0),
    };
    let layout = |i: usize, field: MilestoneField, wrap: TextWrapping| {
        let milestone = &milestones[i];
        let (text, color) = match field {
            MilestoneField::Title => (&milestone.title, t.ink),
            MilestoneField::Date => (&milestone.date, accent),
            MilestoneField::Note => (&milestone.note, t.muted),
        };
        let marks = place_marks(marks, MatchPlace::Milestone(i, field));
        let mut job = marked_job(text, font(field), color, f32::INFINITY, &marks, t);
        job.wrap = wrap;
        ui.painter().layout_job(job)
    };
    // Titles and dates take one line, cut to their milestone's room so neighbours never
    // run into each other; notes wrap in it.
    let label = TextWrapping::truncate_at_width((slot - 6.0).max(1.0));
    let one_line = |field| -> Vec<_> {
        (0..milestones.len())
            .map(|i| layout(i, field, label.clone()))
            .collect()
    };
    let titles = one_line(MilestoneField::Title);
    let dates = one_line(MilestoneField::Date);
    let notes = |max_rows: usize| -> Vec<_> {
        let wrap = TextWrapping {
            max_width: (slot - 8.0).max(1.0),
            max_rows,
            ..Default::default()
        };
        (0..milestones.len())
            .map(|i| layout(i, MilestoneField::Note, wrap.clone()))
            .collect()
    };
    let row_height = |field| ui.fonts_mut(|fonts| fonts.row_height(&font(field)));
    let tallest = |galleys: &[std::sync::Arc<egui::Galley>], field| {
        let galleys = galleys.iter().map(|galley| galley.size().y);
        galleys.fold(row_height(field), f32::max)
    };
    let mut note_galleys = notes(usize::MAX);
    let note_height = tallest(&note_galleys, MilestoneField::Note);
    let rows = frise_rows(
        rect,
        tallest(&titles, MilestoneField::Title),
        tallest(&dates, MilestoneField::Date),
        note_height,
    );
    if note_height > rows.note_room {
        // As many lines as fit (none in a frise below the smallest size), then "…".
        note_galleys = notes((rows.note_room / row_height(MilestoneField::Note)).floor() as usize);
    }
    ui.painter().line_segment(
        [
            pos2(rect.left() + 34.0, rows.line),
            pos2(rect.right() - 34.0, rows.line),
        ],
        Stroke::new(2.0, accent),
    );
    for (i, milestone) in milestones.iter().enumerate() {
        let x = rect.left() + 52.0 + slot * (i as f32 + 0.5);
        ui.painter()
            .circle_filled(pos2(x, rows.line), FRISE_DOT, accent);
        ui.painter()
            .circle_filled(pos2(x, rows.line), 3.0, t.raised);
        let date = &dates[i];
        let at = pos2(x - date.size().x / 2.0, rows.date_bottom - date.size().y);
        ui.painter().galley(at, date.clone(), accent);
        let title = &titles[i];
        let at = pos2(x - title.size().x / 2.0, rows.title_bottom - title.size().y);
        ui.painter().galley(at, title.clone(), t.ink);
        if !milestone.note.is_empty() {
            let note = &note_galleys[i];
            let at = pos2(x - note.size().x / 2.0, rows.note_top);
            ui.painter().galley(at, note.clone(), t.muted);
        }
    }
}

fn equation_source_id(page_id: &str, id: u64) -> Id {
    Id::unique(("equation-source", page_id, id))
}

/// View ▸ Page Width: zooms so the page's content (or its header, on a page with little on
/// it) fills the width of the page area, with the page's left margin on both sides,
/// keeping the top of the view where it is.
fn page_width(page: &Page, state: &mut CanvasState) {
    let Some(visible) = state.visible else {
        return;
    };
    let mut right = PAGE_LEFT + TITLE_RULE_MIN;
    for item in &page.canvas.items {
        right = right.max(item.rect().right());
    }
    for stroke in &page.canvas.ink {
        for point in &stroke.points {
            right = right.max(point[0]);
        }
    }
    // The page area, in screen points.
    let area = visible.size() * state.zoom;
    let zoom = (area.x / (right + PAGE_LEFT)).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
    state.scene_rect = Some(Rect::from_min_size(pos2(0.0, visible.top()), area / zoom));
}

/// The near-black inks are drawn in the theme's ink colour on dark paper, so writing
/// stays readable in dark mode (the desktop notebook does the same).
fn ink_on_paper(color: Rgb, t: Tokens) -> Color32 {
    if t.dark && color == Rgb::INKS[0] {
        t.ink
    } else if t.dark && color == Rgb::INKS[1] {
        Color32::from_rgb(0xb4, 0xb2, 0xc2)
    } else {
        color.to_color32()
    }
}

fn paint_ink(ui: &mut Ui, page: &Page, state: &CanvasState, view: Rect, t: Tokens) {
    // One pass over the selected indices, so a page full of selected ink (Ctrl+A) stays
    // cheap to paint.
    let mut selected = vec![false; page.canvas.ink.len()];
    for index in &state.selection.strokes {
        if let Some(flag) = selected.get_mut(*index) {
            *flag = true;
        }
    }
    for (stroke, selected) in page.canvas.ink.iter().zip(selected) {
        paint_stroke(ui, stroke, view, t, selected);
    }
    if let Some(stroke) = &state.stroke {
        paint_stroke(ui, stroke, view, t, false);
    }
}

fn stroke_width(stroke: &InkStroke) -> f32 {
    if stroke.highlighter {
        stroke.width * 4.0
    } else {
        stroke.width
    }
}

/// A stroke's extent, including its width.
fn stroke_bounds(stroke: &InkStroke) -> Rect {
    let mut bounds = Rect::NOTHING;
    for point in &stroke.points {
        bounds.extend_with(pos2(point[0], point[1]));
    }
    bounds.expand(stroke_width(stroke))
}

/// Draws one stroke; a `selected` stroke gets an accent halo under it.
fn paint_stroke(ui: &mut Ui, stroke: &InkStroke, view: Rect, t: Tokens, selected: bool) {
    if !stroke_bounds(stroke).intersects(view) {
        return;
    }
    let color = if stroke.highlighter {
        theme::with_alpha(stroke.color.to_color32(), 75)
    } else {
        ink_on_paper(stroke.color, t)
    };
    let width = stroke_width(stroke);
    let points: Vec<Pos2> = stroke
        .points
        .iter()
        .map(|point| pos2(point[0], point[1]))
        .collect();
    if selected {
        let halo = theme::with_alpha(t.accent, 70);
        if points.len() == 1 {
            ui.painter()
                .circle_filled(points[0], width / 2.0 + 3.0, halo);
        } else {
            ui.painter().add(egui::Shape::line(
                points.clone(),
                Stroke::new(width + 6.0, halo),
            ));
        }
    }
    if points.len() == 1 {
        ui.painter().circle_filled(points[0], width / 2.0, color);
    } else {
        ui.painter()
            .add(egui::Shape::line(points, Stroke::new(width, color)));
    }
}

fn ink_tool(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    canvas_rect: Rect,
    pointer: Option<Pos2>,
    vp: Viewport,
    t: Tokens,
) -> bool {
    let mut changed = false;
    let overlay = ui.interact(
        canvas_rect,
        Id::unique(("canvas-ink", page.id.as_str())),
        Sense::click_and_drag(),
    );
    // Middle- and right-drag pan rather than draw.
    if pan_by_drag(ui, &overlay, state) {
        return false;
    }
    if overlay.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
    }
    let Some(pointer) = pointer else {
        return false;
    };
    let eraser = state.tool == CanvasTool::Eraser;
    if eraser && matches!(state.eraser, EraserMode::Point(_)) && overlay.hovered() {
        // Show the eraser's footprint.
        ui.painter().circle_stroke(
            pointer,
            vp.px(state.eraser.reach()),
            Stroke::new(vp.px(1.0), t.muted),
        );
    }
    if overlay.drag_started_by(PointerButton::Primary) {
        let origin = press_origin_scene(ui).unwrap_or(pointer);
        if eraser {
            state.erasing = false;
            changed |= erase_gesture(page, state, origin, pointer, vp);
            state.erase_from = Some(pointer);
        } else {
            // Start where the button went down, not where the drag threshold was crossed.
            let mut points = vec![[origin.x, origin.y]];
            if origin.distance(pointer) > vp.px(1.5) {
                points.push([pointer.x, pointer.y]);
            }
            state.stroke = Some(InkStroke {
                points,
                color: state.ink_color,
                width: state.ink_width,
                highlighter: state.tool == CanvasTool::Highlighter,
            });
        }
    }
    if overlay.dragged_by(PointerButton::Primary) {
        if eraser {
            let from = state.erase_from.unwrap_or(pointer);
            changed |= erase_gesture(page, state, from, pointer, vp);
            state.erase_from = Some(pointer);
        } else if let Some(stroke) = &mut state.stroke {
            let last = stroke
                .points
                .last()
                .copied()
                .unwrap_or([pointer.x, pointer.y]);
            if pos2(last[0], last[1]).distance(pointer) > vp.px(1.5) {
                stroke.points.push([pointer.x, pointer.y]);
            }
        }
    }
    if overlay.drag_stopped() {
        state.erasing = false;
        state.erase_from = None;
        if let Some(stroke) = state.stroke.take() {
            state.push_undo(&page.canvas);
            page.canvas.ink.push(stroke);
            if state.ink_to_shape {
                shape_from_last_stroke(page, state);
            }
            changed = true;
        }
    }
    if overlay.clicked() {
        state.selection.clear();
        state.active_text = None;
        editor_state.pending_style = None;
        if eraser {
            state.erasing = false;
            changed |= erase_gesture(page, state, pointer, pointer, vp);
            state.erasing = false;
        } else {
            state.push_undo(&page.canvas);
            page.canvas.ink.push(InkStroke {
                points: vec![[pointer.x, pointer.y]],
                color: state.ink_color,
                width: state.ink_width,
                highlighter: state.tool == CanvasTool::Highlighter,
            });
            changed = true;
        }
    }
    changed
}

fn distance_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let length = ab.length_sq();
    if length <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / length).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

/// Whether the eraser, swept from `from` to `to` with a `reach`-wide tip, touches the
/// stroke (whose own half width counts too). With `from == to` it is a single dab.
fn swipe_hits(stroke: &InkStroke, from: Pos2, to: Pos2, reach: f32) -> bool {
    let reach = reach + stroke_width(stroke) / 2.0;
    let at = |point: &[f32; 2]| pos2(point[0], point[1]);
    match stroke.points.as_slice() {
        [] => false,
        [only] => distance_to_segment(at(only), from, to) <= reach,
        points => points
            .windows(2)
            .any(|pair| segment_distance(at(&pair[0]), at(&pair[1]), from, to) <= reach),
    }
}

/// The shortest distance between segments `a → b` and `c → d`.
fn segment_distance(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> f32 {
    let side = |o: Pos2, p: Pos2, q: Pos2| (p - o).x * (q - o).y - (p - o).y * (q - o).x;
    let opposite = |x: f32, y: f32| (x > 0.0 && y < 0.0) || (x < 0.0 && y > 0.0);
    if opposite(side(c, d, a), side(c, d, b)) && opposite(side(a, b, c), side(a, b, d)) {
        return 0.0;
    }
    distance_to_segment(a, c, d)
        .min(distance_to_segment(b, c, d))
        .min(distance_to_segment(c, a, b))
        .min(distance_to_segment(d, a, b))
}

/// Stroke eraser: removes every stroke the swipe touches. Selected strokes are indices
/// into the ink list, so the selection follows the strokes that stay, and loses the
/// erased ones.
fn erase_strokes(ink: &mut Vec<InkStroke>, selection: &mut Selection, erased: &[usize]) {
    let mut kept = 0;
    let mut gone = erased.iter().peekable();
    // Each stroke's index once the erased ones are gone, or `None` when it is erased.
    let moved: Vec<Option<usize>> = (0..ink.len())
        .map(|index| {
            if gone.next_if_eq(&&index).is_some() {
                None
            } else {
                kept += 1;
                Some(kept - 1)
            }
        })
        .collect();
    let mut index = 0;
    ink.retain(|_| {
        index += 1;
        moved[index - 1].is_some()
    });
    selection.strokes = selection
        .strokes
        .iter()
        .filter_map(|old| moved.get(*old).copied().flatten())
        .collect();
}

/// What the point eraser leaves of each stroke a swipe from `from` to `to` rubs
/// through: (index, pieces), in ink order. Strokes away from the swipe are skipped
/// without copying anything.
fn rub_out_ink(
    ink: &[InkStroke],
    from: Pos2,
    to: Pos2,
    radius: f32,
) -> Vec<(usize, Vec<Vec<[f32; 2]>>)> {
    let swept = Rect::from_two_pos(from, to).expand(radius);
    ink.iter()
        .enumerate()
        // A stroke's bounds already include its whole width.
        .filter(|(_, stroke)| stroke_bounds(stroke).intersects(swept))
        .filter_map(|(index, stroke)| {
            let reach = radius + stroke_width(stroke) / 2.0;
            rub_out(
                &stroke.points,
                eraser_samples(from, to, radius * 0.5),
                reach,
            )
            .map(|pieces| (index, pieces))
        })
        .collect()
}

/// Replaces each rubbed-out stroke with what is left of it, in the same style.
fn apply_rub_out(ink: &mut Vec<InkStroke>, cuts: Vec<(usize, Vec<Vec<[f32; 2]>>)>) {
    // From the back, so the indices still to come stay put.
    for (index, pieces) in cuts.into_iter().rev() {
        let InkStroke {
            color,
            width,
            highlighter,
            ..
        } = ink[index];
        ink.splice(
            index..=index,
            pieces.into_iter().map(|points| InkStroke {
                points,
                color,
                width,
                highlighter,
            }),
        );
    }
}

/// What is left of a stroke after the eraser's dabs at `centers`, or `None` when they
/// all miss it.
fn rub_out(
    points: &[[f32; 2]],
    centers: impl Iterator<Item = Pos2>,
    reach: f32,
) -> Option<Vec<Vec<[f32; 2]>>> {
    let mut pieces: Option<Vec<Vec<[f32; 2]>>> = None;
    for center in centers {
        let Some(pieces) = &mut pieces else {
            pieces = cut_stroke(points, center, reach);
            continue;
        };
        let mut i = 0;
        while i < pieces.len() {
            match cut_stroke(&pieces[i], center, reach) {
                None => i += 1,
                Some(cut) => {
                    let count = cut.len();
                    pieces.splice(i..=i, cut);
                    i += count;
                }
            }
        }
    }
    pieces
}

/// What is left of a stroke after rubbing out a disc, or `None` when the disc misses it.
fn cut_stroke(points: &[[f32; 2]], center: Pos2, reach: f32) -> Option<Vec<Vec<[f32; 2]>>> {
    let at = |point: [f32; 2]| pos2(point[0], point[1]);
    let first = *points.first()?;
    if points.len() == 1 {
        return (at(first).distance(center) <= reach).then(Vec::new);
    }
    // Most strokes are missed: find that out before copying anything.
    if !points
        .windows(2)
        .any(|pair| disc_span(at(pair[0]), at(pair[1]), center, reach).is_some())
    {
        return None;
    }
    let mut pieces = Vec::new();
    let mut piece = Vec::new();
    if at(first).distance(center) > reach {
        piece.push(first);
    }
    for pair in points.windows(2) {
        let (a, b) = (at(pair[0]), at(pair[1]));
        let Some((enter, leave)) = disc_span(a, b, center, reach) else {
            piece.push(pair[1]);
            continue;
        };
        if enter > 0.0 {
            let cut = a.lerp(b, enter);
            piece.push([cut.x, cut.y]);
        }
        if piece.len() > 1 {
            pieces.push(std::mem::take(&mut piece));
        } else {
            piece.clear();
        }
        if leave < 1.0 {
            let cut = a.lerp(b, leave);
            piece.push([cut.x, cut.y]);
            piece.push(pair[1]);
        }
    }
    if piece.len() > 1 {
        pieces.push(piece);
    }
    Some(pieces)
}

/// How far inside the eraser's edge a segment must reach to be cut. A stroke the eraser
/// has already cut ends exactly on its edge, and rounding must not cut it again (which
/// would add an undo step that changes nothing).
const ERASER_GRAZE: f32 = 0.02;

/// Where segment `a → b` runs inside the disc, as fractions along it.
fn disc_span(a: Pos2, b: Pos2, center: Pos2, reach: f32) -> Option<(f32, f32)> {
    if distance_to_segment(center, a, b) >= reach - ERASER_GRAZE {
        return None;
    }
    let d = b - a;
    let f = a - center;
    let qa = d.length_sq();
    if qa <= f32::EPSILON {
        return Some((0.0, 1.0));
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.length_sq() - reach * reach;
    let discriminant = qb * qb - 4.0 * qa * qc;
    if discriminant <= 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let (enter, leave) = ((-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa));
    if leave <= 0.0 || enter >= 1.0 {
        return None;
    }
    Some((enter.max(0.0), leave.min(1.0)))
}

/// The eraser's dabs along `from → to`, `step` apart, both ends included.
fn eraser_samples(from: Pos2, to: Pos2, step: f32) -> impl Iterator<Item = Pos2> {
    let count = if from == to {
        0
    } else {
        ((from.distance(to) / step.max(0.5)).ceil() as usize).clamp(1, 400)
    };
    (0..=count).map(move |i| {
        if count == 0 {
            from
        } else {
            from.lerp(to, i as f32 / count as f32)
        }
    })
}

fn scene_pointer(ui: &Ui) -> Option<Pos2> {
    let pointer = ui.ctx().input(|input| input.pointer.latest_pos())?;
    let transform = ui.ctx().layer_transform_from_global(ui.layer_id())?;
    Some(transform * pointer)
}

/// Erases along `from → to` with the current eraser. The first erase that changes
/// something records the undo step, so one drag undoes in one go. Only strokes near the
/// swipe are looked at, and only the ones it really touches are copied.
fn erase_gesture(
    page: &mut Page,
    state: &mut CanvasState,
    from: Pos2,
    to: Pos2,
    vp: Viewport,
) -> bool {
    let reach = vp.px(state.eraser.reach());
    match state.eraser {
        EraserMode::Stroke => {
            let swept = Rect::from_two_pos(from, to).expand(reach);
            let erased: Vec<usize> = page
                .canvas
                .ink
                .iter()
                .enumerate()
                .filter(|(_, stroke)| {
                    stroke_bounds(stroke).intersects(swept) && swipe_hits(stroke, from, to, reach)
                })
                .map(|(index, _)| index)
                .collect();
            if erased.is_empty() {
                return false;
            }
            begin_erase_undo(page, state);
            // The selection follows the strokes that stay (M6).
            erase_strokes(&mut page.canvas.ink, &mut state.selection, &erased);
        }
        EraserMode::Point(_) => {
            let cuts = rub_out_ink(&page.canvas.ink, from, to, reach);
            if cuts.is_empty() {
                return false;
            }
            begin_erase_undo(page, state);
            apply_rub_out(&mut page.canvas.ink, cuts);
            // Cutting and removing ink shifts the stroke indices.
            state.selection.strokes.clear();
        }
    }
    true
}

fn begin_erase_undo(page: &Page, state: &mut CanvasState) {
    if !state.erasing {
        state.push_undo(&page.canvas);
        state.erasing = true;
    }
}

/// Ink to Shape: swaps the stroke just drawn for the shape it looks like. The stroke is
/// committed first, so the first undo brings the hand-drawn ink back (as in the desktop notebook).
fn shape_from_last_stroke(page: &mut Page, state: &mut CanvasState) -> bool {
    let Some(stroke) = page.canvas.ink.last() else {
        return false;
    };
    if stroke.highlighter {
        return false;
    }
    let Some(found) = crate::ink_shapes::recognize_shape(&stroke.points) else {
        return false;
    };
    let (color, thickness) = (stroke.color, stroke.width);
    let connector = found.kind == ShapeKind::Line;
    let rect = if connector {
        found.rect
    } else {
        Rect::from_min_size(found.rect.min, found.rect.size().max(vec2(24.0, 24.0)))
    };
    state.push_undo(&page.canvas);
    page.canvas.ink.pop();
    page.canvas.add(CanvasItem::new(
        CanvasKind::Shape {
            shape: found.kind,
            stroke: color,
            fill: None,
            label: String::new(),
            flip_x: found.flip_x,
            flip_y: found.flip_y,
            // The pen's width carries over, as in the desktop notebook.
            thickness: Some(thickness),
        },
        rect.left().round(),
        rect.top().round(),
        rect.width().round(),
        rect.height().round(),
    ));
    state.selection.strokes.clear(); // C6
    true
}

/// Starts a note where the page was clicked: its first line's caret lands on the click.
fn add_text(page: &mut Page, state: &mut CanvasState, editor_state: &mut EditorState, click: Pos2) {
    // On a ruled page the text goes on the rule under the click, as in the desktop notebook.
    let top = match ruled_pitch(page) {
        Some(pitch) => rule_at_or_below(click.y, pitch) - (FIRST_RULE - PAGE_CONTENT_TOP),
        None => click.y - NOTE_PAD_TOP - editor::BODY_LINE / 2.0,
    };
    add_note(page, state, editor_state, pos2(click.x - NOTE_PAD_X, top));
}

/// Starts an empty note with its frame's top-left at `origin`, small: it widens as you
/// type.
fn add_note(
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    origin: Pos2,
) {
    state.push_undo(&page.canvas);
    // Room on its left for the paragraph handles.
    let pos = pos2(
        origin.x.max(editor::HANDLE_GUTTER).round(),
        origin.y.max(0.0).round(),
    );
    let mut item = CanvasItem::new(
        CanvasKind::Text {
            doc: Document::default(),
        },
        pos.x,
        pos.y,
        NOTE_MIN_WIDTH,
        40.0,
    );
    item.auto_width = true;
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = Some(id);
    state.tool = CanvasTool::Select;
    editor_state.reset();
    if let Some(block) = page
        .canvas
        .item(id)
        .and_then(|item| item.doc())
        .and_then(|doc| doc.blocks.first())
    {
        editor_state.focus_block(block.id, 0);
    }
}

fn add_shape(page: &mut Page, state: &mut CanvasState, shape: ShapeKind, start: Pos2, end: Pos2) {
    let connector = matches!(shape, ShapeKind::Line | ShapeKind::Arrow);
    let mut rect = Rect::from_two_pos(start, end);
    let (mut flip_x, mut flip_y) = (end.x < start.x, end.y < start.y);
    if rect.width().max(rect.height()) < 6.0 {
        // A click rather than a drag: drop a default-sized shape.
        rect = if connector {
            Rect::from_min_size(start, vec2(160.0, 0.0))
        } else {
            Rect::from_min_size(start, vec2(160.0, 100.0))
        };
        flip_x = false;
        flip_y = false;
    } else if !connector {
        rect = Rect::from_min_size(rect.min, rect.size().max(vec2(24.0, 24.0)));
    }
    if !connector {
        // Only lines and arrows have a direction; drawing a box upwards gives the same box.
        // (A flipped triangle points down, which only Ink to Shape produces.)
        flip_x = false;
        flip_y = false;
    }
    state.push_undo(&page.canvas);
    let item = CanvasItem::new(
        CanvasKind::Shape {
            shape,
            stroke: state.ink_color,
            fill: if connector { None } else { state.shape_fill },
            label: String::new(),
            flip_x,
            flip_y,
            thickness: None,
        },
        rect.left().round(),
        rect.top().round(),
        rect.width().round(),
        rect.height().round(),
    );
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = None;
    // Lock Drawing Mode keeps the shape tool on for the next one, until Esc.
    state.tool = if state.lock_drawing {
        CanvasTool::Shape(shape)
    } else {
        CanvasTool::Select
    };
}

/// Insert › Table: an empty table at the caret of the note being typed in, else in a new
/// note at the insertion point, with the caret in its first cell.
fn insert_table_here(
    ui: &Ui,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    rows: usize,
    cols: usize,
) -> bool {
    let now = ui.input(|input| input.time);
    let typing = active_note(page, state).is_some();
    // No note is being typed in: a new one at the insertion point (one page undo step),
    // whose empty first line the table takes. In a note being typed in, the note's own
    // undo takes the table back, and the page's step comes when its edit ends.
    let Some(id) = note_to_insert_into(page, state, editor_state) else {
        return false;
    };
    let doc_id = format!("{}:{}", page.id, id);
    let Some(doc) = page.canvas.item_mut(id).and_then(CanvasItem::doc_mut) else {
        // A new note changed the page even if no table could go into it.
        return !typing;
    };
    editor::insert_table(doc, &doc_id, editor_state, rows.max(1), cols.max(1), now);
    true
}

/// Insert › Table's dropdown, the desktop notebook's grid: 10 columns × 8 rows of cells. Hovering a
/// cell lights every cell from the top-left to it and names that size above the grid; a
/// click inserts a table that size.
pub(crate) fn table_grid(ui: &mut Ui, t: Tokens) -> Option<Cmd> {
    const COLS: u8 = 10;
    const ROWS: u8 = 8;
    const CELL: f32 = 18.0;
    const GAP: f32 = 2.0;
    let width = f32::from(COLS) * CELL + f32::from(COLS - 1) * GAP;
    let (caption_rect, caption) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let mut cells = Vec::with_capacity(usize::from(COLS) * usize::from(ROWS));
    ui.scope(|ui| {
        let spacing = ui.spacing_mut();
        spacing.item_spacing = vec2(GAP, GAP);
        // Rows exactly a cell high.
        spacing.interact_size.y = CELL;
        for row in 1..=ROWS {
            ui.horizontal(|ui| {
                for col in 1..=COLS {
                    let (rect, response) = ui.allocate_exact_size(vec2(CELL, CELL), Sense::click());
                    let label = format!("{col} × {row}");
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                    });
                    cells.push((col, row, rect, response));
                }
            });
        }
    });
    // The cell under the pointer, the gaps between cells counting as the cell before them,
    // so the size does not flicker while the pointer crosses them.
    let pointer = ui.ctx().pointer_hover_pos();
    let hovered = pointer.and_then(|pos| {
        cells.iter().find_map(|(col, row, rect, _)| {
            let area = Rect::from_min_size(rect.min, rect.size() + Vec2::splat(GAP));
            area.contains(pos).then_some((*col, *row))
        })
    });
    let mut picked = None;
    for (col, row, rect, response) in &cells {
        let lit = hovered.is_some_and(|(cols, rows)| col <= &cols && row <= &rows);
        // The cells the table will take are framed in Office's orange, as the desktop notebook's.
        let (fill, stroke) = if lit {
            (t.raised, t.warning)
        } else {
            (t.raised, t.line_strong)
        };
        ui.painter().rect(
            *rect,
            CornerRadius::ZERO,
            fill,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
        if response.clicked() {
            picked = Some(Cmd::InsertTable {
                rows: *row,
                cols: *col,
            });
        }
    }
    let (cols, rows) = hovered.unwrap_or((1, 1));
    let text = t!("ribbon-table-size", cols = cols, rows = rows);
    ui.painter().text(
        caption_rect.left_center(),
        Align2::LEFT_CENTER,
        text.as_ref(),
        theme::semibold(13.0),
        t.ink,
    );
    caption.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text.as_ref()));
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// Starts typing equation `id`'s source. Another equation still being typed is finished
/// first (returns whether that changed the page); this one just gets the caret back.
fn begin_equation_edit(page: &mut Page, state: &mut CanvasState, id: u64) -> bool {
    if let Some(edit) = state.equation_edit.as_mut().filter(|edit| edit.id == id) {
        edit.focus = true;
        return false;
    }
    let changed = finish_equation_edit(page, state);
    if page.canvas.item(id).is_some() {
        state.equation_edit = Some(EquationEdit {
            id,
            before: page.canvas.clone(),
            focus: true,
            resnap: false,
        });
    }
    changed
}

fn add_frise(page: &mut Page, state: &mut CanvasState) {
    let center = state.view_center();
    state.push_undo(&page.canvas);
    let milestones = vec![
        Milestone {
            title: t!("canvas-frise-before").into_owned(),
            date: t!("canvas-frise-date").into_owned(),
            note: t!("canvas-frise-before-note").into_owned(),
        },
        Milestone {
            title: t!("canvas-frise-event").into_owned(),
            date: t!("canvas-frise-date").into_owned(),
            note: t!("canvas-frise-event-note").into_owned(),
        },
        Milestone {
            title: t!("canvas-frise-after").into_owned(),
            date: t!("canvas-frise-date").into_owned(),
            note: t!("canvas-frise-after-note").into_owned(),
        },
    ];
    let item = CanvasItem::new(
        CanvasKind::Timeline {
            color: Rgb(0x5b, 0x4b, 0xdb),
            milestones: milestones.clone(),
        },
        (center.x - FRISE_WIDTH / 2.0).round(),
        (center.y - FRISE_HEIGHT / 2.0).round(),
        FRISE_WIDTH,
        FRISE_HEIGHT,
    );
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = None;
    state.tool = CanvasTool::Select;
    state.timeline_editor = Some(TimelineEditor {
        item_id: id,
        milestones,
    });
}

/// The size a picture is placed at: its own proportions, no bigger than `PICTURE_MAX`.
fn picture_size(natural: Option<[f32; 2]>) -> Vec2 {
    let [width, height] = natural.unwrap_or([440.0, 300.0]);
    let scale = (PICTURE_MAX.x / width).min(PICTURE_MAX.y / height).min(1.0);
    vec2(
        (width * scale).max(40.0).round(),
        (height * scale).max(40.0).round(),
    )
}

fn place_picture(
    page: &mut Page,
    state: &mut CanvasState,
    attachment: String,
    size: Vec2,
    at: Pos2,
) {
    state.push_undo(&page.canvas);
    let item = CanvasItem::new(
        CanvasKind::image(attachment),
        (at.x - size.x / 2.0).round(),
        (at.y - size.y / 2.0).round(),
        size.x,
        size.y,
    );
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = None;
    state.tool = CanvasTool::Select;
}

fn insert_picture_file(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    path: &Path,
    at: Pos2,
    toast: &mut Option<(String, bool)>,
) -> bool {
    match library.import_image(&page.id, path) {
        Ok(attachment) => {
            let size = picture_size(images::upright_size(path));
            place_picture(page, state, attachment.id, size, at);
            true
        }
        Err(error) => {
            *toast = Some((
                t!("canvas-error-insert-picture", error = error.to_string()).into_owned(),
                true,
            ));
            false
        }
    }
}

fn insert_picture_bytes(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    bytes: &[u8],
    name: &str,
    at: Pos2,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let Some(decoded) = images::decode(bytes, u32::MAX) else {
        *toast = Some((t!("canvas-error-not-picture").into_owned(), true));
        return false;
    };
    match library.save_image_bytes(&page.id, "png", "image/png", bytes, name) {
        Ok(attachment) => {
            let size = picture_size(Some([decoded.width() as f32, decoded.height() as f32]));
            place_picture(page, state, attachment.id, size, at);
            true
        }
        Err(error) => {
            *toast = Some((
                t!("canvas-error-insert-picture", error = error.to_string()).into_owned(),
                true,
            ));
            false
        }
    }
}

fn place_file(
    page: &mut Page,
    state: &mut CanvasState,
    attachment: crate::model::Attachment,
    at: Pos2,
) {
    state.push_undo(&page.canvas);
    let item = CanvasItem::new(
        CanvasKind::File {
            attachment: attachment.id,
            name: attachment.file_name,
            size: attachment.byte_size,
        },
        (at.x - FILE_SIZE.x / 2.0).round(),
        (at.y - FILE_SIZE.y / 2.0).round(),
        FILE_SIZE.x,
        FILE_SIZE.y,
    );
    let id = page.canvas.add(item);
    state.selection.set_one(id);
    state.active_text = None;
    state.tool = CanvasTool::Select;
}

/// Attaches a file from disk as a file card (the desktop notebook's File Attachment).
pub(crate) fn insert_file(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    path: &Path,
    at: Pos2,
    toast: &mut Option<(String, bool)>,
) -> bool {
    match library.import_file(&page.id, path) {
        Ok(attachment) => {
            place_file(page, state, attachment, at);
            true
        }
        Err(error) => {
            *toast = Some((
                t!("canvas-error-attach-file", error = error.to_string()).into_owned(),
                true,
            ));
            false
        }
    }
}

fn insert_file_bytes(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    bytes: &[u8],
    name: &str,
    at: Pos2,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mime = crate::db::mime_for_extension(extension);
    match library.save_attachment_bytes(&page.id, extension, mime, bytes, name) {
        Ok(attachment) => {
            place_file(page, state, attachment, at);
            true
        }
        Err(error) => {
            *toast = Some((
                t!(
                    "canvas-error-attach-named",
                    name = name.to_string(),
                    error = error.to_string()
                )
                .into_owned(),
                true,
            ));
            false
        }
    }
}

/// A file from the file manager: pictures become pictures, anything else a file card.
fn insert_path(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    path: &Path,
    at: Pos2,
    toast: &mut Option<(String, bool)>,
) -> bool {
    if is_picture_path(path) {
        insert_picture_file(page, state, library, path, at, toast)
    } else {
        insert_file(page, state, library, path, at, toast)
    }
}

fn png_bytes(image: &egui::ColorImage) -> Option<Vec<u8>> {
    let [width, height] = image.size;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    let buffer = image::RgbaImage::from_raw(width as u32, height as u32, rgba)?;
    let mut bytes = Vec::new();
    buffer
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .ok()?;
    Some(bytes)
}

fn is_picture_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        )
    })
}

/// A file named `….pdf`, in any case: it can become a printout.
pub(crate) fn is_pdf_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// File managers copy files as `file://` URIs, one per line (Nautilus puts a "copy"
/// line first). Returns the files when that is all the pasted text holds.
fn pasted_paths(text: &str) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if matches!(line, "copy" | "cut" | "x-special/nautilus-clipboard") {
            continue;
        }
        let raw = percent_decode(line.strip_prefix("file://")?);
        // file:///C:/Users/… on Windows.
        let raw = if cfg!(windows) {
            raw.trim_start_matches('/').to_string()
        } else {
            raw
        };
        let path = PathBuf::from(raw);
        if !path.is_absolute() || !path.is_file() {
            return None;
        }
        paths.push(path);
    }
    (!paths.is_empty()).then_some(paths)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], escaped) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Files dragged in from the file manager land where they are dropped: pictures as pictures, anything else as a file card.
/// A PDF first asks whether it becomes a printout or a file card (`poll_printout`).
fn dropped_files(
    ui: &mut Ui,
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
    pointer: Option<Pos2>,
    view: Rect,
    t: Tokens,
) -> bool {
    let (hovering, dropped) = ui.input(|input| {
        (
            !input.raw.hovered_files.is_empty(),
            input.raw.dropped_files.clone(),
        )
    });
    if hovering {
        ui.painter().rect_filled(
            view.shrink(view.width() * 0.01),
            CornerRadius::same(6),
            theme::with_alpha(t.accent, 26),
        );
        ui.painter().text(
            view.center(),
            Align2::CENTER_CENTER,
            t!("canvas-drop-files"),
            theme::ui(15.0 / state.zoom),
            t.accent,
        );
    }
    let mut changed = false;
    let mut at = pointer
        .filter(|pos| view.contains(*pos))
        .unwrap_or_else(|| view.center());
    for file in dropped {
        let path = file.path();
        let name = path.file_name().map_or_else(
            || t!("canvas-file-fallback").into_owned(),
            |name| name.to_string_lossy().to_string(),
        );
        if is_pdf_name(&name) {
            let bytes = if path.is_absolute() {
                None
            } else {
                file.bytes().ok().map(Arc::new)
            };
            state.pending_pdfs.push_back(PendingPdf {
                name,
                path: path.is_absolute().then(|| path.to_path_buf()),
                bytes,
                at,
            });
            at += vec2(24.0, 24.0);
            continue;
        }
        if path.is_absolute() {
            changed |= insert_path(page, state, library, path, at, toast);
        } else {
            match file.bytes() {
                Ok(bytes) if is_picture_path(path) => {
                    changed |= insert_picture_bytes(page, state, library, &bytes, &name, at, toast);
                }
                Ok(bytes) => {
                    changed |= insert_file_bytes(page, state, library, &bytes, &name, at, toast);
                }
                Err(error) => {
                    *toast = Some((
                        t!(
                            "canvas-error-read-named",
                            name = name.clone(),
                            error = error.to_string()
                        )
                        .into_owned(),
                        true,
                    ))
                }
            }
        }
        at += vec2(24.0, 24.0);
    }
    changed
}

/// Files the tests pick in place of the system file dialog, first in, first out.
#[cfg(test)]
pub(crate) static PICKS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Asks for a file (pictures only when `images`). Tests never open the system dialog: they
/// take the file a test queued, or nothing, as when the dialog is cancelled.
#[cfg(test)]
fn pick_file(_images: bool) -> Option<PathBuf> {
    let mut picks = PICKS.lock().unwrap_or_else(|poison| poison.into_inner());
    (!picks.is_empty()).then(|| picks.remove(0))
}

/// Asks for a file (pictures only when `images`).
#[cfg(not(test))]
fn pick_file(images: bool) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if images {
        dialog = dialog.add_filter(
            t!("app-file-filter-images"),
            &["png", "jpg", "jpeg", "webp"],
        );
    }
    dialog.pick_file()
}

fn add_image_dialog(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let Some(path) = pick_file(true) else {
        return false;
    };
    let at = state.view_center();
    insert_picture_file(page, state, library, &path, at, toast)
}

/// The PDF to print out, from the system's file picker.
#[cfg(not(test))]
fn pick_pdf() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter(t!("printout-filter"), &["pdf"])
        .pick_file()
}

/// Tests never open a file picker: they say which file it would have returned.
#[cfg(test)]
fn pick_pdf() -> Option<PathBuf> {
    PICKED_PDF.lock().unwrap().take()
}

#[cfg(test)]
pub(crate) static PICKED_PDF: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Insert › File Printout: asks for a PDF and inserts it as a printout where the next
/// insert goes.
pub(crate) fn insert_printout_dialog(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
) {
    if state.printout_busy() {
        return;
    }
    let Some(path) = pick_pdf() else {
        return;
    };
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| t!("canvas-file-fallback").into_owned());
    match read_pdf(&path, &name) {
        Ok(bytes) => {
            let at = printout_origin(page, state);
            start_printout(ctx, page, state, library, &name, bytes, at, toast);
        }
        Err(error) => {
            *toast = Some((
                t!("canvas-error-read-named", name = name, error = error).into_owned(),
                true,
            ));
        }
    }
}

fn add_file_dialog(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let Some(path) = pick_file(false) else {
        return false;
    };
    let at = state.view_center();
    insert_file(page, state, library, &path, at, toast)
}

/// Opens the microphone for a recording into `path`.
fn open_recorder(path: &Path, attachment: &str, page_id: &str) -> Result<audio::Recorder, String> {
    // The test machine has no microphone: tests record silence.
    #[cfg(test)]
    return Ok(audio::Recorder::silent(path, attachment, page_id));
    #[cfg(not(test))]
    audio::Recorder::start(path, attachment, page_id)
}

/// Starts recording into a new clip: a recording block at the caret of the note being
/// typed in, or in a new note at the insertion point, as the desktop notebook puts the recording's
/// icon at the cursor. Typing carries on while it records.
fn start_recording(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    editor_state: &mut EditorState,
    library: &mut Library,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H.%M").to_string();
    let name = format!("{}.wav", t!("canvas-recording-file", date = stamp));
    let attachment = match library.reserve_attachment(&page.id, "wav", "audio/wav", &name) {
        Ok(attachment) => attachment,
        Err(error) => {
            *toast = Some((
                t!("canvas-error-start-recording", error = error.to_string()).into_owned(),
                true,
            ));
            return false;
        }
    };
    let path = library.attachment_path(&attachment.stored_path);
    let recorder = match open_recorder(&path, &attachment.id, &page.id) {
        Ok(recorder) => recorder,
        Err(error) => {
            let _ = library.discard_attachment(&attachment.id);
            *toast = Some((
                t!("canvas-error-start-recording", error = error.to_string()).into_owned(),
                true,
            ));
            return false;
        }
    };
    state.recording = Some(recorder);
    let block = Block::new(BlockKind::Audio {
        attachment: attachment.id,
        duration_ms: 0,
    });
    // Should it fail, stopping the recording puts its clip back on the page.
    if let Some(id) = insert_into_note(ctx, page, state, editor_state, vec![block]) {
        widen_note(page, id, AUDIO_SIZE.x);
    }
    true
}

/// Starts a silent recording on `page` as Insert → Audio does, for tests (they have no
/// microphone). Returns its attachment id.
#[cfg(test)]
pub fn start_test_recording(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
) -> String {
    let attachment = library
        .reserve_attachment(&page.id, "wav", "audio/wav", "Audio recording.wav")
        .unwrap();
    let path = library.attachment_path(&attachment.stored_path);
    state.recording = Some(audio::Recorder::silent(&path, &attachment.id, &page.id));
    place_recording(page, state, attachment.id.clone());
    attachment.id
}

/// A new recording's clip as an item, beside the note being typed (which keeps its
/// caret) or in the middle of the view, as one undo step. Recordings now go into notes
/// (`start_recording`); tests still make clip items, as older pages hold them.
#[cfg(test)]
fn place_recording(page: &mut Page, state: &mut CanvasState, attachment: String) -> u64 {
    state.push_undo(&page.canvas);
    let at = match state.active_text.and_then(|id| page.canvas.item(id)) {
        Some(note) => pos2(note.x + note.width + 16.0, note.y),
        None => state.view_center() - AUDIO_SIZE / 2.0,
    };
    page.canvas.add(CanvasItem::new(
        CanvasKind::Audio {
            attachment,
            duration_ms: 0,
        },
        at.x.round(),
        at.y.round(),
        AUDIO_SIZE.x,
        AUDIO_SIZE.y,
    ))
}

/// Shows the page at `zoom` (1.0 is 100 %, kept in `ZOOM_RANGE`) around the middle of
/// the view: View › Zoom Level. Unlike `zoom_scene`, the ends of the range are reached
/// however the division rounds (20 % from 20.1 % could come out as 19.99999 %).
fn zoom_to(state: &mut CanvasState, zoom: f32) {
    let zoom = zoom.clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
    if let Some(rect) = state.visible.or(state.scene_rect) {
        let size = rect.size() * (state.zoom / zoom);
        state.scene_rect = Some(Rect::from_center_size(rect.center(), size));
    }
}

/// Zooms around the middle of the view. `factor` scales the visible area, so values
/// below one zoom in; passing the current zoom returns to 100%.
fn zoom_scene(state: &mut CanvasState, factor: f32) {
    if let Some(rect) = state.visible.or(state.scene_rect) {
        let size = rect.size() * factor;
        let zoom = state.zoom / factor;
        if ZOOM_RANGE.contains(&zoom) {
            state.scene_rect = Some(Rect::from_center_size(rect.center(), size));
        }
    }
}

/// A file printout being inserted (the desktop notebook's Insert › File Printout).
struct PrintoutRun {
    job: pdf::PrintoutJob,
    /// The page it goes on. Leaving that page cancels it.
    page_id: String,
    name: String,
    /// The PDF, kept to try again with a password.
    bytes: Arc<Vec<u8>>,
    /// The PDF attached to the page: its icon heads the printout.
    file: crate::model::Attachment,
    /// That icon was already on the page (its "Insert Printout"): the file card, or the
    /// note holding the PDF's File block. No new icon is placed.
    card: Option<u64>,
    /// Where the printout's top-left corner goes.
    at: Pos2,
    pages: usize,
    stored: Vec<StoredPage>,
    failed: usize,
    form_warning: bool,
    /// A password was given and the job restarted with it.
    tried_password: bool,
    password: Option<PasswordPrompt>,
}

/// A printout page already saved as an attachment.
struct StoredPage {
    attachment: String,
    /// 1-based.
    number: u32,
    size: Vec2,
    text: String,
}

/// A PDF dropped on the page, waiting for "Insert printout or attach file?".
struct PendingPdf {
    name: String,
    path: Option<PathBuf>,
    bytes: Option<Arc<Vec<u8>>>,
    at: Pos2,
}

/// What the printout dialogs ask for.
#[derive(Debug, Clone, PartialEq)]
enum RunChoice {
    Waiting,
    Cancel,
    Password(String),
}

/// The password a locked PDF needs, as it is typed.
#[derive(Debug, Default)]
struct PasswordPrompt {
    text: String,
    /// A password was tried and didn't open it.
    wrong: bool,
    focus: bool,
}

/// "Inserting printout": the PDF's name, which page, a progress bar and Cancel.
fn printout_progress_dialog(
    ctx: &egui::Context,
    page_id: &str,
    name: &str,
    done: usize,
    pages: usize,
    t: Tokens,
) -> RunChoice {
    let mut choice = RunChoice::Waiting;
    let response = egui::Modal::new(Id::unique(("printout-progress", page_id)))
        .backdrop_color(Color32::from_black_alpha(if t.dark { 150 } else { 76 }))
        .frame(
            egui::Frame::new()
                .fill(t.raised)
                .stroke(Stroke::new(1.0, t.line_strong))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                RichText::new(t!("printout-progress-title"))
                    .font(theme::semibold(20.0))
                    .color(t.ink),
            );
            ui.add_space(3.0);
            let status = if pages == 0 {
                t!("printout-progress-opening", name = name.to_string())
            } else {
                t!(
                    "printout-progress-pages",
                    done = (done + 1).min(pages),
                    count = pages
                )
            };
            ui.add(
                egui::Label::new(RichText::new(status).font(theme::ui(12.0)).color(t.muted)).wrap(),
            );
            ui.add_space(10.0);
            let fraction = if pages == 0 {
                0.0
            } else {
                done as f32 / pages as f32
            };
            // A Fluent determinate bar: a thin track, the done part in the accent.
            // (egui's ProgressBar paints its track in the dialog's own colour, so only a
            // round stub showed at the start.)
            let (track, _) = ui.allocate_exact_size(vec2(440.0, 6.0), Sense::hover());
            ui.painter()
                .rect_filled(track, CornerRadius::same(3), t.line);
            if fraction > 0.0 {
                let done_rect = Rect::from_min_size(
                    track.min,
                    vec2(
                        (track.width() * fraction.clamp(0.0, 1.0)).max(6.0),
                        track.height(),
                    ),
                );
                ui.painter()
                    .rect_filled(done_rect, CornerRadius::same(3), t.accent);
            }
            ui.add_space(14.0);
            if widgets::secondary_button(ui, None, &t!("common-cancel"), t).clicked() {
                choice = RunChoice::Cancel;
            }
        });
    // Only Cancel or Esc stops it, as in the desktop notebook: a stray click beside the dialog (to
    // bring the window forward) must not throw the pages done so far away.
    let escape = response.is_top_modal
        && !response.any_popup_open
        && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
    if choice == RunChoice::Waiting && escape {
        choice = RunChoice::Cancel;
    }
    choice
}

/// "Password required" for a locked PDF.
fn printout_password_dialog(
    ctx: &egui::Context,
    page_id: &str,
    name: &str,
    prompt: &mut PasswordPrompt,
    t: Tokens,
) -> RunChoice {
    let mut choice = RunChoice::Waiting;
    let response = egui::Modal::new(Id::unique(("printout-password", page_id)))
        .backdrop_color(Color32::from_black_alpha(if t.dark { 150 } else { 76 }))
        .frame(
            egui::Frame::new()
                .fill(t.raised)
                .stroke(Stroke::new(1.0, t.line_strong))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                RichText::new(t!("printout-password-title"))
                    .font(theme::semibold(20.0))
                    .color(t.ink),
            );
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(t!("printout-password-body", name = name.to_string()))
                        .font(theme::ui(12.0))
                        .color(t.muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            let field = ui.add(
                TextEdit::singleline(&mut prompt.text)
                    .password(true)
                    .desired_width(f32::INFINITY)
                    .font(theme::ui(13.0))
                    .text_color(t.ink),
            );
            // A new Modal spends its first frame measuring itself, disabled, and a
            // disabled field drops the caret: give it once it can keep it.
            if prompt.focus && ui.is_enabled() {
                prompt.focus = false;
                field.request_focus();
            }
            if prompt.wrong {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t!("printout-password-wrong"))
                        .font(theme::ui(12.0))
                        .color(t.danger),
                );
            }
            let entered =
                field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, None, &t!("printout-password-insert"), t).clicked()
                    || entered
                {
                    choice = RunChoice::Password(prompt.text.clone());
                }
                if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                    choice = RunChoice::Cancel;
                }
            });
        });
    if choice == RunChoice::Waiting && response.should_close() {
        choice = RunChoice::Cancel;
    }
    choice
}

/// The answer to "Insert file": a dropped PDF becomes a printout or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdfChoice {
    Waiting,
    Printout,
    Attach,
    Cancel,
}

/// the desktop notebook's Insert File question for a dropped or pasted PDF.
fn printout_choice_dialog(ctx: &egui::Context, page_id: &str, name: &str, t: Tokens) -> PdfChoice {
    let mut choice = PdfChoice::Waiting;
    let response = egui::Modal::new(Id::unique(("printout-choice", page_id)))
        .backdrop_color(Color32::from_black_alpha(if t.dark { 150 } else { 76 }))
        .frame(
            egui::Frame::new()
                .fill(t.raised)
                .stroke(Stroke::new(1.0, t.line_strong))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(
                RichText::new(t!("printout-choice-title"))
                    .font(theme::semibold(20.0))
                    .color(t.ink),
            );
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(t!("printout-choice-body", name = name.to_string()))
                        .font(theme::ui(12.0))
                        .color(t.muted),
                )
                .wrap(),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, None, &t!("printout-choice-printout"), t).clicked() {
                    choice = PdfChoice::Printout;
                }
                if widgets::secondary_button(ui, None, &t!("printout-choice-attach"), t).clicked() {
                    choice = PdfChoice::Attach;
                }
                if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                    choice = PdfChoice::Cancel;
                }
            });
        });
    if choice == PdfChoice::Waiting && response.should_close() {
        choice = PdfChoice::Cancel;
    }
    choice
}

/// Where a printout's top-left corner goes: under the note being typed in (the desktop notebook
/// puts it at the cursor), lined up with its text, else at the insertion point (the
/// last click on empty paper, or near the middle of the view), below the title. A note
/// still empty (just a click on paper) is removed when the printout lands, which then
/// takes its place.
fn printout_origin(page: &Page, state: &CanvasState) -> Pos2 {
    if let Some(note) = state.active_text.and_then(|id| page.canvas.item(id)) {
        let y = if note.doc().is_some_and(doc_is_blank) {
            note.y.max(PAGE_CONTENT_TOP)
        } else {
            note.y + note.height + PRINTOUT_GAP
        };
        return pos2(note.x + NOTE_PAD_X, y).round();
    }
    let at = insert_point(state);
    pos2(at.x, at.y.max(PAGE_CONTENT_TOP)).round()
}

/// A PDF on disk, read whole (it is attached anyway), within the attachment limit.
fn read_pdf(path: &Path, name: &str) -> Result<Arc<Vec<u8>>, String> {
    let size = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    if size > crate::db::MAX_ATTACHMENT_BYTES {
        return Err(t!(
            "data-error-attachment-too-big",
            name = name.to_string(),
            size = util::file_size(size),
            max = util::file_size(crate::db::MAX_ATTACHMENT_BYTES)
        )
        .into_owned());
    }
    std::fs::read(path)
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

/// Starts making a printout of `bytes` at `at`. The PDF is attached first (its icon
/// heads the pages), then a worker renders the pages while a dialog shows progress.
#[allow(clippy::too_many_arguments)]
fn start_printout(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    library: &mut Library,
    name: &str,
    bytes: Arc<Vec<u8>>,
    at: Pos2,
    toast: &mut Option<(String, bool)>,
) {
    match library.save_attachment_bytes(&page.id, "pdf", "application/pdf", &bytes, name) {
        Ok(file) => begin_printout(ctx, page, state, name, bytes, file, None, at),
        Err(error) => {
            *toast = Some((
                t!(
                    "canvas-error-attach-named",
                    name = name.to_string(),
                    error = error.to_string()
                )
                .into_owned(),
                true,
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn begin_printout(
    ctx: &egui::Context,
    page: &Page,
    state: &mut CanvasState,
    name: &str,
    bytes: Arc<Vec<u8>>,
    file: crate::model::Attachment,
    card: Option<u64>,
    at: Pos2,
) {
    state.printout = Some(PrintoutRun {
        job: pdf::PrintoutJob::start(Arc::clone(&bytes), None, ctx.clone()),
        page_id: page.id.clone(),
        name: name.to_string(),
        bytes,
        file,
        card,
        at,
        pages: 0,
        stored: Vec::new(),
        failed: 0,
        form_warning: false,
        tried_password: false,
        password: None,
    });
}

/// Removes what a cancelled or failed printout stored.
fn discard_printout(library: &mut Library, run: &PrintoutRun) {
    for page in &run.stored {
        let _ = library.discard_attachment(&page.attachment);
    }
    if run.card.is_none() {
        let _ = library.discard_attachment(&run.file.id);
    }
}

/// Moves a printout along, once a frame. It stores the pages the worker finished and
/// shows the dialog, and once the last page is in, it places them all.
fn poll_printout(
    ctx: &egui::Context,
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    t: Tokens,
    toast: &mut Option<(String, bool)>,
) -> bool {
    let mut changed = false;
    if state.printout.is_none() {
        if let Some(pending) = state.pending_pdfs.pop_front() {
            match printout_choice_dialog(ctx, &page.id, &pending.name, t) {
                PdfChoice::Waiting => state.pending_pdfs.push_front(pending),
                PdfChoice::Cancel => {}
                PdfChoice::Attach => {
                    changed |= match (&pending.path, &pending.bytes) {
                        (Some(path), _) => {
                            insert_file(page, state, library, path, pending.at, toast)
                        }
                        (None, Some(bytes)) => insert_file_bytes(
                            page,
                            state,
                            library,
                            bytes,
                            &pending.name,
                            pending.at,
                            toast,
                        ),
                        (None, None) => false,
                    };
                }
                PdfChoice::Printout => {
                    let bytes = match (&pending.path, &pending.bytes) {
                        (_, Some(bytes)) => Ok(Arc::clone(bytes)),
                        (Some(path), None) => read_pdf(path, &pending.name),
                        (None, None) => Err(String::new()),
                    };
                    match bytes {
                        Ok(bytes) => start_printout(
                            ctx,
                            page,
                            state,
                            library,
                            &pending.name,
                            bytes,
                            pending.at,
                            toast,
                        ),
                        Err(error) => {
                            *toast = Some((
                                t!(
                                    "canvas-error-read-named",
                                    name = pending.name.clone(),
                                    error = error
                                )
                                .into_owned(),
                                true,
                            ))
                        }
                    }
                }
            }
        }
    }
    // "Insert Printout" on an attached PDF: its icon is on the page already, so the pages
    // go under it (a file card), or under the note holding its File block, lined up with
    // the note's text. One printout at a time.
    if let Some((head, attachment)) = state
        .printout_from_card
        .take()
        .filter(|_| state.printout.is_none())
    {
        let at = page.canvas.item(head).map(|item| {
            let x = match item.kind {
                CanvasKind::File { .. } => item.x,
                _ => item.x + NOTE_PAD_X,
            };
            pos2(x, item.y + item.height + PRINTOUT_GAP).round()
        });
        if let Some(at) = at {
            let read = library
                .attachment(&attachment)
                .ok()
                .flatten()
                .and_then(|meta| {
                    std::fs::read(library.attachment_path(&meta.stored_path))
                        .ok()
                        .map(|bytes| (meta, Arc::new(bytes)))
                });
            match read {
                Some((meta, bytes)) => {
                    let name = meta.file_name.clone();
                    begin_printout(ctx, page, state, &name, bytes, meta, Some(head), at);
                }
                None => *toast = Some((t!("canvas-error-file-missing").into_owned(), true)),
            }
        }
    }
    let Some(mut run) = state.printout.take() else {
        return changed;
    };
    if run.page_id != page.id {
        run.job.cancel();
        discard_printout(library, &run);
        *toast = Some((t!("printout-toast-cancelled").into_owned(), false));
        return changed;
    }
    let stem = Path::new(&run.name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(&run.name)
        .to_string();
    let mut done = false;
    for event in run.job.poll() {
        match event {
            pdf::JobEvent::Opened {
                pages,
                form_warning,
            } => {
                run.pages = pages;
                run.form_warning = form_warning;
            }
            pdf::JobEvent::Page(image) => {
                let number = image.index as u32 + 1;
                let file_name =
                    t!("printout-page-file", name = stem.clone(), page = number).into_owned();
                match library.save_image_bytes(
                    &page.id,
                    "webp",
                    "image/webp",
                    &image.webp,
                    &file_name,
                ) {
                    Ok(attachment) => {
                        let mut size =
                            vec2(image.size_pt.0, image.size_pt.1) * pdf::UNITS_PER_POINT;
                        if size.x > PRINTOUT_MAX_WIDTH {
                            size *= PRINTOUT_MAX_WIDTH / size.x;
                        }
                        run.stored.push(StoredPage {
                            attachment: attachment.id,
                            number,
                            size: size.round(),
                            text: image.text,
                        });
                    }
                    Err(_) => run.failed += 1,
                }
            }
            pdf::JobEvent::PageFailed(_) => run.failed += 1,
            pdf::JobEvent::Failed(pdf::PdfError::Password) => {
                run.password = Some(PasswordPrompt {
                    text: String::new(),
                    wrong: run.tried_password,
                    focus: true,
                });
            }
            pdf::JobEvent::Failed(error) => {
                discard_printout(library, &run);
                *toast = Some((error.message(&run.name), true));
                return changed;
            }
            pdf::JobEvent::Done => done = true,
        }
    }
    if done {
        return changed | finish_printout(page, state, library, run, toast);
    }
    let choice = match run.password.as_mut() {
        Some(prompt) => printout_password_dialog(ctx, &page.id, &run.name, prompt, t),
        None => printout_progress_dialog(
            ctx,
            &page.id,
            &run.name,
            run.stored.len() + run.failed,
            run.pages,
            t,
        ),
    };
    match choice {
        RunChoice::Waiting => state.printout = Some(run),
        RunChoice::Cancel => {
            run.job.cancel();
            discard_printout(library, &run);
        }
        RunChoice::Password(password) => {
            run.tried_password = true;
            run.password = None;
            run.job = pdf::PrintoutJob::start(Arc::clone(&run.bytes), Some(password), ctx.clone());
            state.printout = Some(run);
        }
    }
    changed
}

/// Places a finished printout: the PDF's icon, then its pages top to bottom, as one
/// undo step.
fn finish_printout(
    page: &mut Page,
    state: &mut CanvasState,
    library: &mut Library,
    run: PrintoutRun,
    toast: &mut Option<(String, bool)>,
) -> bool {
    if run.stored.is_empty() {
        discard_printout(library, &run);
        *toast = Some((pdf::PdfError::Invalid.message(&run.name), true));
        return false;
    }
    // The note being typed in ends first, as a click elsewhere ends it: its typing is
    // one step, and one left empty (a click on paper, then the printout) goes.
    if let Some(id) = state.active_text.take() {
        finish_text_edit(page, state, id);
    }
    state.push_undo(&page.canvas);
    // Options › Advanced › Printouts: the pages go in the background, to write on.
    let background = library
        .setting("printout_background")
        .ok()
        .flatten()
        .as_deref()
        == Some("true");
    let x = run.at.x;
    let mut y = run.at.y;
    if run.card.is_none() {
        // The PDF's icon is a file card, as a dropped file's is.
        page.canvas.add(CanvasItem::new(
            CanvasKind::File {
                attachment: run.file.id.clone(),
                name: run.file.file_name.clone(),
                size: run.file.byte_size,
            },
            x,
            y,
            FILE_SIZE.x,
            FILE_SIZE.y,
        ));
        y += FILE_SIZE.y + PRINTOUT_GAP;
    }
    for stored in run.stored {
        let mut kind = CanvasKind::image(stored.attachment);
        if let CanvasKind::Image { printout, meta, .. } = &mut kind {
            *printout = Some(Box::new(PrintoutPage {
                source: run.file.id.clone(),
                page: stored.number,
                text: stored.text,
                size: Some([stored.size.x, stored.size.y]),
            }));
            meta.background = background;
        }
        page.canvas
            .add(CanvasItem::new(kind, x, y, stored.size.x, stored.size.y));
        y += stored.size.y + PRINTOUT_GAP;
    }
    // PDFs dropped together wait their turn 24 px apart: the next ones go below this one
    // rather than over it.
    let height = y - run.at.y;
    for pending in &mut state.pending_pdfs {
        if (run.at.y..y).contains(&pending.at.y) {
            pending.at.y += height;
        }
    }
    state.selection.clear();
    state.tool = CanvasTool::Select;
    if run.failed > 0 {
        *toast = Some((
            t!("printout-toast-failed-pages", count = run.failed).into_owned(),
            true,
        ));
    } else if run.form_warning {
        *toast = Some((
            t!("printout-toast-form", name = run.name.clone()).into_owned(),
            false,
        ));
    }
    true
}

/// "Insert files from your computer?", asked before pasted file paths are attached.
fn pasted_files_dialog(
    ctx: &egui::Context,
    page_id: &str,
    pending: &PendingFiles,
    t: Tokens,
) -> FilesChoice {
    let mut choice = FilesChoice::Waiting;
    let response = egui::Modal::new(Id::unique(("pasted-files", page_id)))
        .backdrop_color(Color32::from_black_alpha(if t.dark { 150 } else { 76 }))
        .frame(
            egui::Frame::new()
                .fill(t.raised)
                .stroke(Stroke::new(1.0, t.line_strong))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(
                RichText::new(t!("canvas-files-title"))
                    .font(theme::semibold(20.0))
                    .color(t.ink),
            );
            ui.add_space(3.0);
            ui.label(
                RichText::new(t!("canvas-files-body"))
                    .font(theme::ui(12.0))
                    .color(t.muted),
            );
            ui.add_space(12.0);
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (path, size) in &pending.files {
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(file_glyph(&name))
                                    .font(theme::icon(16.0))
                                    .color(t.accent),
                            );
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(path.display().to_string())
                                            .font(theme::ui(12.0))
                                            .color(t.ink),
                                    )
                                    .wrap(),
                                );
                                ui.label(
                                    RichText::new(util::file_size(*size))
                                        .font(theme::ui(11.5))
                                        .color(t.faint),
                                );
                            });
                        });
                        ui.add_space(4.0);
                    }
                });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(
                    ui,
                    Some(icons::PAPERCLIP),
                    &t!("canvas-files-attach"),
                    t,
                )
                .clicked()
                {
                    choice = FilesChoice::Attach;
                }
                if widgets::secondary_button(ui, None, &t!("canvas-files-paste-text"), t).clicked()
                {
                    choice = FilesChoice::PasteText;
                }
                if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                    choice = FilesChoice::Cancel;
                }
            });
        });
    if choice == FilesChoice::Waiting && response.should_close() {
        choice = FilesChoice::Cancel;
    }
    choice
}

/// The Alt Text dialog (Picture › Alt Text, or Alt Text… in a picture's menu): the
/// description a screen reader and the exports give for the picture. `Some(true)` is OK,
/// `Some(false)` Cancel or Esc; `None` while it stays open.
fn alt_text_dialog(
    ctx: &egui::Context,
    page_id: &str,
    edit: &mut AltTextEdit,
    t: Tokens,
) -> Option<bool> {
    let mut answer = None;
    let field_id = Id::unique(("alt-text-field", page_id));
    let response = egui::Modal::new(Id::unique(("alt-text", page_id)))
        .backdrop_color(Color32::from_black_alpha(if t.dark { 150 } else { 76 }))
        .frame(
            egui::Frame::new()
                .fill(t.raised)
                .stroke(Stroke::new(1.0, t.line_strong))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(
                RichText::new(t!("picture-alt-text"))
                    .font(theme::semibold(20.0))
                    .color(t.ink),
            );
            ui.add_space(3.0);
            ui.add(
                egui::Label::new(
                    RichText::new(t!("picture-alt-body"))
                        .font(theme::ui(12.0))
                        .color(t.muted),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            ui.add(
                TextEdit::multiline(&mut edit.text)
                    .id(field_id)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .font(theme::ui(13.0))
                    .text_color(t.ink)
                    .hint_text(
                        RichText::new(t!("picture-alt-hint"))
                            .font(theme::ui(13.0))
                            .color(t.faint),
                    ),
            );
            // A new Modal spends its first frame measuring itself, disabled, and a
            // disabled field drops the caret: give it once it can keep it, at the end.
            if edit.focus && ui.is_enabled() {
                edit.focus = false;
                editor::set_focus(ui.ctx(), field_id, edit.text.chars().count(), None);
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                    answer = Some(true);
                }
                if widgets::secondary_button(ui, None, &t!("common-cancel"), t).clicked() {
                    answer = Some(false);
                }
            });
        });
    if answer.is_none() && response.should_close() {
        answer = Some(false);
    }
    answer
}

fn timeline_dialog(
    ctx: &egui::Context,
    page: &Page,
    mut editor: TimelineEditor,
    t: Tokens,
) -> TimelineOutcome {
    let mut close = false;
    let mut save = false;
    let response = egui::Modal::new(Id::unique(("timeline-editor", page.id.as_str())))
        .frame(
            egui::Frame::new()
                .fill(t.raised)
                .stroke(Stroke::new(1.0, t.line_strong))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!("canvas-edit-frise"))
                        .font(theme::semibold(20.0))
                        .color(t.ink),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button_sized(ui, icons::X, &t!("common-close"), 28.0, 14.0, t)
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.add_space(4.0);
            widgets::caption(ui, &t!("canvas-frise-milestones"), t.muted);
            ui.add_space(4.0);
            let mut remove = None;
            let mut swap = None;
            let count = editor.milestones.len();
            for (index, milestone) in editor.milestones.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{:02}", index + 1))
                            .font(theme::ui(11.5))
                            .color(t.faint),
                    );
                    ui.add(
                        TextEdit::singleline(&mut milestone.title)
                            .hint_text(t!("canvas-frise-title-hint").as_ref())
                            .font(theme::ui(12.0))
                            .desired_width(140.0),
                    );
                    ui.add(
                        TextEdit::singleline(&mut milestone.date)
                            .hint_text(t!("canvas-frise-date").as_ref())
                            .font(theme::ui(12.0))
                            .desired_width(90.0),
                    );
                    ui.add(
                        TextEdit::singleline(&mut milestone.note)
                            .hint_text(t!("canvas-frise-note-hint").as_ref())
                            .font(theme::ui(12.0))
                            .desired_width(170.0),
                    );
                    if index > 0
                        && widgets::icon_button_sized(
                            ui,
                            icons::ARROW_UP,
                            &t!("canvas-frise-move-earlier"),
                            24.0,
                            12.0,
                            t,
                        )
                        .clicked()
                    {
                        swap = Some(index);
                    }
                    if count > 1
                        && widgets::icon_button_sized(
                            ui,
                            icons::TRASH,
                            &t!("canvas-frise-remove"),
                            24.0,
                            12.0,
                            t,
                        )
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = swap {
                editor.milestones.swap(index - 1, index);
            }
            if let Some(index) = remove {
                editor.milestones.remove(index);
            }
            if widgets::text_button(ui, Some(icons::PLUS), &t!("canvas-frise-add"), t).clicked() {
                editor.milestones.push(Milestone {
                    title: t!("canvas-frise-new-milestone").into_owned(),
                    date: t!("canvas-frise-date").into_owned(),
                    note: String::new(),
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, None, &t!("canvas-frise-save"), t).clicked() {
                    save = true;
                }
                if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                    close = true;
                }
            });
        });
    if save {
        TimelineOutcome::Save(editor)
    } else if close || response.should_close() {
        TimelineOutcome::Closed
    } else {
        TimelineOutcome::Editing(editor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resized_page_area_keeps_the_zoom() {
        let old = Rect::from_min_size(pos2(300.0, 120.0), vec2(1000.0, 700.0));
        // At 125%: 800 x 560 page units on screen.
        let view = Rect::from_min_size(pos2(40.0, 60.0), vec2(800.0, 560.0));
        let narrower = resized_view(old, vec2(700.0, 700.0), view).expect("a new view");
        assert_eq!(narrower.min, view.min, "the top-left corner stays");
        assert!(
            (700.0 / narrower.width() - 1.25).abs() < 1e-4,
            "{narrower:?}"
        );
        assert!(
            (700.0 / narrower.height() - 1.25).abs() < 1e-4,
            "{narrower:?}"
        );
        assert_eq!(resized_view(old, old.size(), view), None, "same size");
        assert_eq!(
            resized_view(Rect::NOTHING, old.size(), view),
            None,
            "first frame"
        );
        let fit_request = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 900.0));
        assert_eq!(
            resized_view(old, vec2(700.0, 700.0), fit_request),
            None,
            "a fit-to-page rectangle is left to Scene"
        );
    }

    #[test]
    fn lasso_and_selection_rules() {
        let square = [
            pos2(0.0, 0.0),
            pos2(10.0, 0.0),
            pos2(10.0, 10.0),
            pos2(0.0, 10.0),
        ];
        assert!(point_in_polygon(pos2(5.0, 5.0), &square));
        assert!(!point_in_polygon(pos2(15.0, 5.0), &square));
        assert!(!point_in_polygon(pos2(5.0, 5.0), &square[..2]));

        let mut selection = Selection::one(7);
        assert_eq!(selection.primary(), Some(7));
        selection.toggle(9);
        assert_eq!(selection.primary(), None);
        selection.toggle(7);
        assert_eq!(selection.primary(), Some(9));
        selection.strokes.push(0);
        assert_eq!(selection.primary(), None, "ink makes it a group");
    }

    #[test]
    fn item_menus_follow_notebook_order() {
        use ItemRow as R;
        let (mut page, ids) = stacked_page();
        let shape = ids[0];
        let picture = CanvasItem::new(CanvasKind::image("a"), 300.0, 100.0, 80.0, 60.0);
        let picture = page.canvas.add(picture);
        let mut state = CanvasState::default();
        // One item selected: its own menu; with another, or from the frame, several items'.
        state.selection.set_one(picture);
        assert_eq!(
            MenuKind::of(&page, &state, Some(picture)),
            MenuKind::Picture { printout: false }
        );
        assert_eq!(MenuKind::of(&page, &state, None), MenuKind::Several);

        // An attached PDF offers Insert Printout until its printout is on the page; a
        // printout page has its own rows.
        let file = |name: &str| CanvasKind::File {
            attachment: format!("file-{name}"),
            name: name.to_string(),
            size: 10,
        };
        let pdf = page
            .canvas
            .add(CanvasItem::new(file("sheet.PDF"), 20.0, 400.0, 260.0, 56.0));
        let text = page
            .canvas
            .add(CanvasItem::new(file("sheet.txt"), 20.0, 480.0, 260.0, 56.0));
        state.selection.set_one(pdf);
        assert_eq!(
            MenuKind::of(&page, &state, Some(pdf)),
            MenuKind::File { pdf: true }
        );
        state.selection.set_one(text);
        assert_eq!(
            MenuKind::of(&page, &state, Some(text)),
            MenuKind::File { pdf: false }
        );
        let mut sheet = CanvasKind::image("sheet-1");
        if let CanvasKind::Image { printout, .. } = &mut sheet {
            *printout = Some(Box::new(PrintoutPage {
                source: "file-sheet.PDF".into(),
                page: 1,
                text: String::new(),
                size: None,
            }));
        }
        let sheet = page
            .canvas
            .add(CanvasItem::new(sheet, 20.0, 560.0, 816.0, 1056.0));
        state.selection.set_one(sheet);
        assert_eq!(
            MenuKind::of(&page, &state, Some(sheet)),
            MenuKind::Picture { printout: true }
        );
        state.selection.set_one(pdf);
        assert_eq!(
            MenuKind::of(&page, &state, Some(pdf)),
            MenuKind::File { pdf: false },
            "no second printout over the first"
        );
        state.selection.set_one(shape);
        assert_eq!(
            MenuKind::of(&page, &state, Some(shape)),
            MenuKind::Object {
                edit: true,
                latex: false
            }
        );
        state.selection.toggle(picture);
        assert_eq!(MenuKind::of(&page, &state, Some(shape)), MenuKind::Several);

        // Reference §5: Cut, Copy and Paste first; Select › and Order ›; Delete; no
        // Duplicate row.
        assert_eq!(
            MenuKind::Note.rows(),
            [
                R::Cut,
                R::Copy,
                R::Paste,
                R::Delete,
                R::Separator,
                R::Select,
                R::Order
            ]
        );
        assert_eq!(
            MenuKind::Picture { printout: false }.rows(),
            [
                R::Cut,
                R::Copy,
                R::Paste,
                R::Separator,
                R::SaveAs,
                R::Separator,
                R::Order,
                R::Separator,
                R::Rotate,
                R::RestoreSize,
                R::Background,
                R::AltText,
                R::Separator,
                R::Delete,
            ]
        );
        // A printout page: its two Copy Text rows and Open Original right after Copy.
        assert_eq!(
            MenuKind::Picture { printout: true }.rows(),
            [
                R::Cut,
                R::Copy,
                R::CopyPrintoutPage,
                R::CopyPrintoutAll,
                R::OpenOriginal,
                R::Paste,
                R::Separator,
                R::SaveAs,
                R::Separator,
                R::Order,
                R::Separator,
                R::Rotate,
                R::RestoreSize,
                R::Background,
                R::AltText,
                R::Separator,
                R::Delete,
            ]
        );
        assert_eq!(
            MenuKind::File { pdf: false }.rows(),
            [
                R::Open,
                R::SaveAs,
                R::Separator,
                R::Cut,
                R::Copy,
                R::Separator,
                R::Delete
            ]
        );
        assert_eq!(
            MenuKind::File { pdf: true }.rows(),
            [
                R::Open,
                R::SaveAs,
                R::Separator,
                R::InsertPrintout,
                R::Separator,
                R::Cut,
                R::Copy,
                R::Separator,
                R::Delete
            ]
        );
        assert_eq!(
            MenuKind::Recording.rows(),
            [
                R::PlayPause,
                R::SaveAs,
                R::Separator,
                R::Cut,
                R::Copy,
                R::Separator,
                R::Delete
            ]
        );
        assert_eq!(
            MenuKind::Object {
                edit: true,
                latex: true
            }
            .rows(),
            [
                R::Cut,
                R::Copy,
                R::Paste,
                R::Separator,
                R::Edit,
                R::CopyLatex,
                R::Separator,
                R::Order,
                R::Separator,
                R::Delete,
            ]
        );
        // A connector has no Edit row, and no empty group between two separators.
        assert_eq!(
            MenuKind::Object {
                edit: false,
                latex: false
            }
            .rows(),
            [
                R::Cut,
                R::Copy,
                R::Paste,
                R::Separator,
                R::Order,
                R::Separator,
                R::Delete
            ]
        );
        assert_eq!(
            MenuKind::Several.rows(),
            [R::Cut, R::Copy, R::Delete, R::Separator, R::Order]
        );

        // The rows speak the interface's language; Delete hints Del.
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        assert_eq!(R::Paste.text(None).1, "Coller");
        assert_eq!(R::Order.text(None).1, "Ordre");
        assert_eq!(R::Delete.text(None).2, "Suppr");
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        assert_eq!(R::Paste.text(None).1, "Paste");
        assert_eq!(R::Order.text(None).1, "Order");
        assert_eq!(R::Delete.text(None).2, "Del");
        let frise = CanvasKind::Timeline {
            color: Rgb(0, 0, 0),
            milestones: Vec::new(),
        };
        assert_eq!(R::Edit.text(Some(&frise)).1, "Edit frise");
        assert_eq!(R::InsertPrintout.text(None).1, "Insert Printout");
    }

    /// A printout pasted on another page brings one copy of its PDF, for its icon and its
    /// pages alike: the icon then offers no second Insert Printout over them.
    #[test]
    fn a_printout_pasted_on_another_page_brings_its_own_pdf() {
        let mut library = Library::open_in_memory().unwrap();
        let notebook = library.create_notebook("N", "#745cf5").unwrap();
        let section = notebook.sections[0].id.clone();
        let from = library.create_canvas_page(&section, "From").unwrap();
        let mut to = library.create_canvas_page(&section, "To").unwrap();
        let pdf = library
            .save_attachment_bytes(&from.id, "pdf", "application/pdf", b"%PDF-1.4", "a.pdf")
            .unwrap();
        let picture = library
            .save_image_bytes(&from.id, "webp", "image/webp", b"page", "a 1.webp")
            .unwrap();
        let mut sheet = CanvasKind::image(picture.id.clone());
        if let CanvasKind::Image { printout, .. } = &mut sheet {
            *printout = Some(Box::new(PrintoutPage {
                source: pdf.id.clone(),
                page: 1,
                text: String::new(),
                size: None,
            }));
        }
        // The page comes before its icon: either order gives one copy.
        let mut clip = Clip {
            items: vec![
                CanvasItem::new(sheet, 0.0, 68.0, 816.0, 1056.0),
                CanvasItem::new(
                    CanvasKind::File {
                        attachment: pdf.id.clone(),
                        name: "a.pdf".into(),
                        size: pdf.byte_size,
                    },
                    0.0,
                    0.0,
                    FILE_SIZE.x,
                    FILE_SIZE.y,
                ),
            ],
            ink: Vec::new(),
        };
        adopt_attachments(&to.id, &mut library, &mut clip);
        let source = clip.items[0].kind.printout().unwrap().source.clone();
        assert_ne!(source, pdf.id);
        assert_eq!(clip.items[1].kind.attachment(), Some(source.as_str()));
        assert_eq!(library.attachment(&source).unwrap().unwrap().page_id, to.id);
        assert_ne!(clip.items[0].kind.attachment(), Some(picture.id.as_str()));
        to.canvas.items = clip.items;
        let icon = to.canvas.items[1].id;
        let mut state = CanvasState::default();
        state.selection.set_one(icon);
        assert_eq!(
            MenuKind::of(&to, &state, Some(icon)),
            MenuKind::File { pdf: false },
            "no second Insert Printout"
        );
    }

    #[test]
    fn printout_text_joins_its_pages_in_order() {
        let mut page = blank_page();
        let mut add = |source: &str, number: u32, text: &str| {
            let mut kind = CanvasKind::image(format!("{source}-{number}"));
            if let CanvasKind::Image { printout, .. } = &mut kind {
                *printout = Some(Box::new(PrintoutPage {
                    source: source.into(),
                    page: number,
                    text: text.into(),
                    size: None,
                }));
            }
            page.canvas.add(CanvasItem::new(
                kind,
                0.0,
                number as f32 * 100.0,
                80.0,
                90.0,
            ));
        };
        // Moved around on the page, a scan with no text, and another printout.
        add("a", 3, "Third\n");
        add("a", 1, "First");
        add("a", 2, "  \n");
        add("b", 1, "Other");
        assert_eq!(printout_text(&page, "a"), "First\n\nThird");
        assert_eq!(printout_text(&page, "b"), "Other");
        assert_eq!(printout_text(&page, "c"), "");
        assert!(has_printout(&page, "a"));
        assert!(!has_printout(&page, "c"));
        // The word count, taken item by item, is the page text's.
        page.canvas.add(CanvasItem::new(
            CanvasKind::File {
                attachment: "a".into(),
                name: "two words.pdf".into(),
                size: 0,
            },
            0.0,
            0.0,
            FILE_SIZE.x,
            FILE_SIZE.y,
        ));
        assert_eq!(
            page.word_count(),
            page.plain_text().split_whitespace().count() as i64
        );
        assert_eq!(page.word_count(), 5);
    }

    #[test]
    fn segment_distance_and_stroke_hits() {
        let a = pos2(0.0, 0.0);
        let b = pos2(100.0, 0.0);
        assert_eq!(distance_to_segment(pos2(50.0, 5.0), a, b), 5.0);
        assert_eq!(distance_to_segment(pos2(-3.0, 4.0), a, b), 5.0);
        // A fast stroke has far-apart samples; its middle must still be erasable.
        let stroke = InkStroke {
            points: vec![[0.0, 0.0], [200.0, 0.0]],
            color: Rgb::INKS[0],
            width: 2.0,
            highlighter: false,
        };
        let dab = |at: Pos2| swipe_hits(&stroke, at, at, 10.0);
        assert!(dab(pos2(100.0, 6.0)));
        assert!(!dab(pos2(100.0, 30.0)));
        // A quick swipe right across the stroke erases it, though neither end is near.
        assert!(swipe_hits(
            &stroke,
            pos2(100.0, -50.0),
            pos2(100.0, 50.0),
            2.0
        ));
        assert!(!swipe_hits(
            &stroke,
            pos2(-50.0, -50.0),
            pos2(-50.0, 50.0),
            2.0
        ));
        let (c, d) = (pos2(50.0, -10.0), pos2(50.0, 10.0));
        assert_eq!(segment_distance(a, b, c, d), 0.0, "crossing");
        assert_eq!(
            segment_distance(a, b, c + vec2(0.0, 20.0), d + vec2(0.0, 20.0)),
            10.0
        );
        assert_eq!(
            segment_distance(a, b, pos2(103.0, 4.0), pos2(110.0, 4.0)),
            5.0
        );
    }

    /// One dab of the point eraser, as `erase_gesture` does it.
    fn erase_disc(ink: &mut Vec<InkStroke>, center: Pos2, radius: f32) -> bool {
        let cuts = rub_out_ink(ink, center, center, radius);
        let changed = !cuts.is_empty();
        apply_rub_out(ink, cuts);
        changed
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01
    }

    #[test]
    fn the_point_eraser_cuts_strokes_and_keeps_their_style() {
        let line = InkStroke {
            points: vec![[0.0, 0.0], [200.0, 0.0]],
            color: Rgb::INKS[2],
            width: 4.0,
            highlighter: false,
        };
        let mut ink = vec![line.clone()];
        // Reach 10 plus half the 4 px width: 12 either side of x = 100.
        assert!(erase_disc(&mut ink, pos2(100.0, 0.0), 10.0));
        assert_eq!(ink.len(), 2);
        assert!(near(ink[0].points[1], [88.0, 0.0]) && near(ink[1].points[0], [112.0, 0.0]));
        assert!(ink
            .iter()
            .all(|stroke| stroke.color == line.color && stroke.width == line.width));
        let cut = ink.clone();
        assert!(
            !erase_disc(&mut ink, pos2(100.0, 0.0), 10.0),
            "the same spot again changes nothing: the ends sit on the eraser's edge"
        );
        assert_eq!(ink, cut);
        assert!(
            !erase_disc(&mut ink, pos2(100.0, 50.0), 10.0),
            "a miss changes nothing"
        );
        assert!(
            erase_disc(&mut ink, pos2(0.0, 0.0), 10.0),
            "covering an end trims it"
        );
        assert_eq!(ink.len(), 2);
        assert!(near(ink[0].points[0], [12.0, 0.0]));
        let mut dot = vec![InkStroke {
            points: vec![[5.0, 5.0]],
            ..line.clone()
        }];
        assert!(erase_disc(&mut dot, pos2(6.0, 5.0), 4.0));
        assert!(dot.is_empty(), "a dot under the eraser goes");

        // A swipe cuts all the way along it, and leaves other strokes alone.
        let far = InkStroke {
            points: vec![[0.0, 300.0], [200.0, 300.0]],
            ..line.clone()
        };
        let mut ink = vec![line.clone(), far.clone()];
        let cuts = rub_out_ink(&ink, pos2(100.0, -50.0), pos2(100.0, 50.0), 10.0);
        assert_eq!(cuts.len(), 1, "only the stroke under the swipe");
        apply_rub_out(&mut ink, cuts);
        assert_eq!(ink.len(), 3);
        assert!(near(ink[0].points[1], [88.0, 0.0]) && near(ink[1].points[0], [112.0, 0.0]));
        assert_eq!(ink[2], far);
        assert_eq!(cut_stroke(&far.points, pos2(100.0, 0.0), 12.0), None);
    }

    #[test]
    fn painted_text_highlights_only_the_matches() {
        let t = Tokens::light();
        let job = marked_job(
            "Wörld world",
            theme::ui(13.0),
            t.ink,
            f32::INFINITY,
            &[(0..5, true), (6..11, false)],
            t,
        );
        let parts: Vec<_> = job
            .sections
            .iter()
            .map(|section| {
                (
                    &job.text[section.byte_range.start.0..section.byte_range.end.0],
                    section.format.background,
                )
            })
            .collect();
        assert_eq!(
            parts,
            vec![
                ("Wörld", theme::find_highlight(t, true)),
                (" ", Color32::TRANSPARENT),
                ("world", theme::find_highlight(t, false)),
            ]
        );
    }

    #[test]
    fn pictures_keep_their_proportions() {
        assert_eq!(picture_size(Some([4000.0, 3000.0])), vec2(480.0, 360.0));
        assert_eq!(picture_size(Some([3000.0, 4000.0])), vec2(270.0, 360.0));
        assert_eq!(picture_size(Some([120.0, 80.0])), vec2(120.0, 80.0));
    }

    #[test]
    fn turning_a_page_picture_swaps_its_box() {
        let library = Library::open_in_memory().unwrap();
        let mut page = blank_page();
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            100.0,
            100.0,
            200.0,
            100.0,
        ));
        let mut state = CanvasState::default();
        let meta = |page: &Page, id: u64| match &page.canvas.item(id).unwrap().kind {
            CanvasKind::Image { meta, .. } => meta.clone(),
            _ => unreachable!(),
        };
        let mut run = |page: &mut Page, id: u64, command: PictureCmd| {
            run_picture_cmd(page, &mut state, &library, PictureTarget::Item(id), command)
        };
        assert!(run(&mut page, id, PictureCmd::RotateRight));
        let rect = page.canvas.item(id).unwrap().rect();
        assert_eq!(
            rect.size(),
            vec2(100.0, 200.0),
            "a quarter turn swaps the box"
        );
        assert_eq!(rect.center(), pos2(200.0, 150.0), "around its centre");
        assert_eq!(meta(&page, id).turns, 1);
        // Flipping what is shown left to right turns the other way round and mirrors it.
        assert!(run(&mut page, id, PictureCmd::FlipHorizontal));
        let flipped = meta(&page, id);
        assert_eq!((flipped.turns, flipped.flip), (3, true));
        assert_eq!(
            page.canvas.item(id).unwrap().rect(),
            rect,
            "a flip keeps the box"
        );
        // At the page's top edge, a turn keeps the picture on the page.
        let top = page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            0.0,
            ITEM_TOP,
            200.0,
            100.0,
        ));
        assert!(run(&mut page, top, PictureCmd::RotateLeft));
        let rect = page.canvas.item(top).unwrap().rect();
        assert_eq!(rect.size(), vec2(100.0, 200.0));
        assert_eq!(rect.left_top(), pos2(50.0, ITEM_TOP), "not above the page");
        assert_eq!(meta(&page, top).turns, 3);
        assert_eq!(state.undo_depth(), 3, "one undo step each");
    }

    #[test]
    fn committing_a_crop_keeps_the_kept_part_in_place() {
        let mut page = blank_page();
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            100.0,
            100.0,
            200.0,
            100.0,
        ));
        let mut state = CanvasState::default();
        state.selection.set_one(id);
        let meta = |page: &Page| match &page.canvas.item(id).unwrap().kind {
            CanvasKind::Image { meta, .. } => meta.clone(),
            _ => unreachable!(),
        };
        let before = page.canvas.clone();
        assert!(start_crop(&page, &mut state, id));
        // Left as it was, it changes nothing and takes no undo step.
        assert!(!commit_crop(&mut page, &mut state));
        assert_eq!(state.undo_depth(), 0);
        assert!(start_crop(&page, &mut state, id));
        let crop = state.cropping.as_mut().unwrap();
        crop.keep = Rect::from_min_max(pos2(200.0, 100.0), crop.keep.max);
        assert!(commit_crop(&mut page, &mut state));
        assert!(state.cropping.is_none());
        assert_eq!(
            page.canvas.item(id).unwrap().rect(),
            Rect::from_min_size(pos2(200.0, 100.0), vec2(100.0, 100.0))
        );
        assert_eq!(meta(&page).crop, Some([0.5, 0.0, 1.0, 1.0]));
        // Cropping again lays the whole picture out where it was.
        assert!(start_crop(&page, &mut state, id));
        let full = state.cropping.unwrap().full;
        let original = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 100.0));
        assert!(
            (full.min - original.min).length() < 0.01 && (full.max - original.max).length() < 0.01,
            "{full:?}"
        );
        // Esc drops it with no change; one undo takes the crop back.
        state.cropping = None;
        assert!(state.undo(&mut page));
        assert_eq!(page.canvas, before);
        assert_eq!(meta(&page).crop, None);
        // A quarter-turned picture crops in what is shown: its kept right half is the top of
        // the picture's own space (a right turn puts its top at the right).
        let turned = page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            100.0,
            300.0,
            100.0,
            200.0,
        ));
        if let CanvasKind::Image { meta, .. } = &mut page.canvas.item_mut(turned).unwrap().kind {
            meta.turns = 1;
        }
        state.selection.set_one(turned);
        assert!(start_crop(&page, &mut state, turned));
        let crop = state.cropping.as_mut().unwrap();
        crop.keep = Rect::from_min_max(pos2(150.0, 300.0), crop.keep.max);
        assert!(commit_crop(&mut page, &mut state));
        let CanvasKind::Image { meta, .. } = &page.canvas.item(turned).unwrap().kind else {
            unreachable!()
        };
        assert_eq!(meta.crop, Some([0.0, 0.0, 1.0, 0.5]));
        // Printouts and background pictures are not cropped.
        if let CanvasKind::Image { meta, .. } = &mut page.canvas.item_mut(turned).unwrap().kind {
            meta.background = true;
        }
        assert!(!start_crop(&page, &mut state, turned));
    }

    /// A crop's handles stop at the least a crop keeps: 2% of a wide picture, so what is
    /// kept is what the handles held (`crop_box` would widen a thinner strip).
    #[test]
    fn crop_handles_keep_at_least_what_a_crop_can_show() {
        let full = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 100.0));
        let kept = moved_crop(full, full, (-1, 0), vec2(10_000.0, 0.0));
        assert!((kept.width() - 16.0).abs() < 1e-3, "{kept:?}");
        let kept = moved_crop(full, full, (0, 1), vec2(0.0, -10_000.0));
        assert!((kept.height() - CROP_MIN).abs() < 1e-3, "{kept:?}");
        let meta = cropped_meta(&PictureMeta::default(), crop_fractions(full, kept));
        assert_eq!(meta.crop_box(), meta.crop.unwrap(), "no clamping");
    }

    /// Leaving the page (another page, search, closing) keeps a crop under way, as a click
    /// elsewhere does.
    #[test]
    fn leaving_the_page_keeps_a_crop_under_way() {
        let mut page = blank_page();
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            100.0,
            100.0,
            200.0,
            100.0,
        ));
        let mut state = CanvasState {
            page_id: page.id.clone(),
            ..CanvasState::default()
        };
        state.selection.set_one(id);
        assert!(start_crop(&page, &mut state, id));
        let crop = state.cropping.as_mut().unwrap();
        crop.keep = Rect::from_min_max(pos2(200.0, 100.0), crop.keep.max);
        let mut editor = EditorState::default();
        assert!(finish_editing(&mut page, &mut state, &mut editor));
        assert!(state.cropping.is_none());
        assert_eq!(page.canvas.item(id).unwrap().width, 100.0);
        // A picture with no area is not cropped.
        let flat = page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            100.0,
            300.0,
            0.0,
            100.0,
        ));
        assert!(!start_crop(&page, &mut state, flat));
    }

    /// An empty page, as the unit tests below need one.
    fn blank_page() -> Page {
        crate::model::Page {
            id: "p".into(),
            section_id: String::new(),
            section_name: String::new(),
            section_color: String::new(),
            notebook_id: String::new(),
            notebook_name: String::new(),
            notebook_color: String::new(),
            title: String::new(),
            rule: crate::model::RuleLines::None,
            color: None,
            rule_color: None,
            title_hidden: false,
            pinned: false,
            trashed: false,
            word_count: 0,
            created_at: 0,
            updated_at: 0,
            canvas: CanvasDoc::default(),
            tags: Vec::new(),
            notice: None,
        }
    }

    /// The page's far corner is measured once and remembered while the page holds as many
    /// items and strokes; a new item, or clearing it (as a move does), measures it again.
    #[test]
    fn the_far_corner_is_remembered_until_the_page_changes() {
        let mut page = blank_page();
        let note = |y| {
            CanvasItem::new(
                CanvasKind::Text {
                    doc: Document::default(),
                },
                100.0,
                y,
                600.0,
                40.0,
            )
        };
        page.canvas.add(note(1000.0));
        let mut state = CanvasState::default();
        let far = cached_far(&mut state, &page);
        assert_eq!(far, content_far(&page));
        page.canvas.items[0].y = 2000.0;
        assert_eq!(cached_far(&mut state, &page), far, "remembered");
        state.far = None;
        assert_eq!(cached_far(&mut state, &page), content_far(&page));
        page.canvas.add(note(3000.0));
        assert_eq!(cached_far(&mut state, &page), pos2(700.0, 3040.0));
        state.repair_selection(&page);
        assert!(state.far.is_none(), "undo and redo measure it again");
    }

    #[test]
    fn the_vertical_scrollbar_can_run_down_the_left_edge() {
        let mut page = blank_page();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            100.0,
            5000.0,
            2400.0,
            40.0,
        ));
        let vp = Viewport {
            zoom: 1.0,
            hover: None,
        };
        let far = content_far(&page);
        let view = Rect::from_min_size(pos2(200.0, 0.0), vec2(800.0, 600.0));
        let extent = page_extent(far, view.size());
        let bar = |left: bool| {
            let bars = scrollbar_layout(view, extent, far, vp, left);
            assert_eq!(bars.len(), 2, "both axes have somewhere to go");
            let vertical = bars.iter().find(|bar| bar.vertical).unwrap().track;
            let horizontal = bars.iter().find(|bar| !bar.vertical).unwrap().track;
            (vertical, horizontal)
        };
        // the desktop notebook's default: down the right edge, the bottom bar stopping short of it.
        let (down, across) = bar(false);
        assert_eq!(down.right(), view.right());
        assert_eq!(down.width(), SCROLLBAR);
        assert_eq!(across.left(), view.left());
        assert_eq!(across.right(), view.right() - SCROLLBAR);
        // Moved to the left: the same bar mirrored, and the bottom bar starts after it.
        let (down, across) = bar(true);
        assert_eq!(down.left(), view.left());
        assert_eq!(down.width(), SCROLLBAR);
        assert_eq!(down.bottom(), view.bottom() - SCROLLBAR);
        assert_eq!(across.left(), view.left() + SCROLLBAR);
        assert_eq!(across.right(), view.right());
    }

    #[test]
    fn the_page_reaches_a_view_past_its_content_and_the_view_stays_on_it() {
        let mut page = blank_page();
        let view = vec2(800.0, 600.0);
        let extent = page_extent(content_far(&page), view);
        assert_eq!(extent.min, Pos2::ZERO);
        assert!(extent.width() >= 800.0 && extent.height() >= 600.0);
        // A page that fits in the view has no scrollbars.
        let vp = Viewport {
            zoom: 1.0,
            hover: None,
        };
        let shown = Rect::from_min_size(Pos2::ZERO, view);
        assert!(scrollbar_layout(shown, extent, content_far(&page), vp, false).is_empty());
        // Far content pushes the page out to it, plus one view.
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            100.0,
            5000.0,
            400.0,
            40.0,
        ));
        let far_corner = content_far(&page);
        let extent = page_extent(far_corner, view);
        assert_eq!(extent.max.y, 5040.0 + 600.0);
        // Only the axis with somewhere to go gets a bar: down, not across.
        let bars = scrollbar_layout(shown, extent, far_corner, vp, false);
        assert_eq!(bars.len(), 1);
        assert!(bars[0].vertical);
        assert_eq!(bars[0].track.bottom(), shown.bottom(), "no corner gap");
        // A view scrolled across has a bar to come back with.
        let across = shown.translate(vec2(200.0, 0.0));
        assert_eq!(
            scrollbar_layout(across, extent, far_corner, vp, false).len(),
            2
        );
        // Above or left of the corner snaps back, past the end too, and a view larger than the
        // page sits at the corner.
        let corner = clamp_view(Rect::from_min_size(pos2(-50.0, -80.0), view), extent);
        assert_eq!(corner.min, Pos2::ZERO);
        let far = clamp_view(Rect::from_min_size(pos2(0.0, 9000.0), view), extent);
        assert_eq!(far.max.y, extent.max.y);
        assert_eq!(far.size(), view, "clamping never resizes");
        let wide = clamp_view(
            Rect::from_min_size(pos2(30.0, 30.0), vec2(9000.0, 9000.0)),
            extent,
        );
        assert_eq!(wide.min, Pos2::ZERO);
        // Nothing is added above or left of the corner, and an item keeps room above it
        // for its grab bar.
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            -20.0,
            -5.0,
            100.0,
            40.0,
        ));
        assert_eq!(
            page.canvas.item(id).unwrap().rect().min,
            pos2(0.0, ITEM_TOP)
        );
    }

    #[test]
    fn arrow_keys_never_push_things_off_the_page() {
        let mut page = blank_page();
        let mut state = CanvasState::default();
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            3.0,
            ITEM_TOP + 4.0,
            100.0,
            60.0,
        ));
        page.canvas.ink.push(InkStroke {
            points: vec![[40.0, 50.0], [60.0, 80.0]],
            color: Rgb::INKS[0],
            width: 2.0,
            highlighter: false,
        });
        state.selection.set_one(id);
        // Shift+Left and Shift+Up stop at the edge (the item keeps its grab bar's room).
        assert!(nudge_selection(
            &mut page,
            &mut state,
            vec2(-10.0, -10.0),
            0.0
        ));
        assert_eq!(page.canvas.items[0].rect().min, pos2(0.0, ITEM_TOP));
        // Pressed again at the edge, nothing moves and no undo step is added.
        let steps = state.undo.len();
        assert!(!nudge_selection(
            &mut page,
            &mut state,
            vec2(-10.0, 0.0),
            5.0
        ));
        assert_eq!(state.undo.len(), steps);
        assert_eq!(page.canvas.items[0].rect().min, pos2(0.0, ITEM_TOP));
        // Ink stops at the page's edge too, and moves along with the item until either stops.
        state.selection = Selection {
            items: vec![id],
            strokes: vec![0],
        };
        assert!(nudge_selection(
            &mut page,
            &mut state,
            vec2(10.0, -10.0),
            10.0
        ));
        assert_eq!(
            page.canvas.ink[0].points[0],
            [50.0, 50.0],
            "the item has no room up"
        );
        state.selection = Selection {
            items: Vec::new(),
            strokes: vec![0],
        };
        assert!(nudge_selection(
            &mut page,
            &mut state,
            vec2(-80.0, -80.0),
            20.0
        ));
        assert_eq!(page.canvas.ink[0].points, vec![[0.0, 0.0], [20.0, 30.0]]);
    }

    #[test]
    fn rule_lines_fall_under_the_date_and_through_the_first_notes_baseline() {
        use crate::model::{GridSize, RuleLines, RuleSize};
        for rule in [
            RuleLines::Lines(RuleSize::Narrow),
            RuleLines::Lines(RuleSize::College),
            RuleLines::Lines(RuleSize::Standard),
            RuleLines::Lines(RuleSize::Wide),
            RuleLines::Grid(GridSize::Small),
            RuleLines::Grid(GridSize::VeryLarge),
        ] {
            let step = rule.spacing().unwrap();
            let first = rule_at_or_below(RULE_TOP, step);
            assert!(
                first >= RULE_TOP && first < RULE_TOP + step,
                "{rule:?}: {first}"
            );
            // Under the date, and on the line the first note's text sits on.
            assert!(first > DATE_TOP + DATE_SIZE + 2.0, "{rule:?}: {first}");
            let lines = ((FIRST_RULE - first) / step).round();
            assert!(
                (first + lines * step - FIRST_RULE).abs() < 0.01,
                "{rule:?}: {first}"
            );
            assert_eq!(rule_at_or_below(FIRST_RULE, step), FIRST_RULE);
            assert_eq!(rule_at_or_below(FIRST_RULE + 0.5, step), FIRST_RULE + step);
        }
    }

    #[test]
    fn pens_round_trip_through_the_setting() {
        let mut library = Library::open_in_memory().unwrap();
        assert_eq!(load_pens(&library), PenPreset::defaults(), "no setting yet");
        let mut pens = PenPreset::defaults();
        pens.push(PenPreset {
            highlighter: true,
            color: Rgb(0x77, 0x19, 0xaa),
            width: 4.0,
        });
        pens[1].width = 1.5;
        save_pens(&mut library, &pens).unwrap();
        assert_eq!(load_pens(&library), pens);
        // A broken or empty setting gives the desktop notebook's pens back.
        library.set_setting("pens", "{not json").unwrap();
        assert_eq!(load_pens(&library), PenPreset::defaults());
        library.set_setting("pens", "[]").unwrap();
        assert_eq!(load_pens(&library), PenPreset::defaults());
    }

    #[test]
    fn pen_thickness_reads_in_millimetres() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let pen = |highlighter, width| PenPreset {
            highlighter,
            color: Rgb(0, 0, 0),
            width,
        };
        assert_eq!(pen(false, 2.0).millimetres(), "0.5 mm");
        assert_eq!(pen(false, 4.0).millimetres(), "1 mm");
        // A highlighter draws four times as wide; a width off the table is a quarter mm a
        // point.
        assert_eq!(pen(true, 3.0).millimetres(), "2.8 mm");
        assert_eq!(pen(false, 6.0).millimetres(), "1.5 mm");
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        assert_eq!(pen(false, 1.5).millimetres(), "0,35\u{a0}mm");
        assert_eq!(pen(true, 1.0).millimetres(), "1\u{a0}mm");
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
    }

    /// Three overlapping rectangles, z 1, 2 and 3, as a page with them.
    fn stacked_page() -> (Page, Vec<u64>) {
        let mut page = blank_page();
        let ids = (1..=3)
            .map(|z| {
                let mut item = CanvasItem::new(
                    CanvasKind::Shape {
                        shape: ShapeKind::Rect,
                        stroke: Rgb(0, 0, 0),
                        fill: None,
                        label: String::new(),
                        flip_x: false,
                        flip_y: false,
                        thickness: None,
                    },
                    100.0 + z as f32 * 20.0,
                    100.0,
                    120.0,
                    80.0,
                );
                item.z = z;
                let id = item.id;
                page.canvas.items.push(item);
                id
            })
            .collect();
        (page, ids)
    }

    /// The page's items from the bottom of the stack to the top.
    fn stacking(page: &Page) -> Vec<u64> {
        let mut order: Vec<(i32, u64)> = page
            .canvas
            .items
            .iter()
            .map(|item| (item.z, item.id))
            .collect();
        order.sort_unstable();
        order.into_iter().map(|(_, id)| id).collect()
    }

    #[test]
    fn send_backward_moves_one_place() {
        let (mut page, ids) = stacked_page();
        let mut state = CanvasState::default();
        state.selection.set_one(ids[2]);
        assert!(restack_step(&mut page, &mut state, false));
        assert_eq!(stacking(&page), vec![ids[0], ids[2], ids[1]]);
        assert_eq!(state.undo_depth(), 1, "one undo step");
        // Once more puts it at the bottom, then it has nowhere further to go.
        assert!(restack_step(&mut page, &mut state, false));
        assert_eq!(stacking(&page), vec![ids[2], ids[0], ids[1]]);
        assert!(!restack_step(&mut page, &mut state, false));
        assert_eq!(state.undo_depth(), 2);
        // Bring Forward moves the bottom two up together, past the one above them.
        state.selection = Selection {
            items: vec![ids[2], ids[0]],
            strokes: Vec::new(),
        };
        assert!(restack_step(&mut page, &mut state, true));
        assert_eq!(stacking(&page), vec![ids[1], ids[2], ids[0]]);
        assert!(!restack_step(&mut page, &mut state, true), "already on top");
    }

    #[test]
    fn bring_forward_moves_one_place() {
        let (mut page, ids) = stacked_page();
        let mut state = CanvasState::default();
        state.selection.set_one(ids[0]);
        assert!(restack_step(&mut page, &mut state, true));
        assert_eq!(stacking(&page), vec![ids[1], ids[0], ids[2]]);
        assert_eq!(state.undo_depth(), 1, "one undo step");
        // Once more puts it on top, then it has nowhere further to go.
        assert!(restack_step(&mut page, &mut state, true));
        assert_eq!(stacking(&page), vec![ids[1], ids[2], ids[0]]);
        assert!(!restack_step(&mut page, &mut state, true));
        assert_eq!(state.undo_depth(), 2);
        // Two selected items apart each rise one place, past their unselected neighbour,
        // and keep their order among themselves.
        let (mut page, ids) = stacked_page();
        let top = page.canvas.add(CanvasItem::new(
            CanvasKind::image("a"),
            300.0,
            100.0,
            80.0,
            60.0,
        ));
        assert_eq!(stacking(&page), vec![ids[0], ids[1], ids[2], top]);
        state.selection = Selection {
            items: vec![ids[2], ids[0]],
            strokes: Vec::new(),
        };
        assert!(restack_step(&mut page, &mut state, true));
        assert_eq!(stacking(&page), vec![ids[1], ids[0], top, ids[2]]);
    }

    #[test]
    fn rotating_ink_swaps_its_extent() {
        let mut page = blank_page();
        page.canvas.ink.push(InkStroke {
            points: vec![[100.0, 300.0], [150.0, 300.0], [200.0, 300.0]],
            color: Rgb(0, 0, 0),
            width: 2.0,
            highlighter: false,
        });
        let mut state = CanvasState::default();
        state.selection.strokes = vec![0];
        let extent = |page: &Page| {
            let mut bounds = Rect::NOTHING;
            for point in &page.canvas.ink[0].points {
                bounds.extend_with(pos2(point[0], point[1]));
            }
            bounds
        };
        let before = extent(&page);
        assert!(rotate_selection(&mut page, &mut state, Rotation::Right));
        let after = extent(&page);
        assert_eq!((after.width(), after.height()), (0.0, 100.0));
        assert_eq!(after.center(), before.center(), "about the same centre");
        // Clockwise: the stroke's start, on the left, is now at the top.
        assert_eq!(page.canvas.ink[0].points[0], [150.0, 250.0]);
        assert_eq!(state.undo_depth(), 1, "one undo step");
        assert!(rotate_selection(&mut page, &mut state, Rotation::Left));
        assert_eq!(page.canvas.ink[0].points[0], [100.0, 300.0]);
        // Turned at the page's top edge, it is brought back onto the page.
        page.canvas.ink[0].points = vec![[0.0, 0.0], [100.0, 0.0]];
        assert!(rotate_selection(&mut page, &mut state, Rotation::Right));
        let after = extent(&page);
        assert_eq!(
            (after.min.y, after.height(), after.width()),
            (0.0, 100.0, 0.0)
        );
        // Nothing turnable selected: nothing happens.
        state.selection = Selection::default();
        assert!(!rotate_selection(&mut page, &mut state, Rotation::Right));
    }

    #[test]
    fn a_45_degree_turn_turns_ink_about_the_selections_centre() {
        let mut page = blank_page();
        page.canvas.ink.push(InkStroke {
            points: vec![[100.0, 300.0], [110.0, 300.0]],
            color: Rgb(0, 0, 0),
            width: 2.0,
            highlighter: false,
        });
        let mut state = CanvasState::default();
        state.selection.strokes = vec![0];
        assert!(ribbon_snapshot(&page, &state).turns_45, "ink turns by 45°");
        assert!(rotate_selection(&mut page, &mut state, Rotation::Right45));
        // About the pivot (105, 300), clockwise on screen as Right is: y grows downwards.
        let quarter = std::f32::consts::FRAC_1_SQRT_2;
        let [x, y] = page.canvas.ink[0].points[1];
        assert!((x - (105.0 + 5.0 * quarter)).abs() < 1e-3, "{x}");
        assert!((y - (300.0 + 5.0 * quarter)).abs() < 1e-3, "{y}");
        let stroke = &page.canvas.ink[0];
        assert_eq!((stroke.width, stroke.color), (2.0, Rgb(0, 0, 0)));
        assert_eq!(state.undo_depth(), 1, "one undo step");
        assert!(rotate_selection(&mut page, &mut state, Rotation::Left45));
        let [x, y] = page.canvas.ink[0].points[1];
        assert!((x - 110.0).abs() < 1e-3 && (y - 300.0).abs() < 1e-3);

        // A line turns with it: its ends move, and its box follows them.
        add_shape(
            &mut page,
            &mut state,
            ShapeKind::Line,
            pos2(200.0, 300.0),
            pos2(300.0, 300.0),
        );
        let line = state.selection.primary().unwrap();
        state.selection.strokes = vec![0];
        assert!(ribbon_snapshot(&page, &state).turns_45, "so do lines");
        assert!(rotate_selection(&mut page, &mut state, Rotation::Right45));
        let rect = page.canvas.item(line).unwrap().rect();
        assert!(rect.height() > 1.0, "the line now slants: {rect:?}");

        // With a boxed shape in the selection, it has no angle to keep: no 45° turns.
        add_shape(
            &mut page,
            &mut state,
            ShapeKind::Rect,
            pos2(400.0, 300.0),
            pos2(500.0, 360.0),
        );
        state.selection.strokes = vec![0];
        assert!(!ribbon_snapshot(&page, &state).turns_45);
        assert!(
            ribbon_snapshot(&page, &state).rotatable,
            "quarter turns still apply"
        );
    }

    #[test]
    fn flipping_a_triangle_points_it_down() {
        let mut page = blank_page();
        let mut state = CanvasState::default();
        add_shape(
            &mut page,
            &mut state,
            ShapeKind::Triangle,
            pos2(100.0, 100.0),
            pos2(220.0, 180.0),
        );
        let id = page.canvas.items[0].id;
        let flips = |page: &Page| match page.canvas.items[0].kind {
            CanvasKind::Shape { flip_x, flip_y, .. } => (flip_x, flip_y),
            _ => unreachable!(),
        };
        let rect = page.canvas.items[0].rect();
        assert_eq!(state.selection.primary(), Some(id));
        assert!(rotate_selection(
            &mut page,
            &mut state,
            Rotation::FlipVertical
        ));
        assert_eq!(flips(&page), (false, true));
        assert_eq!(page.canvas.items[0].rect(), rect, "it stays where it was");
        assert!(rotate_selection(
            &mut page,
            &mut state,
            Rotation::FlipVertical
        ));
        assert_eq!(flips(&page), (false, false));
        // A quarter turn keeps its middle and swaps its width and height.
        assert!(rotate_selection(&mut page, &mut state, Rotation::Right));
        let turned = page.canvas.items[0].rect();
        assert_eq!(turned.center(), rect.center());
        assert_eq!(turned.size(), vec2(rect.height(), rect.width()));
    }

    #[test]
    fn a_turned_arrow_points_the_new_way() {
        let mut page = blank_page();
        let mut state = CanvasState::default();
        // Drawn right to left.
        add_shape(
            &mut page,
            &mut state,
            ShapeKind::Arrow,
            pos2(300.0, 200.0),
            pos2(100.0, 200.0),
        );
        let ends = |page: &Page| {
            let item = &page.canvas.items[0];
            match item.kind {
                CanvasKind::Shape { flip_x, flip_y, .. } => {
                    connector_ends(item.rect(), flip_x, flip_y)
                }
                _ => unreachable!(),
            }
        };
        assert_eq!(ends(&page), (pos2(300.0, 200.0), pos2(100.0, 200.0)));
        // A clockwise turn: it now points up.
        assert!(rotate_selection(&mut page, &mut state, Rotation::Right));
        assert_eq!(ends(&page), (pos2(200.0, 300.0), pos2(200.0, 100.0)));
        // Mirrored top to bottom: it points the other way along the same line.
        assert!(rotate_selection(
            &mut page,
            &mut state,
            Rotation::FlipVertical
        ));
        assert_eq!(ends(&page), (pos2(200.0, 100.0), pos2(200.0, 300.0)));
    }

    #[test]
    fn lock_drawing_mode_keeps_the_shape_tool() {
        let mut page = blank_page();
        let mut state = CanvasState::default();
        let rect = CanvasTool::Shape(ShapeKind::Rect);
        state.tool = rect;
        state.lock_drawing = true;
        add_shape(
            &mut page,
            &mut state,
            ShapeKind::Rect,
            pos2(100.0, 100.0),
            pos2(200.0, 160.0),
        );
        assert_eq!(state.tool, rect, "still drawing shapes");
        state.lock_drawing = false;
        add_shape(
            &mut page,
            &mut state,
            ShapeKind::Rect,
            pos2(100.0, 300.0),
            pos2(200.0, 360.0),
        );
        assert_eq!(state.tool, CanvasTool::Select);
        assert_eq!(page.canvas.items.len(), 2);
    }

    #[test]
    fn horizontal_lines_stay_horizontal() {
        let mut page_state = CanvasState::default();
        let mut page = blank_page();
        add_shape(
            &mut page,
            &mut page_state,
            ShapeKind::Arrow,
            pos2(10.0, 50.0),
            pos2(210.0, 50.0),
        );
        let item = &page.canvas.items[0];
        assert_eq!((item.width, item.height), (200.0, 0.0));
        page.canvas.repair();
        assert_eq!(page.canvas.items[0].height, 0.0);
    }

    #[test]
    fn copied_files_paste_as_paths_and_get_an_icon() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("my notes.txt");
        std::fs::write(&file, "x").unwrap();
        let uri = format!("file://{}", file.display().to_string().replace(' ', "%20"));
        let nautilus = format!("x-special/nautilus-clipboard\ncopy\n{uri}\n");
        assert_eq!(pasted_paths(&nautilus), Some(vec![file.clone()]));
        assert_eq!(pasted_paths(&uri), Some(vec![file]));
        assert_eq!(pasted_paths("just some words"), None);
        assert_eq!(pasted_paths(&format!("{uri}\nand words")), None);
        assert_eq!(pasted_paths("file:///no/such/file.txt"), None);
        assert_eq!(percent_decode("caf%C3%A9%20au%20lait%"), "café au lait%");
        assert_eq!(file_glyph("Report.PDF"), icons::FILE_PDF);
        assert_eq!(file_glyph("archive.tar.gz"), icons::FILE_ZIP);
        assert_eq!(file_glyph("README"), icons::FILE);
    }
}

#[cfg(test)]
mod recording_undo_tests {
    use super::*;

    pub(super) fn canvas_page() -> Page {
        Page {
            id: "p".into(),
            section_id: String::new(),
            section_name: String::new(),
            section_color: String::new(),
            notebook_id: String::new(),
            notebook_name: String::new(),
            notebook_color: String::new(),
            title: String::new(),
            rule: crate::model::RuleLines::None,
            color: None,
            rule_color: None,
            title_hidden: false,
            pinned: false,
            trashed: false,
            word_count: 0,
            created_at: 0,
            updated_at: 0,
            canvas: CanvasDoc::default(),
            tags: Vec::new(),
            notice: None,
        }
    }

    /// A note created on the paper, with the caret in it (as `add_text` and the next
    /// frame's `canvas_contents` leave it).
    pub(super) fn new_note(page: &mut Page, state: &mut CanvasState) -> u64 {
        state.push_undo(&page.canvas);
        let id = page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::paragraph("")],
                },
            },
            40.0,
            40.0,
            TEXT_WIDTH,
            60.0,
        ));
        state.typing_in(&page.canvas, id);
        id
    }

    /// Typing in the note: its text becomes `text`, then the frame ends with the caret
    /// still there.
    fn type_into(page: &mut Page, state: &mut CanvasState, id: u64, text: &str) {
        let doc = page
            .canvas
            .item_mut(id)
            .and_then(CanvasItem::doc_mut)
            .unwrap();
        doc.blocks[0].text = crate::model::StyledText::new(text);
        state.typing_in(&page.canvas, id);
    }

    fn note_text(page: &Page, id: u64) -> String {
        page.canvas
            .item(id)
            .and_then(CanvasItem::doc)
            .map(Document::plain_text)
            .unwrap_or_default()
    }

    fn has_clip(page: &Page) -> bool {
        page.canvas
            .items
            .iter()
            .any(|item| matches!(item.kind, CanvasKind::Audio { .. }))
    }

    #[test]
    fn undoing_typing_done_while_recording_keeps_the_clip() {
        let mut page = canvas_page();
        let mut state = CanvasState::default();
        let note = new_note(&mut page, &mut state);
        type_into(&mut page, &mut state, note, "Before");
        // Insert → Audio while typing: the clip appears and the note keeps the caret.
        place_recording(&mut page, &mut state, "clip".into());
        state.typing_in(&page.canvas, note);
        type_into(&mut page, &mut state, note, "Before and an hour of notes");
        // Leaving the note (Esc, a click on the paper), then Ctrl+Z.
        state.active_text = None;
        finish_text_edit(&mut page, &mut state, note);
        assert!(state.undo(&mut page));
        assert_eq!(note_text(&page, note), "Before");
        assert!(has_clip(&page), "the first undo takes back the typing only");
        assert!(state.undo(&mut page));
        assert_eq!(note_text(&page, note), "Before");
        assert!(!has_clip(&page), "the second takes back the clip");
        assert!(state.undo(&mut page));
        assert!(page.canvas.item(note).is_none(), "the third, the note");
    }

    #[test]
    fn recording_from_a_blank_note_keeps_typing_and_clip_apart() {
        let mut page = canvas_page();
        let mut state = CanvasState::default();
        let note = new_note(&mut page, &mut state);
        place_recording(&mut page, &mut state, "clip".into());
        state.typing_in(&page.canvas, note);
        type_into(&mut page, &mut state, note, "Notes taken while it records");
        // Ctrl+Z on the page right away (the note still has the caret).
        assert!(state.undo(&mut page));
        assert_eq!(note_text(&page, note), "");
        assert!(has_clip(&page), "undo takes back the typing, not the clip");

        // A note created and typed in at once is still one undo step.
        let mut page = canvas_page();
        let mut state = CanvasState::default();
        let note = new_note(&mut page, &mut state);
        type_into(&mut page, &mut state, note, "Quick thought");
        state.active_text = None;
        finish_text_edit(&mut page, &mut state, note);
        assert!(state.undo(&mut page));
        assert!(page.canvas.items.is_empty(), "{:?}", page.canvas.items);
        assert!(!state.undo(&mut page));
    }

    #[test]
    fn a_recording_in_a_note_gets_its_length() {
        let mut library = Library::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut page = canvas_page();
        let mut state = CanvasState::default();
        let note = page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![
                        Block::paragraph("Lecture"),
                        Block::new(BlockKind::Audio {
                            attachment: "a".into(),
                            duration_ms: 0,
                        }),
                    ],
                },
            },
            40.0,
            40.0,
            TEXT_WIDTH,
            60.0,
        ));
        // A silent recorder, as `start_test_recording` makes one (no microphone here).
        let path = dir.path().join("a.wav");
        state.recording = Some(audio::Recorder::silent(&path, "a", &page.id));
        assert!(
            page_has_attachment(&page, "a"),
            "a block's clip is on the page"
        );
        assert!(!page_has_attachment(&page, "b"));

        assert_eq!(
            state.finish_recording(Some(&mut page), &mut EditorState::default(), &mut library),
            Ok(true)
        );
        assert_eq!(page.canvas.items.len(), 1, "no clip item is added");
        let blocks = &page
            .canvas
            .item(note)
            .and_then(CanvasItem::doc)
            .unwrap()
            .blocks;
        match &blocks[1].kind {
            BlockKind::Audio {
                attachment,
                duration_ms,
            } => {
                assert_eq!(attachment, "a");
                assert!(
                    *duration_ms > 0,
                    "the silent clip lasts a tenth of a second"
                );
            }
            kind => panic!("not a recording: {kind:?}"),
        }
        assert!(state.recording.is_none());
    }
}

#[cfg(test)]
mod ribbon_cmd_tests {
    use super::recording_undo_tests::{canvas_page, new_note};
    use super::*;
    use crate::model::{PageStyle, ParaAlign};
    use crate::table::TableCmd;

    /// A canvas page and what the app hands `run_ribbon_cmd` with it.
    struct Bench {
        page: Page,
        state: CanvasState,
        editor: EditorState,
        library: Library,
        ctx: egui::Context,
    }

    impl Bench {
        fn new() -> Self {
            Self {
                page: canvas_page(),
                state: CanvasState::default(),
                editor: EditorState::default(),
                library: Library::open_in_memory().unwrap(),
                ctx: egui::Context::default(),
            }
        }

        /// Runs `cmd` as `show` does when the app hands it a ribbon command: in a frame.
        fn run(&mut self, cmd: Cmd) -> bool {
            let mut cmd = Some(cmd);
            let mut toast = None;
            let mut changed = false;
            let Self {
                page,
                state,
                editor,
                library,
                ctx,
            } = self;
            ctx.run_ui(egui::RawInput::default(), |ui| {
                if let Some(cmd) = cmd.take() {
                    changed = run_ribbon_cmd(ui, cmd, page, state, editor, library, &mut toast);
                }
            })
            .drop_without_applying_deltas();
            changed
        }

        fn snapshot(&self) -> RibbonSnapshot {
            ribbon_snapshot(&self.page, &self.state)
        }
    }

    #[test]
    fn the_table_grid_puts_a_headerless_table_at_the_caret_or_in_a_new_note() {
        let mut bench = Bench::new();
        bench.state.visible = Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        // The table in note `note`, as the desktop notebook makes them: empty cells, no header row, and
        // the caret in its first cell. Returns the note's blocks and the table's size.
        let table = |bench: &Bench, note: u64| {
            let doc = bench
                .page
                .canvas
                .item(note)
                .and_then(CanvasItem::doc)
                .expect("a note")
                .clone();
            let (block, table) = doc
                .blocks
                .iter()
                .find_map(|block| match &block.kind {
                    BlockKind::Table(table) => Some((block.id, table.clone())),
                    _ => None,
                })
                .expect("a table in the note");
            assert!(
                !table.header,
                "the desktop notebook's tables have no header row"
            );
            assert!(table
                .rows
                .iter()
                .flatten()
                .all(|cell| cell.text.text.is_empty()));
            assert_eq!(
                bench.state.active_text,
                Some(note),
                "still typing in the note"
            );
            assert_eq!(bench.editor.active_block, Some(block));
            assert_eq!(bench.editor.active_cell, Some((0, 0)), "the caret is in A1");
            (doc.blocks, table.rows.len(), table.cols())
        };
        let typed_line =
            |block: &Block, text: &str| block.kind.is_textual() && block.text.text == text;

        // Nothing typed in: a new note at the last click holds the table, with a line
        // under it to keep typing on. No page table any more.
        let click = pos2(300.0, 400.0);
        bench.state.last_click = Some(click);
        assert!(bench.run(Cmd::InsertTable { rows: 2, cols: 4 }));
        assert_eq!(bench.page.canvas.items.len(), 1);
        let note = bench.page.canvas.items[0].clone();
        assert!(note.doc().is_some(), "{:?} is a note", note.kind);
        assert!(
            note.rect().contains(click),
            "{:?} starts at the click",
            note.rect()
        );
        let (blocks, rows, cols) = table(&bench, note.id);
        assert_eq!((rows, cols), (2, 4));
        assert_eq!(blocks.len(), 2);
        assert!(typed_line(&blocks[1], ""));
        // The note's own undo takes the table back, leaving the line it replaced.
        assert!(bench.run(Cmd::Undo));
        let doc = bench
            .page
            .canvas
            .item(note.id)
            .and_then(CanvasItem::doc)
            .unwrap();
        assert_eq!(doc.blocks.len(), 1);
        assert!(typed_line(&doc.blocks[0], ""));

        // Typing in a note: the table goes in after the line with the caret, in the same
        // note, however many columns it has.
        let note = new_note(&mut bench.page, &mut bench.state);
        let agenda = {
            let doc = bench
                .page
                .canvas
                .item_mut(note)
                .and_then(CanvasItem::doc_mut)
                .unwrap();
            doc.blocks[0].text.text = "Agenda".into();
            doc.blocks[0].id
        };
        bench.editor.focus_block(agenda, 6);
        let frame = bench.page.canvas.item(note).unwrap().rect();
        assert!(bench.run(Cmd::InsertTable { rows: 1, cols: 10 }));
        let (blocks, rows, cols) = table(&bench, note);
        assert_eq!((rows, cols), (1, 10));
        assert_eq!(blocks.len(), 3);
        assert!(typed_line(&blocks[0], "Agenda"));
        assert!(matches!(blocks[1].kind, BlockKind::Table(_)));
        assert!(typed_line(&blocks[2], ""));
        assert_eq!(bench.page.canvas.items.len(), 2, "no new note");
        assert_eq!(bench.page.canvas.item(note).unwrap().rect().min, frame.min);

        // Typing in an empty note: the table takes the empty line's place, and the note's
        // undo brings the line back.
        let empty = new_note(&mut bench.page, &mut bench.state);
        let line = bench
            .page
            .canvas
            .item(empty)
            .and_then(CanvasItem::doc)
            .unwrap()
            .blocks[0]
            .id;
        bench.editor.focus_block(line, 0);
        assert!(bench.run(Cmd::InsertTable { rows: 3, cols: 3 }));
        let (blocks, rows, cols) = table(&bench, empty);
        assert_eq!((rows, cols), (3, 3));
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0].kind, BlockKind::Table(_)));
        assert!(
            blocks.iter().all(|block| block.id != line),
            "the table took its place"
        );
        assert!(bench.run(Cmd::Undo));
        let doc = bench
            .page
            .canvas
            .item(empty)
            .and_then(CanvasItem::doc)
            .unwrap();
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].id, line);
    }

    #[test]
    fn insert_space_moves_only_what_is_past_its_line() {
        let mut page = canvas_page();
        let above = page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            40.0,
            100.0,
            200.0,
            40.0,
        ));
        let below = page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            40.0,
            400.0,
            200.0,
            40.0,
        ));
        page.canvas.ink.push(InkStroke {
            points: vec![[60.0, 520.0], [90.0, 540.0]],
            color: Rgb(0, 0, 0),
            width: 2.0,
            highlighter: false,
        });
        let mut space = SpaceDrag {
            line: pos2(10.0, 300.0),
            down: Some(true),
            items: Vec::new(),
            strokes: Vec::new(),
            room: 0.0,
        };
        collect_past_line(&page, &mut space, true);
        assert_eq!(space.items, vec![(below, pos2(40.0, 400.0))]);
        assert_eq!(space.strokes.len(), 1);
        assert_eq!(space.room, 100.0, "the note below is 100 past the line");

        shift_past_line(&mut page, &space, 80.0);
        assert_eq!(page.canvas.item(above).unwrap().y, 100.0);
        assert_eq!(page.canvas.item(below).unwrap().y, 480.0);
        assert_eq!(page.canvas.ink[0].points[0], [60.0, 600.0]);
        shift_past_line(&mut page, &space, -space.room);
        assert_eq!(page.canvas.item(below).unwrap().y, 300.0, "up to the line");
    }

    /// Draw's pen gallery: a pen is its tool, colour and width; Add Pen, Remove Pen and a
    /// colour or width picked while drawing with a pen change the gallery, which is saved
    /// and carries over from page to page.
    #[test]
    fn pen_presets_pick_their_pen_and_the_gallery_is_saved() {
        let mut bench = Bench::new();
        assert_eq!(bench.state.pens, PenPreset::defaults());
        let red = Rgb(0xe7, 0x12, 0x24);
        assert!(!bench.run(Cmd::PenPreset(2)), "the page does not change");
        assert_eq!(
            (
                bench.state.tool,
                bench.state.ink_color,
                bench.state.ink_width
            ),
            (CanvasTool::Pen, red, 2.0)
        );
        let snapshot = bench.snapshot();
        assert_eq!((snapshot.pen, snapshot.pens.len()), (Some(2), 6));
        // The yellow highlighter; a width picked while drawing with it retunes it.
        bench.run(Cmd::PenPreset(4));
        assert_eq!(
            (bench.state.tool, bench.state.ink_width),
            (CanvasTool::Highlighter, 3.0)
        );
        bench.run(Cmd::PenWidth(4.0));
        assert_eq!(bench.state.pens[4].width, 4.0);
        assert_eq!(load_pens(&bench.library), bench.state.pens, "saved");
        // Add Pen appends a pen and draws with it; Remove Pen takes it off again and
        // picks the one before it.
        let purple = Rgb(0x77, 0x19, 0xaa);
        bench.run(Cmd::AddPen {
            highlighter: true,
            color: purple,
        });
        assert_eq!((bench.state.pens.len(), bench.state.pen), (7, Some(6)));
        assert_eq!(
            (
                bench.state.tool,
                bench.state.ink_color,
                bench.state.ink_width
            ),
            (CanvasTool::Highlighter, purple, 3.0)
        );
        assert_eq!(load_pens(&bench.library).len(), 7);
        bench.run(Cmd::RemovePen);
        assert_eq!((bench.state.pens.len(), bench.state.pen), (6, Some(5)));
        assert_eq!(load_pens(&bench.library), bench.state.pens);
        // With another tool on, a colour no longer retunes the pen.
        let lime = bench.state.pens[5].color;
        bench.run(Cmd::Tool(CanvasTool::Lasso));
        bench.run(Cmd::Ink(purple));
        assert_eq!(bench.state.pens[5].color, lime);
        // The pens, the pen and Lock Drawing Mode carry over to the next page.
        assert!(!bench.run(Cmd::LockDrawing));
        assert!(bench.snapshot().lock_drawing);
        bench.state.reset("another page");
        assert_eq!((bench.state.pens.len(), bench.state.pen), (6, Some(5)));
        assert!(bench.state.lock_drawing);
        assert_eq!(bench.state.ink_color, purple);
        // The gallery always keeps one pen.
        for _ in 0..6 {
            bench.run(Cmd::RemovePen);
        }
        assert_eq!((bench.state.pens.len(), bench.state.pen), (1, Some(0)));
        assert_eq!(load_pens(&bench.library), bench.state.pens);
    }

    /// Draw › pen ▾ › Recent Colors: the colours picked outside the desktop notebook's 16, newest
    /// first, six at most, kept in the library beside the pens.
    #[test]
    fn recent_pen_colours_keep_six_new_ones() {
        let mut bench = Bench::new();
        let picked: Vec<Rgb> = (1..=8).map(|n| Rgb(n * 20, 0x34, 0x56)).collect();
        assert!(picked
            .iter()
            .all(|color| PEN_COLORS.iter().all(|(listed, _)| listed != color)));
        for &color in &picked {
            bench.run(Cmd::Ink(color));
        }
        // A colour of the palette is no recent colour.
        bench.run(Cmd::Ink(PEN_COLORS[2].0));
        let newest: Vec<Rgb> = picked.iter().rev().take(6).copied().collect();
        assert_eq!(bench.state.recent_colors, newest);
        assert_eq!(bench.snapshot().recent_colors, newest);
        // Picked again, a recent colour comes first and is not listed twice.
        bench.run(Cmd::Ink(picked[4]));
        assert_eq!(bench.state.recent_colors[0], picked[4]);
        assert_eq!(bench.state.recent_colors.len(), 6);
        assert_eq!(
            load_recent_colors(&bench.library),
            bench.state.recent_colors,
            "saved"
        );
        assert_eq!(
            bench
                .library
                .setting("recent_pen_colors")
                .unwrap()
                .as_deref(),
            Some("#643456,#a03456,#8c3456,#783456,#503456,#3c3456")
        );
        // They carry over to the next page, as the pens do.
        let kept = bench.state.recent_colors.clone();
        bench.state.reset("another page");
        assert_eq!(bench.state.recent_colors, kept);
    }

    /// Arrange and Rotate act on the selection, each in one undo step; Rotate only when
    /// it holds ink, a shape or a picture (see `draw_rotate_turns_the_selected_pictures`).
    #[test]
    fn arrange_and_rotate_act_on_the_selection() {
        let mut bench = Bench::new();
        let note = new_note(&mut bench.page, &mut bench.state);
        bench.state.active_text = None;
        let above = bench.page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            300.0,
            300.0,
            200.0,
            60.0,
        ));
        bench.state.selection.set_one(note);
        assert!(!bench.snapshot().rotatable, "a note does not turn");
        assert!(!bench.run(Cmd::Rotate(Rotation::Right)));
        let z = |bench: &Bench, id| bench.page.canvas.item(id).unwrap().z;
        assert!(z(&bench, note) < z(&bench, above));
        let depth = bench.state.undo_depth();
        assert!(bench.run(Cmd::BringForward));
        assert!(z(&bench, note) > z(&bench, above));
        assert!(bench.run(Cmd::SendBackward));
        assert!(z(&bench, note) < z(&bench, above));
        assert_eq!(bench.state.undo_depth(), depth + 2);
        bench.page.canvas.ink.push(InkStroke {
            points: vec![[100.0, 300.0], [200.0, 300.0]],
            color: Rgb(0, 0, 0),
            width: 2.0,
            highlighter: false,
        });
        bench.state.selection.strokes = vec![0];
        assert!(bench.snapshot().rotatable);
        assert!(bench.run(Cmd::Rotate(Rotation::Left)));
        assert_eq!(
            bench.page.canvas.ink[0].points,
            vec![[150.0, 350.0], [150.0, 250.0]]
        );
        assert_eq!(
            bench.page.canvas.item(note).unwrap().rect().min,
            pos2(40.0, 40.0),
            "the note stays where it was"
        );
    }

    /// Draw › Rotate turns pictures as the Picture tab does, in the desktop notebook: a page picture
    /// in its own box, together with the ink and shapes beside it in one undo step, and
    /// the picture clicked in a note.
    #[test]
    fn draw_rotate_turns_the_selected_pictures() {
        let mut bench = Bench::new();
        let picture = bench.page.canvas.add(CanvasItem::new(
            CanvasKind::image("picture"),
            100.0,
            100.0,
            200.0,
            100.0,
        ));
        let meta = |bench: &Bench| match &bench.page.canvas.item(picture).unwrap().kind {
            CanvasKind::Image { meta, .. } => meta.clone(),
            _ => unreachable!(),
        };
        bench.state.selection.set_one(picture);
        let snapshot = bench.snapshot();
        assert_eq!(snapshot.single_kind, Some(SingleKind::Picture));
        assert!(snapshot.rotatable, "a picture turns");
        let depth = bench.state.undo_depth();
        assert!(bench.run(Cmd::Rotate(Rotation::Right)));
        let rect = bench.page.canvas.item(picture).unwrap().rect();
        assert_eq!(
            rect.size(),
            vec2(100.0, 200.0),
            "a quarter turn swaps the box"
        );
        assert_eq!(rect.center(), pos2(200.0, 150.0), "around its centre");
        assert_eq!(meta(&bench).turns, 1);
        assert_eq!(bench.state.undo_depth(), depth + 1);
        // With a stroke beside it, both turn, in one step.
        bench.page.canvas.ink.push(InkStroke {
            points: vec![[400.0, 300.0], [500.0, 300.0]],
            color: Rgb(0, 0, 0),
            width: 2.0,
            highlighter: false,
        });
        bench.state.selection.strokes = vec![0];
        let before = bench.page.canvas.ink[0].points.clone();
        assert!(bench.run(Cmd::Rotate(Rotation::FlipHorizontal)));
        assert_ne!(bench.page.canvas.ink[0].points, before, "the ink turned");
        let flipped = meta(&bench);
        assert_eq!((flipped.turns, flipped.flip), (3, true));
        assert_eq!(bench.state.undo_depth(), depth + 2, "one step for both");
        assert!(bench.run(Cmd::Undo));
        assert_eq!(meta(&bench).turns, 1);
        assert_eq!(bench.page.canvas.ink[0].points, before);

        // A picture in a note, clicked: it turns in place (its note keeps its box).
        let image = Block::new(BlockKind::Image {
            attachment: "picture".into(),
            width: 1.0,
            meta: PictureMeta::default(),
        });
        let block = image.id;
        let note = bench.page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![Block::paragraph("Diagram"), image],
                },
            },
            40.0,
            400.0,
            300.0,
            200.0,
        ));
        bench.state.selection.clear();
        bench.state.active_text = Some(note);
        bench.editor.active_block = Some(block);
        assert!(
            !bench.snapshot().rotatable,
            "the page has nothing selected to turn"
        );
        assert!(bench.run(Cmd::Rotate(Rotation::Left)));
        let turns = match &bench.page.canvas.item(note).unwrap().doc().unwrap().blocks[1].kind {
            BlockKind::Image { meta, .. } => meta.turns,
            _ => unreachable!(),
        };
        assert_eq!(turns, 3);
    }

    #[test]
    fn ribbon_commands_run_on_the_page_and_the_snapshot_follows() {
        let mut bench = Bench::new();
        let snapshot = bench.snapshot();
        assert!(!snapshot.can_undo && !snapshot.can_redo);
        assert_eq!(snapshot.selection_count, 0);
        assert_eq!(snapshot.single_kind, None);

        // Fill recolours the selected shapes in one undo step; notes take no fill.
        let note = bench.page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            0.0,
            0.0,
            240.0,
            60.0,
        ));
        bench.state.selection.set_one(note);
        assert!(!bench.snapshot().fillable, "a note container has no fill");
        let shape = bench.page.canvas.add(CanvasItem::new(
            CanvasKind::Shape {
                shape: ShapeKind::Rect,
                stroke: Rgb::INKS[0],
                fill: None,
                label: String::new(),
                flip_x: false,
                flip_y: false,
                thickness: None,
            },
            0.0,
            0.0,
            240.0,
            150.0,
        ));
        bench.state.selection.set_one(shape);
        let snapshot = bench.snapshot();
        assert!(snapshot.fillable);
        assert_eq!(snapshot.single_kind, Some(SingleKind::Other));
        let depth = bench.state.undo_depth();
        let fill = Rgb::HIGHLIGHTERS[2];
        assert!(bench.run(Cmd::Fill(fill)));
        assert!(matches!(
            bench.page.canvas.item(shape).unwrap().kind,
            CanvasKind::Shape { fill: Some(color), .. } if color == fill
        ));
        assert_eq!(bench.state.undo_depth(), depth + 1);

        // With no note being typed in, Undo and Redo are the page's.
        let filled = |bench: &Bench| match bench.page.canvas.item(shape).unwrap().kind {
            CanvasKind::Shape { fill, .. } => fill,
            ref kind => panic!("not a shape: {kind:?}"),
        };
        assert!(bench.snapshot().can_undo);
        assert!(bench.run(Cmd::Undo));
        assert_eq!(filled(&bench), None);
        assert!(bench.snapshot().can_redo);
        assert!(bench.run(Cmd::Redo));
        assert_eq!(filled(&bench), Some(fill));

        // Delete takes the selection off the page.
        assert!(bench.run(Cmd::Delete));
        assert!(bench.page.canvas.item(shape).is_none());
        assert_eq!(bench.snapshot().selection_count, 0);

        // View › Zoom moves the view and never changes the page: no save, no undo step,
        // and the page keeps its style (plain white, no rule lines).
        let depth = bench.state.undo_depth();
        assert!(!bench.run(Cmd::ZoomIn));
        assert!(!bench.run(Cmd::ZoomOut));
        assert_eq!(bench.state.undo_depth(), depth);
        assert_eq!(PageStyle::of(&bench.page), PageStyle::default());

        // Draw: tools and pens change no page, and a pen from Draw's gallery takes up the
        // pen tool (the desktop notebook has no Insert › Drawing).
        assert!(!bench.run(Cmd::Tool(CanvasTool::Lasso)));
        assert_eq!(bench.snapshot().tool, CanvasTool::Lasso);
        let large = EraserMode::Point(EraserSize::Large);
        bench.run(Cmd::Eraser(large));
        let (widest, _) = PEN_THICKNESS[4];
        bench.run(Cmd::PenWidth(widest));
        let snapshot = bench.snapshot();
        assert_eq!(
            (snapshot.tool, snapshot.eraser),
            (CanvasTool::Eraser, large)
        );
        assert_eq!(snapshot.ink_width, widest);
        assert!(!bench.run(Cmd::PenPreset(0)));
        assert_eq!(bench.state.tool, CanvasTool::Pen);

        // While a note is typed in, text commands format it and Undo is the note's own.
        let note = new_note(&mut bench.page, &mut bench.state);
        let depth = bench.state.undo_depth();
        let align = |bench: &Bench| {
            bench
                .page
                .canvas
                .item(note)
                .and_then(CanvasItem::doc)
                .unwrap()
                .blocks[0]
                .align
        };
        assert!(bench.run(Cmd::Text(ToolbarCommand::Align(ParaAlign::Center))));
        assert_eq!(align(&bench), ParaAlign::Center);
        assert!(bench.run(Cmd::Undo));
        assert_eq!(align(&bench), ParaAlign::Left);
        assert_eq!(
            bench.state.undo_depth(),
            depth,
            "the page's undo is left alone"
        );

        // The Table tab's commands edit the table holding the note's caret, one note undo
        // step each; with the caret outside a table they do nothing.
        let doc_id = format!("{}:{}", bench.page.id, note);
        let doc = bench
            .page
            .canvas
            .item_mut(note)
            .and_then(CanvasItem::doc_mut)
            .unwrap();
        bench.editor.focus_block(doc.blocks[0].id, 0);
        editor::insert_table(doc, &doc_id, &mut bench.editor, 2, 2, 0.0);
        let grid = |bench: &Bench| {
            let doc = bench.page.canvas.item(note).and_then(CanvasItem::doc)?;
            doc.blocks.iter().find_map(|block| match &block.kind {
                BlockKind::Table(table) => Some((table.rows.len(), table.cols())),
                _ => None,
            })
        };
        assert_eq!(grid(&bench), Some((2, 2)));
        assert!(bench.run(Cmd::TableEdit(TableCmd::InsertBelow)));
        assert!(bench.run(Cmd::TableEdit(TableCmd::InsertRight)));
        assert_eq!(grid(&bench), Some((3, 3)));
        assert_eq!(bench.editor.active_cell, Some((1, 1)), "the caret follows");
        assert!(
            !bench.run(Cmd::TableEdit(TableCmd::SelectTable)),
            "selecting changes nothing"
        );
        assert!(bench.run(Cmd::TableEdit(TableCmd::DeleteRows)));
        assert_eq!(grid(&bench), None, "every row went, so the table did");
        assert!(!bench.run(Cmd::TableEdit(TableCmd::InsertBelow)));
        assert!(bench.run(Cmd::Undo));
        assert_eq!(grid(&bench), Some((3, 3)));
        assert_eq!(
            bench.state.undo_depth(),
            depth,
            "the page's undo is left alone"
        );

        // The app's own commands do nothing here.
        assert!(!bench.run(Cmd::Backstage));
        assert!(!bench.run(Cmd::StopRecording));
    }

    /// the desktop notebook's insert rule: while a note is typed in, what Insert puts goes in at its
    /// caret (the note's own undo); otherwise into a new note at the last place the paper
    /// was clicked, or near the middle of the view once that is out of sight. Cut and Copy
    /// with no note typed in take the selected items.
    #[test]
    fn inserting_goes_in_at_the_caret_or_into_a_new_note_at_the_last_click() {
        let mut bench = Bench::new();
        // A page the library knows, for the recording's attachment.
        let notebook = bench.library.create_notebook("N", "#745cf5").unwrap();
        let section = bench
            .library
            .create_section(&notebook.id, "S", "#745cf5")
            .unwrap();
        bench.page.id = bench
            .library
            .create_canvas_page(&section.id, "")
            .unwrap()
            .id;
        bench.state.visible = Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        let blocks = |bench: &Bench, id: u64| {
            bench
                .page
                .canvas
                .item(id)
                .and_then(CanvasItem::doc)
                .unwrap()
                .blocks
                .clone()
        };

        // A symbol with no note typed in: a new note where the paper was clicked.
        bench.state.last_click = Some(pos2(300.0, 400.0));
        assert!(bench.run(Cmd::Symbol('π')));
        let first = bench.state.active_text.expect("a new note being typed in");
        let note = bench.page.canvas.item(first).unwrap();
        assert_eq!(note.x, 300.0 - NOTE_PAD_X, "its text starts at the click");
        assert!(note.y < 400.0 && note.y + 40.0 > 400.0, "{}", note.y);
        assert_eq!(blocks(&bench, first)[0].text.text, "π");
        assert_eq!(bench.state.undo_depth(), 1, "one page step: the new note");

        // Insert › Equation while typing in it: an equation block after the paragraph,
        // its source field taking the keyboard, in the note's own history.
        assert!(bench.run(Cmd::Equation));
        let doc = blocks(&bench, first);
        assert_eq!(doc.len(), 3, "{doc:?}");
        assert!(matches!(&doc[1].kind, BlockKind::Equation { source, .. } if source.is_empty()));
        assert_eq!(doc[2].kind, BlockKind::Paragraph);
        assert_eq!(bench.editor.equation_block, Some(doc[1].id));
        assert_eq!(
            bench.state.undo_depth(),
            1,
            "the page's history is left alone"
        );

        // Recording while typing puts its card at the caret too, and the note widens round
        // it; stopping gives the card its length.
        bench.editor.equation_block = None;
        bench.editor.active_block = Some(doc[2].id);
        assert!(bench.run(Cmd::RecordAudio));
        let doc = blocks(&bench, first);
        assert!(
            matches!(doc[2].kind, BlockKind::Audio { duration_ms: 0, .. }),
            "{doc:?}"
        );
        let note = bench.page.canvas.item(first).unwrap();
        assert!(note.width >= AUDIO_SIZE.x + 2.0 * NOTE_PAD_X && !note.auto_width);
        assert!(bench.run(Cmd::RecordAudio));
        assert!(matches!(
            blocks(&bench, first)[2].kind,
            BlockKind::Audio { duration_ms, .. } if duration_ms > 0
        ));

        // The last click scrolled out of view: the date goes near the middle of the view.
        bench.state.active_text = None;
        bench.state.last_click = Some(pos2(2000.0, 2000.0));
        assert!(bench.run(Cmd::Date));
        let second = bench.state.active_text.expect("another new note");
        assert_ne!(second, first);
        let note = bench.page.canvas.item(second).unwrap();
        assert_eq!(note.x, 400.0 - TEXT_WIDTH / 2.0 - NOTE_PAD_X);
        assert_eq!(blocks(&bench, second)[0].text.text, editor::today());
        assert_eq!(bench.state.undo_depth(), 2);

        // With no note typed in, Copy and Cut take the selected items.
        bench.state.active_text = None;
        bench.state.selection.set_one(first);
        assert!(!bench.run(Cmd::Copy), "copying changes nothing");
        assert!(bench.page.canvas.item(first).is_some());
        assert!(bench.run(Cmd::Cut));
        assert!(bench.page.canvas.item(first).is_none());
        assert!(bench.page.canvas.item(second).is_some());
    }

    /// View › Zoom Level reaches the ends of the zoom range from any zoom: 20 % from
    /// 20.095 % used to round past the range and do nothing.
    #[test]
    fn the_zoom_level_reaches_the_ends_of_the_range() {
        for (from, percent) in [(0.20095f32, 20u16), (1.0, 20), (0.87, 400), (3.9, 400)] {
            let mut bench = Bench::new();
            let view = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
            bench.state.visible = Some(view);
            bench.state.zoom = from;
            bench.run(Cmd::Zoom(percent));
            let shown = bench.state.scene_rect.expect("the view zoomed");
            let zoom = from * view.width() / shown.width();
            assert!(
                (zoom - f32::from(percent) / 100.0).abs() < 1e-4,
                "{from} → {percent} %: {zoom}"
            );
            assert_eq!(shown.center(), view.center());
        }
    }

    /// View › Page Width zooms so the page's widest content, with the page's left margin
    /// on both sides, fills the page area, and keeps the top of the view; on a page with
    /// little on it, the header's rule is what fills it.
    #[test]
    fn page_width_fits_the_content_across_the_page_area() {
        let mut bench = Bench::new();
        // Shown at 100 % in an 800 × 600 page area, scrolled 100 down.
        bench.state.visible = Some(Rect::from_min_size(pos2(0.0, 100.0), vec2(800.0, 600.0)));
        bench.state.zoom = 1.0;
        bench.page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            PAGE_LEFT,
            300.0,
            1200.0,
            40.0,
        ));
        assert!(!bench.run(Cmd::PageWidth), "only the view changes");
        let zoom = 800.0 / (PAGE_LEFT + 1200.0 + PAGE_LEFT);
        let view = bench.state.scene_rect.expect("a view");
        assert_eq!(view.min, pos2(0.0, 100.0));
        assert!(
            (view.size() - vec2(800.0, 600.0) / zoom).length() < 0.01,
            "{view:?}"
        );

        // Nothing but the header, seen at 200 %: its rule and the margins fill the area.
        let mut bench = Bench::new();
        bench.state.visible = Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)));
        bench.state.zoom = 2.0;
        bench.run(Cmd::PageWidth);
        let zoom = 800.0 / (PAGE_LEFT + TITLE_RULE_MIN + PAGE_LEFT);
        let view = bench.state.scene_rect.expect("a view");
        assert!(
            (view.size() - vec2(800.0, 600.0) / zoom).length() < 0.01,
            "{view:?}"
        );
    }
}
