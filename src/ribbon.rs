//! The Office-style ribbon, as the desktop notebook shows it: tabs, labelled groups and controls,
//! drawn in the Classic (large buttons, group names) or Simplified (one row) layout and
//! scaled to the window by measuring in the current language. It knows nothing about
//! pages: a `RibbonHost` answers what is enabled, checked and current, and draws the
//! contents of menus; `show` returns the command picked, which the app runs.

use std::borrow::Cow;

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Id, Painter, Pos2, Rect, Response,
    RichText, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
};

use crate::canvas::{pen_label, rule_colors, CanvasTool, EraserMode, PenPreset, RULE_MARGIN};
use crate::db::Library;
use crate::editor::{self, Mark, TagCommand, ToolbarCommand};
use crate::i18n::t;
use crate::icons;
use crate::model::{BlockKind, PageColor, ParaAlign, Rgb, RuleLines, ShapeKind};
use crate::tags::TagGallery;
use crate::theme::{self, Tokens};
use crate::widgets;

// Classic metrics (the desktop notebook's ribbon at 100 %, in logical px).
/// The Classic card: the control area, the group-label row and 4 px padding above and below.
pub const CLASSIC_HEIGHT: f32 = 96.0;
const CONTROLS_HEIGHT: f32 = 66.0;
const LABEL_ROW: f32 = 18.0;
/// The Simplified card: one row of controls.
pub const SIMPLIFIED_HEIGHT: f32 = 44.0;
pub const TAB_ROW: f32 = 30.0;
const SMALL_HEIGHT: f32 = 22.0;
const LARGE_GLYPH: f32 = 24.0;
const SMALL_GLYPH: f32 = 16.0;
/// Undo and Redo's glyphs, the desktop notebook's size.
const UNDO_GLYPH: f32 = 20.0;
const GROUP_PAD: f32 = 6.0;
const COLUMN_GAP: f32 = 2.0;
/// A gallery shows this many columns of three in the band; the rest is behind its "▾".
const GALLERY_COLUMNS: usize = 3;
/// A Large label wider than this breaks onto two lines.
const LARGE_LINE: f32 = 64.0;
/// An Icon control's width, and what a menu arrow adds to it.
const ICON_WIDTH: f32 = 24.0;
const ICON_ARROW: f32 = 10.0;
/// What a menu arrow adds to a Small control.
const SMALL_ARROW: f32 = 12.0;
/// A Split's arrow part: the right of a Small or Icon control, the bottom of a Large one.
const SPLIT_ARROW: f32 = 12.0;
const LARGE_SPLIT_ARROW: f32 = 22.0;
const SWATCH: f32 = 20.0;
const SWATCH_GAP: f32 = 2.0;
/// A gallery's icon item (a pen, a shape), and its glyph.
const ICON_ITEM: Vec2 = Vec2::new(24.0, 22.0);
const ICON_ITEM_GLYPH: f32 = 18.0;
/// The gallery's "▾" column, which opens its full menu.
const MORE_COLUMN: f32 = 14.0;
/// A pen of Draw's gallery as a picture: the desktop notebook draws its pens and highlighters as
/// little marker drawings in one row.
const PEN_CELL: Vec2 = Vec2::new(34.0, 56.0);
/// The most pen pictures the band shows; the pen in hand always among them.
const PENS_SHOWN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RibbonTab {
    File,
    Home,
    Insert,
    Draw,
    History,
    Review,
    View,
    Help,
    Table,
    Picture,
    Equation,
    Recording,
}

impl RibbonTab {
    /// The tabs always shown, in the desktop notebook's order. Contextual tabs follow them when the
    /// host asks for them.
    pub const MAIN: [RibbonTab; 8] = [
        RibbonTab::File,
        RibbonTab::Home,
        RibbonTab::Insert,
        RibbonTab::Draw,
        RibbonTab::History,
        RibbonTab::Review,
        RibbonTab::View,
        RibbonTab::Help,
    ];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            RibbonTab::File => t!("ribbon-tab-file"),
            RibbonTab::Home => t!("ribbon-tab-home"),
            RibbonTab::Insert => t!("ribbon-tab-insert"),
            RibbonTab::Draw => t!("ribbon-tab-draw"),
            RibbonTab::History => t!("ribbon-tab-history"),
            RibbonTab::Review => t!("ribbon-tab-review"),
            RibbonTab::View => t!("ribbon-tab-view"),
            RibbonTab::Help => t!("ribbon-tab-help"),
            RibbonTab::Table => t!("ribbon-tab-table"),
            RibbonTab::Picture => t!("ribbon-tab-picture"),
            RibbonTab::Equation => t!("ribbon-tab-equation"),
            RibbonTab::Recording => t!("ribbon-tab-recording"),
        }
    }

    pub fn contextual(self) -> bool {
        matches!(
            self,
            RibbonTab::Table | RibbonTab::Picture | RibbonTab::Equation | RibbonTab::Recording
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonLayout {
    Classic,
    Simplified,
}

impl RibbonLayout {
    pub fn code(self) -> &'static str {
        match self {
            RibbonLayout::Classic => "classic",
            RibbonLayout::Simplified => "simplified",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "simplified" => RibbonLayout::Simplified,
            _ => RibbonLayout::Classic,
        }
    }
}

/// View › Tabs Layout: the desktop notebook's two ways of listing the sections, in the navigation pane
/// (Vertical) or as tabs across the top of the page (Horizontal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsLayout {
    Vertical,
    Horizontal,
}

impl TabsLayout {
    pub const ALL: [TabsLayout; 2] = [TabsLayout::Vertical, TabsLayout::Horizontal];

    /// The code kept in the `tabs_layout` setting.
    pub fn code(self) -> &'static str {
        match self {
            TabsLayout::Vertical => "vertical",
            TabsLayout::Horizontal => "horizontal",
        }
    }

    /// Reads a stored code; anything unknown is Vertical, the desktop notebook's default.
    pub fn from_code(code: &str) -> Self {
        match code {
            "horizontal" => TabsLayout::Horizontal,
            _ => TabsLayout::Vertical,
        }
    }

    pub fn label(self) -> Cow<'static, str> {
        match self {
            TabsLayout::Vertical => t!("ribbon-vertical-tabs"),
            TabsLayout::Horizontal => t!("ribbon-horizontal-tabs"),
        }
    }
}

/// Every command the ribbon can give. The app routes each one (see `FolioApp::run_cmd`):
/// page commands run in the canvas, the rest in the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Formatting for the text being typed in a note.
    Text(ToolbarCommand),
    /// Home › Font Color › More Colors…: a dialog to pick any colour for the text.
    MoreFontColors,
    /// Draw › pen ▾ › More Colors…: a dialog to pick any colour for the pen.
    MorePenColors,
    /// Home › Numbering ▾ › Customize Numbering…: a dialog for the numbering style and the
    /// number the paragraph at the caret starts at.
    CustomizeNumbering,
    /// Home › Paragraph Alignment ▾ › Paragraph Spacing Options…: a dialog for the space
    /// before and after the paragraph and its least line height.
    ParagraphSpacing,
    Undo,
    Redo,
    /// Home › Delete: the selected text or the paragraph being typed in, else the selected
    /// items.
    Delete,
    // Home
    Paste(PasteMode),
    Cut,
    Copy,
    /// Copies the formatting at the caret onto the next selection (P6 makes it work).
    FormatPainter,
    // Insert
    Picture,
    Attachment,
    RecordAudio,
    Timeline,
    Equation,
    /// Insert › Equation ▾: one of `math::READY_EQUATIONS`, by index.
    ReadyEquation(u8),
    Date,
    Link,
    InsertTable {
        rows: u8,
        cols: u8,
    },
    /// Insert › File Printout: each page of a PDF as a picture.
    FilePrintout,
    Time,
    DateTime,
    Symbol(char),
    /// Insert › Symbol ▾ › More Symbols…: the Symbol dialog, its sets and its search.
    MoreSymbols,
    PageTemplate(crate::templates::PageTemplate),
    // Draw
    Tool(CanvasTool),
    Eraser(EraserMode),
    InkToShape,
    Ink(Rgb),
    PenWidth(f32),
    Shape(ShapeKind),
    Fill(Rgb),
    BringToFront,
    SendToBack,
    PenPreset(usize),
    AddPen {
        highlighter: bool,
        color: Rgb,
    },
    RemovePen,
    LockDrawing,
    BringForward,
    SendBackward,
    Rotate(crate::canvas::Rotation),
    // Contextual tabs
    /// A Table tab command, for the table holding the caret.
    TableEdit(crate::table::TableCmd),
    /// A Picture tab command, for the selected page picture or the picture clicked in a
    /// note.
    PictureEdit(crate::canvas::PictureCmd),
    EditEquation,
    CopyLatex,
    PauseRecording,
    StopRecording,
    PlayPause,
    SaveAs,
    // File, History, Review, View and Help
    /// File (also Alt+F): opens Backstage, the full-window view, at Info.
    Backstage,
    /// History › Page Versions: opens the versions pane, or closes it when it is open.
    PageVersions,
    RecycleBin,
    /// History › Notebook Recycle Bin ▾ › Empty Recycle Bin (after a confirmation).
    EmptyRecycleBin,
    /// History › Page Versions ▾: deletes every kept version of the page, the selected
    /// section's pages or the selected notebook's (after a confirmation).
    DeleteVersions(VersionScope),
    /// History › Recent Edits: the pages edited in a range, listed in the results pane.
    RecentEdits(EditsRange),
    /// Review › Spelling (F7): opens the Spelling pane at the page's first misspelling, or
    /// moves an open one on to the next.
    Spelling,
    ProofingLanguage(String),
    NormalView,
    FullPageView,
    Reader,
    /// View › Tabs Layout (also Options › Display): sets and stores the layout.
    TabsLayout(TabsLayout),
    /// View › Switch Background (a toggle): the page in the other theme's colours.
    SwitchBackground,
    /// View › New Quick Note (also Ctrl+Shift+M): a new page in the Quick Notes section.
    NewQuickNote,
    /// View › Page Color: `None` is No Color.
    PageColor(Option<PageColor>),
    /// View › Page Color › More Colors…: a dialog to pick any colour for the page.
    MorePageColors,
    /// View › Rule Lines.
    RuleLines(RuleLines),
    /// View › Rule Lines › Rule Line Color: `None` is Automatic (the desktop notebook's blue).
    RuleLineColor(Option<Rgb>),
    /// View › Rule Lines › Always Create Pages with Rule Lines (a toggle).
    AlwaysRuleLines,
    /// View › Hide Page Title (a toggle).
    HidePageTitle,
    ZoomOut,
    ZoomIn,
    /// View › 100%.
    Zoom100,
    /// View › Zoom Level: the page at this percentage (20–400).
    Zoom(u16),
    /// View › Page Width.
    PageWidth,
    Shortcuts,
    /// Help › About Folio: the version, what Folio is and where its licences are listed.
    About,
    // The ribbon itself
    RibbonLayout(RibbonLayout),
    CollapseRibbon(bool),
}

/// A menu the host draws: what a Menu, Split arrow or Combo opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuId {
    Fonts,
    Sizes,
    Script,
    Highlight,
    FontColor,
    Align,
    Styles,
    Tags,
    Eraser,
    Shapes,
    Fill,
    Arrange,
    Proofing,
    /// View › Page Color: `page_color_menu`.
    PageColor,
    /// View › Rule Lines: `rule_lines_menu`.
    RuleLines,
    /// Home › Paste ▾ (named Paste Options): `editor::paste_menu`.
    Paste,
    /// Home › Bullets ▾ and Numbering ▾: the marker libraries.
    Bullets,
    Numbering,
    /// Insert › Table: the 10 × 8 grid picker.
    TableGrid,
    PageTemplates,
    /// Insert › Equation ▾: `editor::equation_gallery`.
    EquationGallery,
    Symbols,
    /// Draw › the pens gallery's "▾": the current pen's thickness and colour.
    PenSettings,
    AddPen,
    Rotate,
    /// Table › Shading: `editor::shading_menu`.
    Shading,
    /// Table › Sort: the two sorts and Header Row.
    TableSort,
    /// Picture › Order: Bring to Front and Send to Back.
    PictureOrder,
    /// Picture › Rotate: the quarter turns and the flips.
    PictureRotate,
    /// History › Page Versions ▾: the pane, then deleting versions.
    PageVersions,
    /// History › Notebook Recycle Bin ▾: the bin, then Empty Recycle Bin.
    RecycleBin,
    /// History › Recent Edits: one row per `EditsRange`.
    RecentEdits,
    /// View › Tabs Layout: Vertical Tabs and Horizontal Tabs, the current one checked.
    TabsLayout,
    /// View › Zoom Level: the desktop notebook's percentages, the current one checked, and a box to
    /// type one.
    Zoom,
}

/// History › Recent Edits: how far back the listed edits go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditsRange {
    Today,
    Yesterday,
    Days7,
    Days14,
    Days30,
    Months3,
    Months6,
    All,
}

impl EditsRange {
    /// the desktop notebook's order: the menu's rows from the newest edits to every page.
    pub const ALL: [EditsRange; 8] = [
        EditsRange::Today,
        EditsRange::Yesterday,
        EditsRange::Days7,
        EditsRange::Days14,
        EditsRange::Days30,
        EditsRange::Months3,
        EditsRange::Months6,
        EditsRange::All,
    ];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            EditsRange::Today => t!("ribbon-recent-today"),
            EditsRange::Yesterday => t!("ribbon-recent-yesterday"),
            EditsRange::Days7 => t!("ribbon-recent-7"),
            EditsRange::Days14 => t!("ribbon-recent-14"),
            EditsRange::Days30 => t!("ribbon-recent-30"),
            EditsRange::Months3 => t!("ribbon-recent-90"),
            EditsRange::Months6 => t!("ribbon-recent-180"),
            EditsRange::All => t!("ribbon-recent-all"),
        }
    }

    /// The oldest edit time to list (ms since the epoch), from local midnight for Today and
    /// Since Yesterday; `None` lists every page.
    pub fn since(self, now_ms: i64) -> Option<i64> {
        use chrono::{Local, TimeZone};
        const DAY: i64 = 24 * 60 * 60 * 1000;
        let midnight = || {
            let today = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
            Local
                .from_local_datetime(&today)
                .earliest()
                .map_or(now_ms - DAY, |at| at.timestamp_millis())
        };
        match self {
            EditsRange::Today => Some(midnight()),
            EditsRange::Yesterday => Some(midnight() - DAY),
            EditsRange::Days7 => Some(now_ms - 7 * DAY),
            EditsRange::Days14 => Some(now_ms - 14 * DAY),
            EditsRange::Days30 => Some(now_ms - 30 * DAY),
            EditsRange::Months3 => Some(now_ms - 91 * DAY),
            EditsRange::Months6 => Some(now_ms - 182 * DAY),
            EditsRange::All => None,
        }
    }
}

/// What History › Delete All Versions clears: the open page, or every page of the
/// selected section or notebook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionScope {
    Page,
    Section,
    Notebook,
}

/// How Paste brings content in (the desktop notebook's paste options); it lives with the paste code.
pub use crate::paste::PasteMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size {
    Large,
    Small,
    Icon,
}

/// One choice in a gallery, drawn by the fields it has: with a preview, as its label in
/// that style's look; with a label, as a Small button; with no label and no glyph but a
/// colour, as a swatch; with no label but a glyph, as an icon (a pen, a shape).
#[derive(Debug, Clone)]
pub struct GalleryItem {
    pub cmd: Cmd,
    pub label: Cow<'static, str>,
    pub glyph: Option<&'static str>,
    /// A swatch colour (ink colours), the glyph's colour (tags), or the colour `filled`
    /// is painted in (pens).
    pub color: Option<Color32>,
    /// Shown on hover and read by screen readers (the label of an icon-only item).
    pub tooltip: Cow<'static, str>,
    /// A Filled glyph painted under `glyph` in `color`: the pens look like coloured pens.
    pub filled: Option<&'static str>,
    /// A style shown as a styled preview of its name (Styles), with no glyph.
    pub preview: Option<BlockKind>,
}

#[derive(Debug, Clone)]
pub enum ControlKind {
    Button,
    Toggle,
    Split(MenuId),
    Menu(MenuId),
    Combo {
        menu: MenuId,
        width: f32,
    },
    Gallery {
        items: Vec<GalleryItem>,
        more: Option<MenuId>,
    },
}

#[derive(Debug, Clone)]
pub struct Control {
    pub kind: ControlKind,
    /// What a click runs (the main part of a Split). `None` for Menu, Combo and Gallery,
    /// and for a read-only readout.
    pub cmd: Option<Cmd>,
    pub glyph: &'static str,
    pub label: Cow<'static, str>,
    pub tooltip: Cow<'static, str>,
    /// Its largest size; scaling only ever makes it smaller.
    pub size: Size,
    /// A colour bar under the glyph (Highlight, Font colour).
    pub accent: Option<Color32>,
    /// The glyph's colour, when not the ink colour (Home › Delete is red).
    pub glyph_color: Option<Color32>,
    /// The accessible name of a Split's arrow when "{label} ▾" would not say what it opens
    /// (the Eraser's arrow picks the eraser size).
    pub arrow_label: Option<Cow<'static, str>>,
    /// The glyph's size when Small or Icon; `SMALL_GLYPH` unless set (Undo and Redo are
    /// the desktop notebook's 20 px).
    pub glyph_size: f32,
}

#[derive(Debug, Clone)]
pub enum GroupLayout {
    /// Large controls side by side; Small ones stacked three to a column.
    Flow,
    /// Fixed rows of this many controls each (Basic Text's two rows).
    Rows(Vec<usize>),
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: &'static str,
    pub label: Cow<'static, str>,
    /// Shown on the group's button when it collapses.
    pub glyph: &'static str,
    /// Lower collapses first.
    pub priority: u8,
    pub layout: GroupLayout,
    pub controls: Vec<Control>,
}

/// What the ribbon asks of the app.
pub trait RibbonHost {
    fn enabled(&self, cmd: &Cmd) -> bool;
    fn checked(&self, cmd: &Cmd) -> bool;
    fn menu_enabled(&self, menu: MenuId) -> bool;
    /// The value a Combo shows ("Carlito", "11").
    fn combo_text(&self, menu: MenuId) -> String;
    /// The contextual tabs to show now, in order.
    fn contextual(&self) -> Vec<RibbonTab>;
    /// Draws a menu's contents; returns the command picked.
    fn menu(&self, ui: &mut Ui, menu: MenuId, t: Tokens) -> Option<Cmd>;
    /// The Audio & Video tab's time readout and whether the recording is paused, while
    /// recording.
    fn recording(&self) -> Option<(String, bool)>;
    /// Exactly one audio item is selected (the Playback group).
    fn audio_selected(&self) -> bool;
    /// The pen presets for Draw's gallery, in order.
    fn pens(&self) -> Vec<crate::canvas::PenPreset> {
        Vec::new()
    }
    /// The tags for Home's Tags gallery, in Customize Tags' order.
    fn tag_gallery(&self) -> TagGallery {
        TagGallery::default()
    }
}

pub struct RibbonState {
    pub tab: RibbonTab,
    pub layout: RibbonLayout,
    /// Show tabs only (Ctrl+F1): the band opens over the page when a tab is clicked.
    pub collapsed: bool,
    /// The band shown over the page while collapsed or in Full Page View.
    pub overlay: bool,
    /// Set by the app each frame: Full Page View hides the band like `collapsed`.
    pub full_page: bool,
    /// Set by `show` each frame: a click landed on the ribbon (a tab, the band's empty
    /// space, a group label, a greyed control) and ran nothing, nor opened a menu. egui
    /// took the keyboard from the text at the press; the app gives it back, as the desktop notebook's
    /// caret stays in the note when the ribbon is clicked.
    pub idle_click: bool,
}

impl Default for RibbonState {
    fn default() -> Self {
        Self {
            tab: RibbonTab::Home,
            layout: RibbonLayout::Classic,
            collapsed: false,
            overlay: false,
            full_page: false,
            idle_click: false,
        }
    }
}

impl RibbonState {
    pub fn load(library: &Library) -> Self {
        let setting = |key: &str| library.setting(key).ok().flatten();
        Self {
            layout: setting("ribbon_layout")
                .map_or(RibbonLayout::Classic, |code| RibbonLayout::from_code(&code)),
            collapsed: setting("ribbon_collapsed").is_some_and(|value| value == "true"),
            ..Self::default()
        }
    }

    pub fn save_layout(&self, library: &mut Library) -> crate::db::DbResult<()> {
        library.set_setting("ribbon_layout", self.layout.code())
    }

    pub fn save_collapsed(&self, library: &mut Library) -> crate::db::DbResult<()> {
        library.set_setting(
            "ribbon_collapsed",
            if self.collapsed { "true" } else { "false" },
        )
    }

    /// The band is drawn in the card (not hidden behind the tabs).
    pub fn band_shown(&self) -> bool {
        !self.collapsed && !self.full_page
    }
}

/// The ribbon panel's height: the tab row, plus the card and its 8 px bottom margin when
/// the band shows.
pub fn panel_height(state: &RibbonState) -> f32 {
    if !state.band_shown() {
        TAB_ROW
    } else {
        match state.layout {
            RibbonLayout::Classic => TAB_ROW + CLASSIC_HEIGHT + 8.0,
            RibbonLayout::Simplified => TAB_ROW + SIMPLIFIED_HEIGHT + 8.0,
        }
    }
}

// Sizes. Every width is measured in the current language (through the galley cache), so
// French labels get the room they need and the band scales on what it really holds.

fn label_font() -> FontId {
    theme::ui(12.0)
}

/// A Large control's label as one or two lines: a label wider than 64 px breaks at the
/// space closest to its middle, as Office's large buttons do.
fn large_lines(ctx: &egui::Context, label: &str) -> (String, Option<String>) {
    if widgets::text_width(ctx, label, label_font()) <= LARGE_LINE {
        return (label.to_owned(), None);
    }
    let middle = label.chars().count() / 2;
    // A plain space only: a no-break space ("100 %") never breaks.
    let space = label
        .char_indices()
        .enumerate()
        .filter(|(_, (_, ch))| *ch == ' ')
        .min_by_key(|(index, _)| index.abs_diff(middle))
        .map(|(_, (byte, _))| byte);
    match space {
        Some(byte) => (label[..byte].to_owned(), Some(label[byte + 1..].to_owned())),
        None => (label.to_owned(), None),
    }
}

/// Room after a Large button's second line for its chevron (Office: "Page Templates ⌄").
const LARGE_CHEVRON: f32 = 12.0;

/// Where a Large menu button's chevron goes, as Office draws it: on a line of its own
/// under a one-line label (Paste, Table), right after the second line of a two-line one
/// (Format Background, Page Templates).
fn chevron_after_label(second: Option<&str>) -> bool {
    second.is_some()
}

/// Opens a menu (Split, Menu): Large controls show it as a chevron under or after the
/// label, smaller ones as a chevron at the right.
fn has_arrow(control: &Control) -> bool {
    matches!(control.kind, ControlKind::Split(_) | ControlKind::Menu(_))
}

/// A Large control's size: a glyph over a label of one or two lines, with a chevron when
/// it opens a menu (`chevron_after_label`). A collapsed group's button is one too.
fn large_size(ctx: &egui::Context, label: &str, arrow: bool) -> Vec2 {
    let (first, second) = large_lines(ctx, label);
    let width = |line: &str| widgets::text_width(ctx, line, label_font());
    let mut widest = width(&first);
    if let Some(second) = &second {
        let chevron = if arrow { LARGE_CHEVRON } else { 0.0 };
        widest = widest.max(width(second) + chevron);
    }
    vec2((widest + 12.0).max(44.0), CONTROLS_HEIGHT)
}

/// The width and height `control` takes at `size`.
fn control_size(ctx: &egui::Context, control: &Control, size: Size) -> Vec2 {
    let arrow = has_arrow(control);
    match (&control.kind, size) {
        (ControlKind::Combo { width, .. }, _) => vec2(*width, SMALL_HEIGHT),
        (ControlKind::Gallery { .. }, Size::Icon) => vec2(ICON_WIDTH + ICON_ARROW, SMALL_HEIGHT),
        (ControlKind::Gallery { items, more }, _) => {
            let columns = gallery_columns(items, more.is_some(), size);
            gallery_layout(ctx, items, more.is_some(), columns).1
        }
        (_, Size::Large) => large_size(ctx, &control.label, arrow),
        (_, Size::Small) => {
            let label = widgets::text_width(ctx, &control.label, label_font());
            let arrow = if arrow { SMALL_ARROW } else { 0.0 };
            vec2(6.0 + SMALL_GLYPH + 4.0 + label + arrow + 6.0, SMALL_HEIGHT)
        }
        (_, Size::Icon) => vec2(
            ICON_WIDTH + if arrow { ICON_ARROW } else { 0.0 },
            SMALL_HEIGHT,
        ),
    }
}

/// A gallery item drawn as a colour swatch: it has a colour, and neither a label nor a
/// glyph (ink colours).
fn swatch_color(item: &GalleryItem) -> Option<Color32> {
    item.color
        .filter(|_| item.label.is_empty() && item.glyph.is_none())
}

/// A gallery item drawn as an icon: a glyph and no label (pens, shapes).
fn icon_item(item: &GalleryItem) -> bool {
    item.label.is_empty() && item.glyph.is_some()
}

/// A pen of Draw's gallery, drawn as a picture of the pen (`paint_pen_picture`).
fn pen_picture(item: &GalleryItem) -> bool {
    matches!(item.cmd, Cmd::PenPreset(_)) && item.filled.is_some()
}

/// What names a gallery item for screen readers and menus: its label, else its tooltip.
fn item_name(item: &GalleryItem) -> &str {
    if item.label.is_empty() {
        &item.tooltip
    } else {
        &item.label
    }
}

/// How many columns of three a gallery shows at `size`: `GALLERY_COLUMNS` as designed,
/// one once its group has shrunk (Large made Small) for a gallery of labelled items whose
/// "▾" lists them all (Styles, Tags), as the desktop notebook's scrolls. Pens and shapes keep theirs.
fn gallery_columns(items: &[GalleryItem], more: bool, size: Size) -> usize {
    if size == Size::Large || !more || items.iter().any(|item| item.label.is_empty()) {
        GALLERY_COLUMNS
    } else {
        1
    }
}

/// Where a gallery's items sit, relative to its top-left, in columns of three (at most
/// `columns` of them), and the strip's size, with the "▾" column when `more`. Pens are
/// pictures in one row (at most `PENS_SHOWN`), with no "▾": the pen in hand opens the
/// menu.
fn gallery_layout(
    ctx: &egui::Context,
    items: &[GalleryItem],
    more: bool,
    columns: usize,
) -> (Vec<Rect>, Vec2) {
    if items.first().is_some_and(pen_picture) {
        let shown = items.len().min(PENS_SHOWN);
        let cells = (0..shown)
            .map(|index| {
                let x = index as f32 * (PEN_CELL.x + COLUMN_GAP);
                Rect::from_min_size(pos2(x, 0.0), PEN_CELL)
            })
            .collect();
        let width = shown as f32 * PEN_CELL.x + shown.saturating_sub(1) as f32 * COLUMN_GAP;
        return (cells, vec2(width, PEN_CELL.y));
    }
    let shown = &items[..items.len().min(columns * 3)];
    let mut cells = Vec::with_capacity(shown.len());
    let (mut x, mut height) = (0.0_f32, 0.0_f32);
    for (index, column) in shown.chunks(3).enumerate() {
        if index > 0 {
            x += COLUMN_GAP;
        }
        let sizes: Vec<Vec2> = column
            .iter()
            .map(|item| {
                if swatch_color(item).is_some() {
                    Vec2::splat(SWATCH)
                } else if icon_item(item) {
                    ICON_ITEM
                } else if let Some(kind) = &item.preview {
                    // Light and dark previews only differ in colour.
                    let font = editor::style_preview(kind, Tokens::light()).font_id;
                    let label = widgets::text_width(ctx, &item.label, font);
                    vec2(6.0 + label + 8.0, SMALL_HEIGHT)
                } else {
                    let label = widgets::text_width(ctx, &item.label, label_font());
                    vec2(6.0 + SMALL_GLYPH + 4.0 + label + 6.0, SMALL_HEIGHT)
                }
            })
            .collect();
        let width = sizes.iter().map(|size| size.x).fold(0.0, f32::max);
        let mut y = 0.0;
        for (row, (item, size)) in column.iter().zip(&sizes).enumerate() {
            let swatch = swatch_color(item).is_some();
            if row > 0 && swatch {
                y += SWATCH_GAP;
            }
            // Swatches and icons keep their own size; labelled items share the column's
            // width so their hover fills line up.
            let cell_width = if swatch || icon_item(item) {
                size.x
            } else {
                width
            };
            cells.push(Rect::from_min_size(pos2(x, y), vec2(cell_width, size.y)));
            y += size.y;
        }
        height = height.max(y);
        x += width;
    }
    if more {
        x += MORE_COLUMN;
        height = height.max(SMALL_HEIGHT);
    }
    (cells, vec2(x, height))
}

// Levels: as the window narrows, a group shrinks one level at a time (Large buttons made
// Small, then everything an icon, then the whole group one button), lowest priority first.

/// The last level: the group is one button whose menu lists its controls.
const COLLAPSED: usize = 3;
/// Where the control area starts in the Classic card's inner height (88 px): the room
/// the control area and the label row leave, split above and below.
const BAND_TOP: f32 = (CLASSIC_HEIGHT - 8.0 - CONTROLS_HEIGHT - LABEL_ROW) / 2.0;
/// The line between two Classic groups.
const SEPARATOR_HEIGHT: f32 = 52.0;
/// The room kept at the band's bottom-right for the display-options chevron.
const OPTIONS_ROOM: f32 = 28.0;

fn group_label_font() -> FontId {
    theme::ui(11.0)
}

/// The sizes a group's Large and Small controls take at each level: 0 as designed, 1 with
/// Large made Small (only when it has a Large control), 2 with everything an icon (not for
/// a group with a gallery of named items). `None` where a level does not apply; level 3,
/// the collapsed group, has no control sizes. A `Rows` group keeps its rows until it
/// collapses.
fn level_sizes(group: &Group) -> [Option<(Size, Size)>; 4] {
    match group.layout {
        GroupLayout::Rows(_) => [Some((Size::Large, Size::Small)), None, None, None],
        GroupLayout::Flow => {
            let has_large = group
                .controls
                .iter()
                .any(|control| control.size == Size::Large);
            // A gallery of named items (Styles, Tags) is never shown as bare icons: the desktop notebook
            // folds such a group into one button once its names no longer fit.
            let named_gallery = group.controls.iter().any(|control| {
                matches!(&control.kind, ControlKind::Gallery { items, more: Some(_) }
                    if !items.is_empty() && items.iter().all(|item| !item.label.is_empty()))
            });
            [
                Some((Size::Large, Size::Small)),
                has_large.then_some((Size::Small, Size::Small)),
                (!named_gallery).then_some((Size::Icon, Size::Icon)),
                None,
            ]
        }
    }
}

/// One control placed in its group: its index, the size it is drawn at, and its rect
/// relative to the top-left of the group's content in the control area.
type Cell = (usize, Size, Rect);

/// Stacks `column` at `x`, centred vertically in the control area, and returns its width.
fn place_column(cells: &mut Vec<Cell>, column: &[(usize, Size, Vec2)], x: f32) -> f32 {
    let height: f32 = column.iter().map(|(_, _, extent)| extent.y).sum();
    let mut y = ((CONTROLS_HEIGHT - height) / 2.0).max(0.0);
    let mut width = 0.0_f32;
    for &(index, size, extent) in column {
        cells.push((index, size, Rect::from_min_size(pos2(x, y), extent)));
        y += extent.y;
        width = width.max(extent.x);
    }
    width
}

/// Where each control of `group` goes at `level` (0–2), and the content's width; `None`
/// when the group has no such level.
fn group_cells(ctx: &egui::Context, group: &Group, level: usize) -> Option<(Vec<Cell>, f32)> {
    let (large, small) = level_sizes(group).get(level).copied().flatten()?;
    // A control declared Icon stays one.
    let size_of = |control: &Control| match control.size {
        Size::Large => large,
        Size::Small => small,
        Size::Icon => Size::Icon,
    };
    let mut cells = Vec::with_capacity(group.controls.len());
    let width = match &group.layout {
        GroupLayout::Flow => {
            // Large controls and gallery strips each take a column; consecutive smaller
            // controls stack three to a column.
            let mut columns: Vec<Vec<(usize, Size, Vec2)>> = Vec::new();
            let mut stacking = false;
            for (index, control) in group.controls.iter().enumerate() {
                let size = size_of(control);
                let extent = control_size(ctx, control, size);
                let tall = size == Size::Large
                    || (matches!(control.kind, ControlKind::Gallery { .. }) && size != Size::Icon);
                match columns.last_mut() {
                    Some(column) if stacking && !tall && column.len() < 3 => {
                        column.push((index, size, extent));
                    }
                    _ => columns.push(vec![(index, size, extent)]),
                }
                stacking = !tall;
            }
            let mut x = 0.0;
            for (number, column) in columns.iter().enumerate() {
                if number > 0 {
                    x += COLUMN_GAP;
                }
                x += place_column(&mut cells, column, x);
            }
            x
        }
        GroupLayout::Rows(counts) => {
            // Fixed rows, spread evenly over the control area; controls the counts leave
            // out go on a last row rather than vanish.
            let total = group.controls.len();
            let mut start = 0;
            let mut rows: Vec<std::ops::Range<usize>> = counts
                .iter()
                .map(|&count| {
                    let row = start..(start + count).min(total);
                    start = row.end;
                    row
                })
                .collect();
            if start < total {
                rows.push(start..total);
            }
            rows.retain(|row| !row.is_empty());
            let count = rows.len() as f32;
            let gap = ((CONTROLS_HEIGHT - count * SMALL_HEIGHT) / (count + 1.0)).max(0.0);
            let mut widest = 0.0_f32;
            for (number, row) in rows.into_iter().enumerate() {
                let y = gap + number as f32 * (SMALL_HEIGHT + gap);
                let mut x = 0.0;
                for index in row.clone() {
                    if index > row.start {
                        x += COLUMN_GAP;
                    }
                    let control = &group.controls[index];
                    let size = size_of(control);
                    let extent = control_size(ctx, control, size);
                    let top = y + (SMALL_HEIGHT - extent.y) / 2.0;
                    cells.push((index, size, Rect::from_min_size(pos2(x, top), extent)));
                    x += extent.x;
                }
                widest = widest.max(x);
            }
            widest
        }
    };
    Some((cells, width))
}

/// A collapsed group's button: a Large menu button with the group's glyph and label.
fn collapsed_size(ctx: &egui::Context, group: &Group) -> Vec2 {
    large_size(ctx, &group.label, true)
}

/// How wide `group` is at `level`, padding included; `None` when it has no such level.
/// Its label (under the controls) is never cut: the group is at least that wide.
fn group_width(ctx: &egui::Context, group: &Group, level: usize) -> Option<f32> {
    if level == COLLAPSED {
        // The button carries the group's name, so there is no label row to fit.
        return Some(collapsed_size(ctx, group).x + 2.0 * GROUP_PAD);
    }
    let (_, content) = group_cells(ctx, group, level)?;
    let label = widgets::text_width(ctx, &group.label, group_label_font()) + 16.0;
    Some((content + 2.0 * GROUP_PAD).max(label))
}

/// Each group's level (0 = as designed … 3 = one button) so the band fits `available`.
/// Groups of lower priority shrink first; among equals, the rightmost. A group only ever
/// steps to a narrower level: a level it does not have (`None`), or one no narrower than
/// where it is (a collapsed button is often wider than the group's icons, which keep the
/// label row), is skipped. Once the band fits, the room a big step freed goes back to the
/// groups shrunk before it, the most important first, each as designed as it still fits.
pub fn fit_levels(widths: &[[Option<f32>; 4]], priorities: &[u8], available: f32) -> Vec<usize> {
    let mut levels = vec![0usize; widths.len()];
    let width = |index: usize, level: usize| widths[index][level].unwrap_or(0.0);
    let total = |levels: &[usize]| -> f32 {
        levels
            .iter()
            .enumerate()
            .map(|(index, &level)| width(index, level))
            .sum()
    };
    while total(&levels) > available {
        let next = |index: usize| {
            let now = width(index, levels[index]);
            ((levels[index] + 1)..4)
                .find(|&level| widths[index][level].is_some_and(|narrower| narrower < now))
        };
        let pick = (0..widths.len())
            .filter(|&index| next(index).is_some())
            .min_by(|&a, &b| priorities[a].cmp(&priorities[b]).then(b.cmp(&a)));
        let Some(index) = pick else { break };
        levels[index] = next(index).unwrap();
    }
    let mut order: Vec<usize> = (0..widths.len()).collect();
    order.sort_by(|&a, &b| priorities[b].cmp(&priorities[a]).then(a.cmp(&b)));
    for index in order {
        let others = total(&levels) - width(index, levels[index]);
        if let Some(level) = (0..levels[index])
            .find(|&level| widths[index][level].is_some_and(|wider| others + wider <= available))
        {
            levels[index] = level;
        }
    }
    levels
}

// Drawing.

/// A control's accessible name: its label, or its tooltip when it has none (a swatch
/// gallery is named by its tooltip, "Ink colour").
fn control_name(control: &Control) -> &str {
    if control.label.is_empty() {
        &control.tooltip
    } else {
        &control.label
    }
}

/// What hovering shows: the tooltip when it says more than the label, else the label.
fn control_tip(control: &Control) -> &str {
    if control.tooltip.is_empty() || control.tooltip == control.label {
        control_name(control)
    } else {
        &control.tooltip
    }
}

/// A Split arrow's (or a gallery's "▾") accessible name.
fn arrow_name(control: &Control) -> String {
    match &control.arrow_label {
        Some(label) => label.to_string(),
        None => format!("{} ▾", control_name(control)),
    }
}

/// A stable id, so an open menu stays open when the window resizes and the band
/// re-lays out. Only one tab's band is drawn at a time, so a control is unique.
fn control_id(control: &Control) -> Id {
    Id::unique((
        "folio-ribbon-control",
        control.glyph,
        control.label.as_ref(),
        control.tooltip.as_ref(),
    ))
}

/// A disabled control only senses hover: it still shows its tooltip, but a click does
/// nothing.
fn interact(ui: &Ui, rect: Rect, id: Id, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    ui.interact(rect, id, sense)
}

/// Names the widget for screen readers (and tests) and attaches its tooltip.
fn describe(
    response: Response,
    role: Role,
    enabled: bool,
    checked: Option<bool>,
    name: &str,
    tip: &str,
) -> Response {
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(role, enabled, name);
        info.selected = checked;
        info
    });
    response.on_hover_text(tip).on_disabled_hover_text(tip)
}

/// The fill behind a control: checked toggles are grey with a grey frame, as the desktop notebook's
/// (the current pen, Normal View), hovered ones a lighter grey.
fn paint_back(
    painter: &Painter,
    rect: Rect,
    enabled: bool,
    checked: bool,
    hovered: bool,
    t: Tokens,
) {
    if checked {
        painter.rect_filled(rect, CornerRadius::same(4), t.nav_selected);
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, t.container_border),
            StrokeKind::Inside,
        );
    }
    if hovered && enabled {
        painter.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
}

fn paint_chevron(painter: &Painter, center: Pos2, enabled: bool, t: Tokens) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        icons::CARET_DOWN,
        theme::icon(10.0),
        if enabled { t.muted } else { t.faint },
    );
}

/// The colour bar under a glyph (Highlight, Font colour), centred on `top`'s x.
fn paint_accent(painter: &Painter, control: &Control, top: Pos2, enabled: bool) {
    if let Some(color) = control.accent {
        let color = if enabled {
            color
        } else {
            theme::with_alpha(color, 90)
        };
        painter.rect_filled(
            Rect::from_min_size(pos2(top.x - 7.0, top.y), vec2(14.0, 3.0)),
            CornerRadius::ZERO,
            color,
        );
    }
}

/// Home › Delete's glyph and the recording dot are red. The builders have no theme, so
/// they name the red with this colour and drawing swaps in the theme's own danger red,
/// which is lighter in dark mode.
const DANGER: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);

/// A control's own glyph colour, in the current theme.
fn glyph_tint(color: Color32, t: Tokens) -> Color32 {
    if color == DANGER {
        t.danger
    } else {
        color
    }
}

/// The glyph's and the label's colours: faint when disabled.
fn face_colors(control: &Control, enabled: bool, t: Tokens) -> (Color32, Color32) {
    if enabled {
        let glyph = control
            .glyph_color
            .map_or(t.ink, |color| glyph_tint(color, t));
        (glyph, t.ink)
    } else {
        (t.faint, t.faint)
    }
}

/// A colour bar sits under the glyph, so the glyph moves up to make room.
fn glyph_lift(control: &Control) -> f32 {
    if control.accent.is_some() {
        2.0
    } else {
        0.0
    }
}

/// A Large face: the glyph near the top, the label on one or two lines under it and, when
/// `arrow`, a chevron where `chevron_after_label` puts it. A second line and its chevron
/// are centred together.
fn paint_large(ui: &Ui, rect: Rect, control: &Control, arrow: bool, enabled: bool, t: Tokens) {
    let painter = ui.painter();
    let (glyph_color, text_color) = face_colors(control, enabled, t);
    let center = pos2(
        rect.center().x,
        rect.top() + 8.0 + LARGE_GLYPH / 2.0 - glyph_lift(control),
    );
    painter.text(
        center,
        Align2::CENTER_CENTER,
        control.glyph,
        theme::icon(LARGE_GLYPH),
        glyph_color,
    );
    paint_accent(
        painter,
        control,
        pos2(center.x, center.y + LARGE_GLYPH / 2.0),
        enabled,
    );
    let (first, second) = large_lines(ui.ctx(), &control.label);
    let after = chevron_after_label(second.as_deref());
    let first = painter.text(
        pos2(rect.center().x, rect.top() + 34.0),
        Align2::CENTER_TOP,
        first,
        label_font(),
        text_color,
    );
    let second = second.map(|line| {
        let shift = if arrow { LARGE_CHEVRON / 2.0 } else { 0.0 };
        painter.text(
            pos2(rect.center().x - shift, first.bottom()),
            Align2::CENTER_TOP,
            line,
            label_font(),
            text_color,
        )
    });
    if arrow {
        let center = match second {
            Some(last) if after => pos2(last.right() + LARGE_CHEVRON / 2.0, last.center().y),
            _ => pos2(rect.center().x, second.unwrap_or(first).bottom() + 6.0),
        };
        paint_chevron(painter, center, enabled, t);
    }
}

/// A control's glyph, colour bar and label at `size`. `chevron` is where a Small or Icon
/// control's menu arrow goes; a Large one draws its own under or after its label.
fn paint_face(
    ui: &Ui,
    rect: Rect,
    control: &Control,
    size: Size,
    enabled: bool,
    chevron: Option<Rect>,
    t: Tokens,
) {
    let painter = ui.painter();
    let (glyph_color, text_color) = face_colors(control, enabled, t);
    let lift = glyph_lift(control);
    match size {
        Size::Large => paint_large(ui, rect, control, has_arrow(control), enabled, t),
        Size::Small | Size::Icon => {
            let glyph_area = match chevron {
                Some(arrow) => Rect::from_min_max(rect.min, pos2(arrow.left(), rect.bottom())),
                None => rect,
            };
            let glyph_x = if size == Size::Small {
                rect.left() + 6.0 + SMALL_GLYPH / 2.0
            } else {
                glyph_area.center().x
            };
            let center = pos2(glyph_x, rect.center().y - lift);
            painter.text(
                center,
                Align2::CENTER_CENTER,
                control.glyph,
                theme::icon(control.glyph_size),
                glyph_color,
            );
            paint_accent(
                painter,
                control,
                pos2(center.x, center.y + control.glyph_size / 2.0 - 2.0),
                enabled,
            );
            if size == Size::Small {
                painter.text(
                    pos2(rect.left() + 6.0 + SMALL_GLYPH + 4.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    control.label.as_ref(),
                    label_font(),
                    text_color,
                );
            }
            if let Some(arrow) = chevron {
                paint_chevron(painter, arrow.center(), enabled, t);
            }
        }
    }
}

/// Where a Small or Icon Menu control draws its chevron (Large ones draw theirs under or
/// after their label).
fn menu_chevron(rect: Rect, size: Size) -> Option<Rect> {
    let width = match size {
        Size::Large => return None,
        Size::Small => SMALL_ARROW,
        Size::Icon => ICON_ARROW,
    };
    // A Small control keeps 6 px of padding after its arrow.
    let right = if size == Size::Small {
        rect.right() - 6.0
    } else {
        rect.right()
    };
    Some(Rect::from_min_max(
        pos2(right - width, rect.top()),
        pos2(right, rect.bottom()),
    ))
}

/// A Split's two parts: the main action and the arrow that opens its menu.
fn split_rects(rect: Rect, size: Size) -> (Rect, Rect) {
    if size == Size::Large {
        let cut = rect.bottom() - LARGE_SPLIT_ARROW;
        (
            Rect::from_min_max(rect.min, pos2(rect.right(), cut)),
            Rect::from_min_max(pos2(rect.left(), cut), rect.max),
        )
    } else {
        let cut = rect.right() - SPLIT_ARROW;
        (
            Rect::from_min_max(rect.min, pos2(cut, rect.bottom())),
            Rect::from_min_max(pos2(cut, rect.top()), rect.max),
        )
    }
}

/// Whether the menu `response` opens (`menu_popup`, `egui::Popup::menu`) is open: a menu
/// button is drawn pressed meanwhile, as the desktop notebook's are.
fn menu_open(response: &Response) -> bool {
    egui::Popup::is_id_open(&response.ctx, egui::Popup::default_response_id(response))
}

/// Shows `menu` under `response` when it is clicked; the host draws its contents.
fn menu_popup(response: &Response, menu: MenuId, host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    egui::Popup::menu(response)
        .close_behavior(close_behavior(menu))
        .show(|ui| {
            let picked = host.menu(ui, menu, t);
            if picked.is_some() {
                ui.close();
            }
            picked
        })
        .and_then(|shown| shown.inner)
}

/// Zoom Level's menu holds a box to type in, and the pen menu a custom thickness slider
/// (and the "+" that shows it): a click inside leaves them open (a pick still closes
/// them). Other menus close on any click.
fn close_behavior(menu: MenuId) -> egui::PopupCloseBehavior {
    if matches!(menu, MenuId::Zoom | MenuId::PenSettings) {
        egui::PopupCloseBehavior::CloseOnClickOutside
    } else {
        egui::PopupCloseBehavior::CloseOnClick
    }
}

/// Draws `control` at `size` in `rect` and returns the command it gave this frame: a
/// click on a button, or a pick in the menu it opens.
fn draw_control(
    ui: &mut Ui,
    rect: Rect,
    control: &Control,
    size: Size,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    ui.advance_cursor_after_rect(rect);
    let id = control_id(control);
    let name = control_name(control);
    let tip = control_tip(control);
    match &control.kind {
        ControlKind::Button | ControlKind::Toggle => {
            let Some(cmd) = &control.cmd else {
                readout(ui, rect, id, control, size, t);
                return None;
            };
            let enabled = host.enabled(cmd);
            let toggle = matches!(control.kind, ControlKind::Toggle);
            let checked = toggle && host.checked(cmd);
            let response = interact(ui, rect, id, enabled);
            paint_back(ui.painter(), rect, enabled, checked, response.hovered(), t);
            paint_face(ui, rect, control, size, enabled, None, t);
            let response = describe(
                response,
                Role::Button,
                enabled,
                toggle.then_some(checked),
                name,
                tip,
            );
            (enabled && response.clicked()).then(|| cmd.clone())
        }
        ControlKind::Split(menu) => {
            let (main, arrow) = split_rects(rect, size);
            let cmd = control.cmd.as_ref();
            let enabled = cmd.is_some_and(|cmd| host.enabled(cmd));
            let menu_enabled = host.menu_enabled(*menu);
            let checked = cmd.is_some_and(|cmd| host.checked(cmd));
            let main_response = interact(ui, main, id.with("main"), enabled);
            let arrow_response = interact(ui, arrow, id.with("arrow"), menu_enabled);
            // Drawn pressed, both parts, while its menu is open.
            let open = menu_open(&arrow_response);
            let painter = ui.painter();
            paint_back(painter, rect, enabled, checked || open, false, t);
            paint_back(painter, main, enabled, false, main_response.hovered(), t);
            paint_back(
                painter,
                arrow,
                menu_enabled,
                false,
                arrow_response.hovered(),
                t,
            );
            let chevron = (size != Size::Large).then_some(arrow);
            paint_face(ui, rect, control, size, enabled, chevron, t);
            let main_response = describe(main_response, Role::Button, enabled, None, name, tip);
            let arrow_response = describe(
                arrow_response,
                Role::Button,
                menu_enabled,
                None,
                &arrow_name(control),
                &arrow_name(control),
            );
            // The popup runs every frame, so a click on the main part still closes it.
            let picked = if menu_enabled {
                menu_popup(&arrow_response, *menu, host, t)
            } else {
                None
            };
            if enabled && main_response.clicked() {
                cmd.cloned()
            } else {
                picked
            }
        }
        ControlKind::Menu(menu) => {
            let enabled = host.menu_enabled(*menu);
            let response = interact(ui, rect, id, enabled);
            let open = menu_open(&response);
            paint_back(ui.painter(), rect, enabled, open, response.hovered(), t);
            paint_face(
                ui,
                rect,
                control,
                size,
                enabled,
                menu_chevron(rect, size),
                t,
            );
            let response = describe(response, Role::Button, enabled, None, name, tip);
            if enabled {
                menu_popup(&response, *menu, host, t)
            } else {
                None
            }
        }
        ControlKind::Combo { menu, .. } => {
            let enabled = host.menu_enabled(*menu);
            let response = interact(ui, rect, id, enabled);
            paint_combo(
                ui,
                rect,
                control,
                host.combo_text(*menu),
                enabled,
                &response,
                t,
            );
            // Named by what it picks ("Font"), not by the value it shows.
            let response = describe(response, Role::Button, enabled, None, name, tip);
            if enabled {
                menu_popup(&response, *menu, host, t)
            } else {
                None
            }
        }
        ControlKind::Gallery { items, more } if size == Size::Icon => {
            gallery_button(ui, rect, id, control, items, *more, host, t)
        }
        ControlKind::Gallery { items, more } => {
            let columns = gallery_columns(items, more.is_some(), size);
            let (cells, strip) = gallery_layout(ui.ctx(), items, more.is_some(), columns);
            let mut picked = None;
            for (index, (item, cell)) in items.iter().zip(&cells).enumerate() {
                let cell = cell.translate(rect.min.to_vec2());
                let id = id.with(index);
                if let Some(cmd) = gallery_item(ui, cell, id, item, control, *more, host, t) {
                    picked = Some(cmd);
                }
            }
            // Pens have no "▾" column: the pen in hand opens their menu.
            if let Some(more) = more.filter(|_| !items.first().is_some_and(pen_picture)) {
                let column = Rect::from_min_size(
                    pos2(rect.left() + strip.x - MORE_COLUMN, rect.top()),
                    vec2(MORE_COLUMN, strip.y),
                );
                let enabled = host.menu_enabled(more);
                let response = interact(ui, column, id.with("more"), enabled);
                paint_back(ui.painter(), column, enabled, false, response.hovered(), t);
                paint_chevron(ui.painter(), column.center(), enabled, t);
                let label = arrow_name(control);
                let response = describe(response, Role::Button, enabled, None, &label, &label);
                if enabled {
                    if let Some(cmd) = menu_popup(&response, more, host, t) {
                        picked = Some(cmd);
                    }
                }
            }
            picked
        }
    }
}

/// A Button with no command: a read-only value (the recording's elapsed time). Its text
/// keeps the ink colour, as it is not a disabled command; its glyph is faint unless it has
/// a colour of its own (the red dot while recording, faint while paused).
fn readout(ui: &Ui, rect: Rect, id: Id, control: &Control, size: Size, t: Tokens) {
    let response = ui.interact(rect, id, Sense::hover());
    let face = Control {
        glyph_color: Some(control.glyph_color.unwrap_or(t.faint)),
        ..control.clone()
    };
    paint_face(ui, rect, &face, size, true, None, t);
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, control.label.as_ref()));
    if !control.tooltip.is_empty() {
        response.on_hover_text(control.tooltip.as_ref());
    }
}

/// A font or size box: its current value on white, with a chevron.
fn paint_combo(
    ui: &Ui,
    rect: Rect,
    control: &Control,
    value: String,
    enabled: bool,
    response: &Response,
    t: Tokens,
) {
    let painter = ui.painter();
    let border = if enabled && response.hovered() {
        t.muted
    } else {
        t.line_strong
    };
    painter.rect_filled(rect, CornerRadius::same(4), t.raised);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    if !control.glyph.is_empty() {
        painter.text(
            pos2(rect.left() + 6.0 + SMALL_GLYPH / 2.0, rect.center().y),
            Align2::CENTER_CENTER,
            control.glyph,
            theme::icon(SMALL_GLYPH),
            if enabled { t.ink } else { t.faint },
        );
    }
    let text_left = if control.glyph.is_empty() {
        6.0
    } else {
        6.0 + SMALL_GLYPH + 4.0
    };
    widgets::fitted_text(
        painter,
        pos2(rect.left() + text_left, rect.center().y),
        Align2::LEFT_CENTER,
        value,
        label_font(),
        if enabled { t.ink } else { t.faint },
        rect.width() - text_left - 18.0,
    );
    paint_chevron(
        painter,
        pos2(rect.right() - 9.0, rect.center().y),
        enabled,
        t,
    );
}

/// A glyph item's own colour (a tag's), lifted in dark mode until it reads on the band,
/// as the tag column lifts it on the page. A pen keeps its ink's colour.
fn glyph_color(item: &GalleryItem, t: Tokens) -> Option<Color32> {
    item.color.map(|color| {
        if t.dark && item.filled.is_none() {
            theme::lifted_on(color, t.raised, 3.0)
        } else {
            color
        }
    })
}

/// An item's glyph centred on `center`: a pen's Filled body in its colour under its
/// outline in ink, or the glyph alone in its own colour (a tag's), else in ink.
fn paint_item_glyph(
    painter: &Painter,
    center: Pos2,
    item: &GalleryItem,
    size: f32,
    enabled: bool,
    t: Tokens,
) {
    let Some(glyph) = item.glyph else {
        return;
    };
    match item.filled {
        Some(filled) => {
            let color = item.color.unwrap_or(t.ink);
            let (color, ink) = if enabled {
                (color, t.ink)
            } else {
                (theme::with_alpha(color, 90), t.faint)
            };
            painter.text(
                center,
                Align2::CENTER_CENTER,
                filled,
                theme::icon_filled(size),
                color,
            );
            painter.text(center, Align2::CENTER_CENTER, glyph, theme::icon(size), ink);
        }
        None => {
            let color = if enabled {
                glyph_color(item, t).unwrap_or(t.ink)
            } else {
                t.faint
            };
            painter.text(
                center,
                Align2::CENTER_CENTER,
                glyph,
                theme::icon(size),
                color,
            );
        }
    }
}

/// One gallery item in its cell: a colour swatch, an icon (a pen, a shape) named by its
/// tooltip, or a glyph and label like a Small button. Checked when it is the current
/// pick.
/// `more` is the gallery's full menu, which the pen in hand opens (Draw's pens).
#[allow(clippy::too_many_arguments)]
fn gallery_item(
    ui: &Ui,
    rect: Rect,
    id: Id,
    item: &GalleryItem,
    control: &Control,
    more: Option<MenuId>,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let swatch_tip = control_tip(control);
    let enabled = host.enabled(&item.cmd);
    let checked = host.checked(&item.cmd);
    let response = interact(ui, rect, id, enabled);
    let painter = ui.painter();
    let response = if let Some(color) = swatch_color(item) {
        paint_swatch(
            painter,
            rect,
            color,
            enabled,
            checked,
            response.hovered(),
            t,
        );
        // Named by its colour, as the editor's colour menus name their swatches, so each
        // one can be told apart.
        let hex = format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b());
        let tip = if swatch_tip.is_empty() {
            hex.as_str()
        } else {
            swatch_tip
        };
        describe(response, Role::Button, enabled, Some(checked), &hex, tip)
    } else if pen_picture(item) {
        // Boxed grey while it is the pen in hand, as the desktop notebook's.
        paint_back(painter, rect, enabled, checked, response.hovered(), t);
        let color = item.color.unwrap_or(t.ink);
        let highlighter = item.glyph == Some(icons::HIGHLIGHTER);
        paint_pen_picture(painter, rect, color, highlighter, enabled, t);
        let name = item_name(item);
        let response = describe(response, Role::Button, enabled, Some(checked), name, name);
        // The pen in hand carries the gallery's menu, as the desktop notebook's does: a chevron at its
        // bottom-right, and a click on the pen again opens it too.
        if let Some(menu) = more.filter(|_| checked) {
            let chevron =
                Rect::from_min_max(rect.right_bottom() - vec2(12.0, 12.0), rect.right_bottom());
            let menu_enabled = host.menu_enabled(menu);
            let arrow = interact(ui, chevron, id.with("more"), menu_enabled);
            paint_chevron(painter, chevron.center(), menu_enabled, t);
            let label = arrow_name(control);
            let arrow = describe(arrow, Role::Button, menu_enabled, None, &label, &label);
            if !menu_enabled {
                return None;
            }
            if enabled && response.clicked() {
                egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&arrow));
            }
            return menu_popup(&arrow, menu, host, t);
        }
        response
    } else if icon_item(item) {
        paint_back(painter, rect, enabled, checked, response.hovered(), t);
        paint_item_glyph(painter, rect.center(), item, ICON_ITEM_GLYPH, enabled, t);
        let name = item_name(item);
        describe(response, Role::Button, enabled, Some(checked), name, name)
    } else if let Some(kind) = &item.preview {
        // The style's name in its own look (a job, so a synthetic slant shows too).
        paint_back(painter, rect, enabled, checked, response.hovered(), t);
        let mut format = editor::style_preview(kind, t);
        if !enabled {
            format.color = t.faint;
        }
        let job = LayoutJob::single_section(item.label.to_string(), format);
        let galley = painter.layout_job(job);
        let at = pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0);
        painter.galley(at, galley, t.ink);
        describe(
            response,
            Role::Button,
            enabled,
            Some(checked),
            &item.label,
            &item.label,
        )
    } else {
        paint_back(painter, rect, enabled, checked, response.hovered(), t);
        let glyph_center = pos2(rect.left() + 6.0 + SMALL_GLYPH / 2.0, rect.center().y);
        let ink = if enabled { t.ink } else { t.faint };
        if item.glyph.is_some() {
            paint_item_glyph(painter, glyph_center, item, SMALL_GLYPH, enabled, t);
        } else if let Cmd::PenWidth(width) = item.cmd {
            // A pen width shows its nib: a dot as thick as the line it draws.
            painter.circle_filled(glyph_center, width / 2.0 + 1.0, ink);
        }
        painter.text(
            pos2(rect.left() + 6.0 + SMALL_GLYPH + 4.0, rect.center().y),
            Align2::LEFT_CENTER,
            item.label.as_ref(),
            label_font(),
            ink,
        );
        describe(
            response,
            Role::Button,
            enabled,
            Some(checked),
            &item.label,
            &item.label,
        )
    };
    (enabled && response.clicked()).then(|| item.cmd.clone())
}

/// One pen of the gallery, painted as the desktop notebook's are: a barrel in the pen's colour over a
/// dark cone and a nib in its colour; a highlighter has a wider barrel, a collar and a
/// chisel tip.
fn paint_pen_picture(
    painter: &Painter,
    cell: Rect,
    color: Color32,
    highlighter: bool,
    enabled: bool,
    t: Tokens,
) {
    let color = if enabled {
        color
    } else {
        theme::with_alpha(color, 90)
    };
    let dark = if enabled {
        Color32::from_rgb(0x24, 0x24, 0x24)
    } else {
        t.faint
    };
    // In dark mode the black pen and the cone keep an edge against the band.
    let edge = Stroke::new(
        1.0,
        if t.dark {
            t.line_strong
        } else {
            Color32::TRANSPARENT
        },
    );
    let cx = cell.center().x;
    let top = cell.top() + 6.0;
    let (w, h) = if highlighter {
        (20.0, 22.0)
    } else {
        (14.0, 24.0)
    };
    let barrel = Rect::from_min_size(pos2(cx - w / 2.0, top), vec2(w, h));
    let cap = CornerRadius {
        nw: 2,
        ne: 2,
        sw: 0,
        se: 0,
    };
    painter.rect_filled(barrel, cap, color);
    painter.rect_stroke(barrel, cap, edge, StrokeKind::Inside);
    let y = barrel.bottom();
    if highlighter {
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(barrel.left(), y),
                pos2(barrel.right(), y),
                pos2(cx + 6.0, y + 10.0),
                pos2(cx - 6.0, y + 10.0),
            ],
            dark,
            edge,
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(cx - 6.0, y + 10.0),
                pos2(cx + 6.0, y + 10.0),
                pos2(cx + 6.0, y + 13.0),
                pos2(cx - 6.0, y + 18.0),
            ],
            color,
            edge,
        ));
    } else {
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(barrel.left(), y),
                pos2(barrel.right(), y),
                pos2(cx + 2.5, y + 14.0),
                pos2(cx - 2.5, y + 14.0),
            ],
            dark,
            edge,
        ));
        painter.rect_filled(
            Rect::from_min_size(pos2(cx - 2.5, y + 14.0), vec2(5.0, 5.0)),
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 2,
                se: 2,
            },
            color,
        );
    }
}

fn paint_swatch(
    painter: &Painter,
    rect: Rect,
    color: Color32,
    enabled: bool,
    checked: bool,
    hovered: bool,
    t: Tokens,
) {
    let fill = if enabled {
        color
    } else {
        theme::with_alpha(color, 90)
    };
    painter.rect_filled(rect, CornerRadius::same(3), fill);
    let (width, stroke) = if checked {
        (2.0, t.accent)
    } else if hovered && enabled {
        (1.0, t.muted)
    } else {
        (1.0, t.line_strong)
    };
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(width, stroke),
        StrokeKind::Inside,
    );
}

/// A gallery folded to an Icon menu button (scaled down, or Simplified), or to a labelled
/// dropdown in a rect wider than an icon (Simplified's "Styles", `keeps_label`). It opens
/// the gallery's full menu, or its items when it has none.
#[allow(clippy::too_many_arguments)]
fn gallery_button(
    ui: &Ui,
    rect: Rect,
    id: Id,
    control: &Control,
    items: &[GalleryItem],
    more: Option<MenuId>,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let enabled = match more {
        Some(menu) => host.menu_enabled(menu),
        None => items.iter().any(|item| host.enabled(&item.cmd)),
    };
    let response = interact(ui, rect, id, enabled);
    let painter = ui.painter();
    paint_back(painter, rect, enabled, false, response.hovered(), t);
    // Wider than an icon: Simplified's labelled dropdown (`keeps_label`), glyph, label
    // and chevron as a Small menu button draws them.
    let labelled = rect.width() > ICON_WIDTH + ICON_ARROW + 0.5;
    let (arrow, center) = if labelled {
        painter.text(
            pos2(rect.left() + 6.0 + SMALL_GLYPH + 4.0, rect.center().y),
            Align2::LEFT_CENTER,
            control.label.as_ref(),
            label_font(),
            if enabled { t.ink } else { t.faint },
        );
        let arrow = menu_chevron(rect, Size::Small).unwrap_or(rect);
        (
            arrow,
            pos2(rect.left() + 6.0 + SMALL_GLYPH / 2.0, rect.center().y),
        )
    } else {
        let arrow = menu_chevron(rect, Size::Icon).unwrap_or(rect);
        (
            arrow,
            pos2((rect.left() + arrow.left()) / 2.0, rect.center().y),
        )
    };
    // Its own glyph, else the current (or first) item's: a glyph, or its swatch.
    let current = items
        .iter()
        .find(|item| host.checked(&item.cmd))
        .or(items.first());
    if !control.glyph.is_empty() {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            control.glyph,
            theme::icon(SMALL_GLYPH),
            if enabled {
                control
                    .glyph_color
                    .map_or(t.ink, |color| glyph_tint(color, t))
            } else {
                t.faint
            },
        );
    } else if let Some(item) = current {
        match (item.glyph, swatch_color(item)) {
            (_, Some(color)) => paint_swatch(
                painter,
                Rect::from_center_size(center, Vec2::splat(14.0)),
                color,
                enabled,
                false,
                false,
                t,
            ),
            (Some(_), None) => {
                paint_item_glyph(painter, center, item, SMALL_GLYPH, enabled, t);
            }
            (None, None) => {}
        }
    }
    paint_chevron(painter, arrow.center(), enabled, t);
    let response = describe(
        response,
        Role::Button,
        enabled,
        None,
        control_name(control),
        control_tip(control),
    );
    if !enabled {
        return None;
    }
    match more {
        Some(menu) => menu_popup(&response, menu, host, t),
        None => egui::Popup::menu(&response)
            .show(|ui| gallery_list(ui, control, items, None, host, t))
            .and_then(|shown| shown.inner),
    }
}

// The Classic band: labelled groups side by side, each at the level that lets them all
// fit the window.

/// Draws `groups` from the cursor, shrinking them to fit the width left (minus the room
/// for the display-options chevron), and returns the first command picked.
fn classic_band(ui: &mut Ui, groups: &[Group], host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    let ctx = ui.ctx().clone();
    let widths: Vec<[Option<f32>; 4]> = groups
        .iter()
        .map(|group| std::array::from_fn(|level| group_width(&ctx, group, level)))
        .collect();
    let priorities: Vec<u8> = groups.iter().map(|group| group.priority).collect();
    let available = ui.available_width() - OPTIONS_ROOM;
    let levels = fit_levels(&widths, &priorities, available);
    let origin = ui.cursor().min + vec2(0.0, BAND_TOP);
    let right = ui.max_rect().right();
    let mut x = origin.x;
    let mut picked = None;
    for (index, (group, &level)) in groups.iter().zip(&levels).enumerate() {
        // Only when even the smallest levels do not fit: what is past the card's edge
        // stays out rather than widening the card.
        if x >= right {
            break;
        }
        let width = widths[index][level].unwrap_or(0.0);
        let rect = Rect::from_min_size(pos2(x, origin.y), vec2(width, CONTROLS_HEIGHT + LABEL_ROW));
        let cmd = draw_group(ui, rect, group, level, host, t);
        picked = picked.or(cmd);
        x += width;
        if index + 1 < groups.len() {
            let middle = rect.center().y;
            ui.painter().vline(
                x,
                (middle - SEPARATOR_HEIGHT / 2.0)..=(middle + SEPARATOR_HEIGHT / 2.0),
                Stroke::new(1.0, t.line),
            );
        }
    }
    picked
}

/// One group in `rect` (its control area and label row) at `level`.
fn draw_group(
    ui: &mut Ui,
    rect: Rect,
    group: &Group,
    level: usize,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let area = Rect::from_min_size(rect.min, vec2(rect.width(), CONTROLS_HEIGHT));
    if level == COLLAPSED {
        return collapsed_group(ui, area, group, host, t);
    }
    let (cells, content) = group_cells(ui.ctx(), group, level)?;
    // A label wider than the controls centres them under it.
    let offset = vec2(rect.left() + (rect.width() - content) / 2.0, rect.top());
    let mut picked = None;
    for (index, size, cell) in cells {
        let cmd = draw_control(
            ui,
            cell.translate(offset),
            &group.controls[index],
            size,
            host,
            t,
        );
        picked = picked.or(cmd);
    }
    let label_row = Rect::from_min_size(
        pos2(rect.left(), area.bottom()),
        vec2(rect.width(), LABEL_ROW),
    );
    ui.painter().text(
        label_row.center(),
        Align2::CENTER_CENTER,
        group.label.as_ref(),
        group_label_font(),
        t.muted,
    );
    picked
}

/// A collapsed group: one Large menu button with the group's glyph and name, whose menu
/// lists the group's controls as rows. It stays clickable even when they are all greyed,
/// so their state can still be seen.
fn collapsed_group(
    ui: &mut Ui,
    area: Rect,
    group: &Group,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let size = collapsed_size(ui.ctx(), group);
    let rect = Rect::from_center_size(pos2(area.center().x, area.top() + size.y / 2.0), size);
    ui.advance_cursor_after_rect(rect);
    let id = Id::unique(("folio-ribbon-group", group.id));
    let response = interact(ui, rect, id, true);
    let open = menu_open(&response);
    paint_back(ui.painter(), rect, true, open, response.hovered(), t);
    // Painted as a Large control would be; it is not one of the group's own controls.
    let face = Control {
        kind: ControlKind::Button,
        cmd: None,
        glyph: group.glyph,
        label: group.label.clone(),
        tooltip: group.label.clone(),
        size: Size::Large,
        accent: None,
        glyph_color: None,
        arrow_label: None,
        glyph_size: SMALL_GLYPH,
    };
    paint_large(ui, rect, &face, true, true, t);
    let response = describe(
        response,
        Role::Button,
        true,
        None,
        &group.label,
        &group.label,
    );
    egui::Popup::menu(&response)
        .show(|ui| match group.controls.as_slice() {
            // A group that is one gallery (Styles) drops the gallery itself, as Office
            // does, rather than a menu whose one row opens it.
            [control @ Control {
                kind: ControlKind::Gallery { items, more },
                ..
            }] => {
                ui.set_min_width(200.0);
                gallery_list(ui, control, items, *more, host, t)
            }
            controls => menu_rows(ui, controls, host, t),
        })
        .and_then(|shown| shown.inner)
}

// Menus: a collapsed group and Simplified's "⋯" list their controls as menu rows.

/// `controls` as menu rows: buttons run their command, menus, combos and galleries
/// become submenus, and a Split gives both a row and a submenu. Checked commands carry a
/// check mark.
pub fn menu_rows(
    ui: &mut Ui,
    controls: &[Control],
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    ui.set_min_width(200.0);
    let mut picked = None;
    for control in controls {
        let name = control_name(control);
        let glyph = (!control.glyph.is_empty()).then_some(control.glyph);
        let row = match &control.kind {
            ControlKind::Button | ControlKind::Toggle => match &control.cmd {
                Some(cmd) => {
                    let checkable = matches!(control.kind, ControlKind::Toggle);
                    command_row(ui, glyph, name, cmd, checkable, host, t)
                }
                None => {
                    // A readout: shown, not clickable.
                    ui.add_enabled_ui(false, |ui| widgets::menu_item(ui, glyph, name, t));
                    None
                }
            },
            ControlKind::Split(menu) => {
                let main = control
                    .cmd
                    .as_ref()
                    .and_then(|cmd| command_row(ui, glyph, name, cmd, true, host, t));
                let arrow = submenu(
                    ui,
                    glyph,
                    &arrow_name(control),
                    host.menu_enabled(*menu),
                    t,
                    |ui| host.menu(ui, *menu, t),
                );
                main.or(arrow)
            }
            ControlKind::Menu(menu) | ControlKind::Combo { menu, .. } => {
                submenu(ui, glyph, name, host.menu_enabled(*menu), t, |ui| {
                    host.menu(ui, *menu, t)
                })
            }
            ControlKind::Gallery { items, more } => submenu(ui, glyph, name, true, t, |ui| {
                gallery_list(ui, control, items, *more, host, t)
            }),
        };
        if row.is_some() {
            picked = row;
        }
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// One command as a menu row, greyed when disabled.
fn command_row(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: &str,
    cmd: &Cmd,
    checkable: bool,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let enabled = host.enabled(cmd);
    let row = if checkable {
        widgets::checked_menu_row(ui, glyph, label, "", enabled, host.checked(cmd), t)
    } else {
        ui.add_enabled_ui(enabled, |ui| widgets::menu_item(ui, glyph, label, t))
            .inner
    };
    row.clicked().then(|| cmd.clone())
}

/// A row that opens `contents` as a submenu.
fn submenu(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: &str,
    enabled: bool,
    t: Tokens,
    contents: impl FnOnce(&mut Ui) -> Option<Cmd>,
) -> Option<Cmd> {
    let text = match glyph {
        Some(glyph) => format!("{glyph}    {label}"),
        None => label.to_owned(),
    };
    ui.add_enabled_ui(enabled, |ui| {
        ui.menu_button(
            RichText::new(text).font(theme::ui(12.5)).color(t.ink),
            |ui| {
                let picked = contents(ui);
                if picked.is_some() {
                    ui.close();
                }
                picked
            },
        )
        .inner
        .flatten()
    })
    .inner
}

/// A gallery's items as a menu: swatches in a row, the others as rows, then its full
/// menu (`more`) as a submenu.
fn gallery_list(
    ui: &mut Ui,
    control: &Control,
    items: &[GalleryItem],
    more: Option<MenuId>,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let mut picked = None;
    let base = control_id(control).with("list");
    if items.iter().any(|item| swatch_color(item).is_some()) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SWATCH_GAP;
            for (index, item) in items.iter().enumerate() {
                if swatch_color(item).is_none() {
                    continue;
                }
                let (_, rect) = ui.allocate_space(Vec2::splat(SWATCH));
                let id = base.with(index);
                if let Some(cmd) = gallery_item(ui, rect, id, item, control, None, host, t) {
                    picked = Some(cmd);
                }
            }
        });
    }
    for item in items.iter().filter(|item| swatch_color(item).is_none()) {
        let enabled = host.enabled(&item.cmd);
        // An icon item's row is named by its tooltip ("Red pen, 0.5 mm").
        let checked = host.checked(&item.cmd);
        let row = ui
            .add_enabled_ui(enabled, |ui| match &item.preview {
                // A style keeps its preview, named by its label alone.
                Some(kind) => {
                    let format = editor::style_preview(kind, t);
                    let job = LayoutJob::single_section(item.label.to_string(), format);
                    widgets::checked_preview_item(ui, job, &item.label, checked, t)
                }
                None => widgets::tinted_checked_menu_item(
                    ui,
                    item.glyph,
                    glyph_color(item, t),
                    item_name(item),
                    checked,
                    t,
                ),
            })
            .inner;
        if row.clicked() {
            picked = Some(item.cmd.clone());
        }
    }
    if let Some(more) = more {
        ui.separator();
        let label = arrow_name(control);
        if let Some(cmd) = submenu(ui, None, &label, host.menu_enabled(more), t, |ui| {
            host.menu(ui, more, t)
        }) {
            picked = Some(cmd);
        }
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

// The tab row, the card around the band, the Simplified layout, the display options and
// `show`, which puts them together.

/// The tabs start 12 px in, 4 px apart, with 10 px of padding each side of their text;
/// the contextual tabs keep 12 px from Help, as Office sets them apart.
const TAB_START: f32 = 12.0;
const TAB_GAP: f32 = 4.0;
const TAB_PAD: f32 = 10.0;
const CONTEXTUAL_GAP: f32 = 12.0;
/// The selected tab's purple underline: 3 px thick, 3 px above the row's bottom.
const UNDERLINE: f32 = 3.0;
/// The Simplified row of controls, and the line between two of its groups.
const SIMPLIFIED_ROW: f32 = 28.0;
const SIMPLIFIED_SEPARATOR: f32 = 20.0;
/// The room "⋯" takes after the last Simplified group that fits.
const MORE_ROOM: f32 = 32.0;
/// The display-options chevron at the card's bottom-right corner.
const OPTIONS_SIZE: f32 = 20.0;

fn tab_font() -> FontId {
    theme::ui(13.0)
}

/// The selected tab's font: Office sets it in semibold.
fn selected_tab_font() -> FontId {
    theme::semibold(13.0)
}

/// The tab row: File, the band tabs and the contextual tabs the host asks for, the
/// current one underlined in purple. Clicking a tab shows its band, over the page when
/// the band is hidden; File returns `Cmd::Backstage`, which opens Backstage.
fn tab_row(ui: &mut Ui, state: &mut RibbonState, host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    let contextual = host.contextual();
    // A contextual tab leaves with what it acts on; File opens Backstage, never a band.
    if state.tab == RibbonTab::File || (state.tab.contextual() && !contextual.contains(&state.tab))
    {
        state.tab = RibbonTab::Home;
    }
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_ROW), Sense::hover());
    let ctx = ui.ctx().clone();
    let mut x = row.left() + TAB_START;
    let mut picked = None;
    for (index, &tab) in RibbonTab::MAIN.iter().chain(&contextual).enumerate() {
        if index == RibbonTab::MAIN.len() {
            x += CONTEXTUAL_GAP;
        } else if index > 0 {
            x += TAB_GAP;
        }
        let label = tab.label();
        let selected = tab == state.tab;
        // Office sets the selected tab in semibold; every slot is measured in semibold so
        // nothing shifts when the selection moves.
        let slot = widgets::text_width(&ctx, &label, selected_tab_font());
        let font = if selected {
            selected_tab_font()
        } else {
            tab_font()
        };
        let text = widgets::text_width(&ctx, &label, font.clone());
        let rect = Rect::from_min_size(pos2(x, row.top()), vec2(slot + 2.0 * TAB_PAD, TAB_ROW));
        x = rect.right();
        let response = ui.interact(rect, Id::unique(("folio-ribbon-tab", tab)), Sense::click());
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect.shrink2(vec2(0.0, 3.0)), CornerRadius::same(4), t.hover);
        }
        // Office colours its contextual tabs so they stand out from the fixed ones.
        let color = if selected {
            t.ink
        } else if tab.contextual() {
            t.accent
        } else {
            t.muted
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label.as_ref(),
            font,
            color,
        );
        if selected {
            let underline = Rect::from_center_size(
                pos2(rect.center().x, row.bottom() - UNDERLINE * 1.5),
                vec2(text, UNDERLINE),
            );
            painter.rect_filled(underline, CornerRadius::same(2), t.accent);
        }
        response.widget_info(|| {
            let mut info = WidgetInfo::labeled(Role::Tab, true, label.as_ref());
            info.selected = Some(selected);
            info
        });
        if tab == RibbonTab::File {
            if response.clicked() {
                picked = Some(Cmd::Backstage);
            }
        } else if response.clicked() {
            state.tab = tab;
            if !state.band_shown() {
                state.overlay = true;
            }
        }
    }
    picked
}

/// Controls the Simplified ribbon labels, as the desktop notebook's does: Styles. (Email Page,
/// Meeting Details and Copilot, which the desktop notebook also labels, are not in Folio.)
fn keeps_label(control: &Control) -> bool {
    matches!(
        &control.kind,
        ControlKind::Gallery {
            more: Some(MenuId::Styles),
            ..
        }
    )
}

/// The size a control takes in the Simplified row: an icon (a combo keeps its box). A
/// readout keeps its text, which is all it is there for.
fn simplified_size(control: &Control) -> Size {
    if matches!(control.kind, ControlKind::Button) && control.cmd.is_none() {
        Size::Small
    } else {
        Size::Icon
    }
}

/// How many Simplified groups fit in `available`, given the room each takes (the gap
/// before it included): all of them when they fit, else those that leave room for "⋯"
/// after them, as the rest go into its menu.
fn simplified_fit(widths: &[f32], available: f32) -> usize {
    if widths.iter().sum::<f32>() <= available {
        return widths.len();
    }
    let mut used = 0.0;
    widths
        .iter()
        .take_while(|&&width| {
            used += width;
            used <= available - MORE_ROOM
        })
        .count()
}

/// The Simplified band: one row of icons with a short line between groups; Styles keeps
/// its label (`keeps_label`). The groups that do not fit go, whole, into the "⋯" menu
/// that ends the row.
fn simplified_band(ui: &mut Ui, groups: &[Group], host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    let ctx = ui.ctx().clone();
    let inner = ui.max_rect();
    let middle = inner.center().y;
    let sized: Vec<Vec<(Size, Vec2)>> = groups
        .iter()
        .map(|group| {
            group
                .controls
                .iter()
                .map(|control| {
                    let size = simplified_size(control);
                    if keeps_label(control) {
                        // A labelled dropdown at Small height ("Styles ⌄"), still drawn
                        // by `gallery_button` as an Icon gallery is.
                        let label = widgets::text_width(&ctx, &control.label, label_font());
                        let width = 6.0 + SMALL_GLYPH + 4.0 + label + SMALL_ARROW + 6.0;
                        return (size, vec2(width, SMALL_HEIGHT));
                    }
                    (size, control_size(&ctx, control, size))
                })
                .collect()
        })
        .collect();
    // The gap before a group: padding, then the line and padding after the first.
    let lead = |index: usize| {
        if index == 0 {
            GROUP_PAD
        } else {
            2.0 * GROUP_PAD + 1.0
        }
    };
    let widths: Vec<f32> = sized
        .iter()
        .enumerate()
        .map(|(index, controls)| {
            let content: f32 = controls.iter().map(|(_, extent)| extent.x).sum();
            let gaps = COLUMN_GAP * controls.len().saturating_sub(1) as f32;
            lead(index) + content + gaps
        })
        .collect();
    let fit = simplified_fit(&widths, inner.width() - OPTIONS_ROOM);
    let mut picked = None;
    let mut x = inner.left();
    for (index, (group, controls)) in groups.iter().zip(&sized).enumerate().take(fit) {
        if index > 0 {
            ui.painter().vline(
                x + GROUP_PAD + 0.5,
                (middle - SIMPLIFIED_SEPARATOR / 2.0)..=(middle + SIMPLIFIED_SEPARATOR / 2.0),
                Stroke::new(1.0, t.line),
            );
        }
        x += lead(index);
        for (number, (control, &(size, extent))) in group.controls.iter().zip(controls).enumerate()
        {
            if number > 0 {
                x += COLUMN_GAP;
            }
            // A combo keeps its box's height; the other controls fill the row.
            let height = if matches!(control.kind, ControlKind::Combo { .. }) {
                extent.y
            } else {
                SIMPLIFIED_ROW
            };
            let rect = Rect::from_min_size(pos2(x, middle - height / 2.0), vec2(extent.x, height));
            let cmd = draw_control(ui, rect, control, size, host, t);
            picked = picked.or(cmd);
            x += extent.x;
        }
    }
    if fit < groups.len() {
        let rect = Rect::from_min_size(
            pos2(
                x + MORE_ROOM - SIMPLIFIED_ROW,
                middle - SIMPLIFIED_ROW / 2.0,
            ),
            Vec2::splat(SIMPLIFIED_ROW),
        );
        let cmd = more_button(ui, rect, &groups[fit..], host, t);
        picked = picked.or(cmd);
    }
    picked
}

/// Simplified's "⋯": the groups that did not fit in the row, as menu rows.
fn more_button(
    ui: &mut Ui,
    rect: Rect,
    groups: &[Group],
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    ui.advance_cursor_after_rect(rect);
    let response = interact(ui, rect, Id::unique("folio-ribbon-more"), true);
    paint_back(ui.painter(), rect, true, false, response.hovered(), t);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icons::DOTS_THREE,
        theme::icon(SMALL_GLYPH),
        t.ink,
    );
    let label = t!("ribbon-more");
    let response = describe(response, Role::Button, true, None, &label, &label);
    egui::Popup::menu(&response)
        .show(|ui| {
            let mut picked = None;
            for (index, group) in groups.iter().enumerate() {
                if index > 0 {
                    ui.separator();
                }
                let cmd = menu_rows(ui, &group.controls, host, t);
                picked = picked.or(cmd);
            }
            picked
        })
        .and_then(|shown| shown.inner)
}

/// The chevron in the card's bottom-right `corner` and its menu: the Classic or the
/// Simplified layout, and whether the band always shows or only the tabs do.
fn options_button(ui: &mut Ui, corner: Pos2, state: &RibbonState, t: Tokens) -> Option<Cmd> {
    let rect = Rect::from_min_max(corner - Vec2::splat(OPTIONS_SIZE), corner);
    let response = interact(ui, rect, Id::unique("folio-ribbon-options"), true);
    paint_back(ui.painter(), rect, true, false, response.hovered(), t);
    paint_chevron(ui.painter(), rect.center(), true, t);
    let label = t!("ribbon-display-options");
    let response = describe(response, Role::Button, true, None, &label, &label);
    egui::Popup::menu(&response)
        .show(|ui| {
            ui.set_min_width(220.0);
            let mut picked = None;
            let mut row = |ui: &mut Ui, label: Cow<'static, str>, checked: bool, cmd: Cmd| {
                let row = widgets::checked_menu_item(ui, None, &label, checked, t);
                if row.clicked() {
                    picked = Some(cmd);
                }
            };
            for (layout, label) in [
                (RibbonLayout::Classic, t!("ribbon-classic")),
                (RibbonLayout::Simplified, t!("ribbon-simplified")),
            ] {
                row(ui, label, state.layout == layout, Cmd::RibbonLayout(layout));
            }
            ui.separator();
            for (collapsed, label) in [
                (false, t!("ribbon-always-show")),
                (true, t!("ribbon-tabs-only")),
            ] {
                row(
                    ui,
                    label,
                    state.collapsed == collapsed,
                    Cmd::CollapseRibbon(collapsed),
                );
            }
            if picked.is_some() {
                ui.close();
            }
            picked
        })
        .and_then(|shown| shown.inner)
}

/// The band for `groups` in its card, in the current layout, with the display options
/// at its bottom-right.
fn band(
    ui: &mut Ui,
    groups: &[Group],
    state: &RibbonState,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let mut picked = None;
    widgets::card(t)
        .inner_margin(egui::Margin::symmetric(4, 4))
        .outer_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 0,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let height = match state.layout {
                RibbonLayout::Classic => CLASSIC_HEIGHT - 8.0,
                RibbonLayout::Simplified => SIMPLIFIED_HEIGHT - 8.0,
            };
            ui.set_height(height);
            picked = match state.layout {
                RibbonLayout::Classic => classic_band(ui, groups, host, t),
                RibbonLayout::Simplified => simplified_band(ui, groups, host, t),
            };
            let corner = ui.max_rect().right_bottom();
            if let Some(cmd) = options_button(ui, corner, state, t) {
                picked = Some(cmd);
            }
        });
    picked
}

/// The ribbon: the tab row, then the current tab's band in its card. When the band is
/// hidden (Show tabs only, Full Page View), clicking a tab opens it over the page until a
/// command runs, Esc is pressed or the click lands elsewhere. Returns the command picked.
pub fn show(ui: &mut Ui, t: Tokens, state: &mut RibbonState, host: &dyn RibbonHost) -> Option<Cmd> {
    // The tab row and the card stack flush, so the panel is exactly `panel_height` tall.
    ui.spacing_mut().item_spacing.y = 0.0;
    let panel = ui.max_rect();
    let mut overlay_layer = None;
    // A click a popup took, or that closed it, is not a click away from the band.
    let popup_open = egui::Popup::is_any_open(ui.ctx());
    let (tab_before, open_before) = (state.tab, state.overlay);
    let mut picked = tab_row(ui, state, host, t);
    // Nor is the click on a tab that opened the band over the page or switched it.
    let tab_clicked = state.tab != tab_before || (state.overlay && !open_before);
    let groups = groups(state.tab, host);
    if state.band_shown() {
        state.overlay = false;
        let cmd = band(ui, &groups, state, host, t);
        picked = picked.or(cmd);
    } else if state.overlay {
        let top = ui.min_rect().bottom();
        let screen = ui.ctx().content_rect();
        let shown: &RibbonState = state;
        let area = egui::Area::new(Id::unique("folio-ribbon-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.left(), top))
            .show(ui.ctx(), |ui| {
                ui.set_width(screen.width());
                band(ui, &groups, shown, host, t)
            });
        // Esc is only read, not consumed: it still leaves Full Page View or a note.
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        let elsewhere = area.response.clicked_elsewhere() && !tab_clicked && !popup_open;
        if area.inner.is_some() || escape || elsewhere {
            state.overlay = false;
        }
        overlay_layer = Some(area.response.layer_id);
        picked = picked.or(area.inner);
    }
    // A click on the ribbon itself: the panel (on the background, under every area) or
    // the band over the page, not a menu or dialog above them.
    let ctx = ui.ctx().clone();
    let panel_layer = ui.layer_id();
    let on_ribbon = |pos: Pos2| match ctx.layer_id_at(pos) {
        Some(layer) if Some(layer) == overlay_layer => true,
        Some(layer) if layer != panel_layer => false,
        _ => panel.contains(pos),
    };
    let click = ctx.input(|input| {
        input
            .pointer
            .primary_clicked()
            .then(|| input.pointer.interact_pos())
            .flatten()
    });
    state.idle_click =
        picked.is_none() && !egui::Popup::is_any_open(&ctx) && click.is_some_and(on_ribbon);
    picked
}

// The tabs' contents: the desktop notebook's groups, holding the commands Folio has. Each band is
// data, rebuilt every frame, so its labels follow the interface language.

/// Each tab's groups, left to right. File opens Backstage, so it has no band.
pub fn groups(tab: RibbonTab, host: &dyn RibbonHost) -> Vec<Group> {
    match tab {
        RibbonTab::File => Vec::new(),
        RibbonTab::Home => home_groups(&host.tag_gallery()),
        RibbonTab::Insert => insert_groups(),
        RibbonTab::Draw => draw_groups(host),
        RibbonTab::History => history_groups(),
        RibbonTab::Review => review_groups(),
        RibbonTab::View => view_groups(),
        RibbonTab::Help => help_groups(),
        RibbonTab::Table => table_groups(),
        RibbonTab::Picture => picture_groups(),
        RibbonTab::Equation => equation_groups(),
        RibbonTab::Recording => recording_groups(host),
    }
}

fn control(
    kind: ControlKind,
    cmd: Option<Cmd>,
    size: Size,
    glyph: &'static str,
    label: Cow<'static, str>,
) -> Control {
    Control {
        kind,
        cmd,
        glyph,
        label,
        tooltip: Cow::Borrowed(""),
        size,
        accent: None,
        glyph_color: None,
        arrow_label: None,
        glyph_size: SMALL_GLYPH,
    }
}

fn button(size: Size, glyph: &'static str, label: Cow<'static, str>, cmd: Cmd) -> Control {
    control(ControlKind::Button, Some(cmd), size, glyph, label)
}

fn toggle(size: Size, glyph: &'static str, label: Cow<'static, str>, cmd: Cmd) -> Control {
    control(ControlKind::Toggle, Some(cmd), size, glyph, label)
}

fn split(
    size: Size,
    glyph: &'static str,
    label: Cow<'static, str>,
    cmd: Cmd,
    menu: MenuId,
) -> Control {
    control(ControlKind::Split(menu), Some(cmd), size, glyph, label)
}

fn menu(size: Size, glyph: &'static str, label: Cow<'static, str>, menu: MenuId) -> Control {
    control(ControlKind::Menu(menu), None, size, glyph, label)
}

/// A font or size box, named by what it picks; it shows the host's current value.
fn combo(label: Cow<'static, str>, menu: MenuId, width: f32) -> Control {
    control(
        ControlKind::Combo { menu, width },
        None,
        Size::Small,
        "",
        label,
    )
}

fn gallery(label: Cow<'static, str>, items: Vec<GalleryItem>, more: Option<MenuId>) -> Control {
    control(
        ControlKind::Gallery { items, more },
        None,
        Size::Large,
        "",
        label,
    )
}

fn item(
    cmd: Cmd,
    label: Cow<'static, str>,
    glyph: Option<&'static str>,
    color: Option<Color32>,
) -> GalleryItem {
    GalleryItem {
        cmd,
        label,
        glyph,
        color,
        // A labelled item is named by its label; a swatch by the gallery's tooltip.
        tooltip: Cow::Borrowed(""),
        filled: None,
        preview: None,
    }
}

fn tip(mut control: Control, tooltip: Cow<'static, str>) -> Control {
    control.tooltip = tooltip;
    control
}

fn group(
    id: &'static str,
    label: Cow<'static, str>,
    glyph: &'static str,
    priority: u8,
    controls: Vec<Control>,
) -> Group {
    Group {
        id,
        label,
        glyph,
        priority,
        layout: GroupLayout::Flow,
        controls,
    }
}

fn text(command: ToolbarCommand) -> Cmd {
    Cmd::Text(command)
}

/// Undo and Redo, first on Home and Draw as in the desktop notebook: two stacked icons with no label,
/// as the desktop notebook draws them (the label stays the tooltip and the accessible name).
fn undo_group() -> Group {
    // the desktop notebook draws Undo and Redo bigger than the other icon buttons.
    let big = |mut control: Control| {
        control.glyph_size = UNDO_GLYPH;
        control
    };
    group(
        "undo",
        t!("ribbon-group-undo"),
        icons::ARROW_COUNTER_CLOCKWISE,
        9,
        vec![
            big(button(
                Size::Icon,
                icons::ARROW_COUNTER_CLOCKWISE,
                t!("common-undo"),
                Cmd::Undo,
            )),
            big(button(
                Size::Icon,
                icons::ARROW_CLOCKWISE,
                t!("common-redo"),
                Cmd::Redo,
            )),
        ],
    )
}

/// Delete: the selected text or the paragraph being typed in, else the selected items.
fn delete_button(size: Size, glyph: &'static str) -> Control {
    tip(
        button(size, glyph, t!("ribbon-delete"), Cmd::Delete),
        t!("ribbon-delete-tip"),
    )
}

/// The red of the Font Colour button: the red among the ink colours.
pub(crate) const FONT_RED: Rgb = Rgb::INKS[3];

fn home_groups(tags: &TagGallery) -> Vec<Group> {
    use ToolbarCommand as Do;
    let mark = |glyph: &'static str, label: Cow<'static, str>, mark: Mark| {
        toggle(Size::Icon, glyph, label, text(Do::Toggle(mark)))
    };
    // Office's yellow, the first of Text Highlight Color's colours.
    let highlighter = Rgb::TEXT_HIGHLIGHTS[0].0;
    // the desktop notebook's Clipboard: Paste (its arrow holds the paste options) beside Cut, Copy and
    // Format Painter stacked.
    let clipboard = group(
        "clipboard",
        t!("ribbon-group-clipboard"),
        icons::CLIPBOARD_TEXT,
        7,
        vec![
            Control {
                arrow_label: Some(t!("paste-options")),
                ..tip(
                    split(
                        Size::Large,
                        icons::CLIPBOARD_TEXT,
                        t!("ribbon-paste"),
                        Cmd::Paste(PasteMode::Default),
                        MenuId::Paste,
                    ),
                    t!("ribbon-paste-tip"),
                )
            },
            tip(
                button(Size::Small, icons::SCISSORS, t!("ribbon-cut"), Cmd::Cut),
                t!("ribbon-cut-tip"),
            ),
            tip(
                button(Size::Small, icons::COPY, t!("ribbon-copy"), Cmd::Copy),
                t!("ribbon-copy-tip"),
            ),
            tip(
                toggle(
                    Size::Small,
                    icons::PAINT_BRUSH,
                    t!("ribbon-format-painter"),
                    Cmd::FormatPainter,
                ),
                t!("ribbon-format-painter-tip"),
            ),
        ],
    );
    // the desktop notebook's two rows: font, size, lists, indents and Clear Formatting over the marks,
    // scripts, colours, alignment and Delete.
    let basic_text = Group {
        layout: GroupLayout::Rows(vec![7, 9]),
        ..group(
            "basic-text",
            t!("ribbon-group-basic-text"),
            icons::TEXT_AA,
            8,
            vec![
                combo(t!("editor-font"), MenuId::Fonts, 120.0),
                combo(t!("editor-font-size"), MenuId::Sizes, 44.0),
                // The main part makes a list or ends it, as P2's toggles did; the arrow
                // opens the library of markers.
                split(
                    Size::Icon,
                    icons::TEXT_BULLET_LIST,
                    t!("ribbon-bullets"),
                    text(Do::SetKind(BlockKind::Bullet)),
                    MenuId::Bullets,
                ),
                split(
                    Size::Icon,
                    icons::LIST_NUMBERS,
                    t!("ribbon-numbering"),
                    text(Do::SetKind(BlockKind::Numbered)),
                    MenuId::Numbering,
                ),
                tip(
                    button(
                        Size::Icon,
                        icons::TEXT_INDENT_DECREASE_LTR,
                        t!("ribbon-decrease-indent"),
                        text(Do::Indent(-1)),
                    ),
                    t!("ribbon-decrease-indent-tip"),
                ),
                tip(
                    button(
                        Size::Icon,
                        icons::TEXT_INDENT_INCREASE_LTR,
                        t!("ribbon-increase-indent"),
                        text(Do::Indent(1)),
                    ),
                    t!("ribbon-increase-indent-tip"),
                ),
                button(
                    Size::Icon,
                    icons::TEXT_CLEAR_FORMATTING,
                    t!("editor-clear-formatting"),
                    text(Do::ClearFormatting),
                ),
                mark(icons::TEXT_B, t!("editor-bold"), Mark::Bold),
                mark(icons::TEXT_ITALIC, t!("editor-italic"), Mark::Italic),
                mark(
                    icons::TEXT_UNDERLINE,
                    t!("editor-underline"),
                    Mark::Underline,
                ),
                mark(
                    icons::TEXT_STRIKETHROUGH,
                    t!("editor-strikethrough"),
                    Mark::Strike,
                ),
                split(
                    Size::Icon,
                    icons::TEXT_SUBSCRIPT,
                    t!("editor-subscript"),
                    text(Do::Toggle(Mark::Subscript)),
                    MenuId::Script,
                ),
                Control {
                    accent: Some(highlighter.to_color32()),
                    ..split(
                        Size::Icon,
                        icons::HIGHLIGHTER,
                        t!("editor-highlight"),
                        text(Do::Highlight(Some(highlighter))),
                        MenuId::Highlight,
                    )
                },
                Control {
                    accent: Some(FONT_RED.to_color32()),
                    ..split(
                        Size::Icon,
                        icons::TEXT_COLOR,
                        t!("editor-text-color"),
                        text(Do::Color(Some(FONT_RED))),
                        MenuId::FontColor,
                    )
                },
                menu(
                    Size::Icon,
                    icons::TEXT_ALIGN_LEFT,
                    t!("editor-paragraph-alignment"),
                    MenuId::Align,
                ),
                Control {
                    glyph_color: Some(DANGER),
                    ..delete_button(Size::Icon, icons::X)
                },
            ],
        )
    };
    // the desktop notebook's Styles gallery: six styles in the band, in two columns; its "▾" lists all
    // eleven in the desktop notebook's order, then Clear Formatting.
    // Its glyph shows where the gallery is one button (Simplified's "Styles ⌄").
    let styles = group(
        "styles",
        t!("ribbon-group-styles"),
        icons::TEXT_HEADER_1,
        6,
        vec![Control {
            glyph: icons::TEXT_HEADER_1,
            ..gallery(
                t!("ribbon-group-styles"),
                [
                    BlockKind::Paragraph,
                    BlockKind::heading(1),
                    BlockKind::heading(2),
                    BlockKind::heading(3),
                    BlockKind::PAGE_TITLE,
                    BlockKind::Citation,
                ]
                .into_iter()
                .map(|kind| GalleryItem {
                    preview: Some(kind.clone()),
                    ..item(text(Do::SetKind(kind.clone())), kind.label(), None, None)
                })
                .collect(),
                Some(MenuId::Styles),
            )
        }],
    );
    // The tags in the gallery's order (To Do first, then the desktop notebook's, unless Customize Tags
    // moved them), each glyph in its tag's colour; To Do's in the accent.
    let tag_items = tags
        .order
        .iter()
        .map(|&key| {
            item(
                text(Do::Tag(TagCommand::of(key))),
                tags.label(key),
                Some(tags.icon(key)),
                tags.color(key).map(Rgb::to_color32),
            )
        })
        .collect();
    // Tags drop to their icons before Styles shrinks: the other way round, a band too
    // narrow for both whole would show Styles whole while a wider one folded it.
    let tags = group(
        "tags",
        t!("ribbon-group-tags"),
        icons::TAG,
        5,
        vec![
            gallery(t!("ribbon-group-tags"), tag_items, Some(MenuId::Tags)),
            button(
                Size::Large,
                icons::CHECK_SQUARE,
                t!("ribbon-todo-tag"),
                text(Do::Tag(TagCommand::Todo)),
            ),
            tip(
                button(
                    Size::Large,
                    icons::TAG_SEARCH,
                    t!("ribbon-find-tags"),
                    text(Do::Tag(TagCommand::FindTags)),
                ),
                t!("ribbon-find-tags-tip"),
            ),
        ],
    );
    vec![undo_group(), clipboard, basic_text, styles, tags]
}

/// Insert, in the desktop notebook's group order with Folio's frise last. Pictures, files, equations,
/// recordings, time stamps and symbols go into the note being typed in, at its caret; a
/// note starts with a click on the paper, so there is no Text Box.
fn insert_groups() -> Vec<Group> {
    let large = |glyph: &'static str, label: Cow<'static, str>, cmd: Cmd, tooltip| {
        tip(button(Size::Large, glyph, label, cmd), tooltip)
    };
    let small = |glyph: &'static str, label: Cow<'static, str>, cmd: Cmd, tooltip| {
        tip(button(Size::Small, glyph, label, cmd), tooltip)
    };
    let dropdown = |glyph: &'static str, label: Cow<'static, str>, menu_id: MenuId, tooltip| {
        tip(menu(Size::Large, glyph, label, menu_id), tooltip)
    };
    vec![
        group(
            "insert",
            t!("ribbon-group-insert"),
            icons::ARROW_AUTOFIT_HEIGHT,
            3,
            vec![tool(
                Size::Large,
                icons::ARROW_AUTOFIT_HEIGHT,
                t!("ribbon-insert-space"),
                t!("ribbon-insert-space-tip"),
                CanvasTool::InsertSpace,
            )],
        ),
        // Table opens the desktop notebook's grid: hover a size, click to insert it.
        group(
            "tables",
            t!("ribbon-group-tables"),
            icons::TABLE,
            7,
            vec![dropdown(
                icons::TABLE,
                t!("ribbon-table"),
                MenuId::TableGrid,
                t!("ribbon-table-tip"),
            )],
        ),
        group(
            "files",
            t!("ribbon-group-files"),
            icons::PAPERCLIP,
            6,
            vec![
                large(
                    icons::FILE_PDF,
                    t!("ribbon-file-printout"),
                    Cmd::FilePrintout,
                    t!("ribbon-file-printout-tip"),
                ),
                large(
                    icons::PAPERCLIP,
                    t!("ribbon-file"),
                    Cmd::Attachment,
                    t!("ribbon-file-tip"),
                ),
            ],
        ),
        group(
            "images",
            t!("ribbon-group-images"),
            icons::IMAGE,
            7,
            vec![large(
                icons::IMAGE,
                t!("ribbon-picture"),
                Cmd::Picture,
                t!("ribbon-picture-tip"),
            )],
        ),
        group(
            "links",
            t!("ribbon-group-links"),
            icons::LINK,
            5,
            vec![large(
                icons::LINK,
                t!("ribbon-link"),
                Cmd::Link,
                t!("ribbon-link-tip"),
            )],
        ),
        group(
            "recording",
            t!("ribbon-group-recording"),
            icons::MICROPHONE,
            4,
            vec![tip(
                toggle(
                    Size::Large,
                    icons::MICROPHONE,
                    t!("ribbon-audio"),
                    Cmd::RecordAudio,
                ),
                t!("ribbon-audio-tip"),
            )],
        ),
        group(
            "time-stamp",
            t!("ribbon-group-time-stamp"),
            icons::CALENDAR_BLANK,
            4,
            vec![
                small(
                    icons::CALENDAR_BLANK,
                    t!("ribbon-date"),
                    Cmd::Date,
                    t!("ribbon-date-tip"),
                ),
                small(
                    icons::CLOCK,
                    t!("ribbon-time"),
                    Cmd::Time,
                    t!("ribbon-time-tip"),
                ),
                small(
                    icons::CALENDAR_CLOCK,
                    t!("ribbon-date-time"),
                    Cmd::DateTime,
                    t!("ribbon-date-time-tip"),
                ),
            ],
        ),
        // Page Templates adds a page laid out from one of the built-in templates.
        group(
            "pages",
            t!("ribbon-group-pages"),
            icons::DOCUMENT_ONE_PAGE_MULTIPLE,
            3,
            vec![dropdown(
                icons::DOCUMENT_ONE_PAGE_MULTIPLE,
                t!("ribbon-page-templates"),
                MenuId::PageTemplates,
                t!("ribbon-page-templates-tip"),
            )],
        ),
        group(
            "symbols",
            t!("ribbon-group-symbols"),
            icons::MATH_OPERATIONS,
            5,
            vec![
                // Its ▾ lists Office's ready-made equations, as the desktop notebook's does.
                tip(
                    split(
                        Size::Large,
                        icons::MATH_OPERATIONS,
                        t!("ribbon-equation"),
                        Cmd::Equation,
                        MenuId::EquationGallery,
                    ),
                    t!("ribbon-equation-tip"),
                ),
                dropdown(
                    icons::SYMBOLS,
                    t!("ribbon-symbol"),
                    MenuId::Symbols,
                    t!("ribbon-symbol-tip"),
                ),
            ],
        ),
        group(
            "timeline",
            t!("ribbon-group-timeline"),
            icons::LINE_SEGMENTS,
            4,
            vec![large(
                icons::LINE_SEGMENTS,
                t!("ribbon-frise"),
                Cmd::Timeline,
                t!("ribbon-frise-tip"),
            )],
        ),
    ]
}

/// A canvas tool, checked while it is the current one.
fn tool(
    size: Size,
    glyph: &'static str,
    label: Cow<'static, str>,
    tooltip: Cow<'static, str>,
    tool: CanvasTool,
) -> Control {
    tip(toggle(size, glyph, label, Cmd::Tool(tool)), tooltip)
}

/// One pen of Draw's gallery, as the desktop notebook draws it: the pen's (or highlighter's) body in
/// its colour under its outline, named by its colour and thickness.
fn pen_item(index: usize, pen: &PenPreset) -> GalleryItem {
    let (glyph, filled) = if pen.highlighter {
        (icons::HIGHLIGHTER, icons::HIGHLIGHT_FILLED)
    } else {
        (icons::PENCIL_SIMPLE, icons::PEN_FILLED)
    };
    GalleryItem {
        tooltip: Cow::Owned(pen_label(pen)),
        filled: Some(filled),
        ..item(
            Cmd::PenPreset(index),
            Cow::Borrowed(""),
            Some(glyph),
            Some(pen.color.to_color32()),
        )
    }
}

/// A shape's glyph in Draw's Shapes gallery.
fn shape_glyph(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Rect => icons::RECTANGLE_LANDSCAPE,
        ShapeKind::RoundRect => icons::SQUARE,
        ShapeKind::Ellipse => icons::OVAL,
        ShapeKind::Triangle => icons::TRIANGLE,
        ShapeKind::Diamond => icons::DIAMOND,
        ShapeKind::Line => icons::LINE,
        ShapeKind::Arrow => icons::ARROW_RIGHT,
    }
}

/// Draw, as the desktop notebook lays it out: the tools, the eraser and the gallery of pens (the
/// host's, in order), the shapes, the edit commands and Ink to Shape.
fn draw_groups(host: &dyn RibbonHost) -> Vec<Group> {
    let eraser = Control {
        arrow_label: Some(t!("ribbon-eraser-size")),
        ..tip(
            split(
                Size::Large,
                icons::ERASER,
                t!("ribbon-eraser"),
                Cmd::Tool(CanvasTool::Eraser),
                MenuId::Eraser,
            ),
            t!("ribbon-eraser-tip"),
        )
    };
    // The pens' "▾" holds the current pen's thickness and colour, as the desktop notebook's does.
    // The band shows `PENS_SHOWN` of them: a pen in hand beyond those takes the last cell.
    let mut pen_items: Vec<GalleryItem> = host
        .pens()
        .iter()
        .enumerate()
        .map(|(index, pen)| pen_item(index, pen))
        .collect();
    let in_hand = pen_items.iter().position(|item| host.checked(&item.cmd));
    if let Some(index) = in_hand.filter(|&index| index >= PENS_SHOWN) {
        let current = pen_items.swap_remove(index);
        pen_items.truncate(PENS_SHOWN - 1);
        pen_items.push(current);
    }
    let pens = Control {
        arrow_label: Some(t!("ribbon-pen-settings")),
        ..gallery(t!("ribbon-pens"), pen_items, Some(MenuId::PenSettings))
    };
    // Shapes are icons named by their tooltips; the "▾" lists them by name.
    let shapes = tip(
        gallery(
            t!("ribbon-shapes"),
            ShapeKind::ALL
                .into_iter()
                .map(|kind| GalleryItem {
                    tooltip: kind.label(),
                    ..item(
                        Cmd::Shape(kind),
                        Cow::Borrowed(""),
                        Some(shape_glyph(kind)),
                        None,
                    )
                })
                .collect(),
            Some(MenuId::Shapes),
        ),
        t!("ribbon-shape-tip"),
    );
    vec![
        undo_group(),
        group(
            "tools",
            t!("ribbon-group-tools"),
            icons::CURSOR,
            8,
            vec![
                tool(
                    Size::Large,
                    icons::CURSOR,
                    t!("ribbon-type"),
                    t!("ribbon-type-tip"),
                    CanvasTool::Select,
                ),
                tool(
                    Size::Small,
                    icons::LASSO,
                    t!("ribbon-lasso"),
                    t!("ribbon-lasso-tip"),
                    CanvasTool::Lasso,
                ),
                tool(
                    Size::Small,
                    icons::HAND,
                    t!("ribbon-pan"),
                    t!("ribbon-pan-tip"),
                    CanvasTool::Pan,
                ),
            ],
        ),
        group(
            "drawing-tools",
            t!("ribbon-group-drawing-tools"),
            icons::PENCIL_SIMPLE,
            7,
            vec![
                eraser,
                pens,
                tip(
                    menu(
                        Size::Large,
                        icons::PLUS,
                        t!("ribbon-add-pen"),
                        MenuId::AddPen,
                    ),
                    t!("ribbon-add-pen-tip"),
                ),
            ],
        ),
        group(
            "shapes",
            t!("ribbon-group-shapes"),
            icons::SHAPES,
            6,
            vec![
                shapes,
                tip(
                    toggle(
                        Size::Small,
                        icons::LOCK_CLOSED,
                        t!("ribbon-lock-drawing"),
                        Cmd::LockDrawing,
                    ),
                    t!("ribbon-lock-drawing-tip"),
                ),
                menu(
                    Size::Small,
                    icons::COLOR_FILL,
                    t!("ribbon-fill"),
                    MenuId::Fill,
                ),
            ],
        ),
        group(
            "edit",
            t!("ribbon-group-edit"),
            icons::POSITION_TO_FRONT,
            5,
            vec![
                tool(
                    Size::Small,
                    icons::ARROW_AUTOFIT_HEIGHT,
                    t!("ribbon-insert-space"),
                    t!("ribbon-insert-space-tip"),
                    CanvasTool::InsertSpace,
                ),
                delete_button(Size::Small, icons::TRASH),
                menu(
                    Size::Small,
                    icons::POSITION_TO_FRONT,
                    t!("ribbon-arrange"),
                    MenuId::Arrange,
                ),
                menu(
                    Size::Small,
                    icons::ARROW_ROTATE_CLOCKWISE,
                    t!("ribbon-rotate"),
                    MenuId::Rotate,
                ),
            ],
        ),
        group(
            "convert",
            t!("ribbon-group-convert"),
            icons::DRAW_SHAPE,
            4,
            vec![tip(
                toggle(
                    Size::Large,
                    icons::DRAW_SHAPE,
                    t!("ribbon-ink-to-shape"),
                    Cmd::InkToShape,
                ),
                t!("ribbon-ink-to-shape-tip"),
            )],
        ),
    ]
}

fn history_groups() -> Vec<Group> {
    vec![group(
        "history",
        t!("ribbon-group-history"),
        icons::CLOCK_COUNTER_CLOCKWISE,
        9,
        vec![
            // The versions pane; its arrow deletes versions.
            split(
                Size::Large,
                icons::CLOCK_COUNTER_CLOCKWISE,
                t!("common-page-versions"),
                Cmd::PageVersions,
                MenuId::PageVersions,
            ),
            // Checked while the page list shows the Recycle Bin; its arrow empties it.
            split(
                Size::Large,
                icons::TRASH,
                t!("app-recycle-bin"),
                Cmd::RecycleBin,
                MenuId::RecycleBin,
            ),
            // the desktop notebook keeps it in an Authors group; Folio has no authors.
            menu(
                Size::Large,
                icons::CLOCK,
                t!("ribbon-recent-edits"),
                MenuId::RecentEdits,
            ),
        ],
    )]
}

fn review_groups() -> Vec<Group> {
    vec![
        // the desktop notebook's Spelling group: the Spelling pane walks the page's misspellings.
        group(
            "spelling",
            t!("ribbon-group-spelling"),
            icons::TEXT_PROOFING_TOOLS,
            9,
            vec![tip(
                button(
                    Size::Large,
                    icons::TEXT_PROOFING_TOOLS,
                    t!("ribbon-spelling"),
                    Cmd::Spelling,
                ),
                t!("ribbon-spelling-tip"),
            )],
        ),
        group(
            "language",
            t!("ribbon-group-language"),
            icons::LOCAL_LANGUAGE,
            9,
            vec![menu(
                Size::Large,
                icons::LOCAL_LANGUAGE,
                t!("ribbon-proofing-language"),
                MenuId::Proofing,
            )],
        ),
    ]
}

fn view_groups() -> Vec<Group> {
    vec![
        group(
            "views",
            t!("ribbon-group-views"),
            icons::DOCUMENT_ONE_PAGE,
            7,
            vec![
                toggle(
                    Size::Large,
                    icons::DOCUMENT_ONE_PAGE,
                    t!("app-normal-view"),
                    Cmd::NormalView,
                ),
                toggle(
                    Size::Large,
                    icons::FULL_SCREEN_MAXIMIZE,
                    t!("app-full-page-view"),
                    Cmd::FullPageView,
                ),
            ],
        ),
        // the desktop notebook's Tabs Layout: the sections in the navigation pane, or as tabs across
        // the top of the page.
        group(
            "tabs-layout",
            t!("ribbon-group-tabs-layout"),
            icons::TAB_DESKTOP,
            6,
            vec![menu(
                Size::Large,
                icons::TAB_DESKTOP,
                t!("ribbon-tabs-layout"),
                MenuId::TabsLayout,
            )],
        ),
        group(
            "view-tools",
            t!("ribbon-group-view-tools"),
            icons::IMMERSIVE_READER,
            8,
            vec![button(
                Size::Large,
                icons::IMMERSIVE_READER,
                t!("common-immersive-reading"),
                Cmd::Reader,
            )],
        ),
        // the desktop notebook's Page Setup: the page's colour, its rule lines and its header.
        group(
            "page-setup",
            t!("ribbon-group-page-setup"),
            icons::PAINT_BUCKET,
            6,
            vec![
                tip(
                    menu(
                        Size::Large,
                        icons::PAINT_BUCKET,
                        t!("ribbon-page-color"),
                        MenuId::PageColor,
                    ),
                    t!("ribbon-page-color-tip"),
                ),
                tip(
                    menu(
                        Size::Large,
                        icons::NOTEPAD,
                        t!("ribbon-rule-lines"),
                        MenuId::RuleLines,
                    ),
                    t!("ribbon-rule-lines-tip"),
                ),
                tip(
                    toggle(
                        Size::Large,
                        icons::DOCUMENT_HEADER_DISMISS,
                        t!("ribbon-hide-page-title"),
                        Cmd::HidePageTitle,
                    ),
                    t!("ribbon-hide-page-title-tip"),
                ),
                // the desktop notebook's moon: the page in the other theme, the window as it is.
                tip(
                    toggle(
                        Size::Large,
                        icons::DARK_THEME,
                        t!("ribbon-switch-background"),
                        Cmd::SwitchBackground,
                    ),
                    t!("ribbon-switch-background-tip"),
                ),
            ],
        ),
        group(
            "zoom",
            t!("ribbon-group-zoom"),
            icons::ZOOM_IN,
            5,
            vec![
                button(
                    Size::Large,
                    icons::ZOOM_OUT,
                    t!("ribbon-zoom-out"),
                    Cmd::ZoomOut,
                ),
                button(
                    Size::Large,
                    icons::ZOOM_IN,
                    t!("ribbon-zoom-in"),
                    Cmd::ZoomIn,
                ),
                tip(
                    combo(t!("ribbon-zoom-box"), MenuId::Zoom, 64.0),
                    t!("ribbon-zoom-box-tip"),
                ),
                tip(
                    button(
                        Size::Small,
                        icons::RATIO_ONE_TO_ONE,
                        t!("ribbon-zoom-100"),
                        Cmd::Zoom100,
                    ),
                    t!("ribbon-zoom-100-tip"),
                ),
                tip(
                    button(
                        Size::Small,
                        icons::PAGE_FIT,
                        t!("ribbon-page-width"),
                        Cmd::PageWidth,
                    ),
                    t!("ribbon-page-width-tip"),
                ),
            ],
        ),
        // the desktop notebook's Window group. Folio has one window: New Quick Note adds a page to
        // the Quick Notes section instead of opening a small one.
        group(
            "window",
            t!("ribbon-group-window"),
            icons::WINDOW,
            3,
            vec![tip(
                button(
                    Size::Large,
                    icons::NOTE_ADD,
                    t!("ribbon-new-quick-note"),
                    Cmd::NewQuickNote,
                ),
                t!("ribbon-new-quick-note-tip"),
            )],
        ),
    ]
}

/// The Zoom Level menu's steps, in the desktop notebook's order (per cent).
pub const ZOOM_STEPS: [u16; 7] = [400, 300, 200, 150, 100, 75, 50];

/// A typed zoom ("150", "150%", " 75 % ") as a percentage in the page's range, 20–400.
pub fn zoom_percent(typed: &str) -> Option<u16> {
    let digits = typed.trim().trim_end_matches('%').trim();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // However many digits are typed, past 400 is 400.
    let percent = digits.parse::<u32>().unwrap_or(u32::MAX);
    Some(percent.clamp(20, 400) as u16)
}

/// View › Page Color's dropdown: the desktop notebook's sixteen pale colours in a 4 × 4 grid, each named
/// by its colour, the page's own framed in purple, then No Color and More Colors… as menu
/// rows under them.
pub fn page_color_menu(ui: &mut Ui, host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    const CELL: Vec2 = vec2(40.0, 34.0);
    const GAP: f32 = 4.0;
    let columns = 4;
    let grid = vec2(
        columns as f32 * CELL.x + (columns - 1) as f32 * GAP,
        (PageColor::ALL.len() / columns) as f32 * (CELL.y + GAP) - GAP,
    );
    let (area, _) = ui.allocate_exact_size(grid, Sense::hover());
    let mut picked = None;
    for (index, color) in PageColor::ALL.into_iter().enumerate() {
        let (row, column) = (index / columns, index % columns);
        let rect = Rect::from_min_size(
            area.min + vec2(column as f32 * (CELL.x + GAP), row as f32 * (CELL.y + GAP)),
            CELL,
        );
        let cmd = Cmd::PageColor(Some(color));
        let enabled = host.enabled(&cmd);
        let checked = host.checked(&cmd);
        let response = interact(
            ui,
            rect,
            Id::unique(("folio-page-color", color.code())),
            enabled,
        );
        let fill = if t.dark { color.dark() } else { color.light() };
        paint_swatch(
            ui.painter(),
            rect,
            fill.to_color32(),
            enabled,
            checked,
            response.hovered(),
            t,
        );
        let label = color.label();
        let response = describe(
            response,
            Role::Button,
            enabled,
            Some(checked),
            &label,
            &label,
        );
        if enabled && response.clicked() {
            picked = Some(cmd);
        }
    }
    ui.add_space(2.0);
    // No Color, then More Colors… (any colour, in a dialog): menu rows under the grid,
    // as in the desktop notebook, checked when they are the page's.
    let rows = [
        (None, t!("ribbon-no-color"), Cmd::PageColor(None)),
        (
            Some(icons::PALETTE),
            t!("ribbon-more-colors"),
            Cmd::MorePageColors,
        ),
    ];
    // As wide as the grid, wherever the menu is drawn.
    let layout = egui::Layout::top_down_justified(egui::Align::Min);
    ui.allocate_ui_with_layout(vec2(grid.x, 0.0), layout, |ui| {
        for (index, (glyph, label, cmd)) in rows.into_iter().enumerate() {
            if index > 0 {
                ui.separator();
            }
            if let Some(cmd) = command_row(ui, glyph, &label, &cmd, true, host, t) {
                picked = Some(cmd);
            }
        }
    });
    picked
}

/// Rule Line Color ›: Automatic (the desktop notebook's blue), then the desktop notebook's sixteen ink colours
/// (Draw's pen grid), the page's own framed.
fn rule_color_menu(ui: &mut Ui, host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    let mut picked = None;
    let automatic = t!("ribbon-rule-color-automatic");
    if let Some(cmd) = command_row(
        ui,
        None,
        &automatic,
        &Cmd::RuleLineColor(None),
        true,
        host,
        t,
    ) {
        picked = Some(cmd);
    }
    ui.separator();
    let current = crate::canvas::PEN_COLORS
        .iter()
        .map(|(color, _)| *color)
        .find(|color| host.checked(&Cmd::RuleLineColor(Some(*color))));
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().button_padding.x);
        if let Some(color) = crate::canvas::pen_color_grid(ui, current, t) {
            picked = Some(Cmd::RuleLineColor(Some(color)));
        }
        ui.add_space(ui.spacing().button_padding.x);
    });
    picked
}

/// View › Rule Lines' dropdown, as the desktop notebook lays it out: a row of None and the four ruled
/// spacings, a row of the four grids, then Rule Line Color › and Always Create Pages with
/// Rule Lines. Each thumbnail draws its lines as the page does, at a quarter of their
/// spacing.
pub fn rule_lines_menu(ui: &mut Ui, host: &dyn RibbonHost, t: Tokens) -> Option<Cmd> {
    const TILE: f32 = 64.0;
    const GAP: f32 = 6.0;
    let mut picked = None;
    let heading = |ui: &mut Ui, text: Cow<'static, str>| {
        ui.add_space(2.0);
        ui.label(RichText::new(text).font(theme::semibold(12.5)).color(t.ink));
        ui.add_space(2.0);
    };
    let mut tiles = |ui: &mut Ui, rules: &[RuleLines]| {
        let width = rules.len() as f32 * (TILE + GAP) - GAP;
        let (area, _) = ui.allocate_exact_size(vec2(width, TILE), Sense::hover());
        for (index, rule) in rules.iter().enumerate() {
            let rect = Rect::from_min_size(
                area.min + vec2(index as f32 * (TILE + GAP), 0.0),
                Vec2::splat(TILE),
            );
            if let Some(cmd) = rule_tile(ui, rect, *rule, host, t) {
                picked = Some(cmd);
            }
        }
    };
    heading(ui, t!("ribbon-rule-lines-heading"));
    let mut lines = vec![RuleLines::None];
    lines.extend(RuleLines::LINES);
    tiles(ui, &lines);
    ui.add_space(4.0);
    heading(ui, t!("ribbon-grid-lines-heading"));
    tiles(ui, &RuleLines::GRIDS);
    ui.add_space(4.0);
    ui.separator();
    // Rule Line Color ›, greyed while the page has no lines, as in the desktop notebook.
    let color = t!("ribbon-rule-line-color");
    let enabled = host.enabled(&Cmd::RuleLineColor(None));
    if let Some(cmd) = submenu(ui, None, &color, enabled, t, |ui| {
        rule_color_menu(ui, host, t)
    }) {
        picked = Some(cmd);
    }
    let always = t!("ribbon-always-rule-lines");
    if let Some(cmd) = command_row(ui, None, &always, &Cmd::AlwaysRuleLines, true, host, t) {
        picked = Some(cmd);
    }
    picked
}

/// One Rule Lines thumbnail: the page's fill with its lines (blue, with the red margin
/// line on ruled paper) or, for None, the word; framed in purple when it is the page's.
/// The font for the label of a Rule Lines tile `width` wide ("None", "Aucune"): 12 px, or
/// smaller where that would touch the tile's frame.
pub(crate) fn tile_label_font(ctx: &egui::Context, label: &str, width: f32) -> FontId {
    let room = width - 8.0;
    [12.0, 11.0, 10.0]
        .into_iter()
        .map(theme::ui)
        .find(|font| widgets::text_width(ctx, label, font.clone()) <= room)
        .unwrap_or_else(|| theme::ui(9.0))
}

fn rule_tile(
    ui: &Ui,
    rect: Rect,
    rule: RuleLines,
    host: &dyn RibbonHost,
    t: Tokens,
) -> Option<Cmd> {
    let cmd = Cmd::RuleLines(rule);
    let enabled = host.enabled(&cmd);
    let checked = host.checked(&cmd);
    let response = interact(
        ui,
        rect,
        Id::unique(("folio-rule-lines", rule.code())),
        enabled,
    );
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, t.raised);
    let inner = rect.shrink(1.0);
    match rule.spacing() {
        None => {
            let label = rule.label();
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                &label,
                tile_label_font(ui.ctx(), &label, rect.width()),
                if enabled { t.ink } else { t.faint },
            );
        }
        Some(spacing) => {
            let (line, margin) = rule_colors(t);
            let step = spacing / 4.0;
            let stroke = Stroke::new(1.0, line);
            let mut y = inner.top() + step;
            while y < inner.bottom() - 0.5 {
                painter.hline(inner.x_range(), painter.round_to_pixel_center(y), stroke);
                y += step;
            }
            if matches!(rule, RuleLines::Grid(_)) {
                let mut x = inner.left() + step;
                while x < inner.right() - 0.5 {
                    painter.vline(painter.round_to_pixel_center(x), inner.y_range(), stroke);
                    x += step;
                }
            } else {
                let x = painter.round_to_pixel_center(inner.left() + RULE_MARGIN / 4.0);
                painter.vline(x, inner.y_range(), Stroke::new(1.0, margin));
            }
        }
    }
    // The current rule is framed in dark grey, as the desktop notebook's.
    let (width, frame) = if checked {
        (2.0, t.muted)
    } else if response.hovered() && enabled {
        (1.0, t.muted)
    } else {
        (1.0, t.line_strong)
    };
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(width, frame),
        StrokeKind::Inside,
    );
    let label = rule.label();
    let response = describe(
        response,
        Role::Button,
        enabled,
        Some(checked),
        &label,
        &label,
    );
    (enabled && response.clicked()).then_some(cmd)
}

fn help_groups() -> Vec<Group> {
    vec![group(
        "help",
        t!("ribbon-group-help"),
        icons::QUESTION,
        9,
        vec![
            button(
                Size::Large,
                icons::KEYBOARD,
                t!("common-keyboard-shortcuts"),
                Cmd::Shortcuts,
            ),
            button(Size::Large, icons::INFO, t!("ribbon-about"), Cmd::About),
        ],
    )]
}

/// The contextual Table tab, as the desktop notebook lays it out: Select, Delete, Insert, Format,
/// Alignment and Data. Every command acts on the table holding the caret (its selected
/// cells, else the active cell).
fn table_groups() -> Vec<Group> {
    use crate::table::TableCmd as T;
    let edit = Cmd::TableEdit;
    vec![
        group(
            "table-select",
            t!("ribbon-group-table-select"),
            icons::TABLE_CELL_EDIT,
            3,
            vec![
                button(
                    Size::Small,
                    icons::TABLE,
                    t!("table-select-table"),
                    edit(T::SelectTable),
                ),
                button(
                    Size::Small,
                    icons::TABLE_FREEZE_COLUMN,
                    t!("table-select-columns"),
                    edit(T::SelectColumns),
                ),
                button(
                    Size::Small,
                    icons::TABLE_FREEZE_ROW,
                    t!("table-select-rows"),
                    edit(T::SelectRows),
                ),
                button(
                    Size::Small,
                    icons::TABLE_CELL_EDIT,
                    t!("table-select-cell"),
                    edit(T::SelectCell),
                ),
            ],
        ),
        group(
            "table-delete",
            t!("ribbon-group-table-delete"),
            icons::TABLE_DISMISS,
            4,
            vec![
                button(
                    Size::Small,
                    icons::TABLE_DISMISS,
                    t!("table-delete-table"),
                    edit(T::DeleteTable),
                ),
                button(
                    Size::Small,
                    icons::TABLE_DELETE_COLUMN,
                    t!("table-delete-columns"),
                    edit(T::DeleteColumns),
                ),
                button(
                    Size::Small,
                    icons::TABLE_DELETE_ROW,
                    t!("table-delete-rows"),
                    edit(T::DeleteRows),
                ),
            ],
        ),
        group(
            "table-insert",
            t!("ribbon-group-table-insert"),
            icons::TABLE_MOVE_BELOW,
            9,
            vec![
                button(
                    Size::Large,
                    icons::TABLE_MOVE_ABOVE,
                    t!("table-insert-above"),
                    edit(T::InsertAbove),
                ),
                button(
                    Size::Large,
                    icons::TABLE_MOVE_BELOW,
                    t!("table-insert-below"),
                    edit(T::InsertBelow),
                ),
                button(
                    Size::Large,
                    icons::TABLE_MOVE_LEFT,
                    t!("table-insert-left"),
                    edit(T::InsertLeft),
                ),
                button(
                    Size::Large,
                    icons::TABLE_MOVE_RIGHT,
                    t!("table-insert-right"),
                    edit(T::InsertRight),
                ),
            ],
        ),
        group(
            "table-format",
            t!("ribbon-group-table-format"),
            icons::PAINT_BUCKET,
            6,
            vec![
                toggle(
                    Size::Large,
                    icons::BORDER_NONE,
                    t!("table-hide-borders"),
                    edit(T::HideBorders),
                ),
                menu(
                    Size::Large,
                    icons::PAINT_BUCKET,
                    t!("table-shading"),
                    MenuId::Shading,
                ),
            ],
        ),
        group(
            "table-alignment",
            t!("ribbon-group-table-alignment"),
            icons::TEXT_ALIGN_LEFT,
            5,
            vec![
                toggle(
                    Size::Small,
                    icons::TEXT_ALIGN_LEFT,
                    t!("table-align-left"),
                    edit(T::Align(ParaAlign::Left)),
                ),
                toggle(
                    Size::Small,
                    icons::TEXT_ALIGN_CENTER,
                    t!("table-align-center"),
                    edit(T::Align(ParaAlign::Center)),
                ),
                toggle(
                    Size::Small,
                    icons::TEXT_ALIGN_RIGHT,
                    t!("table-align-right"),
                    edit(T::Align(ParaAlign::Right)),
                ),
            ],
        ),
        group(
            "table-data",
            t!("ribbon-group-table-data"),
            icons::ARROW_SORT,
            2,
            vec![menu(
                Size::Large,
                icons::ARROW_SORT,
                t!("table-sort"),
                MenuId::TableSort,
            )],
        ),
    ]
}

/// The contextual Picture tab, as the desktop notebook lays it out: Picture, Accessibility and
/// Arrange. Every command acts on the selected page picture, else on the picture clicked
/// in the note being edited; Set Picture as Background and Order only apply to page
/// pictures (one in a note flows with its text).
fn picture_groups() -> Vec<Group> {
    use crate::canvas::PictureCmd as P;
    vec![
        group(
            "picture",
            t!("ribbon-group-picture"),
            icons::IMAGE,
            5,
            vec![
                toggle(
                    Size::Large,
                    icons::CROP,
                    t!("picture-crop"),
                    Cmd::PictureEdit(P::Crop),
                ),
                toggle(
                    Size::Large,
                    icons::IMAGE_ARROW_BACK,
                    t!("picture-background"),
                    Cmd::PictureEdit(P::Background),
                ),
                button(
                    Size::Small,
                    icons::ARROW_RESET,
                    t!("picture-restore-size"),
                    Cmd::PictureEdit(P::RestoreSize),
                ),
            ],
        ),
        group(
            "picture-accessibility",
            t!("ribbon-group-accessibility"),
            icons::IMAGE_ALT_TEXT,
            6,
            vec![button(
                Size::Large,
                icons::IMAGE_ALT_TEXT,
                t!("picture-alt-text"),
                Cmd::PictureEdit(P::AltText),
            )],
        ),
        group(
            "picture-arrange",
            t!("ribbon-group-arrange"),
            icons::ARROW_ROTATE_CLOCKWISE,
            7,
            vec![
                menu(
                    Size::Small,
                    icons::POSITION_TO_FRONT,
                    t!("picture-order"),
                    MenuId::PictureOrder,
                ),
                menu(
                    Size::Large,
                    icons::ARROW_ROTATE_CLOCKWISE,
                    t!("picture-rotate"),
                    MenuId::PictureRotate,
                ),
            ],
        ),
    ]
}

fn equation_groups() -> Vec<Group> {
    vec![group(
        "equation-tools",
        t!("ribbon-group-equation-tools"),
        icons::MATH_OPERATIONS,
        9,
        vec![
            tip(
                button(
                    Size::Large,
                    icons::MATH_OPERATIONS,
                    t!("canvas-menu-edit-equation"),
                    Cmd::EditEquation,
                ),
                t!("ribbon-edit-equation-tip"),
            ),
            button(
                Size::Large,
                icons::COPY,
                t!("canvas-menu-copy-latex"),
                Cmd::CopyLatex,
            ),
        ],
    )]
}

/// The Audio & Video tab: the recording under way (pause, stop and its time), and the
/// selected recording's playback. The host shows the tab when either applies.
fn recording_groups(host: &dyn RibbonHost) -> Vec<Group> {
    let mut groups = Vec::new();
    if let Some((time, paused)) = host.recording() {
        let pause = if paused {
            tip(
                toggle(
                    Size::Large,
                    icons::RECORD,
                    t!("ribbon-resume"),
                    Cmd::PauseRecording,
                ),
                t!("ribbon-resume-tip"),
            )
        } else {
            tip(
                toggle(
                    Size::Large,
                    icons::PAUSE,
                    t!("ribbon-pause"),
                    Cmd::PauseRecording,
                ),
                t!("ribbon-pause-tip"),
            )
        };
        // The elapsed time is a readout, not a command; its dot is red while recording
        // and faint while paused.
        let elapsed = Control {
            glyph_color: (!paused).then_some(DANGER),
            ..control(
                ControlKind::Button,
                None,
                Size::Large,
                icons::RECORD,
                Cow::Owned(time),
            )
        };
        groups.push(group(
            "recording",
            t!("ribbon-group-recording"),
            icons::MICROPHONE,
            9,
            vec![
                pause,
                button(
                    Size::Large,
                    icons::STOP,
                    t!("ribbon-stop"),
                    Cmd::StopRecording,
                ),
                elapsed,
            ],
        ));
    }
    if host.audio_selected() {
        groups.push(group(
            "playback",
            t!("ribbon-group-playback"),
            icons::PLAY,
            8,
            vec![
                tip(
                    button(
                        Size::Large,
                        icons::PLAY,
                        t!("canvas-menu-play-pause"),
                        Cmd::PlayPause,
                    ),
                    t!("ribbon-play-tip"),
                ),
                tip(
                    button(
                        Size::Large,
                        icons::FLOPPY_DISK,
                        t!("canvas-menu-save-as"),
                        Cmd::SaveAs,
                    ),
                    t!("ribbon-save-recording-tip"),
                ),
            ],
        ));
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_codes_round_trip() {
        for layout in [RibbonLayout::Classic, RibbonLayout::Simplified] {
            assert_eq!(RibbonLayout::from_code(layout.code()), layout);
        }
        assert_eq!(RibbonLayout::from_code("unknown"), RibbonLayout::Classic);
    }

    #[test]
    fn tabs_layout_codes_round_trip() {
        for layout in TabsLayout::ALL {
            assert_eq!(TabsLayout::from_code(layout.code()), layout);
        }
        // An unknown or missing code keeps the desktop notebook's default, the vertical tabs.
        assert_eq!(TabsLayout::from_code(""), TabsLayout::Vertical);
        assert_eq!(TabsLayout::from_code("sideways"), TabsLayout::Vertical);
    }

    #[test]
    fn contextual_tabs_are_the_last_four() {
        assert!(RibbonTab::MAIN.iter().all(|tab| !tab.contextual()));
        for tab in [
            RibbonTab::Table,
            RibbonTab::Picture,
            RibbonTab::Equation,
            RibbonTab::Recording,
        ] {
            assert!(tab.contextual());
        }
    }

    #[test]
    fn tooltips_name_the_control_once() {
        // A tooltip that only repeats the label shows the label once.
        let undo = tip(
            button(Size::Small, "", Cow::Borrowed("Annuler"), Cmd::Undo),
            Cow::Borrowed("Annuler"),
        );
        assert_eq!(control_tip(&undo), "Annuler");
        let zoom = button(Size::Large, "", Cow::Borrowed("Zoom in"), Cmd::ZoomIn);
        assert_eq!(control_tip(&zoom), "Zoom in");
        // One that says more replaces it: the label is already drawn on the button.
        let front = tip(
            button(Size::Small, "", Cow::Borrowed("Front"), Cmd::BringToFront),
            Cow::Borrowed("Bring to front"),
        );
        assert_eq!(control_name(&front), "Front");
        assert_eq!(control_tip(&front), "Bring to front");
        // A control without a label (a swatch gallery) is named by its tooltip.
        let swatches = tip(
            button(Size::Small, "", Cow::Borrowed(""), Cmd::Undo),
            Cow::Borrowed("Ink colour"),
        );
        assert_eq!(control_name(&swatches), "Ink colour");
        assert_eq!(control_tip(&swatches), "Ink colour");
    }

    /// Runs `f` inside one egui pass with Folio's fonts installed (no GPU needed).
    fn with_ctx<R>(f: impl FnOnce(&egui::Context) -> R) -> R {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut f = Some(f);
        let mut out = None;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                out = Some(f(ui.ctx()));
            }
        })
        .drop_without_applying_deltas();
        out.expect("ran one pass")
    }

    #[test]
    fn large_labels_break_at_the_middle_space() {
        with_ctx(|ctx| {
            assert_eq!(
                large_lines(ctx, "File Attachment"),
                ("File".to_owned(), Some("Attachment".to_owned()))
            );
            assert_eq!(large_lines(ctx, "Table"), ("Table".to_owned(), None));
            // A menu's chevron goes on a line of its own under one line, after a second.
            assert!(!chevron_after_label(large_lines(ctx, "Paste").1.as_deref()));
            assert!(chevron_after_label(
                large_lines(ctx, "Page Templates").1.as_deref()
            ));
            // A two-line button is as wide as its widest line, with room for the chevron
            // after the second.
            assert!(
                large_size(ctx, "Page Templates", true).x
                    > large_size(ctx, "Page Templates", false).x
            );
            assert_eq!(
                large_size(ctx, "Paste", true),
                large_size(ctx, "Paste", false)
            );
        });
    }

    #[test]
    fn fit_levels_shrinks_low_priority_groups_first() {
        let widths = [
            [Some(100.0), Some(80.0), Some(60.0), Some(40.0)],
            // No Large control: no level 1.
            [Some(100.0), None, Some(60.0), Some(40.0)],
        ];
        let priorities = [9, 1];
        assert_eq!(fit_levels(&widths, &priorities, 200.0), vec![0, 0]);
        // The low-priority group goes first, skipping the level it lacks…
        assert_eq!(fit_levels(&widths, &priorities, 170.0), vec![0, 2]);
        // …and collapses fully before the other one moves.
        assert_eq!(fit_levels(&widths, &priorities, 140.0), vec![0, 3]);
        assert_eq!(fit_levels(&widths, &priorities, 130.0), vec![1, 3]);
    }

    #[test]
    fn fit_levels_takes_the_rightmost_of_equal_priority() {
        let widths = [[Some(100.0), Some(80.0), Some(60.0), Some(40.0)]; 3];
        assert_eq!(fit_levels(&widths, &[5, 5, 5], 290.0), vec![0, 0, 1]);
        assert_eq!(fit_levels(&widths, &[5, 5, 5], 240.0), vec![0, 0, 3]);
        assert_eq!(fit_levels(&widths, &[5, 5, 5], 230.0), vec![0, 1, 3]);
        // Priority still comes before position.
        assert_eq!(fit_levels(&widths, &[1, 5, 5], 290.0), vec![1, 0, 0]);
    }

    #[test]
    fn fit_levels_stops_when_nothing_can_shrink() {
        let widths = [
            // A `Rows` group: as designed or collapsed.
            [Some(100.0), None, None, Some(50.0)],
            [Some(80.0), Some(60.0), Some(40.0), Some(30.0)],
        ];
        assert_eq!(fit_levels(&widths, &[3, 7], 10.0), vec![3, 3]);
        assert_eq!(fit_levels(&[], &[], 0.0), Vec::<usize>::new());
    }

    /// Widening the window never shows less of Home, in either language: over every width
    /// the app allows, no group is at a smaller level in a wider band. (Tags drop to their
    /// icons before the Styles gallery goes; the other way round, French Home at 1320 px
    /// showed Styles as one icon where 980 px showed it whole.)
    #[test]
    fn home_never_shrinks_as_the_band_widens() {
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Fr] {
            crate::i18n::pin_thread_lang(lang);
            let groups = home_groups(&TagGallery::default());
            let widths: Vec<[Option<f32>; 4]> = with_ctx(|ctx| {
                groups
                    .iter()
                    .map(|group| std::array::from_fn(|level| group_width(ctx, group, level)))
                    .collect()
            });
            let priorities: Vec<u8> = groups.iter().map(|group| group.priority).collect();
            // The band of a 980 px window and up.
            let mut narrower = fit_levels(&widths, &priorities, 900.0);
            for available in (901..=1800).map(|width| width as f32) {
                let levels = fit_levels(&widths, &priorities, available);
                for (index, group) in groups.iter().enumerate() {
                    assert!(
                        levels[index] <= narrower[index],
                        "{lang:?} {available}: {} goes from level {} to {}",
                        group.id,
                        narrower[index],
                        levels[index]
                    );
                }
                narrower = levels;
            }
        }
    }

    /// A step that would widen a group is no step: its icons stay rather than fold into a
    /// wider button (French Styles: 46 px of icons, 61 px folded), and the next group
    /// shrinks instead.
    #[test]
    fn fit_levels_never_widens_a_group() {
        let widths = [
            [Some(100.0), None, None, Some(80.0)],
            // Level 1 is wider than level 0 and level 3 wider than level 2.
            [Some(170.0), Some(180.0), Some(46.0), Some(61.0)],
        ];
        let priorities = [8, 5];
        // Level 1 is skipped for level 2…
        assert_eq!(fit_levels(&widths, &priorities, 200.0), vec![0, 2]);
        // …and level 3 is never taken: the other group folds instead.
        assert_eq!(fit_levels(&widths, &priorities, 140.0), vec![3, 2]);
        // Nothing narrower is left: the band stays as small as it gets.
        assert_eq!(fit_levels(&widths, &priorities, 100.0), vec![3, 2]);
    }

    /// A folded group that is one gallery (Styles) drops the gallery itself, as Office
    /// does: its items are the rows of the button's menu, not of a submenu behind one row.
    #[test]
    fn a_folded_gallery_group_drops_its_items() {
        use egui_kittest::kittest::Queryable;
        let styles = home_groups(&TagGallery::default())
            .into_iter()
            .find(|group| group.id == "styles")
            .expect("Home has Styles");
        let ControlKind::Gallery { items, .. } = &styles.controls[0].kind else {
            panic!("Styles is a gallery")
        };
        // Each style is a preview of itself, named by its label alone.
        assert!(items.iter().all(|item| item.preview.is_some()));
        let rows: Vec<String> = items.iter().map(|item| item.label.to_string()).collect();
        let label = styles.label.to_string();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(400.0, 400.0))
            .with_context_setup(crate::theme::install)
            .build_ui(|ui| {
                let area = Rect::from_min_size(pos2(10.0, 10.0), vec2(80.0, CONTROLS_HEIGHT));
                collapsed_group(ui, area, &styles, &NoHost, Tokens::light());
            });
        assert!(
            harness.query_by_label(&rows[0]).is_none(),
            "the menu is shut"
        );
        harness.get_by_label(&label).click();
        harness.run_steps(4);
        for row in &rows {
            assert!(
                harness.query_by_label(row).is_some(),
                "{row:?} is in the menu"
            );
        }
        // No row stands for the whole gallery any more.
        assert_eq!(
            harness.query_all_by_label(&label).count(),
            1,
            "only the button"
        );
    }

    /// The first pen is the current one; everything is enabled.
    struct FirstPen;

    impl RibbonHost for FirstPen {
        fn enabled(&self, _: &Cmd) -> bool {
            true
        }
        fn checked(&self, cmd: &Cmd) -> bool {
            *cmd == Cmd::PenPreset(0)
        }
        fn menu_enabled(&self, _: MenuId) -> bool {
            true
        }
        fn combo_text(&self, _: MenuId) -> String {
            String::new()
        }
        fn contextual(&self) -> Vec<RibbonTab> {
            Vec::new()
        }
        fn menu(&self, _: &mut Ui, _: MenuId, _: Tokens) -> Option<Cmd> {
            None
        }
        fn recording(&self) -> Option<(String, bool)> {
            None
        }
        fn audio_selected(&self) -> bool {
            false
        }
    }

    #[test]
    fn named_galleries_fold_to_one_button_never_to_icons() {
        for group in home_groups(&TagGallery::default()) {
            if matches!(group.id, "styles" | "tags") {
                assert!(
                    level_sizes(&group)[2].is_none(),
                    "{} has an icon level",
                    group.id
                );
            }
        }
        let draw = draw_groups(&FirstPen);
        let tools = draw
            .iter()
            .find(|group| group.id == "drawing-tools")
            .unwrap();
        assert!(level_sizes(tools)[2].is_some(), "the pens keep theirs");
    }

    /// Pens and shapes are icon items with no label, named by their tooltips, and checked
    /// like any other item when they are the current pick. A pen is a picture of the pen
    /// (`PEN_CELL`), its colour on the barrel and nib, not a swatch; a shape is a 24 × 22
    /// icon.
    #[test]
    fn icon_items_are_named_by_their_tooltips() {
        use egui_kittest::kittest::{NodeT, Queryable};
        let pen = GalleryItem {
            tooltip: Cow::Borrowed("Red pen"),
            filled: Some(icons::PEN_FILLED),
            ..item(
                Cmd::PenPreset(0),
                Cow::Borrowed(""),
                Some(icons::PENCIL_SIMPLE),
                Some(Color32::RED),
            )
        };
        let line = GalleryItem {
            tooltip: Cow::Borrowed("Line"),
            ..item(
                Cmd::Shape(ShapeKind::Line),
                Cow::Borrowed(""),
                Some(icons::LINE),
                None,
            )
        };
        assert!(swatch_color(&pen).is_none() && icon_item(&pen) && icon_item(&line));
        assert!(pen_picture(&pen) && !pen_picture(&line));
        let pens = gallery(Cow::Borrowed("Pens"), vec![pen.clone(), pen], None);
        let shapes = gallery(Cow::Borrowed("Shapes"), vec![line], None);
        let layout = |control: &Control| {
            let ControlKind::Gallery { items, .. } = &control.kind else {
                unreachable!()
            };
            with_ctx(|ctx| gallery_layout(ctx, items, true, GALLERY_COLUMNS))
        };
        // Pens in one row, with no "▾" column; a shape keeps its icon cell.
        let (pen_cells, pen_strip) = layout(&pens);
        assert!(
            pen_cells.iter().all(|cell| cell.size() == PEN_CELL),
            "{pen_cells:?}"
        );
        assert_eq!(pen_cells[0].top(), pen_cells[1].top());
        assert_eq!(pen_strip, vec2(2.0 * PEN_CELL.x + COLUMN_GAP, PEN_CELL.y));
        let (shape_cells, shape_strip) = layout(&shapes);
        assert!(
            shape_cells.iter().all(|cell| cell.size() == ICON_ITEM),
            "{shape_cells:?}"
        );
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(200.0, 160.0))
            .with_context_setup(crate::theme::install)
            .build_ui(|ui| {
                let rect = Rect::from_min_size(pos2(10.0, 10.0), pen_strip);
                draw_control(ui, rect, &pens, Size::Large, &FirstPen, Tokens::light());
                let rect = Rect::from_min_size(pos2(10.0, 80.0), shape_strip);
                draw_control(ui, rect, &shapes, Size::Large, &FirstPen, Tokens::light());
            });
        harness.run();
        let toggled = |label: &str| {
            harness
                .get_all_by_label(label)
                .next()
                .unwrap()
                .accesskit_node()
                .toggled()
        };
        assert_eq!(toggled("Red pen"), Some(egui::accesskit::Toggled::True));
        assert_eq!(toggled("Line"), Some(egui::accesskit::Toggled::False));
    }

    /// When a big step frees more room than needed, the groups shrunk before it grow back,
    /// the most important first, rather than leave the room empty.
    #[test]
    fn fit_levels_gives_freed_room_back() {
        let widths = [
            [Some(170.0), None, Some(46.0), Some(61.0)],
            // Its only smaller level is much smaller.
            [Some(580.0), Some(595.0), Some(70.0), Some(90.0)],
        ];
        // Styles-like (5) shrinks first, which is not enough, then Tags-like (6) drops to
        // its icons (116 px in all), which leaves room for the first as designed again.
        assert_eq!(fit_levels(&widths, &[5, 6], 600.0), vec![0, 2]);
        // Two small groups shrink before a big one; the room it frees fits one of them
        // as designed: the more important one gets it.
        let small = [Some(100.0), None, Some(40.0), None];
        let big = [Some(500.0), None, Some(20.0), None];
        let widths = [small, small, big];
        assert_eq!(fit_levels(&widths, &[5, 6, 7], 200.0), vec![2, 0, 2]);
        assert_eq!(fit_levels(&widths, &[6, 5, 7], 200.0), vec![0, 2, 2]);
        // Equal priorities: the leftmost, as the rightmost shrank first.
        assert_eq!(fit_levels(&widths, &[5, 5, 7], 200.0), vec![0, 2, 2]);
        // Room for both: both grow back.
        assert_eq!(fit_levels(&widths, &[5, 6, 7], 250.0), vec![0, 0, 2]);
    }

    #[test]
    fn the_simplified_ribbon_labels_styles() {
        let styles = home_groups(&TagGallery::default())
            .into_iter()
            .find(|g| g.id == "styles")
            .unwrap();
        assert!(keeps_label(&styles.controls[0]));
        let tags = home_groups(&TagGallery::default())
            .into_iter()
            .find(|g| g.id == "tags")
            .unwrap();
        assert!(!keeps_label(&tags.controls[0]));
    }

    #[test]
    fn simplified_overflow_moves_whole_groups_into_more() {
        let widths = [100.0, 100.0, 100.0];
        // Everything fits: no "⋯" needed, so its room is not kept.
        assert_eq!(simplified_fit(&widths, 300.0), 3);
        // One px short: the groups that fit must leave room for "⋯" after them.
        assert_eq!(simplified_fit(&widths, 299.0), 2);
        assert_eq!(simplified_fit(&widths, 232.0), 2);
        assert_eq!(simplified_fit(&widths, 231.0), 1);
        // Too narrow for even one: all of them are in "⋯".
        assert_eq!(simplified_fit(&widths, 100.0), 0);
        assert_eq!(simplified_fit(&[], 0.0), 0);
    }

    #[test]
    fn typed_zoom_reads_a_percentage_in_range() {
        assert_eq!(zoom_percent("150"), Some(150));
        assert_eq!(zoom_percent(" 75 % "), Some(75));
        assert_eq!(zoom_percent("75\u{a0}%"), Some(75));
        assert_eq!(zoom_percent("1000"), Some(400));
        assert_eq!(zoom_percent("100000"), Some(400), "past u16");
        assert_eq!(zoom_percent("99999999999999999999"), Some(400));
        assert_eq!(zoom_percent("5"), Some(20));
        assert_eq!(zoom_percent("-5"), None);
        assert_eq!(zoom_percent("big"), None);
        assert_eq!(zoom_percent(""), None);
    }

    /// Nothing open: every command off, no menus, nothing recording or selected. The
    /// library's pens are there all the same (the desktop notebook's defaults), as in the app.
    struct NoHost;

    impl RibbonHost for NoHost {
        fn enabled(&self, _: &Cmd) -> bool {
            false
        }
        fn checked(&self, _: &Cmd) -> bool {
            false
        }
        fn menu_enabled(&self, _: MenuId) -> bool {
            false
        }
        fn combo_text(&self, _: MenuId) -> String {
            String::new()
        }
        fn contextual(&self) -> Vec<RibbonTab> {
            Vec::new()
        }
        fn menu(&self, _: &mut Ui, _: MenuId, _: Tokens) -> Option<Cmd> {
            None
        }
        fn recording(&self) -> Option<(String, bool)> {
            None
        }
        fn audio_selected(&self) -> bool {
            false
        }
        fn pens(&self) -> Vec<PenPreset> {
            PenPreset::defaults()
        }
    }

    /// A recording under way (paused or not) with an audio item selected.
    struct Recorder {
        paused: bool,
    }

    impl RibbonHost for Recorder {
        fn enabled(&self, _: &Cmd) -> bool {
            true
        }
        fn checked(&self, cmd: &Cmd) -> bool {
            *cmd == Cmd::PauseRecording && self.paused
        }
        fn menu_enabled(&self, _: MenuId) -> bool {
            true
        }
        fn combo_text(&self, _: MenuId) -> String {
            String::new()
        }
        fn contextual(&self) -> Vec<RibbonTab> {
            vec![RibbonTab::Recording]
        }
        fn menu(&self, _: &mut Ui, _: MenuId, _: Tokens) -> Option<Cmd> {
            None
        }
        fn recording(&self) -> Option<(String, bool)> {
            Some(("0:42".to_owned(), self.paused))
        }
        fn audio_selected(&self) -> bool {
            true
        }
    }

    /// A gallery of colour swatches is named by its tooltip, as each swatch has no label.
    fn swatches(control: &Control) -> bool {
        matches!(&control.kind, ControlKind::Gallery { items, .. }
            if items.iter().all(|item| swatch_color(item).is_some()))
    }

    #[test]
    fn every_tab_has_groups_and_no_control_is_unnamed() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let tabs = RibbonTab::MAIN
            .into_iter()
            .filter(|&tab| tab != RibbonTab::File)
            .chain([RibbonTab::Table, RibbonTab::Picture, RibbonTab::Equation]);
        for tab in tabs {
            let groups = groups(tab, &NoHost);
            assert!(!groups.is_empty(), "{tab:?} has no groups");
            for group in &groups {
                let at = format!("{tab:?} › {}", group.id);
                assert!(!group.label.is_empty(), "{at}: no label");
                // Its glyph is all a collapsed group's button has besides its name.
                assert!(!group.glyph.is_empty(), "{at}: no glyph");
                assert!(!group.controls.is_empty(), "{at}: no controls");
                for control in &group.controls {
                    let name = if swatches(control) {
                        &control.tooltip
                    } else {
                        &control.label
                    };
                    assert!(!name.is_empty(), "{at}: a control with no name");
                    // Combos show their value; a gallery shows its items.
                    if !matches!(
                        control.kind,
                        ControlKind::Combo { .. } | ControlKind::Gallery { .. }
                    ) {
                        assert!(!control.glyph.is_empty(), "{at} › {name}: no glyph");
                    }
                    if let ControlKind::Gallery { items, .. } = &control.kind {
                        assert!(!items.is_empty(), "{at} › {name}: an empty gallery");
                        // A swatch is named by its colour, an icon (a pen, a shape) by
                        // its tooltip.
                        for item in items {
                            assert!(
                                !item_name(item).is_empty() || swatch_color(item).is_some(),
                                "{at} › {name}: an item with no name"
                            );
                        }
                    }
                }
            }
            // A collapsed group's button is found by its id; two alike would share it.
            let mut ids: Vec<_> = groups.iter().map(|group| group.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), groups.len(), "{tab:?}: two groups share an id");
        }
        // File opens Backstage.
        assert!(groups(RibbonTab::File, &NoHost).is_empty());
    }

    /// History is the desktop notebook's group: Page Versions and the Notebook Recycle Bin are splits
    /// whose arrows delete versions and empty the bin, then Recent Edits is a menu.
    #[test]
    fn history_holds_versions_the_recycle_bin_and_recent_edits() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let history = groups(RibbonTab::History, &NoHost);
        assert_eq!(history.len(), 1);
        let controls = &history[0].controls;
        assert_eq!(controls.len(), 3);
        assert!(matches!(
            controls[0].kind,
            ControlKind::Split(MenuId::PageVersions)
        ));
        assert_eq!(controls[0].cmd, Some(Cmd::PageVersions));
        assert!(matches!(
            controls[1].kind,
            ControlKind::Split(MenuId::RecycleBin)
        ));
        assert_eq!(controls[1].cmd, Some(Cmd::RecycleBin));
        assert!(matches!(
            controls[2].kind,
            ControlKind::Menu(MenuId::RecentEdits)
        ));
        assert_eq!(controls[2].label, "Recent Edits");
    }

    /// Recent Edits' rows reach further back one after another, from local midnight for
    /// Today, and the last lists every page.
    #[test]
    fn edit_ranges_reach_further_back_in_menu_order() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        const DAY: i64 = 24 * 60 * 60 * 1000;
        let now = chrono::Local::now().timestamp_millis();
        let since: Vec<Option<i64>> = EditsRange::ALL.iter().map(|r| r.since(now)).collect();
        assert_eq!(since.last(), Some(&None), "All Pages has no bound");
        let bounded: Vec<i64> = since.iter().flatten().copied().collect();
        assert_eq!(bounded.len(), EditsRange::ALL.len() - 1);
        assert!(
            bounded.windows(2).all(|pair| pair[0] > pair[1]),
            "{bounded:?}"
        );
        // Today starts at midnight: not after now, and at most a day (and an hour of a
        // clock change) before.
        assert!(bounded[0] <= now && now - bounded[0] <= DAY + 60 * 60 * 1000);
        assert_eq!(bounded[1], bounded[0] - DAY);
        assert_eq!(EditsRange::Days7.since(now), Some(now - 7 * DAY));
        let mut labels: Vec<_> = EditsRange::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(labels[0], "Today");
        assert_eq!(labels[7], "All Pages Sorted by Date");
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), EditsRange::ALL.len());
    }

    /// Home is the desktop notebook's: Undo, Clipboard, Basic Text in its two rows, Styles and Tags.
    #[test]
    fn home_groups_follow_notebook() {
        use crate::editor::ToolbarCommand as Do;
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let home = groups(RibbonTab::Home, &NoHost);
        let ids: Vec<_> = home.iter().map(|group| group.id).collect();
        assert_eq!(ids, ["undo", "clipboard", "basic-text", "styles", "tags"]);
        // Clipboard: Paste with its options behind the arrow, then Cut, Copy and Format
        // Painter.
        let clipboard = &home[1].controls;
        let labels: Vec<_> = clipboard.iter().map(|c| c.label.as_ref()).collect();
        assert_eq!(labels, ["Paste", "Cut", "Copy", "Format Painter"]);
        assert!(matches!(
            clipboard[0].kind,
            ControlKind::Split(MenuId::Paste)
        ));
        assert_eq!(clipboard[0].size, Size::Large);
        assert_eq!(clipboard[0].cmd, Some(Cmd::Paste(PasteMode::Default)));
        assert_eq!(arrow_name(&clipboard[0]), "Paste Options");
        assert_eq!(clipboard[3].cmd, Some(Cmd::FormatPainter));
        // Basic Text: font, size, Bullets ▾, Numbering ▾, the indents and Clear Formatting
        // over the marks, scripts, colours, Align and Delete.
        let basic = &home[2];
        let GroupLayout::Rows(rows) = &basic.layout else {
            panic!("Basic Text is laid out in rows")
        };
        assert_eq!(rows, &[7, 9]);
        assert_eq!(basic.controls.len(), 16);
        let bullets = &basic.controls[2];
        assert!(matches!(bullets.kind, ControlKind::Split(MenuId::Bullets)));
        assert_eq!(bullets.cmd, Some(Cmd::Text(Do::SetKind(BlockKind::Bullet))));
        let numbering = &basic.controls[3];
        assert!(matches!(
            numbering.kind,
            ControlKind::Split(MenuId::Numbering)
        ));
        assert_eq!(basic.controls[4].cmd, Some(Cmd::Text(Do::Indent(-1))));
        assert_eq!(basic.controls[5].cmd, Some(Cmd::Text(Do::Indent(1))));
        assert_eq!(basic.controls[6].cmd, Some(Cmd::Text(Do::ClearFormatting)));
        assert!(matches!(
            basic.controls[14].kind,
            ControlKind::Menu(MenuId::Align)
        ));
        assert_eq!(basic.controls[15].cmd, Some(Cmd::Delete));
        // Styles: six in the band, in two columns; the rest behind its arrow.
        let ControlKind::Gallery { items, more } = &home[3].controls[0].kind else {
            panic!("Styles is a gallery")
        };
        let kinds: Vec<_> = items.iter().map(|item| item.cmd.clone()).collect();
        let styles = [
            BlockKind::Paragraph,
            BlockKind::heading(1),
            BlockKind::heading(2),
            BlockKind::heading(3),
            BlockKind::PAGE_TITLE,
            BlockKind::Citation,
        ]
        .map(|kind| Cmd::Text(Do::SetKind(kind)));
        assert_eq!(kinds, styles);
        assert_eq!(*more, Some(MenuId::Styles));
        let columns = with_ctx(|ctx| {
            let (cells, _) = gallery_layout(ctx, items, true, GALLERY_COLUMNS);
            let mut lefts: Vec<_> = cells.iter().map(|cell| cell.left()).collect();
            lefts.dedup();
            lefts.len()
        });
        assert_eq!(columns, 2);
    }

    /// Insert is the desktop notebook's, in its order with Folio's frise last, and none of Folio's old
    /// Text Box, Sticky, Divider or Drawing. Draw has the host's pens as coloured icons
    /// named by their tooltips, the shapes as icons, the edit commands and Convert.
    #[test]
    fn insert_and_draw_groups_follow_notebook() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let insert = groups(RibbonTab::Insert, &NoHost);
        let ids: Vec<_> = insert.iter().map(|group| group.id).collect();
        assert_eq!(
            ids,
            [
                "insert",
                "tables",
                "files",
                "images",
                "links",
                "recording",
                "time-stamp",
                "pages",
                "symbols",
                "timeline"
            ]
        );
        let labels: Vec<_> = insert
            .iter()
            .flat_map(|group| &group.controls)
            .map(|control| control.label.to_lowercase())
            .collect();
        for gone in ["text box", "sticky", "divider", "drawing"] {
            assert!(
                !labels.iter().any(|label| label == gone),
                "{gone:?} is gone"
            );
        }
        assert_eq!(
            insert[0].controls[0].cmd,
            Some(Cmd::Tool(CanvasTool::InsertSpace))
        );
        // Table opens the size grid; Page Templates and Symbol are menus too.
        assert!(matches!(
            insert[1].controls[0].kind,
            ControlKind::Menu(MenuId::TableGrid)
        ));
        let cmds = |group: &Group| -> Vec<Option<Cmd>> {
            group.controls.iter().map(|c| c.cmd.clone()).collect()
        };
        assert_eq!(
            cmds(&insert[2]),
            [Some(Cmd::FilePrintout), Some(Cmd::Attachment)]
        );
        assert_eq!(
            cmds(&insert[6]),
            [Some(Cmd::Date), Some(Cmd::Time), Some(Cmd::DateTime)]
        );
        assert!(matches!(
            insert[7].controls[0].kind,
            ControlKind::Menu(MenuId::PageTemplates)
        ));
        assert_eq!(insert[8].controls[0].cmd, Some(Cmd::Equation));
        assert!(matches!(
            insert[8].controls[0].kind,
            ControlKind::Split(MenuId::EquationGallery)
        ));
        assert!(matches!(
            insert[8].controls[1].kind,
            ControlKind::Menu(MenuId::Symbols)
        ));

        let draw = groups(RibbonTab::Draw, &NoHost);
        let ids: Vec<_> = draw.iter().map(|group| group.id).collect();
        assert_eq!(
            ids,
            [
                "undo",
                "tools",
                "drawing-tools",
                "shapes",
                "edit",
                "convert"
            ]
        );
        // The eraser, the pens (their "▾" is the pen settings) and Add Pen.
        let tools = &draw[2].controls;
        assert_eq!(tools[0].cmd, Some(Cmd::Tool(CanvasTool::Eraser)));
        assert!(matches!(tools[2].kind, ControlKind::Menu(MenuId::AddPen)));
        let ControlKind::Gallery { items, more } = &tools[1].kind else {
            panic!("the pens are a gallery")
        };
        assert_eq!(*more, Some(MenuId::PenSettings));
        assert_eq!(arrow_name(&tools[1]), "Pen settings");
        assert_eq!(items.len(), 6);
        for (index, item) in items.iter().enumerate() {
            assert!(icon_item(item), "pen {index} is an icon");
            assert!(!item.tooltip.is_empty() && item.filled.is_some());
            assert_eq!(item.cmd, Cmd::PenPreset(index));
        }
        assert_eq!(items[0].tooltip, "Pen: Black, 0.5 mm");
        assert_eq!(items[4].tooltip, "Highlighter: Bright yellow, 2.8 mm");
        assert_eq!(items[4].filled, Some(icons::HIGHLIGHT_FILLED));
        // Every shape, as an icon named by the shape; its "▾" lists them by name.
        let ControlKind::Gallery { items, more } = &draw[3].controls[0].kind else {
            panic!("the shapes are a gallery")
        };
        assert_eq!(*more, Some(MenuId::Shapes));
        for (item, kind) in items.iter().zip(ShapeKind::ALL) {
            assert!(icon_item(item));
            assert_eq!(
                (&item.cmd, &item.tooltip),
                (&Cmd::Shape(kind), &kind.label())
            );
        }
        assert_eq!(items.len(), ShapeKind::ALL.len());
        assert_eq!(draw[3].controls[1].cmd, Some(Cmd::LockDrawing));
        // Edit: Insert Space, Delete, Arrange ▾ and Rotate ▾. Convert: Ink to Shape.
        let edit = &draw[4].controls;
        assert_eq!(edit[0].cmd, Some(Cmd::Tool(CanvasTool::InsertSpace)));
        assert_eq!(edit[1].cmd, Some(Cmd::Delete));
        assert!(matches!(edit[2].kind, ControlKind::Menu(MenuId::Arrange)));
        assert!(matches!(edit[3].kind, ControlKind::Menu(MenuId::Rotate)));
        let convert = &draw[5].controls[0];
        assert_eq!(
            (convert.cmd.clone(), convert.size),
            (Some(Cmd::InkToShape), Size::Large)
        );
    }

    #[test]
    fn the_audio_tab_follows_the_recording_and_the_selection() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        // Neither recording nor an audio item selected: nothing to show.
        assert!(groups(RibbonTab::Recording, &NoHost).is_empty());
        let recording = groups(RibbonTab::Recording, &Recorder { paused: false });
        let ids: Vec<_> = recording.iter().map(|group| group.id).collect();
        assert_eq!(ids, ["recording", "playback"]);
        // The time is a readout: no command, and a red dot while recording.
        let readout = |groups: &[Group]| {
            groups[0]
                .controls
                .iter()
                .find(|control| control.cmd.is_none())
                .cloned()
                .expect("the time readout")
        };
        let time = readout(&recording);
        assert_eq!(time.label, "0:42");
        assert_eq!(time.glyph_color, Some(DANGER));
        assert_eq!(recording[0].controls[0].label, t!("ribbon-pause"));
        // Paused: the button resumes, and the dot fades.
        let paused = groups(RibbonTab::Recording, &Recorder { paused: true });
        assert_eq!(paused[0].controls[0].label, t!("ribbon-resume"));
        assert_eq!(readout(&paused).glyph_color, None);
    }

    /// A page in Lavender on college-ruled paper, with Always Create Pages with Rule
    /// Lines on. Its menus are the View tab's dropdowns.
    struct StyledPage;

    impl RibbonHost for StyledPage {
        fn enabled(&self, _: &Cmd) -> bool {
            true
        }
        fn checked(&self, cmd: &Cmd) -> bool {
            matches!(
                cmd,
                Cmd::PageColor(Some(PageColor::Lavender))
                    | Cmd::RuleLines(RuleLines::Lines(crate::model::RuleSize::College))
                    | Cmd::AlwaysRuleLines
            )
        }
        fn menu_enabled(&self, _: MenuId) -> bool {
            true
        }
        fn combo_text(&self, _: MenuId) -> String {
            String::new()
        }
        fn contextual(&self) -> Vec<RibbonTab> {
            Vec::new()
        }
        fn menu(&self, ui: &mut Ui, menu: MenuId, t: Tokens) -> Option<Cmd> {
            match menu {
                MenuId::PageColor => page_color_menu(ui, self, t),
                MenuId::RuleLines => rule_lines_menu(ui, self, t),
                _ => None,
            }
        }
        fn recording(&self) -> Option<(String, bool)> {
            None
        }
        fn audio_selected(&self) -> bool {
            false
        }
    }

    /// `menu`'s contents on their own, keeping the last command picked.
    fn dropdown(menu: MenuId) -> egui_kittest::Harness<'static, Option<Cmd>> {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(400.0, 400.0))
            .with_context_setup(crate::theme::install)
            .build_ui_state(
                move |ui, picked: &mut Option<Cmd>| {
                    if let Some(cmd) = StyledPage.menu(ui, menu, Tokens::light()) {
                        *picked = Some(cmd);
                    }
                },
                None,
            );
        harness.run();
        harness
    }

    /// View has the desktop notebook's groups in the desktop notebook's order: Views, Tabs Layout (a menu), Tools,
    /// Page Setup (the Page Color and Rule Lines dropdowns, then the Hide Page Title and
    /// Switch Background toggles), Zoom, and Window last with New Quick Note. The
    /// dropdowns name each colour and style, mark the page's own and give the command
    /// picked.
    #[test]
    fn the_view_ribbon_has_page_setup_and_its_dropdowns_pick() {
        use crate::model::{GridSize, RuleSize};
        use egui_kittest::kittest::{NodeT, Queryable};
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let view = groups(RibbonTab::View, &NoHost);
        let ids: Vec<_> = view.iter().map(|group| group.id).collect();
        assert_eq!(
            ids,
            [
                "views",
                "tabs-layout",
                "view-tools",
                "page-setup",
                "zoom",
                "window"
            ]
        );
        let tabs = &view[1].controls;
        assert_eq!(tabs.len(), 1);
        assert!(matches!(
            tabs[0].kind,
            ControlKind::Menu(MenuId::TabsLayout)
        ));
        assert_eq!(tabs[0].size, Size::Large);
        let setup = &view[3].controls;
        assert!(matches!(
            setup[0].kind,
            ControlKind::Menu(MenuId::PageColor)
        ));
        assert!(matches!(
            setup[1].kind,
            ControlKind::Menu(MenuId::RuleLines)
        ));
        assert!(matches!(setup[2].kind, ControlKind::Toggle));
        assert_eq!(setup[2].cmd, Some(Cmd::HidePageTitle));
        assert!(matches!(setup[3].kind, ControlKind::Toggle));
        assert_eq!(setup[3].cmd, Some(Cmd::SwitchBackground));
        assert_eq!(setup.len(), 4);
        assert!(setup.iter().all(|control| control.size == Size::Large));
        let window = &view[5].controls;
        assert_eq!(window.len(), 1);
        assert!(matches!(window[0].kind, ControlKind::Button));
        assert_eq!(window[0].cmd, Some(Cmd::NewQuickNote));
        assert_eq!(window[0].size, Size::Large);
        let zoom: Vec<_> = view[4].controls.iter().map(|c| c.cmd.clone()).collect();
        assert_eq!(
            zoom,
            [
                Some(Cmd::ZoomOut),
                Some(Cmd::ZoomIn),
                None,
                Some(Cmd::Zoom100),
                Some(Cmd::PageWidth)
            ]
        );
        assert!(matches!(
            view[4].controls[2].kind,
            ControlKind::Combo {
                menu: MenuId::Zoom,
                ..
            }
        ));

        let checked = |harness: &egui_kittest::Harness<'_, Option<Cmd>>, label: &str| {
            harness.get_by_label(label).accesskit_node().toggled()
                == Some(egui::accesskit::Toggled::True)
        };
        // Page Color: sixteen swatches, four to a row, then No Color across under them.
        let mut colors = dropdown(MenuId::PageColor);
        for (index, color) in PageColor::ALL.into_iter().enumerate() {
            let label = color.label();
            assert_eq!(checked(&colors, &label), color == PageColor::Lavender);
            let rect = colors.get_by_label(&label).rect();
            let first = colors
                .get_by_label(&PageColor::ALL[index / 4 * 4].label())
                .rect();
            assert_eq!(rect.top(), first.top(), "{color:?} is on its row");
        }
        let blue = colors.get_by_label(&PageColor::Blue.label()).rect();
        let orange = colors.get_by_label(&PageColor::Orange.label()).rect();
        let none = colors.get_by_label(&t!("ribbon-no-color")).rect();
        assert!(none.top() > orange.bottom() && none.left() <= blue.left());
        assert!(none.right() >= orange.right(), "No Color spans the grid");
        colors.get_by_label(&PageColor::Green.label()).click();
        colors.run();
        assert_eq!(
            *colors.state(),
            Some(Cmd::PageColor(Some(PageColor::Green)))
        );
        colors.get_by_label(&t!("ribbon-no-color")).click();
        colors.run();
        assert_eq!(*colors.state(), Some(Cmd::PageColor(None)));
        // More Colors… under No Color.
        let more = format!("{}    {}", icons::PALETTE, t!("ribbon-more-colors"));
        assert!(colors.get_by_label(&more).rect().top() > none.bottom());
        colors.get_by_label(&more).click();
        colors.run();
        assert_eq!(*colors.state(), Some(Cmd::MorePageColors));

        // Rule Lines: None and the four ruled spacings in a row, the four grids under
        // them, then Always Create Pages with Rule Lines.
        let mut rules = dropdown(MenuId::RuleLines);
        let none = rules.get_by_label(&RuleLines::None.label()).rect();
        let every = std::iter::once(RuleLines::None)
            .chain(RuleLines::LINES)
            .chain(RuleLines::GRIDS);
        for rule in every {
            let label = rule.label();
            assert_eq!(
                checked(&rules, &label),
                rule == RuleLines::Lines(RuleSize::College)
            );
            let rect = rules.get_by_label(&label).rect();
            assert_eq!(rect.size(), vec2(64.0, 64.0), "{rule:?}");
            if matches!(rule, RuleLines::Grid(_)) {
                assert!(rect.top() > none.bottom(), "{rule:?} is on the second row");
            } else {
                assert_eq!(rect.top(), none.top(), "{rule:?} is on the first row");
            }
        }
        let always = t!("ribbon-always-rule-lines");
        let grids = rules
            .get_by_label(&RuleLines::Grid(GridSize::Small).label())
            .rect();
        assert!(rules.get_by_label(&always).rect().top() > grids.bottom());
        // Rule Line Color › sits between the grids and Always…: Automatic, then the ink
        // colours.
        let color = t!("ribbon-rule-line-color");
        let row = rules.get_by_label(&color).rect();
        assert!(row.top() > grids.bottom());
        assert!(row.bottom() <= rules.get_by_label(&always).rect().top());
        rules.get_by_label(&color).click();
        rules.run();
        rules
            .get_by_label(&t!("ribbon-rule-color-automatic"))
            .click();
        rules.run();
        assert_eq!(*rules.state(), Some(Cmd::RuleLineColor(None)));
        rules.get_by_label(&color).click();
        rules.run();
        let (green, _) = crate::canvas::PEN_COLORS[6];
        rules.get_by_label(&green.hex()).click();
        rules.run();
        assert_eq!(*rules.state(), Some(Cmd::RuleLineColor(Some(green))));
        rules
            .get_by_label(&RuleLines::Grid(GridSize::Medium).label())
            .click();
        rules.run();
        assert_eq!(
            *rules.state(),
            Some(Cmd::RuleLines(RuleLines::Grid(GridSize::Medium)))
        );
        rules.get_by_label(&always).click();
        rules.run();
        assert_eq!(*rules.state(), Some(Cmd::AlwaysRuleLines));
    }
}
