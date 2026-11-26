use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, Id, Key, KeyboardShortcut, Layout,
    Margin, Modal, Modifiers, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui,
};

use crate::canvas::{
    self, CanvasState, CanvasTool, EraserMode, PictureCmd, Reveal, Rotation, SingleKind,
};
use crate::db::{Library, PageFilter, SearchFilter, TagScope};
use crate::editor::{
    self, AlignPick, EditorState, Mark, MarkerPick, SymbolPick, TagCommand, TextState,
    ToolbarCommand,
};
use crate::export;
use crate::i18n::{self, t, Lang};
use crate::icons;
use crate::images::TextureCache;
use crate::import_notebook::{
    ImportError, ImportJob, ImportPlan, ImportReport, ImportSummary, Source,
};
use crate::model::{
    display_title, is_openable_link, now_ms, page_link, parse_page_link, section_link, Block,
    BlockKind, CanvasDoc, CanvasItem, CanvasKind, CustomTag, Document, FontFace, ListMarker,
    ListStart, Page, PageColor, PageSummary, PageVersion, ParaAlign, ParaSpacing, Rgb, RuleLines,
    RuleSize, SearchHit, ShapeKind, StyledText, TagMark, TagSymbol, TaggedLine, TrashedSection,
    Workspace, MAX_PARA_SPACING, PAGE_CONTENT_TOP, PAGE_LEFT, SECTION_LINK_PREFIX,
};
use crate::outline;
use crate::reading::{self, ReaderState};
use crate::ribbon::{
    self, Cmd, EditsRange, MenuId, RibbonHost, RibbonLayout, RibbonTab, TabsLayout, VersionScope,
};
use crate::table::TableCmd;
use crate::tags::{TagGallery, TagKey};
use crate::templates::{self, PageTemplate};
use crate::theme::{self, Tokens, SECTION_COLORS};
use crate::util;
use crate::widgets;

/// The navigation pane's default width (the desktop notebook's, resizable from 150 to 360).
const NAV_WIDTH: f32 = 195.0;
/// The page list's default width (resizable from 140 to 400): room for "Ajouter une
/// page" and the sort button side by side.
const PAGE_LIST_WIDTH: f32 = 180.0;
/// The page list's sort button, square, and the gap between its header's controls.
const SORT_BUTTON: f32 = 28.0;
const HEADER_GAP: f32 = 2.0;
/// The task pane's default width (resizable from 240 to 480).
const TASKPANE_WIDTH: f32 = 300.0;
/// The narrowest page card the task pane leaves before the navigation pane, then the page
/// list, fold away.
const MIN_PAGE_WIDTH: f32 = 420.0;
/// The header strip's search box, at most (a third of the window when that is less).
const HEADER_SEARCH_WIDTH: f32 = 470.0;
/// The header strip's title when no page is open: the app's name, never translated.
const APP_NAME: &str = "Folio";
/// The Options dialog: its width, its list of pages (wide enough for the French "Enregistrement
/// et sauvegarde"), the page beside it, and their height (a longer page scrolls).
const OPTIONS_WIDTH: f32 = 800.0;
const OPTIONS_LIST_WIDTH: f32 = 200.0;
const OPTIONS_PAGE_WIDTH: f32 = 560.0;
const OPTIONS_HEIGHT: f32 = 400.0;
/// An Options row's label column, and the width of its combo boxes.
const OPTIONS_LABEL_WIDTH: f32 = 200.0;
const OPTIONS_COMBO_WIDTH: f32 = 220.0;
/// Backstage's left column, and the widest its pages' content grows on a wide window.
const BACKSTAGE_NAV_WIDTH: f32 = 220.0;
const BACKSTAGE_COLUMN: f32 = 640.0;
/// Backstage › Export: the width of its "Export current" column.
const EXPORT_SCOPE_WIDTH: f32 = 260.0;
/// Insert › Symbol's recent symbols, as many as are kept (its row shows eight).
const RECENT_SYMBOLS: usize = 16;
/// More Symbols…: its width, and its grid's columns and the rows shown before it scrolls.
const SYMBOLS_DIALOG_WIDTH: f32 = 440.0;
/// The highest Customize Numbering's Start at takes.
const MAX_LIST_START: u32 = 999;
const SYMBOL_COLUMNS: usize = 12;
const SYMBOL_ROWS: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    fn code(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Light => "light",
            Appearance::Dark => "dark",
        }
    }

    fn from_code(code: &str) -> Self {
        match code {
            "light" => Appearance::Light,
            "dark" => Appearance::Dark,
            _ => Appearance::System,
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            Appearance::System => egui::ThemePreference::System,
            Appearance::Light => egui::ThemePreference::Light,
            Appearance::Dark => egui::ThemePreference::Dark,
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Appearance::System => t!("appearance-system"),
            Appearance::Light => t!("appearance-light"),
            Appearance::Dark => t!("appearance-dark"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum View {
    Section,
    Trash,
}

/// The page list's order, as the desktop notebook's sort button offers it. `None` is the section's own
/// order, the only one in which pages can be dragged and nested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSort {
    None,
    Alphabetical,
    Created,
    Modified,
}

impl PageSort {
    const ALL: [PageSort; 4] = [
        PageSort::None,
        PageSort::Alphabetical,
        PageSort::Created,
        PageSort::Modified,
    ];

    fn code(self) -> &'static str {
        match self {
            PageSort::None => "none",
            PageSort::Alphabetical => "alpha",
            PageSort::Created => "created",
            PageSort::Modified => "modified",
        }
    }

    fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|sort| sort.code() == code)
            .unwrap_or(PageSort::None)
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            PageSort::None => t!("app-sort-none"),
            PageSort::Alphabetical => t!("app-sort-alphabetical"),
            PageSort::Created => t!("app-sort-created"),
            PageSort::Modified => t!("app-sort-modified"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveState {
    Saved,
    Dirty,
    Saving,
    Error,
}

#[derive(Debug, Clone)]
enum ConfirmAction {
    DeleteNotebook(String),
    /// Delete for a section: it goes to the Recycle Bin.
    DeleteSection(String),
    /// Delete Forever for a section in the Recycle Bin.
    DeleteTrashedSection(String),
    DeletePage(String),
    EmptyTrash,
    DeleteGroup(String),
    /// History › Delete All Versions: of a page, a section or a notebook (its id).
    DeleteVersions(VersionScope, String),
}

/// A page cut or copied from the page list, waiting for Paste (the desktop notebook copies pages
/// within its own window this way).
#[derive(Debug, Clone)]
struct PageClip {
    id: String,
    cut: bool,
}

/// What the Move or Copy dialog works on.
#[derive(Debug, Clone)]
enum MoveWhat {
    /// Move or copy a page (with its subpages when moving) into a section.
    Page(String),
    /// Move or copy a section into a notebook or section group.
    Section(String),
    /// Merge a section's pages into another section.
    Merge(String),
}

/// A row picked in the Move or Copy dialog's tree.
#[derive(Debug, Clone, PartialEq)]
enum MoveTarget {
    Notebook(String),
    Group { notebook: String, id: String },
    Section(String),
}

/// The desktop notebook import dialog's steps.
#[derive(Debug, Clone)]
enum ImportDialog {
    Choose,
    Running,
    /// Boxed: the report is much larger than the other steps.
    Done(Box<ImportSummary>),
    Failed(ImportError),
}

#[derive(Debug, Clone)]
enum Dialog {
    NewNotebook {
        name: String,
        color: String,
    },
    NewSection {
        notebook_id: String,
        group_id: Option<String>,
        name: String,
        color: String,
    },
    NewGroup {
        notebook_id: String,
        parent_id: Option<String>,
        name: String,
        color: String,
    },
    RenameNotebook {
        id: String,
        name: String,
        color: String,
    },
    RenameSection {
        id: String,
        name: String,
        color: String,
    },
    RenameGroup {
        id: String,
        name: String,
        color: String,
    },
    Confirm {
        title: String,
        body: String,
        confirm_label: String,
        action: ConfirmAction,
    },
    Settings,
    Shortcuts,
    /// Help › About Folio.
    About,
    /// Move or Copy… (a page or a section) and Merge into Another Section….
    MoveOrCopy {
        what: MoveWhat,
        filter: String,
        target: Option<MoveTarget>,
        /// Merge asks once more before it deletes the section.
        confirm: bool,
    },
    Link {
        edit: editor::LinkEdit,
        query: String,
        searched: Option<String>,
        choices: Vec<LinkChoice>,
    },
    /// View › Page Color › More Colors…: any colour for the open page, picked on a plane
    /// of colours or typed as "#rrggbb" (`hex`, kept as typed).
    PageColor {
        color: Color32,
        hex: String,
    },
    /// More Colors… for the font colour or a pen: any colour, picked on a plane or typed
    /// as "#rrggbb".
    MoreColors {
        target: ColorTarget,
        color: Color32,
        hex: String,
    },
    /// Insert › Symbol ▾ › More Symbols…: the symbols of one set (`SYMBOL_SETS[set]`), or
    /// those matching `query` in every set; `picked` is the one Insert puts in.
    Symbols {
        set: usize,
        query: String,
        picked: Option<char>,
    },
    /// File › Open › Import the desktop notebook…: choose, progress, report.
    ImportNotebook(ImportDialog),
    /// Home › Tags ▾ › Customize Tags…: the gallery's order and the custom tags.
    CustomizeTags(CustomizeTags),
    /// Home › Numbering ▾ › Customize Numbering…: the paragraph's numbering style and the
    /// number it starts at.
    CustomizeNumbering {
        marker: ListMarker,
        start: u32,
    },
    /// Home › Paragraph Alignment ▾ › Paragraph Spacing Options…: points before and after
    /// the paragraph, and its least line height.
    ParagraphSpacing {
        before: f32,
        after: f32,
        line: f32,
    },
}

/// What More Colors… colours: the text (Home › Font Color) or the pen (Draw › pen ▾).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorTarget {
    Font,
    Pen,
}

/// Customize Tags: the desktop notebook's list of tags with New Tag…, Modify Tag…, Delete and ↑ / ↓.
#[derive(Debug, Clone)]
struct CustomizeTags {
    /// A working copy: OK keeps it, Cancel drops it.
    gallery: TagGallery,
    /// The selected row, an index into `gallery.order`.
    selected: usize,
    /// New Tag… / Modify Tag…: the tag being made or changed.
    editing: Option<TagForm>,
    /// The list scrolls to the selected row on its next frame (after ↑ / ↓ or a new tag).
    reveal: bool,
}

impl CustomizeTags {
    /// The custom tag the selected row is, if it is one: Modify and Delete act on it.
    fn selected_custom(&self) -> Option<u16> {
        match self.gallery.order.get(self.selected) {
            Some(TagKey::Custom(index)) => Some(*index),
            _ => None,
        }
    }

    /// ↑ / ↓: the selected tag swaps places with the one above or below it.
    fn move_selected(&mut self, down: bool) {
        let to = if down {
            self.selected + 1
        } else {
            let Some(to) = self.selected.checked_sub(1) else {
                return;
            };
            to
        };
        if to >= self.gallery.order.len() {
            return;
        }
        self.gallery.order.swap(self.selected, to);
        self.selected = to;
        self.reveal = true;
        self.gallery.bump();
    }

    /// Delete: the selected custom tag leaves the gallery, and the custom tags after it
    /// move down one index. Paragraphs already tagged keep their own copy of it.
    fn delete_selected(&mut self) {
        let Some(index) = self.selected_custom() else {
            return;
        };
        self.gallery.custom.remove(usize::from(index));
        self.gallery
            .order
            .retain(|key| *key != TagKey::Custom(index));
        for key in &mut self.gallery.order {
            if let TagKey::Custom(other) = key {
                if *other > index {
                    *other -= 1;
                }
            }
        }
        self.selected = self
            .selected
            .min(self.gallery.order.len().saturating_sub(1));
        self.gallery.bump();
    }

    /// The form's OK. A new tag goes in right after the selected row and is selected; a
    /// changed one replaces the old one under the same key, so it keeps its place.
    fn save_form(&mut self, form: TagForm) {
        let tag = CustomTag {
            name: form.name.trim().to_string(),
            symbol: form.symbol,
            font_color: form.font_color,
            highlight: form.highlight,
            checked: false,
        };
        match form.index {
            Some(index) => {
                if let Some(old) = self.gallery.custom.get_mut(usize::from(index)) {
                    *old = tag;
                }
            }
            None => {
                let Ok(index) = u16::try_from(self.gallery.custom.len()) else {
                    return;
                };
                self.gallery.custom.push(tag);
                let at = (self.selected + 1).min(self.gallery.order.len());
                self.gallery.order.insert(at, TagKey::Custom(index));
                self.selected = at;
            }
        }
        self.reveal = true;
        self.gallery.bump();
    }

    /// Whether `name` is already a tag's, ignoring case: a custom tag's, or a built-in's
    /// in the interface language. `except` is the custom tag being changed.
    fn name_taken(&self, name: &str, except: Option<u16>) -> bool {
        let name = name.trim().to_lowercase();
        self.gallery.order.iter().any(|key| {
            except.map(TagKey::Custom) != Some(*key)
                && self.gallery.label(*key).to_lowercase() == name
        })
    }
}

/// Customize Tags' New Tag / Modify Tag form.
#[derive(Debug, Clone)]
struct TagForm {
    /// The custom tag changed, `None` for a new one.
    index: Option<u16>,
    name: String,
    symbol: TagSymbol,
    font_color: Option<Rgb>,
    highlight: Option<Rgb>,
}

/// File › Backstage: the full-window view the File tab opens, as in Office.
struct Backstage {
    page: BackstagePage,
    /// New: the notebook being named.
    name: String,
    color: String,
    /// The name field takes focus when New opens.
    focus: bool,
    /// Export: what to export.
    export: ExportScope,
    /// Info: the pages in each notebook (by id), counted when Info shows, so the count
    /// is not asked of the library every frame.
    page_counts: Option<HashMap<String, usize>>,
}

/// Backstage's pages, in its left column's order. Options and Close are commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackstagePage {
    Info,
    New,
    Open,
    Print,
    Export,
}

impl BackstagePage {
    const ALL: [BackstagePage; 5] = [
        BackstagePage::Info,
        BackstagePage::New,
        BackstagePage::Open,
        BackstagePage::Print,
        BackstagePage::Export,
    ];

    fn label(self) -> Cow<'static, str> {
        match self {
            BackstagePage::Info => t!("app-backstage-info"),
            BackstagePage::New => t!("app-backstage-new"),
            BackstagePage::Open => t!("app-backstage-open"),
            BackstagePage::Print => t!("app-backstage-print"),
            BackstagePage::Export => t!("app-backstage-export"),
        }
    }
}

/// Backstage › Export: what "Export current" exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportScope {
    Page,
    Section,
    Notebook,
}

impl ExportScope {
    const ALL: [ExportScope; 3] = [
        ExportScope::Page,
        ExportScope::Section,
        ExportScope::Notebook,
    ];

    fn label(self) -> Cow<'static, str> {
        match self {
            ExportScope::Page => t!("app-export-page"),
            ExportScope::Section => t!("app-export-section"),
            ExportScope::Notebook => t!("app-export-notebook"),
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            ExportScope::Page => icons::FILE,
            ExportScope::Section => icons::FOLDER_SIMPLE,
            ExportScope::Notebook => icons::NOTEBOOK,
        }
    }
}

/// The pages of the Options dialog (`Dialog::Settings`), in the desktop notebook's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionsPage {
    General,
    Display,
    Proofing,
    Save,
    Language,
    Advanced,
}

impl OptionsPage {
    const ALL: [OptionsPage; 6] = [
        OptionsPage::General,
        OptionsPage::Display,
        OptionsPage::Proofing,
        OptionsPage::Save,
        OptionsPage::Language,
        OptionsPage::Advanced,
    ];

    fn label(self) -> Cow<'static, str> {
        match self {
            OptionsPage::General => t!("app-options-general"),
            OptionsPage::Display => t!("app-options-display"),
            OptionsPage::Proofing => t!("app-options-proofing"),
            OptionsPage::Save => t!("app-options-save"),
            OptionsPage::Language => t!("app-options-language"),
            OptionsPage::Advanced => t!("app-options-advanced"),
        }
    }

    /// The line at the top of the page that says what it holds.
    fn caption(self) -> Cow<'static, str> {
        match self {
            OptionsPage::General => t!("app-options-general-caption"),
            OptionsPage::Display => t!("app-options-display-caption"),
            OptionsPage::Proofing => t!("app-options-proofing-caption"),
            OptionsPage::Save => t!("app-options-save-caption"),
            OptionsPage::Language => t!("app-options-language-caption"),
            OptionsPage::Advanced => t!("app-options-advanced-caption"),
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            OptionsPage::General => icons::GEAR_SIX,
            OptionsPage::Display => icons::DOCUMENT_ONE_PAGE,
            OptionsPage::Proofing => icons::TEXT_PROOFING_TOOLS,
            OptionsPage::Save => icons::FLOPPY_DISK,
            OptionsPage::Language => icons::LOCAL_LANGUAGE,
            OptionsPage::Advanced => icons::OPTIONS,
        }
    }
}

/// The settings Options can change, as they were when it opened: Cancel puts them back.
#[derive(Debug, Clone, PartialEq)]
struct OptionsSnapshot {
    appearance: Appearance,
    ribbon_layout: RibbonLayout,
    tabs_layout: TabsLayout,
    page_list_left: bool,
    notebooks_pinned: bool,
    scrollbar_left: bool,
    spell_language: String,
    ui_language: Option<Lang>,
    search_scope: ScopeKind,
    printout_background: bool,
    /// S1's Options › General › Show Mini Toolbar on selection.
    mini_on_selection: bool,
    /// S2's Options › Advanced › Calculate mathematical expressions automatically.
    auto_calc: bool,
    /// Whether the navigation pane shows: pinning the notebooks shows it, so Cancel hides
    /// it again.
    show_library: bool,
}

#[derive(Debug, Clone)]
enum DialogAction {
    CreateNotebook {
        name: String,
        color: String,
    },
    CreateSection {
        notebook_id: String,
        group_id: Option<String>,
        name: String,
        color: String,
    },
    CreateGroup {
        notebook_id: String,
        parent_id: Option<String>,
        name: String,
        color: String,
    },
    RenameNotebook {
        id: String,
        name: String,
        color: String,
    },
    RenameSection {
        id: String,
        name: String,
        color: String,
    },
    RenameGroup {
        id: String,
        name: String,
        color: String,
    },
    Confirm(ConfirmAction),
    SetAppearance(Appearance),
    Backup,
    OpenDataFolder,
    ApplyLink(editor::LinkEdit),
    SetSpellLanguage(String),
    SetUiLanguage(Option<Lang>),
    ClearPersonalDictionary,
    RestoreVersion {
        page_id: String,
        version_id: i64,
    },
    DeleteVersions(String),
    /// Delete Version: forgets the one version picked in the Versions pane.
    DeleteVersion {
        page_id: String,
        version_id: i64,
    },
    /// A ribbon command given from a dialog (Options › General › Ribbon, Options ›
    /// Display › Tabs layout).
    Run(Cmd),
    /// Options › Display, each stored as a setting.
    SetPageListLeft(bool),
    SetNotebooksPinned(bool),
    SetScrollbarLeft(bool),
    /// Options › Advanced › Default search scope.
    SetSearchScope(ScopeKind),
    /// Options › Advanced › Printouts › Automatically set inserted file printouts in the
    /// background.
    SetPrintoutBackground(bool),
    /// Options › General › Show Mini Toolbar on selection.
    SetMiniToolbar(bool),
    /// Options › Advanced › Editing › Calculate mathematical expressions automatically.
    SetAutoCalc(bool),
    /// Customize Tags › OK: the gallery's new order and custom tags, kept as settings.
    SaveTags(TagGallery),
    /// The Move or Copy dialog's Move and Copy, for a page and for a section, and Merge.
    MovePage {
        id: String,
        section: String,
    },
    CopyPage {
        id: String,
        section: String,
    },
    MoveSection {
        id: String,
        notebook: String,
        group: Option<String>,
    },
    CopySection {
        id: String,
        notebook: String,
        group: Option<String>,
    },
    MergeSection {
        from: String,
        into: String,
    },
    /// The desktop notebook import dialog's "Choose a file…" (`folder: false`) and "Choose a folder…".
    ImportPick {
        folder: bool,
    },
    /// The desktop notebook import dialog's Cancel while the import runs.
    ImportCancel,
}

#[derive(Debug, Clone)]
enum LibraryAction {
    SelectSection(String),
    ToggleNotebook(String),
    NewSection(String),
    RenameNotebook(String),
    DeleteNotebook(String),
    RenameSection(String),
    DeleteSection(String),
    ToggleGroup(String),
    /// (notebook, parent group)
    NewGroup(String, Option<String>),
    /// (notebook, group)
    NewSectionIn(String, Option<String>),
    RenameGroup(String),
    DeleteGroup(String),
    MoveSection {
        id: String,
        group_id: Option<String>,
        before: Option<String>,
    },
    /// A page row dropped on a section: (page, section).
    MovePageTo(String, String),
    /// A notebook's Move Up (`true`) or Move Down among the notebooks.
    MoveNotebook(String, bool),
    /// A notebook's "Notebook Recycle Bin": the Recycle Bin view, unless it is open.
    OpenRecycleBin,
    /// The empty navigation area's "New notebook".
    NewNotebook,
    /// A section's "Export…": Backstage › Export, with the section to export.
    ExportSection(String),
    /// A section's "Move or Copy…".
    MoveOrCopySection(String),
    /// A section's "Merge into Another Section…".
    MergeSection(String),
    /// A section's "Copy Link to Section".
    CopySectionLink(String),
    /// Section Color ›: (section, colour; `theme::NO_COLOR` for None).
    SectionColor(String, String),
}

#[derive(Debug, Clone)]
enum PageListAction {
    Open(String),
    /// Rename: the page opens with its title selected.
    Rename(String),
    Trash(String),
    Restore(String),
    Delete(String),
    /// Cut and Copy put the page on the page clipboard (`FolioApp::page_clip`).
    CutPage(String),
    CopyPage(String),
    /// Paste the clipped page into the listed section: right after the row `after` and
    /// its subpages, at its level, or at the end (the empty area's Paste).
    PastePage {
        after: Option<String>,
    },
    /// Move or Copy… (Ctrl+Alt+M): the dialog, for this page.
    MoveOrCopyPage(String),
    Move(String, String),
    /// Demote (`1`) or promote (`-1`) a page with its subpages.
    Level(String, i8),
    /// Alt+Shift+↑ (`true`) / ↓: the page and its subpages past the page above or below.
    Step(String, bool),
    Fold(String, bool),
    /// Move a page with its subpages into a section, before a page, at a level.
    Place {
        id: String,
        section_id: String,
        before: Option<String>,
        level: Option<u8>,
    },
    /// A new page below this one (`true`: as its subpage).
    NewBelow(String, bool),
    /// The page's versions (it opens first).
    Versions(String),
    /// A section in the Recycle Bin: picked (a click), restored, or deleted forever.
    PickTrashedSection(String),
    RestoreSection(String),
    DeleteTrashedSection(String),
}

#[derive(Debug, Clone)]
enum EditorAction {
    Restore,
    Delete,
    PrintPage,
    OpenReader,
    Versions,
}

/// Pages visited since Folio started, for the desktop notebook's Back and Forward (Alt+← / Alt+→).
#[derive(Debug, Default)]
struct Visited {
    back: Vec<String>,
    forward: Vec<String>,
    /// Back or Forward is opening a page: that is no new visit.
    walking: bool,
    /// The search's ↓ / ↑ previews are showing pages: the page open before the first one
    /// (`None` inside when none was). The previews are no visits; the page the search
    /// ends on is one, from that page (`FolioApp::settle_preview`).
    previewed: Option<Option<String>>,
}

/// How many visits Back keeps.
const MAX_VISITED: usize = 100;

#[derive(Default)]
struct SearchState {
    open: bool,
    query: String,
    last_query: String,
    results: Vec<SearchHit>,
    recent: Vec<SearchHit>,
    selected: usize,
    focus: bool,
    /// Where this search looks. Each search starts in `default_scope`.
    scope: ScopeKind,
    /// The scope `results` were found in: another one runs the search again.
    last_scope: ScopeKind,
    /// Where searches look when the search opens (Options › Advanced, or Set This Scope
    /// as Default), stored as the setting `search_scope`.
    default_scope: ScopeKind,
    /// The search box (in the header, or at the end of the section tabs) and the dropdown
    /// under it, as last drawn: a press outside both closes the search.
    box_rect: Option<Rect>,
    dropdown_rect: Option<Rect>,
    /// The scope menu (the box's scope chip) was open as the box was drawn this frame:
    /// the dropdown leaves the keys to it, so Escape closes the menu alone.
    scope_menu: bool,
    /// ↓ and ↑ pressed this frame, taken before the box drew: they move through the
    /// dropdown's rows (`search_dropdown`), never the box's caret.
    arrows: (bool, bool),
}

/// A row of the search's scope menu (`FolioApp::scope_menu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopePick {
    /// Find on This Page: find on page, with the words typed.
    Page,
    /// Search again, in this scope.
    Scope(ScopeKind),
    /// Set This Scope as Default.
    SetDefault,
}

/// A search scope as the library filters on it (`FolioApp::search_filter`).
#[derive(Debug, Clone, PartialEq)]
enum OwnedFilter {
    All,
    Notebook(String),
    Sections(Vec<String>),
}

impl OwnedFilter {
    fn as_db(&self) -> SearchFilter<'_> {
        match self {
            OwnedFilter::All => SearchFilter::All,
            OwnedFilter::Notebook(id) => SearchFilter::Notebook(id),
            OwnedFilter::Sections(ids) => SearchFilter::Sections(ids),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagsScope {
    Section,
    Notebook,
    All,
}

/// the desktop notebook's Tags Summary: every tagged line in a scope, grouped by tag (or as Group tags
/// by says).
struct TagsPane {
    scope: TagsScope,
    group_by: TagsGroupBy,
    /// Show only unchecked items: ticked lines are hidden.
    unchecked_only: bool,
    /// The section or notebook the scope meant when the lines were listed. Opening a page
    /// elsewhere does not move the summary there; Refresh or another scope does.
    section_id: Option<String>,
    notebook_id: Option<String>,
    lines: Vec<TaggedLine>,
    /// Each line's group, numbered in order (`group_ids`): the pane finds where a heading
    /// goes by comparing numbers as it draws, not names and dates.
    groups: Vec<usize>,
}

/// Side panels folded away to make room for the task pane, restored when it closes.
#[derive(Default)]
struct PaneFold {
    hid_library: bool,
    hid_pages: bool,
    /// The window width the side panels were last fitted to.
    fitted_width: f32,
}

/// The task pane's content when it is not the Tags pane (`tags_pane`). the desktop notebook docks one
/// task pane at a time: the page's versions, spelling, or a list of pages.
enum TaskPane {
    Versions(VersionsPane),
    Spelling(SpellPane),
    Results(ResultsPane),
}

/// History › Page Versions: the kept versions of the open page, which the pane follows.
struct VersionsPane {
    /// The page the versions are of.
    page_id: String,
    versions: Vec<PageVersion>,
    /// The version previewed.
    selected: usize,
    /// Delete all versions was clicked once: the next click deletes them.
    confirm_delete: bool,
}

/// Review › Spelling: the open page's misspellings, one at a time.
#[derive(Default)]
struct SpellPane {
    /// The page being checked: the pane follows the page opened, from its top.
    page_id: Option<String>,
    /// Words passed over with Ignore: (note, paragraph, first char).
    skipped: HashSet<(u64, u64, usize)>,
    current: Option<SpellHit>,
    /// The highlighted suggestion.
    picked: usize,
    /// The walk reached the end of the page.
    done: bool,
}

/// What a click in the Spelling pane asks for, run once the pane is drawn.
enum SpellStep {
    /// Change the word to this suggestion.
    Change(String),
    Ignore,
    IgnoreAll,
    Add,
    /// Check the page again from its top, forgetting the words ignored.
    StartOver,
    /// Proofing options…
    Options,
}

/// A misspelled word on the page, as the Spelling pane walks them.
#[derive(Debug, Clone, PartialEq)]
struct SpellHit {
    item: u64,
    block: u64,
    range: std::ops::Range<usize>,
    word: String,
    language: Option<usize>,
}

/// The paragraphs the Spelling pane checks, in reading order: notes from the top-left (as
/// `CanvasDoc::plain_text` orders them), their paragraphs in order. Code paragraphs and
/// non-text blocks are skipped.
fn spelled_paragraphs(page: &Page) -> Vec<(u64, &Block)> {
    let mut items: Vec<&CanvasItem> = page
        .canvas
        .items
        .iter()
        .filter(|item| item.doc().is_some())
        .collect();
    items.sort_by_key(|item| (item.y as i32, item.x as i32));
    let mut paragraphs = Vec::new();
    for item in items {
        let doc = item.doc().expect("filtered");
        for block in &doc.blocks {
            if !block.kind.is_textual() || matches!(block.kind, BlockKind::Code) {
                continue;
            }
            paragraphs.push((item.id, block));
        }
    }
    paragraphs
}

/// The page's misspelled words in reading order: notes from the top-left (as
/// `CanvasDoc::plain_text` orders them), their paragraphs in order, words in order. Code
/// paragraphs and non-text blocks are skipped; inline code is skipped by the speller.
fn page_misspellings(page: &Page, spell: &mut crate::spell::Speller) -> Vec<SpellHit> {
    let mut hits = Vec::new();
    for (item, block) in spelled_paragraphs(page) {
        let id = egui::Id::unique(("spell-pane", item, block.id));
        for range in spell.misspelled(id, &block.text).iter() {
            let word: String = block
                .text
                .text
                .chars()
                .skip(range.start)
                .take(range.len())
                .collect();
            let language = spell.language_near(&block.text.text, range.clone());
            hits.push(SpellHit {
                item,
                block: block.id,
                range: range.clone(),
                word,
                language,
            });
        }
    }
    hits
}

/// The first hit after `after` (the same paragraph and a later char, or a later paragraph
/// or note) that was not skipped. `None` starts at the top of the page.
///
/// Positions are compared by their index in `hits`: `after` stands at the last hit of its
/// paragraph that starts at or before its char, or else just before that paragraph's first
/// hit. A paragraph with no hits left is placed by `walk_position` first.
fn next_misspelling(
    hits: &[SpellHit],
    after: Option<(u64, u64, usize)>,
    skipped: &HashSet<(u64, u64, usize)>,
) -> Option<SpellHit> {
    let start = match after {
        None => 0,
        Some((item, block, at)) => {
            let in_paragraph = |hit: &SpellHit| hit.item == item && hit.block == block;
            match hits
                .iter()
                .rposition(|hit| in_paragraph(hit) && hit.range.start <= at)
            {
                Some(index) => index + 1,
                None => hits.iter().position(in_paragraph).unwrap_or(0),
            }
        }
    };
    hits.get(start..)?
        .iter()
        .find(|hit| !skipped.contains(&(hit.item, hit.block, hit.range.start)))
        .cloned()
}

/// `after` as `next_misspelling` can place it among `hits`. When its paragraph has no hit
/// left (its last misspelling was just changed), the walk carries on from the last hit in
/// an earlier paragraph, or from the top when there is none.
fn walk_position(
    page: &Page,
    hits: &[SpellHit],
    after: Option<(u64, u64, usize)>,
) -> Option<(u64, u64, usize)> {
    let (item, block, _) = after?;
    if hits
        .iter()
        .any(|hit| hit.item == item && hit.block == block)
    {
        return after;
    }
    let paragraphs = spelled_paragraphs(page);
    let at = paragraphs
        .iter()
        .position(|(note, paragraph)| *note == item && paragraph.id == block)?;
    let earlier: HashSet<(u64, u64)> = paragraphs[..at]
        .iter()
        .map(|(note, paragraph)| (*note, paragraph.id))
        .collect();
    hits.iter()
        .rev()
        .find(|hit| earlier.contains(&(hit.item, hit.block)))
        .map(|hit| (hit.item, hit.block, hit.range.start))
}

/// Pages listed in the task pane: search results, or History › Recent Edits.
struct ResultsPane {
    kind: ResultsKind,
    hits: Vec<SearchHit>,
}

enum ResultsKind {
    /// A search of the notebooks, in the scope it ran in.
    Search {
        query: String,
        // Recorded with the hits; the pane does not show it (D4).
        #[allow(dead_code)]
        scope: ScopeKind,
    },
    /// History › Recent Edits: the pages edited in the range, newest first.
    Edits(EditsRange),
}

/// Where the header search looks (the desktop notebook's search scopes). "This page" is not a scope
/// here: picking it opens find on page instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ScopeKind {
    Section,
    Group,
    Notebook,
    /// Every notebook: the default until another scope is set as the default.
    #[default]
    All,
}

impl ScopeKind {
    const ALL: [ScopeKind; 4] = [
        ScopeKind::Section,
        ScopeKind::Group,
        ScopeKind::Notebook,
        ScopeKind::All,
    ];

    /// The code kept in the `search_scope` setting.
    fn code(self) -> &'static str {
        match self {
            ScopeKind::Section => "section",
            ScopeKind::Group => "group",
            ScopeKind::Notebook => "notebook",
            ScopeKind::All => "all",
        }
    }

    /// Reads a stored code; anything unknown searches every notebook.
    fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|scope| scope.code() == code)
            .unwrap_or(ScopeKind::All)
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            ScopeKind::Section => t!("app-scope-section"),
            ScopeKind::Group => t!("app-scope-group"),
            ScopeKind::Notebook => t!("app-scope-notebook"),
            ScopeKind::All => t!("app-scope-all"),
        }
    }
}

impl TagsPane {
    fn db_scope(&self) -> TagScope<'_> {
        match (&self.section_id, &self.notebook_id) {
            (Some(id), _) => TagScope::Section(id),
            (None, Some(id)) => TagScope::Notebook(id),
            (None, None) => TagScope::All,
        }
    }

    fn contains(&self, page: &Page) -> bool {
        !page.trashed
            && self
                .section_id
                .as_ref()
                .is_none_or(|id| *id == page.section_id)
            && self
                .notebook_id
                .as_ref()
                .is_none_or(|id| *id == page.notebook_id)
    }

    /// Replaces one page's lines with what is in memory now (it may not be saved yet).
    fn merge_page(&mut self, page: &Page) {
        self.lines.retain(|line| line.page_id != page.id);
        if self.contains(page) {
            self.lines.extend(page.tagged_lines());
        }
        self.sort();
    }

    /// Sorts the lines into their groups (`sort_tagged`) and numbers the groups.
    fn sort(&mut self) {
        sort_tagged(&mut self.lines, self.group_by);
        self.groups = group_ids(&self.lines, self.group_by);
    }
}

/// Tags Summary › Group tags by, in the desktop notebook's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagsGroupBy {
    TagName,
    Section,
    Title,
    Date,
    NoteText,
}

impl TagsGroupBy {
    const ALL: [TagsGroupBy; 5] = [
        TagsGroupBy::TagName,
        TagsGroupBy::Section,
        TagsGroupBy::Title,
        TagsGroupBy::Date,
        TagsGroupBy::NoteText,
    ];

    fn label(self) -> Cow<'static, str> {
        match self {
            TagsGroupBy::TagName => t!("app-tags-group-tag-name"),
            TagsGroupBy::Section => t!("app-tags-group-section"),
            TagsGroupBy::Title => t!("app-tags-group-title"),
            TagsGroupBy::Date => t!("app-tags-group-date"),
            TagsGroupBy::NoteText => t!("app-tags-group-note-text"),
        }
    }
}

/// What the Tags Summary sorts its lines by first, and where a new heading starts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    /// The tag's name as shown, folded so "É" sorts with "e", as the desktop notebook lists them
    /// alphabetically. Its place in the desktop notebook's order and a custom tag's own name then
    /// keep apart tags whose names fold alike.
    Tag(String, usize, String),
    /// A section name, a page title or a note's text, lowercased.
    Text(String),
    /// The page's last change, by local day: newest first.
    Day(std::cmp::Reverse<Option<chrono::NaiveDate>>),
}

fn group_key(line: &TaggedLine, by: TagsGroupBy) -> GroupKey {
    match by {
        TagsGroupBy::TagName => GroupKey::Tag(
            crate::table::fold(&line.tag_label()),
            line.mark.rank(),
            line.custom
                .as_ref()
                .map(|tag| tag.name.clone())
                .unwrap_or_default(),
        ),
        TagsGroupBy::Section => GroupKey::Text(line.section_name.to_lowercase()),
        TagsGroupBy::Title => GroupKey::Text(line.page_title.to_lowercase()),
        TagsGroupBy::Date => GroupKey::Day(std::cmp::Reverse(
            local_time(line.updated_at).map(|day| day.date_naive()),
        )),
        TagsGroupBy::NoteText => GroupKey::Text(line.text.to_lowercase()),
    }
}

/// A time kept in milliseconds, in the local time zone.
fn local_time(ms: i64) -> Option<chrono::DateTime<chrono::Local>> {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single()
}

/// The heading over a group of the Tags Summary (and of the summary page), read from its
/// first line: the tag's name, the section, the page's title or the day. Note text has
/// none: its lines are one flat list.
fn tags_heading_text(line: &TaggedLine, by: TagsGroupBy) -> Option<String> {
    match by {
        TagsGroupBy::TagName => Some(line.tag_label().into_owned()),
        TagsGroupBy::Section => Some(line.section_name.clone()),
        TagsGroupBy::Title => Some(display_title(&line.page_title).into_owned()),
        TagsGroupBy::Date => local_time(line.updated_at).map(util::long_date),
        TagsGroupBy::NoteText => None,
    }
}

/// Tags Summary › Create Summary Page: one note under the title, holding a Heading 2 per
/// group (none for Note text) and each line as a paragraph tagged as the line is. Its
/// page's title follows in brackets, linking back to the page.
fn tags_summary_canvas(
    lines: &[TaggedLine],
    texts: &[Option<StyledText>],
    by: TagsGroupBy,
) -> CanvasDoc {
    let mut blocks = Vec::new();
    let groups = group_ids(lines, by);
    for (index, line) in lines.iter().enumerate() {
        if index == 0 || groups[index - 1] != groups[index] {
            if let Some(heading) = tags_heading_text(line, by) {
                blocks.push(Block {
                    text: StyledText::new(heading),
                    ..Block::new(BlockKind::heading(2))
                });
            }
        }
        let text = texts.get(index).cloned().flatten();
        blocks.push(summary_paragraph(line, text));
    }
    let doc = Document { blocks };
    // Its tags hang left of its text, which starts under the title, as a template's do.
    let lead = editor::tag_lead(&doc);
    let mut summary = CanvasDoc::default();
    summary.add(CanvasItem::new(
        CanvasKind::Text { doc },
        PAGE_LEFT - canvas::NOTE_PAD_X - lead,
        PAGE_CONTENT_TOP,
        templates::NOTE_WIDTH + lead,
        40.0,
    ));
    summary
}

/// A summary page's paragraph: "<text> (<page title>)", tagged as `line` is, ticked when
/// it is, the bracketed title a link to the line's page. `text` is the paragraph's whole
/// text with its formatting, as the desktop notebook copies it; without it, the pane's shortened line.
fn summary_paragraph(line: &TaggedLine, text: Option<StyledText>) -> Block {
    let mut text = text.unwrap_or_else(|| StyledText::new(line.text.as_str()));
    let mut title = StyledText::new(format!(" ({})", display_title(&line.page_title)));
    let end = title.char_count();
    title.set_link(1..end, Some(std::sync::Arc::from(page_link(&line.page_id))));
    text.append(&title);
    let mut block = match line.mark {
        TagMark::Todo => Block::todo("", line.checked),
        TagMark::Line(tag) => {
            let mut block = Block::paragraph("");
            block.tags = vec![tag];
            if line.checked {
                block.checked_tags = vec![tag];
            }
            block
        }
        TagMark::Custom => {
            let mut block = Block::paragraph("");
            block.custom_tags = line.custom.iter().cloned().collect();
            block
        }
    };
    block.text = text;
    block
}

/// Numbers the groups of sorted lines (`sort_tagged`): a line's number is its group's
/// place, so a heading goes wherever the number changes.
fn group_ids(lines: &[TaggedLine], by: TagsGroupBy) -> Vec<usize> {
    let mut ids = Vec::with_capacity(lines.len());
    let mut last: Option<GroupKey> = None;
    let mut id = 0;
    for line in lines {
        let key = group_key(line, by);
        if last.as_ref().is_some_and(|last| *last != key) {
            id += 1;
        }
        last = Some(key);
        ids.push(id);
    }
    ids
}

/// Sorts the Tags Summary's lines into their groups; within one, by page title, then
/// page and reading order.
fn sort_tagged(lines: &mut [TaggedLine], by: TagsGroupBy) {
    lines.sort_by_cached_key(|line| {
        (
            group_key(line, by),
            line.page_title.to_lowercase(),
            line.page_id.clone(),
            line.order,
        )
    });
}

#[derive(Default)]
struct Toast {
    message: String,
    expires_at: f64,
    error: bool,
}

pub struct FolioApp {
    library: Library,
    /// The desktop notebook import reading on its own thread, if one is running.
    import_job: Option<ImportJob>,
    workspace: Workspace,
    view: View,
    selected_section: Option<String>,
    selected_notebook: Option<String>,
    pages: Vec<PageSummary>,
    /// The Recycle Bin's sections, loaded with its pages, and the one last clicked.
    trashed_sections: Vec<TrashedSection>,
    trashed_section_picked: Option<String>,
    list_dirty: bool,
    page: Option<Page>,
    editor: EditorState,
    canvas: CanvasState,
    reader: ReaderState,
    textures: TextureCache,
    dirty: bool,
    save_state: SaveState,
    last_edit_at: f64,
    last_saved_at: Option<i64>,
    search: SearchState,
    dialog: Option<Dialog>,
    dialog_focus: bool,
    /// File › Backstage, when open: it covers the window below the header strip.
    backstage: Option<Backstage>,
    /// The Options dialog's page, kept while the dialog is closed.
    options_page: OptionsPage,
    /// The settings as they were when Options opened, for its Cancel; `None` while it is
    /// closed.
    options_before: Option<OptionsSnapshot>,
    /// Insert › Symbol's recent row: the symbols inserted lately, newest first.
    recent_symbols: Vec<char>,
    last_opened_url: Option<String>,
    /// Text a command asked to put on the clipboard (Copy Link to Section), copied at
    /// the end of the frame: the library's actions run without the egui context.
    pending_copy: Option<String>,
    /// The page a page-list Cut or Copy put aside for Paste.
    page_clip: Option<PageClip>,
    toast: Toast,
    appearance: Appearance,
    /// The interface language chosen in Settings; `None` follows the system.
    ui_language: Option<Lang>,
    /// Full Page View (F11): only the page, the header strip and the ribbon's tabs show.
    full_page: bool,
    /// The ribbon's tab, layout and display options.
    ribbon: ribbon::RibbonState,
    /// The navigation pane's and page list's widths last frame (0 when hidden), for the
    /// task pane's fit.
    // First reader: the task pane's fit (P2.15); delete this allow then.
    #[allow(dead_code)]
    nav_width: f32,
    #[allow(dead_code)]
    page_list_width: f32,
    /// The page list's order and its direction (newest or Z first when set).
    page_sort: PageSort,
    page_sort_desc: bool,
    show_library: bool,
    show_pages: bool,
    /// The page list scrolls the open page's row into view on its next frame (a new page
    /// lands at the end of a section, often below the fold).
    scroll_list_to_open: bool,
    /// The navigation pane scrolls the selected section's row into view on its next frame:
    /// a new or imported notebook lands at the end of the pane, often below the fold.
    scroll_nav_to_selected: bool,
    collapsed_notebooks: HashSet<String>,
    collapsed_groups: HashSet<String>,
    screenshot_path: Option<std::path::PathBuf>,
    screenshot_wait_ms: u64,
    screenshot_started: Option<std::time::Instant>,
    frames: u32,
    /// After a failed save, autosave waits until this time before retrying.
    retry_save_at: f64,
    /// Set when a close was cancelled because the last edits could not be saved.
    close_anyway: bool,
    /// Database and attachment size, refreshed every few seconds.
    storage: Option<(f64, crate::model::StorageInfo)>,
    /// The Find Tags pane, when open.
    tags_pane: Option<TagsPane>,
    /// The task pane's other contents (versions, spelling, a list of pages), when open.
    /// At most one of this and `tags_pane` is open.
    task_pane: Option<TaskPane>,
    /// The side panels either task pane folded away, restored when it closes.
    pane_fold: PaneFold,
    /// The keyboard was last in a note's text: no other field has taken it since. A click
    /// on the ribbon that runs nothing gives it back there (`RibbonState::idle_click`).
    keyboard_in_note: bool,
    /// View › Rule Lines › Always Create Pages with Rule Lines: the lines new pages start
    /// with, or `None` when it is off. Read once here, so the ribbon never asks the
    /// database while it draws.
    new_page_rule: Option<RuleLines>,
    /// The lines Ctrl+Shift+R last took off a page (its id), which it puts back.
    last_rule: Option<(String, RuleLines)>,
    /// View › Tabs Layout: sections in the navigation pane, or as tabs over the page.
    tabs_layout: TabsLayout,
    /// The header strip drew the search box this frame. Otherwise the section tabs draw
    /// it (`search_in_tab_row`), so a layout changed mid-frame never draws it twice.
    search_in_header: bool,
    /// Options › Display, in Horizontal Tabs: the page list on the left of the page,
    /// and the notebooks pinned as a pane on the left.
    page_list_left: bool,
    notebooks_pinned: bool,
    /// Horizontal Tabs: the section group whose sections and groups the tabs show, `None`
    /// for the notebook's top level. Opening a section moves the tabs to its level.
    tab_group: Option<String>,
    /// The section `tab_group` was last set for. When the open section changes by another
    /// way than `select_section` (a search hit, a link, start-up), the tabs follow it.
    tab_group_for: Option<String>,
    /// View › Switch Background: the page drawn in the other theme's colours.
    switch_background: bool,
    /// The egui style of a switched page, built once for the theme it is drawn in
    /// (`true`: dark), not every frame.
    switched_style: Option<(bool, std::sync::Arc<egui::Style>)>,
    /// Options › Display: the page's vertical scroll bar on its left edge.
    scrollbar_left: bool,
    /// Options › Advanced › Printouts: inserted printout pages go in the background. The
    /// canvas reads the setting itself when it places a printout.
    printout_background: bool,
    /// Back and Forward (Alt+← / Alt+→).
    visited: Visited,
    /// The page list's first and last rows wholly in view (indices into `pages`), as last
    /// drawn, for Alt+PgUp / Alt+PgDn.
    pages_in_view: Option<(usize, usize)>,
}

impl FolioApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        theme::install(&cc.egui_ctx);
        // Give up text focus when the pointer is pressed elsewhere, not when it is
        // released. Focus requested on release (a new note, or the text after a ribbon
        // button) would otherwise be dropped in the same frame.
        cc.egui_ctx.options_mut(|options| {
            options.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses;
        });
        // Folio has its own canvas zoom; egui's Ctrl+= / Ctrl+- / Ctrl+0 would zoom the whole
        // app whenever no note has focus and clash with subscript and strikethrough.
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);
        // The language comes first: a first run fills the sample library in it.
        i18n::set_lang(i18n::startup_lang(None));
        let library = Library::open_default()?;
        let ui_language = library
            .setting("ui_language")
            .ok()
            .flatten()
            .and_then(|code| Lang::from_code(&code));
        i18n::set_lang(i18n::startup_lang(ui_language.map(Lang::code)));
        let workspace = library.workspace()?;
        let appearance =
            Appearance::from_code(library.setting("theme")?.as_deref().unwrap_or("system"));
        cc.egui_ctx.set_theme(appearance.preference());
        let ribbon = ribbon::RibbonState::load(&library);
        let page_sort = PageSort::from_code(
            library
                .setting("page_sort")
                .ok()
                .flatten()
                .as_deref()
                .unwrap_or_default(),
        );
        let page_sort_desc = library
            .setting("page_sort_desc")
            .ok()
            .flatten()
            .is_some_and(|value| value == "1");
        let new_page_rule = library.new_page_rule().ok().flatten();
        let setting = |key: &str| library.setting(key).ok().flatten();
        let flag = |key: &str| setting(key).is_some_and(|value| value == "true");
        let tabs_layout = setting("tabs_layout")
            .map_or(TabsLayout::Vertical, |code| TabsLayout::from_code(&code));
        let page_list_left = flag("page_list_left");
        let notebooks_pinned = flag("notebook_pane_pinned");
        let switch_background = flag("switch_background");
        let scrollbar_left = flag("scrollbar_left");
        let printout_background = flag("printout_background");
        // Inline maths is on unless turned off (the desktop notebook's default).
        let auto_calc = !flag("no_auto_calc");
        // Options › General › Show Mini Toolbar on selection: on unless turned off.
        let mini_on_selection =
            setting("mini_toolbar_on_selection").is_none_or(|value| value == "true");
        let default_scope =
            setting("search_scope").map_or(ScopeKind::All, |code| ScopeKind::from_code(&code));
        let recent_symbols = setting("recent_symbols")
            .map(|symbols| symbols.chars().take(RECENT_SYMBOLS).collect())
            .unwrap_or_default();
        let mut app = Self {
            library,
            workspace,
            view: View::Section,
            selected_section: None,
            selected_notebook: None,
            pages: Vec::new(),
            trashed_sections: Vec::new(),
            trashed_section_picked: None,
            list_dirty: true,
            page: None,
            editor: EditorState::default(),
            canvas: CanvasState::default(),
            reader: ReaderState::default(),
            textures: TextureCache::default(),
            dirty: false,
            save_state: SaveState::Saved,
            last_edit_at: 0.0,
            last_saved_at: Some(now_ms()),
            search: SearchState {
                default_scope,
                ..SearchState::default()
            },
            dialog: None,
            dialog_focus: false,
            backstage: None,
            options_page: OptionsPage::General,
            options_before: None,
            recent_symbols,
            last_opened_url: None,
            pending_copy: None,
            page_clip: None,
            toast: Toast::default(),
            appearance,
            ui_language,
            full_page: false,
            ribbon,
            nav_width: 0.0,
            page_list_width: 0.0,
            page_sort,
            page_sort_desc,
            show_library: true,
            show_pages: true,
            scroll_list_to_open: true,
            scroll_nav_to_selected: false,
            collapsed_notebooks: HashSet::new(),
            collapsed_groups: HashSet::new(),
            screenshot_path: None,
            screenshot_wait_ms: 250,
            screenshot_started: None,
            frames: 0,
            retry_save_at: 0.0,
            close_anyway: false,
            storage: None,
            tags_pane: None,
            task_pane: None,
            pane_fold: PaneFold::default(),
            keyboard_in_note: false,
            new_page_rule,
            last_rule: None,
            tabs_layout,
            search_in_header: true,
            page_list_left,
            notebooks_pinned,
            tab_group: None,
            tab_group_for: None,
            switch_background,
            switched_style: None,
            scrollbar_left,
            printout_background,
            visited: Visited::default(),
            pages_in_view: None,
            import_job: None,
        };
        // Draw's pen gallery, as the library last saved it.
        app.canvas.pens = canvas::load_pens(&app.library);
        app.canvas.recent_colors = canvas::load_recent_colors(&app.library);
        // Home's Tags gallery, in the order Customize Tags last saved, with the custom tags.
        app.editor.tags = TagGallery::load(&app.library);
        let language = app.library.setting("spell_language").ok().flatten();
        let personal = app.library.setting("spell_personal").ok().flatten();
        app.editor.spell.configure(
            language.as_deref().unwrap_or("auto"),
            personal.as_deref().unwrap_or_default(),
        );
        app.editor.mini_on_selection = mini_on_selection;
        app.editor.auto_calc = auto_calc;
        app.restore(&cc.egui_ctx);
        Ok(app)
    }

    pub fn set_screenshot_path(&mut self, path: Option<std::path::PathBuf>, wait_ms: u64) {
        self.screenshot_path = path;
        self.screenshot_wait_ms = wait_ms;
    }

    fn restore(&mut self, ctx: &egui::Context) {
        let last = self.library.setting("last_page_id").ok().flatten();
        if let Some(id) = last {
            if let Ok(Some(page)) = self.library.get_page(&id) {
                let trashed = page.trashed;
                self.load_page(page, true);
                if trashed {
                    // Show it where it is, so it doesn't look like an ordinary page.
                    self.view = View::Trash;
                }
                self.refresh_page_list();
                return;
            }
        }
        let section = self.workspace.quick_section_id.clone().or_else(|| {
            self.workspace
                .notebooks
                .iter()
                .flat_map(|notebook| notebook.sections.iter())
                .map(|section| section.id.clone())
                .next()
        });
        if let Some(section_id) = section {
            self.view = View::Section;
            self.selected_section = Some(section_id.clone());
            self.sync_notebook();
            self.refresh_page_list();
            if let Some(first) = self.pages.first().cloned() {
                if let Ok(Some(page)) = self.library.get_page(&first.id) {
                    self.load_page(page, false);
                }
            }
        }
        let _ = ctx;
    }

    fn load_page(&mut self, page: Page, switch_view: bool) {
        if switch_view {
            self.view = View::Section;
            self.selected_section = Some(page.section_id.clone());
            self.selected_notebook = Some(page.notebook_id.clone());
        }
        let id = page.id.clone();
        self.editor.reset();
        self.canvas.reset(&id);
        self.keyboard_in_note = false;
        self.reader.open = false;
        let mut page = page;
        if let Some(notice) = page.notice.take() {
            self.error(notice);
        }
        // A visit, for Back: the page left, unless Back, Forward or a search preview is
        // opening this one. Previews still showing end here: the page shown was kept.
        if !self.visited.walking {
            self.settle_preview();
            if let Some(left) = self
                .page
                .as_ref()
                .map(|open| open.id.clone())
                .filter(|left| *left != id)
            {
                self.visit(left);
            }
        }
        self.page = Some(page);
        self.dirty = false;
        self.save_state = SaveState::Saved;
        self.last_saved_at = Some(now_ms());
        self.scroll_list_to_open = true;
        let _ = self.library.set_setting("last_page_id", &id);
    }

    fn sync_notebook(&mut self) {
        let section = self.selected_section.as_deref();
        self.selected_notebook = self.workspace.notebooks.iter().find_map(|notebook| {
            notebook
                .sections
                .iter()
                .any(|candidate| Some(candidate.id.as_str()) == section)
                .then(|| notebook.id.clone())
        });
    }

    fn refresh_workspace(&mut self) {
        if let Ok(workspace) = self.library.workspace() {
            self.workspace = workspace;
            self.sync_notebook();
        }
    }

    fn refresh_page_list(&mut self) {
        // The bin lists its sections above its pages.
        self.trashed_sections = match self.view {
            View::Trash => self.library.trashed_sections().unwrap_or_else(|error| {
                self.error(t!("error-load-pages", error = error.to_string()));
                Vec::new()
            }),
            View::Section => Vec::new(),
        };
        let filter = match &self.view {
            View::Section => self.selected_section.as_deref().map(PageFilter::Section),
            View::Trash => Some(PageFilter::Trash),
        };
        let Some(filter) = filter else {
            self.pages.clear();
            return;
        };
        match self.library.page_list(filter, "", 500) {
            Ok(pages) => {
                self.pages = pages;
                // The Recycle Bin keeps its own order (newest first).
                if self.view == View::Section {
                    sort_pages(&mut self.pages, self.page_sort, self.page_sort_desc);
                }
            }
            Err(error) => {
                self.pages.clear();
                self.error(t!("error-load-pages", error = error.to_string()));
            }
        }
        self.list_dirty = false;
    }

    /// The sort button's choice: picking the current order turns it around; another
    /// order starts A to Z, or newest first for the dates. It is kept for the next start.
    fn set_page_sort(&mut self, sort: PageSort) {
        if sort == self.page_sort {
            // "None" is the section's own order, which has no direction.
            self.page_sort_desc = sort != PageSort::None && !self.page_sort_desc;
        } else {
            self.page_sort = sort;
            self.page_sort_desc = matches!(sort, PageSort::Created | PageSort::Modified);
        }
        let desc = if self.page_sort_desc { "1" } else { "0" };
        for (key, value) in [
            ("page_sort", self.page_sort.code()),
            ("page_sort_desc", desc),
        ] {
            if let Err(error) = self.library.set_setting(key, value) {
                self.error(t!("error-save-page-sort", error = error.to_string()));
                break;
            }
        }
        self.list_dirty = true;
    }

    fn select_section(&mut self, section_id: &str, open_first: bool) {
        if self
            .page
            .as_ref()
            .is_some_and(|page| page.section_id == section_id)
            && !open_first
        {
            self.view = View::Section;
            self.selected_section = Some(section_id.to_string());
            self.sync_notebook();
            self.show_tabs_of(section_id);
            self.refresh_page_list();
            return;
        }
        if !self.flush_save() {
            return;
        }
        let keep_current = self
            .page
            .as_ref()
            .is_some_and(|page| page.section_id == section_id);
        self.view = View::Section;
        self.selected_section = Some(section_id.to_string());
        self.sync_notebook();
        self.show_tabs_of(section_id);
        self.refresh_page_list();
        if keep_current && !open_first {
            return;
        }
        if let Some(first) = self.pages.first().cloned() {
            if let Ok(Some(page)) = self.library.get_page(&first.id) {
                self.load_page(page, false);
            }
        } else {
            self.page = None;
            self.editor.reset();
            self.dirty = false;
        }
    }

    /// Horizontal Tabs: the tabs show the level `section_id` sits at (its section group,
    /// or the notebook's top level).
    fn show_tabs_of(&mut self, section_id: &str) {
        self.tab_group = self
            .workspace
            .notebooks
            .iter()
            .flat_map(|notebook| notebook.sections.iter())
            .find(|section| section.id == section_id)
            .and_then(|section| section.group_id.clone());
        self.tab_group_for = Some(section_id.to_string());
    }

    /// A section moved (into another group or notebook): when it is the open one, the
    /// Horizontal Tabs follow it to its new level, where its tab is.
    fn section_moved(&mut self, section_id: &str) {
        if self.selected_section.as_deref() == Some(section_id) {
            self.show_tabs_of(section_id);
        }
    }

    /// Options › Display › Show notebook list on the left, and the pins of Horizontal
    /// Tabs: the notebooks as a pane on the left, or only in the notebook picker.
    fn set_notebooks_pinned(&mut self, on: bool) {
        self.notebooks_pinned = on;
        // Pinning shows the pane, even one hidden by hand before.
        if on {
            self.show_library = true;
        }
        self.save_flag("notebook_pane_pinned", on);
        // The navigation pane comes or goes: fit the side panels again.
        self.pane_fold.fitted_width = 0.0;
    }

    /// Whether the navigation pane can show at all: always in Vertical Tabs, and only
    /// pinned in Horizontal Tabs, where the notebook picker stands in for it.
    fn nav_pane_can_show(&self) -> bool {
        self.tabs_layout == TabsLayout::Vertical || self.notebooks_pinned
    }

    /// Horizontal Tabs: the open section's colour, which tints the page list and the band
    /// over the page as it fills the selected tab (D11). `None` in Vertical Tabs, in Full
    /// Page View and in the Recycle Bin.
    fn section_tint(&self, t: Tokens) -> Option<Color32> {
        if self.tabs_layout != TabsLayout::Horizontal
            || self.full_page
            || self.view != View::Section
        {
            return None;
        }
        let id = self.selected_section.as_deref()?;
        self.workspace
            .notebooks
            .iter()
            .flat_map(|notebook| notebook.sections.iter())
            .find(|section| section.id == id)
            .map(|section| theme::section_color(&section.color, t))
    }

    fn select_view(&mut self, view: View) {
        if !self.flush_save() {
            return;
        }
        self.view = view;
        self.refresh_page_list();
        let current_still_visible = self
            .page
            .as_ref()
            .is_some_and(|page| self.pages.iter().any(|candidate| candidate.id == page.id));
        if !current_still_visible {
            if let Some(first) = self.pages.first().cloned() {
                if let Ok(Some(page)) = self.library.get_page(&first.id) {
                    self.load_page(page, false);
                }
            } else {
                self.page = None;
                self.editor.reset();
                self.dirty = false;
            }
        }
    }

    fn open_page(&mut self, id: &str, switch_view: bool) {
        if self.page.as_ref().is_some_and(|page| page.id == id) {
            return;
        }
        if !self.flush_save() {
            return;
        }
        match self.library.get_page(id) {
            Ok(Some(page)) => {
                self.load_page(page, switch_view);
                self.refresh_page_list();
            }
            Ok(None) => self.error(t!("error-page-gone")),
            Err(error) => self.error(t!("error-open-page", error = error.to_string())),
        }
    }

    /// Alt+← (`back`) / Alt+→: the page visited before, or the one Back left, skipping
    /// pages deleted since (to the Recycle Bin or for good). The page left goes on the
    /// other list.
    fn walk_visited(&mut self, back: bool) {
        self.settle_preview();
        let Some(current) = self.page.as_ref().map(|page| page.id.clone()) else {
            return;
        };
        loop {
            let next = if back {
                self.visited.back.pop()
            } else {
                self.visited.forward.pop()
            };
            let Some(id) = next else {
                return;
            };
            if id == current
                || !matches!(self.library.get_page(&id), Ok(Some(page)) if !page.trashed)
            {
                continue;
            }
            self.visited.walking = true;
            self.open_page(&id, true);
            self.visited.walking = false;
            if self.page.as_ref().is_some_and(|page| page.id == id) {
                if back {
                    self.visited.forward.push(current);
                } else {
                    self.visited.back.push(current);
                }
            }
            return;
        }
    }

    /// A visit, for Back: `left` is the page left. A new visit forgets Forward's pages.
    fn visit(&mut self, left: String) {
        self.visited.back.push(left);
        self.visited.forward.clear();
        if self.visited.back.len() > MAX_VISITED {
            self.visited.back.remove(0);
        }
    }

    /// The search's previews are over: the page showing is kept, as one visit from the
    /// page open before them.
    fn settle_preview(&mut self) {
        let Some(before) = self.visited.previewed.take() else {
            return;
        };
        let open = self.page.as_ref().map(|page| page.id.clone());
        if let Some(before) = before.filter(|before| open.is_some_and(|open| open != *before)) {
            self.visit(before);
        }
    }

    /// ↓ / ↑ in the search's results: `id`'s page shows behind the list. It is no visit
    /// until the search ends on it (`settle_preview`).
    fn preview_page(&mut self, id: &str) {
        if self.visited.previewed.is_none() {
            self.visited.previewed = Some(self.page.as_ref().map(|page| page.id.clone()));
        }
        self.close_backstage();
        self.visited.walking = true;
        self.open_page(id, true);
        self.visited.walking = false;
    }

    /// The ids of the page list's rows that show (not folded away under a page), in order.
    fn shown_page_ids(&self) -> Vec<String> {
        let outline_view = self.view == View::Section && self.page_sort == PageSort::None;
        let hidden = if outline_view {
            outline::hidden(
                &self
                    .pages
                    .iter()
                    .map(|page| (page.level, page.collapsed))
                    .collect::<Vec<_>>(),
            )
        } else {
            vec![false; self.pages.len()]
        };
        self.pages
            .iter()
            .zip(hidden)
            .filter(|(_, hidden)| !hidden)
            .map(|(page, _)| page.id.clone())
            .collect()
    }

    /// Opens the shown row `pick` chooses from their count and the open page's place among
    /// them (`None` when it is not listed): the desktop notebook's Ctrl+PgUp / Ctrl+PgDn and Alt+Home /
    /// Alt+End.
    fn open_listed(&mut self, pick: impl FnOnce(usize, Option<usize>) -> Option<usize>) {
        let ids = self.shown_page_ids();
        let at = self
            .page
            .as_ref()
            .and_then(|page| ids.iter().position(|id| *id == page.id));
        if let Some(id) = pick(ids.len(), at)
            .and_then(|index| ids.get(index))
            .cloned()
        {
            self.open_page(&id, false);
        }
    }

    /// Alt+PgUp / Alt+PgDn (`last`): the first or last page row wholly in view in the page
    /// list as last drawn; the list's first or last row when it was not drawn.
    fn open_in_view(&mut self, last: bool) {
        let index = match self.pages_in_view {
            Some((first, end)) => Some(if last { end } else { first }),
            None if self.pages.is_empty() => None,
            None => Some(if last { self.pages.len() - 1 } else { 0 }),
        };
        if let Some(id) = index
            .and_then(|index| self.pages.get(index))
            .map(|page| page.id.clone())
        {
            self.open_page(&id, false);
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab: the next or previous section of the open notebook, in the
    /// navigation pane's order, round again at the ends.
    fn step_section(&mut self, forward: bool) {
        let Some(notebook) = self.selected_notebook.as_ref().and_then(|id| {
            self.workspace
                .notebooks
                .iter()
                .find(|notebook| notebook.id == *id)
        }) else {
            return;
        };
        let ids: Vec<String> = sections_in_order(notebook, None)
            .into_iter()
            .map(|section| section.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        let at = self
            .selected_section
            .as_ref()
            .and_then(|id| ids.iter().position(|candidate| candidate == id));
        let next = match at {
            Some(at) if forward => (at + 1) % ids.len(),
            Some(at) => (at + ids.len() - 1) % ids.len(),
            None => 0,
        };
        let id = ids[next].clone();
        self.select_section(&id, false);
    }

    fn new_page(&mut self) {
        // A section in the Recycle Bin takes no new page (the open page may be one of
        // its pages, deleted on its own): the page goes to a section that is shown.
        let shown = |id: &String| self.find_section(id).is_some();
        let section = self
            .page
            .as_ref()
            .map(|page| page.section_id.clone())
            .filter(shown)
            .or_else(|| self.selected_section.clone().filter(shown))
            .or_else(|| {
                self.workspace
                    .notebooks
                    .first()
                    .and_then(|notebook| notebook.sections.first())
                    .map(|section| section.id.clone())
            });
        let Some(section_id) = section else {
            self.error(t!("error-needs-section"));
            return;
        };
        if !self.flush_save() {
            return;
        }
        match self.library.create_canvas_page(&section_id, "") {
            Ok(page) => {
                self.view = View::Section;
                self.selected_section = Some(section_id);
                self.sync_notebook();
                self.refresh_workspace();
                self.refresh_page_list();
                self.load_page(page, false);
                self.canvas.focus_title = true;
                self.canvas.tool = canvas::CanvasTool::Select;
            }
            Err(error) => self.error(t!("error-create-page", error = error.to_string())),
        }
    }

    /// Insert › Page Templates, as the desktop notebook's Templates pane: a new page at the end of the
    /// section, laid out from the template, with the caret in its empty title.
    fn new_page_from(&mut self, template: PageTemplate, now: f64) {
        let before = self.page.as_ref().map(|page| page.id.clone());
        self.new_page();
        // When no page could be added, the open one is left as it was.
        let Some(page) = self
            .page
            .as_mut()
            .filter(|page| Some(&page.id) != before.as_ref())
        else {
            return;
        };
        // `new_page` loaded the page and reset the canvas state for it, so nothing refers
        // to its old (empty) contents.
        page.canvas = template.canvas();
        if template.ruled() {
            page.rule = RuleLines::Lines(RuleSize::College);
        }
        self.mark_dirty(now);
    }

    /// the desktop notebook's Ctrl+Alt+N (a page below this one) and Ctrl+Alt+Shift+N (a subpage).
    fn new_page_below(&mut self, after: &str, subpage: bool) {
        if !self.flush_save() {
            return;
        }
        match self.library.insert_page_after(after, subpage) {
            Ok(page) => {
                self.view = View::Section;
                self.selected_section = Some(page.section_id.clone());
                self.sync_notebook();
                self.refresh_workspace();
                self.refresh_page_list();
                self.load_page(page, false);
                self.canvas.focus_title = true;
                self.canvas.tool = canvas::CanvasTool::Select;
            }
            Err(error) => self.error(t!("error-create-page", error = error.to_string())),
        }
    }

    fn save_now(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(page) = self.page.as_ref() else {
            self.dirty = false;
            return;
        };
        self.save_state = SaveState::Saving;
        let result = self.library.save_page(page);
        match result {
            Ok(()) => {
                let stamp = now_ms();
                if let Some(page) = self.page.as_mut() {
                    page.updated_at = stamp;
                    page.word_count = page.word_count();
                }
                self.dirty = false;
                self.save_state = SaveState::Saved;
                self.last_saved_at = Some(stamp);
                self.update_page_summary();
                self.refresh_tags_for_page();
            }
            Err(error) => {
                self.save_state = SaveState::Error;
                self.error(t!("error-save", error = error.to_string()));
            }
        }
    }

    /// Saves pending edits. Returns `false` if they could not be written, in which case
    /// callers that would replace the open page must stop, or the edits would be lost.
    #[must_use]
    fn flush_save(&mut self) -> bool {
        // An equation being typed is finished first, as clicking away finishes it: one
        // left empty is not saved, and the page about to open starts with no session.
        if let Some(page) = self.page.as_mut() {
            if canvas::finish_editing(page, &mut self.canvas, &mut self.editor) {
                self.dirty = true;
            }
        }
        if self.dirty {
            self.save_now();
        }
        !self.dirty
    }

    fn mark_dirty(&mut self, now: f64) {
        self.dirty = true;
        self.last_edit_at = now;
        self.save_state = SaveState::Dirty;
        if let Some(page) = self.page.as_mut() {
            page.word_count = page.word_count();
        }
        self.sync_open_title();
    }

    fn update_page_summary(&mut self) {
        let Some(page) = self.page.as_ref() else {
            return;
        };
        if let Some(summary) = self.pages.iter_mut().find(|summary| summary.id == page.id) {
            summary.title = display_title(&page.title).into_owned();
            summary.excerpt = util::truncate(
                &page
                    .plain_text()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                150,
            );
            summary.word_count = page.word_count();
            summary.updated_at = page.updated_at;
            summary.pinned = page.pinned;
            summary.tags = page.tags.clone();
            // The page's date can be changed from its header: a list sorted by date
            // created moves the page to its new place.
            let moved = summary.created_at != page.created_at;
            summary.created_at = page.created_at;
            if moved && self.view == View::Section && self.page_sort == PageSort::Created {
                sort_pages(&mut self.pages, self.page_sort, self.page_sort_desc);
            }
        }
    }

    /// The page list shows the open page's title as it is typed, as the desktop notebook's does. The rest
    /// of its row (excerpt, word count, order) follows on the next save.
    fn sync_open_title(&mut self) {
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let title = display_title(&page.title);
        if let Some(summary) = self.pages.iter_mut().find(|summary| summary.id == page.id) {
            if summary.title != title {
                summary.title = title.into_owned();
            }
        }
    }

    /// The section and notebook a scope means right now (the open page's, else the selection).
    fn tag_scope_ids(&self, scope: TagsScope) -> (Option<String>, Option<String>) {
        let section = self
            .page
            .as_ref()
            .map(|page| page.section_id.clone())
            .or_else(|| self.selected_section.clone());
        let notebook = self
            .page
            .as_ref()
            .map(|page| page.notebook_id.clone())
            .or_else(|| self.selected_notebook.clone());
        match scope {
            TagsScope::Section => (section, None),
            TagsScope::Notebook => (None, notebook),
            TagsScope::All => (None, None),
        }
    }

    /// Tags Summary › Create Summary Page: a new page in the open section, titled "Tags
    /// Summary", listing `lines` as the pane shows them (`tags_summary_canvas`).
    fn create_tags_summary_page(&mut self, lines: &[TaggedLine], by: TagsGroupBy, now: f64) {
        let texts = self.tagged_texts(lines);
        let before = self.page.as_ref().map(|page| page.id.clone());
        self.new_page();
        // When no page could be added, the open one is left as it was.
        let Some(page) = self
            .page
            .as_mut()
            .filter(|page| Some(&page.id) != before.as_ref())
        else {
            return;
        };
        page.title = t!("app-tags-summary-title").into_owned();
        page.canvas = tags_summary_canvas(lines, &texts, by);
        // The list first: marking the page dirty then shows its title there before the
        // save, where the list read from the library would still say "Untitled page".
        self.refresh_page_list();
        self.mark_dirty(now);
    }

    /// Each tagged line's paragraph as its page holds it, whole and formatted (the open
    /// page as it is in memory), for the summary page; `None` where it is gone.
    fn tagged_texts(&self, lines: &[TaggedLine]) -> Vec<Option<StyledText>> {
        let mut pages: HashMap<String, Option<Page>> = HashMap::new();
        lines
            .iter()
            .map(|line| {
                let page = match self.page.as_ref().filter(|page| page.id == line.page_id) {
                    Some(page) => Some(page),
                    None => pages
                        .entry(line.page_id.clone())
                        .or_insert_with(|| self.library.get_page(&line.page_id).ok().flatten())
                        .as_ref(),
                };
                let doc = page?.canvas.item(line.item)?.doc()?;
                let block = &doc.blocks[doc.block_index(line.block)?];
                Some(block.text.clone())
            })
            .collect()
    }

    /// Opens Customize Tags on a copy of the gallery: its OK keeps the copy.
    fn open_customize_tags(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        self.dialog = Some(Dialog::CustomizeTags(CustomizeTags {
            gallery: self.editor.tags.clone(),
            selected: 0,
            editing: None,
            reveal: false,
        }));
        self.dialog_focus = true;
    }

    fn open_tags_pane(&mut self) {
        let scope = self
            .tags_pane
            .as_ref()
            .map_or(TagsScope::Section, |pane| pane.scope);
        // The open page is merged from memory below, so a failed save loses nothing here.
        let _ = self.flush_save();
        self.load_tags(scope);
    }

    fn load_tags(&mut self, scope: TagsScope) {
        let (section_id, notebook_id) = self.tag_scope_ids(scope);
        // Group tags by and Show only unchecked items stay as they were set.
        let (group_by, unchecked_only) = self
            .tags_pane
            .as_ref()
            .map_or((TagsGroupBy::TagName, false), |pane| {
                (pane.group_by, pane.unchecked_only)
            });
        let mut pane = TagsPane {
            scope,
            group_by,
            unchecked_only,
            section_id,
            notebook_id,
            lines: Vec::new(),
            groups: Vec::new(),
        };
        match self.library.tagged_lines(pane.db_scope()) {
            Ok(lines) => {
                pane.lines = lines;
                pane.sort();
                if self.tags_pane.is_none() {
                    // The task pane holds one thing at a time: the summary takes another
                    // content's place without bringing back the panels it folded away,
                    // and the next frame fits the panels again.
                    self.task_pane = None;
                    self.pane_fold.fitted_width = 0.0;
                }
                self.tags_pane = Some(pane);
                self.refresh_tags_for_page();
            }
            Err(error) => self.error(t!("error-list-line-tags", error = error.to_string())),
        }
    }

    /// Lists the pane's section or notebook again, after pages were trashed, restored,
    /// moved or deleted.
    fn reload_tags_pane(&mut self) {
        let Some(pane) = self.tags_pane.as_mut() else {
            return;
        };
        match self.library.tagged_lines(pane.db_scope()) {
            Ok(lines) => {
                pane.lines = lines;
                pane.sort();
                self.refresh_tags_for_page();
            }
            Err(error) => self.error(t!("error-list-line-tags", error = error.to_string())),
        }
    }

    /// After a save (or on load), the open page's tagged lines replace its stored ones.
    fn refresh_tags_for_page(&mut self) {
        if let (Some(pane), Some(page)) = (self.tags_pane.as_mut(), self.page.as_ref()) {
            pane.merge_page(page);
        }
    }

    fn close_tags_pane(&mut self) {
        if self.tags_pane.take().is_some() {
            self.restore_folded_panels();
        }
    }

    /// Docks `pane` in the task pane in place of what it held, the Tags pane included.
    /// The panels folded away for the old content stay folded, and the next frame fits
    /// them again.
    fn open_task_pane(&mut self, pane: TaskPane) {
        self.tags_pane = None;
        self.task_pane = Some(pane);
        self.pane_fold.fitted_width = 0.0;
    }

    /// Review › Spelling: the Spelling pane moves to the first misspelling after `after`
    /// (a note, a paragraph in it and a char; `None` is the top of the page) that was not
    /// ignored, selects it in its note and looks up its suggestions. The pane keeps
    /// working without keyboard focus. Past the last one, the walk is done.
    fn spell_next(&mut self, after: Option<(u64, u64, usize)>) {
        let Some(TaskPane::Spelling(pane)) = self.task_pane.as_mut() else {
            return;
        };
        let spell = &mut self.editor.spell;
        let current = self.page.as_ref().and_then(|page| {
            let hits = page_misspellings(page, spell);
            next_misspelling(&hits, walk_position(page, &hits, after), &pane.skipped)
        });
        pane.current = current.clone();
        pane.picked = 0;
        // A speller still loading found nothing because it could not check yet: the walk
        // is not over.
        pane.done = current.is_none() && spell.is_ready();
        let Some(hit) = current else {
            return;
        };
        self.canvas.reveal = Some(Reveal {
            item: hit.item,
            block: Some(hit.block),
            focus: false,
        });
        self.canvas.active_text = Some(hit.item);
        self.canvas.selection.set_one(hit.item);
        self.editor.focus_selection(hit.block, hit.range.clone());
        self.editor
            .spell
            .request_suggestions(&hit.word, hit.language);
    }

    /// Review › Spelling and F7: opens the Spelling pane at the open page's first
    /// misspelling. With the pane open, they move on to the next one, passing over the
    /// word shown without ignoring it.
    fn spelling(&mut self) {
        if let Some(TaskPane::Spelling(pane)) = &self.task_pane {
            let after = pane
                .current
                .as_ref()
                .map(|hit| (hit.item, hit.block, hit.range.start));
            self.spell_next(after);
            return;
        }
        let page_id = self.spelling_page_id();
        let walk = page_id.is_some();
        self.open_task_pane(TaskPane::Spelling(SpellPane {
            page_id,
            ..SpellPane::default()
        }));
        if walk {
            self.spell_next(None);
        }
    }

    /// The page the Spelling pane checks: the open page, unless it is in the Recycle Bin.
    fn spelling_page_id(&self) -> Option<String> {
        self.page
            .as_ref()
            .filter(|page| !page.trashed)
            .map(|page| page.id.clone())
    }

    /// The Spelling pane follows the open page: another page opened is checked from its
    /// top. A pane opened while the dictionary was still loading starts its walk once the
    /// dictionary is ready.
    fn follow_spelling_pane(&mut self) {
        let page_id = self.spelling_page_id();
        let ready = self.editor.spell.is_ready();
        let Some(TaskPane::Spelling(pane)) = self.task_pane.as_mut() else {
            return;
        };
        let walk = if pane.page_id != page_id {
            *pane = SpellPane {
                page_id,
                ..SpellPane::default()
            };
            pane.page_id.is_some()
        } else {
            pane.page_id.is_some() && ready && !pane.done && pane.current.is_none()
        };
        if walk {
            self.spell_next(None);
        }
    }

    /// Closes the versions, spelling or results pane and brings back the panels it
    /// folded away.
    fn close_task_pane(&mut self) {
        if self.task_pane.take().is_some() {
            self.restore_folded_panels();
        }
    }

    /// A task pane closed: the panels it folded away come back.
    fn restore_folded_panels(&mut self) {
        let fold = std::mem::take(&mut self.pane_fold);
        self.show_library |= fold.hid_library;
        self.show_pages |= fold.hid_pages;
    }

    /// The task pane takes its width from the page card. Where that would leave the page
    /// narrower than `MIN_PAGE_WIDTH`, the navigation pane and then the page list fold
    /// away while the pane is open (the desktop notebook collapses its navigation the same way), and
    /// they come back when the window is wide enough again or the pane closes.
    fn fit_task_pane(&mut self, width: f32) {
        if self.tags_pane.is_none() && self.task_pane.is_none() {
            return;
        }
        let nav_can_show = self.nav_pane_can_show();
        let fold = &mut self.pane_fold;
        // Refit only when the pane opens or the window is resized, so a panel shown
        // again by hand stays shown.
        if self.full_page || fold.fitted_width == width {
            return;
        }
        fold.fitted_width = width;
        // A shown panel counts its measured width, or its default before it was drawn. In
        // Horizontal Tabs the notebooks take no room unless pinned as a pane.
        let nav = if !nav_can_show {
            0.0
        } else if self.nav_width > 0.0 {
            self.nav_width
        } else {
            NAV_WIDTH
        };
        let list = if self.page_list_width > 0.0 {
            self.page_list_width
        } else {
            PAGE_LIST_WIDTH
        };
        let page = |library: bool, pages: bool| {
            width
                - if library { nav } else { 0.0 }
                - if pages { list } else { 0.0 }
                - TASKPANE_WIDTH
                - 16.0
        };
        if fold.hid_pages && page(self.show_library, true) >= MIN_PAGE_WIDTH {
            self.show_pages = true;
            fold.hid_pages = false;
        }
        if fold.hid_library && page(true, self.show_pages) >= MIN_PAGE_WIDTH {
            self.show_library = true;
            fold.hid_library = false;
        }
        // Folding a pane that takes no room would free none.
        if self.show_library && nav > 0.0 && page(true, self.show_pages) < MIN_PAGE_WIDTH {
            self.show_library = false;
            fold.hid_library = true;
        }
        if self.show_pages && page(self.show_library, true) < MIN_PAGE_WIDTH {
            self.show_pages = false;
            fold.hid_pages = true;
        }
    }

    /// The notebooks or page list was shown or hidden by hand: closing the task pane
    /// leaves it that way.
    fn panels_toggled_by_hand(&mut self, library: bool) {
        if self.tags_pane.is_some() || self.task_pane.is_some() {
            if library {
                self.pane_fold.hid_library = false;
            } else {
                self.pane_fold.hid_pages = false;
            }
        }
    }

    /// Full Page View on or off; a band shown over the page goes either way.
    fn set_full_page(&mut self, on: bool) {
        self.full_page = on;
        self.ribbon.overlay = false;
    }

    fn reveal_tagged_line(&mut self, line: TaggedLine) {
        self.open_page(&line.page_id, true);
        if self
            .page
            .as_ref()
            .is_none_or(|page| page.id != line.page_id)
        {
            return;
        }
        self.canvas.reveal = Some(Reveal {
            item: line.item,
            block: Some(line.block),
            focus: true,
        });
    }

    fn tick_save(&mut self, now: f64) {
        if self.dirty && now - self.last_edit_at > 0.65 && now >= self.retry_save_at {
            self.save_now();
            // Don't retry a failing save (full disk, locked file) on every frame.
            self.retry_save_at = if self.dirty { now + 5.0 } else { 0.0 };
        }
    }

    /// A recording carries on whatever is open: a problem that stopped it (the
    /// microphone was unplugged) is shown and the recording finished, and its clock
    /// keeps ticking.
    fn tick_recording(&mut self, ctx: &egui::Context, now: f64) {
        if let Some(error) = self.canvas.take_recording_error() {
            self.stop_recording(now);
            self.error(t!("error-recording-stopped", error = error.to_string()));
        }
        if self.canvas.recording_time().is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    /// Stops the recording in progress, giving its clip its length when its page is open.
    fn stop_recording(&mut self, now: f64) {
        match self
            .canvas
            .finish_recording(self.page.as_mut(), &mut self.editor, &mut self.library)
        {
            Ok(true) => self.mark_dirty(now),
            Ok(false) => {}
            Err(error) => self.error(t!("error-finish-recording", error = error.to_string())),
        }
    }

    /// Stops the recording before its page goes with `container_id` (that page, or the
    /// section, section group or notebook holding it), so its file is complete and
    /// nothing is left writing to it.
    fn stop_recording_within(&mut self, container_id: &str) {
        let doomed = self.canvas.recording_page().is_some_and(|page| {
            self.library
                .page_is_within(page, container_id)
                .unwrap_or(true)
        });
        if doomed {
            // Saved by the flush that comes before every delete.
            self.stop_recording(self.last_edit_at);
        }
    }

    /// Picks up the dictionary once it has loaded, and saves words added to it.
    fn tick_spelling(&mut self, ctx: &egui::Context) {
        self.editor.spell.poll(ctx);
        if let Some(words) = self.editor.spell.take_personal_change() {
            if let Err(error) = self.library.set_setting("spell_personal", &words) {
                self.error(t!("error-save-dictionary", error = error.to_string()));
            }
        }
    }

    fn toast(&mut self, message: impl Into<String>) {
        let now = now_ms() as f64 / 1000.0;
        self.toast = Toast {
            message: message.into(),
            expires_at: now + 2.6,
            error: false,
        };
    }

    fn error(&mut self, message: impl Into<String>) {
        let now = now_ms() as f64 / 1000.0;
        self.toast = Toast {
            message: message.into(),
            expires_at: now + 4.5,
            error: true,
        };
    }

    fn set_appearance(&mut self, appearance: Appearance, ctx: &egui::Context) {
        self.appearance = appearance;
        ctx.set_theme(appearance.preference());
        let _ = self.library.set_setting("theme", appearance.code());
    }

    fn open_new_notebook(&mut self) {
        let color =
            SECTION_COLORS[self.workspace.notebooks.len() % SECTION_COLORS.len()].to_string();
        self.dialog = Some(Dialog::NewNotebook {
            name: String::new(),
            color,
        });
        self.dialog_focus = true;
    }

    /// File › Backstage at `page`. The page is saved first, and what floats over the
    /// window (the ribbon's band, the search) closes, as Backstage covers it all.
    fn open_backstage(&mut self, page: BackstagePage) {
        let _ = self.flush_save();
        self.ribbon.overlay = false;
        self.search.open = false;
        // Export starts on what is open: the page, else its section, else the notebook.
        let export = if self.page.is_some() {
            ExportScope::Page
        } else if self.selected_section.is_some() {
            ExportScope::Section
        } else {
            ExportScope::Notebook
        };
        self.backstage = Some(Backstage {
            page,
            name: String::new(),
            color: SECTION_COLORS[0].to_string(),
            focus: page == BackstagePage::New,
            export,
            page_counts: None,
        });
    }

    /// Back to the page from Backstage (← Back, Esc, or a command that needs the page).
    fn close_backstage(&mut self) {
        self.backstage = None;
    }

    fn open_new_section(&mut self, notebook_id: &str, group_id: Option<String>) {
        let color = self
            .workspace
            .notebooks
            .iter()
            .find(|notebook| notebook.id == notebook_id)
            .map_or(SECTION_COLORS[0], |notebook| notebook.color.as_str())
            .to_string();
        self.dialog = Some(Dialog::NewSection {
            notebook_id: notebook_id.to_string(),
            group_id,
            name: String::new(),
            color,
        });
        self.dialog_focus = true;
    }

    fn open_new_group(&mut self, notebook_id: &str, parent_id: Option<String>) {
        let color = self
            .workspace
            .notebooks
            .iter()
            .find(|notebook| notebook.id == notebook_id)
            .map_or(SECTION_COLORS[0], |notebook| notebook.color.as_str())
            .to_string();
        self.dialog = Some(Dialog::NewGroup {
            notebook_id: notebook_id.to_string(),
            parent_id,
            name: String::new(),
            color,
        });
        self.dialog_focus = true;
    }

    fn open_rename_notebook(&mut self, id: &str) {
        if let Some(notebook) = self
            .workspace
            .notebooks
            .iter()
            .find(|notebook| notebook.id == id)
        {
            self.dialog = Some(Dialog::RenameNotebook {
                id: id.to_string(),
                name: notebook.name.clone(),
                color: notebook.color.clone(),
            });
            self.dialog_focus = true;
        }
    }

    fn open_rename_section(&mut self, id: &str) {
        if let Some(section) = self
            .workspace
            .notebooks
            .iter()
            .flat_map(|notebook| notebook.sections.iter())
            .find(|section| section.id == id)
        {
            self.dialog = Some(Dialog::RenameSection {
                id: id.to_string(),
                name: section.name.clone(),
                color: section.color.clone(),
            });
            self.dialog_focus = true;
        }
    }

    fn open_rename_group(&mut self, id: &str) {
        if let Some(group) = self
            .workspace
            .notebooks
            .iter()
            .flat_map(|notebook| notebook.groups.iter())
            .find(|group| group.id == id)
        {
            self.dialog = Some(Dialog::RenameGroup {
                id: id.to_string(),
                name: group.name.clone(),
                color: group.color.clone(),
            });
            self.dialog_focus = true;
        }
    }

    /// Move or Copy… for a page or a section, or Merge into Another Section…: the dialog
    /// with the tree of notebooks, groups and sections to pick from.
    fn open_move_or_copy(&mut self, what: MoveWhat) {
        self.dialog = Some(Dialog::MoveOrCopy {
            what,
            filter: String::new(),
            target: None,
            confirm: false,
        });
        self.dialog_focus = true;
    }

    /// A page's title and section for the Move or Copy dialog, which asks every frame: the
    /// open page's, or a row's of the page list; only another page's section is read from
    /// the library.
    fn move_page_place(&self, id: &str) -> (String, Option<String>) {
        if let Some(page) = self.page.as_ref().filter(|page| page.id == id) {
            return (page.title.clone(), Some(page.section_id.clone()));
        }
        if let Some(page) = self.pages.iter().find(|page| page.id == id) {
            return (page.title.clone(), Some(page.section_id.clone()));
        }
        (
            String::new(),
            self.library.page_section_id(id).ok().flatten(),
        )
    }

    fn confirm(&mut self, title: &str, body: &str, confirm_label: &str, action: ConfirmAction) {
        self.dialog = Some(Dialog::Confirm {
            title: title.to_string(),
            body: body.to_string(),
            confirm_label: confirm_label.to_string(),
            action,
        });
    }

    fn open_search(&mut self) {
        let _ = self.flush_save();
        self.settle_preview();
        let recent = self
            .library
            .page_list(PageFilter::All, "", 7)
            .unwrap_or_default()
            .into_iter()
            .map(|page| SearchHit {
                page_id: page.id,
                title: page.title,
                excerpt: page.excerpt,
                section_id: page.section_id,
                section_name: page.section_name,
                section_color: page.section_color,
                notebook_id: page.notebook_id,
                notebook_name: page.notebook_name,
                updated_at: page.updated_at,
            })
            .collect();
        let scope = self.search.default_scope;
        self.search = SearchState {
            open: true,
            focus: true,
            recent,
            scope,
            last_scope: scope,
            default_scope: scope,
            // The box stays where it was drawn (this may be its click).
            box_rect: self.search.box_rect,
            ..Default::default()
        };
    }

    /// Every section in the selected section's group and in the groups nested in it, or
    /// `None` when the section sits at its notebook's top level (it has no group).
    fn search_group_sections(&self) -> Option<Vec<String>> {
        let selected = self.selected_section.as_deref()?;
        let (notebook, section) = self.workspace.notebooks.iter().find_map(|notebook| {
            notebook
                .sections
                .iter()
                .find(|section| section.id == selected)
                .map(|section| (notebook, section))
        })?;
        let mut groups = vec![section.group_id.clone()?];
        let mut next = 0;
        while let Some(parent) = groups.get(next).cloned() {
            groups.extend(
                notebook
                    .groups
                    .iter()
                    .filter(|group| group.parent_id.as_deref() == Some(parent.as_str()))
                    .map(|group| group.id.clone()),
            );
            next += 1;
        }
        Some(
            notebook
                .sections
                .iter()
                .filter(|section| {
                    section
                        .group_id
                        .as_ref()
                        .is_some_and(|id| groups.contains(id))
                })
                .map(|section| section.id.clone())
                .collect(),
        )
    }

    /// What a search in `scope` looks in: the selected section, its section group (and
    /// the groups below it; the section alone when it has none), the selected notebook,
    /// or everything. With nothing selected, a narrow scope finds nothing.
    fn search_filter(&self, scope: ScopeKind) -> OwnedFilter {
        let section = || OwnedFilter::Sections(self.selected_section.iter().cloned().collect());
        match scope {
            ScopeKind::Section => section(),
            ScopeKind::Group => self
                .search_group_sections()
                .map_or_else(section, OwnedFilter::Sections),
            ScopeKind::Notebook => self
                .selected_notebook
                .clone()
                .map_or_else(section, OwnedFilter::Notebook),
            ScopeKind::All => OwnedFilter::All,
        }
    }

    /// Makes `scope` the one every search starts in, and keeps it (the setting
    /// `search_scope`). Returns whether it was kept.
    fn set_default_scope(&mut self, scope: ScopeKind) -> bool {
        self.search.default_scope = scope;
        match self.library.set_setting("search_scope", scope.code()) {
            Ok(()) => true,
            Err(error) => {
                self.error(t!("error-save-setting", error = error.to_string()));
                false
            }
        }
    }

    /// What a row of the scope menu does. This Page closes the search and finds the words
    /// typed on the open page, as Ctrl+F does; a scope searches again in it; Set This
    /// Scope as Default keeps the current scope for the searches to come.
    fn pick_scope(&mut self, pick: ScopePick) {
        match pick {
            ScopePick::Page => {
                let query = self.search.query.trim().to_string();
                self.search.open = false;
                if self.page.is_some() && !self.reader.open {
                    // Find on page shows on the page, which Backstage covers.
                    self.close_backstage();
                    self.canvas.open_find((!query.is_empty()).then_some(query));
                }
            }
            ScopePick::Scope(scope) => {
                self.search.scope = scope;
                // The menu took the caret: the words typed can be changed straight away.
                self.search.focus = true;
            }
            ScopePick::SetDefault => {
                let scope = self.search.scope;
                if self.set_default_scope(scope) {
                    self.toast(t!("toast-search-scope-saved", scope = scope.label()));
                }
                self.search.focus = true;
            }
        }
    }

    /// Alt+O, or "Open in the search results pane" under the dropdown: the pages found
    /// (up to 100) listed in the results task pane, which stays docked beside the page
    /// (the desktop notebook's Search Results pane). The dropdown closes.
    fn open_results_pane(&mut self) {
        let query = self.search.query.trim().to_string();
        if query.is_empty() {
            return;
        }
        let scope = self.search.scope;
        let filter = self.search_filter(scope);
        let found = self.library.search_in(&query, filter.as_db(), 100);
        self.search.open = false;
        match found {
            Ok(hits) => {
                // The pane shows beside the page, which Backstage covers.
                self.close_backstage();
                self.open_task_pane(TaskPane::Results(ResultsPane {
                    kind: ResultsKind::Search { query, scope },
                    hits,
                }));
            }
            Err(error) => self.error(t!("error-search", error = error.to_string())),
        }
    }

    fn quick_section(&self) -> Option<String> {
        self.workspace.quick_section_id.clone()
    }

    /// View › New Quick Note (Ctrl+Shift+M). the desktop notebook opens a small window on a new page
    /// of its Quick Notes section; Folio has one window, so the new page opens in it, with
    /// the caret in its title.
    fn new_quick_note(&mut self) {
        let Some(quick) = self.quick_section() else {
            self.error(t!("error-no-quick-section"));
            return;
        };
        self.select_section(&quick, false);
        // When the open page could not be saved, the app stays on it, and adds nothing.
        if self.selected_section.as_deref() == Some(quick.as_str()) {
            self.new_page();
        }
    }

    fn view_title(&self) -> String {
        match &self.view {
            View::Section => self
                .selected_section
                .as_ref()
                .and_then(|id| {
                    self.workspace
                        .notebooks
                        .iter()
                        .flat_map(|notebook| notebook.sections.iter())
                        .find(|section| &section.id == id)
                        .map(|section| section.name.clone())
                })
                .unwrap_or_else(|| t!("app-view-pages").into_owned()),
            View::Trash => t!("app-recycle-bin").into_owned(),
        }
    }

    fn handle_library_action(&mut self, action: LibraryAction) {
        match action {
            LibraryAction::SelectSection(id) => self.select_section(&id, false),
            LibraryAction::ToggleNotebook(id) => {
                if !self.collapsed_notebooks.remove(&id) {
                    self.collapsed_notebooks.insert(id);
                }
            }
            LibraryAction::NewSection(id) => self.open_new_section(&id, None),
            LibraryAction::RenameNotebook(id) => self.open_rename_notebook(&id),
            LibraryAction::DeleteNotebook(id) => self.confirm(
                &t!("dialog-delete-notebook-title"),
                &t!("dialog-delete-contents-body"),
                &t!("dialog-delete-notebook"),
                ConfirmAction::DeleteNotebook(id),
            ),
            LibraryAction::RenameSection(id) => self.open_rename_section(&id),
            LibraryAction::DeleteSection(id) => self.confirm(
                &t!("dialog-delete-section-title"),
                &t!("dialog-delete-section-body"),
                &t!("dialog-delete-section"),
                ConfirmAction::DeleteSection(id),
            ),
            LibraryAction::ToggleGroup(id) => {
                if !self.collapsed_groups.remove(&id) {
                    self.collapsed_groups.insert(id);
                }
            }
            LibraryAction::NewGroup(notebook_id, parent_id) => {
                self.open_new_group(&notebook_id, parent_id)
            }
            LibraryAction::NewSectionIn(notebook_id, group_id) => {
                self.open_new_section(&notebook_id, group_id)
            }
            LibraryAction::RenameGroup(id) => self.open_rename_group(&id),
            LibraryAction::DeleteGroup(id) => self.confirm(
                &t!("dialog-delete-group-title"),
                &t!("dialog-delete-group-body"),
                &t!("dialog-delete-group"),
                ConfirmAction::DeleteGroup(id),
            ),
            LibraryAction::MoveSection {
                id,
                group_id,
                before,
            } => {
                match self
                    .library
                    .move_section(&id, group_id.as_deref(), before.as_deref())
                {
                    Ok(()) => {
                        self.refresh_workspace();
                        self.section_moved(&id);
                        let notebook = self
                            .workspace
                            .notebooks
                            .iter()
                            .find(|notebook| notebook.sections.iter().any(|s| s.id == id))
                            .map(|notebook| notebook.id.clone());
                        if let Some(notebook) = notebook {
                            self.unfold_to(&notebook, group_id.as_deref());
                        }
                    }
                    Err(error) => self.error(t!("error-move-section", error = error.to_string())),
                }
            }
            LibraryAction::MovePageTo(page_id, section_id) => {
                self.handle_page_list_action(PageListAction::Move(page_id, section_id))
            }
            LibraryAction::MoveNotebook(id, up) => match self.library.move_notebook(&id, up) {
                Ok(_) => self.refresh_workspace(),
                Err(error) => self.error(t!("error-move-notebook", error = error.to_string())),
            },
            // What History › Notebook Recycle Bin does from a section; in the bin already,
            // the menu leaves it open rather than going back.
            LibraryAction::OpenRecycleBin => {
                if self.view != View::Trash {
                    self.select_view(View::Trash);
                }
            }
            LibraryAction::NewNotebook => self.open_new_notebook(),
            // the desktop notebook's section Export… is Backstage › Export with the section chosen.
            LibraryAction::ExportSection(id) => {
                self.select_section(&id, true);
                self.open_backstage(BackstagePage::Export);
                if let Some(backstage) = self.backstage.as_mut() {
                    backstage.export = ExportScope::Section;
                }
            }
            LibraryAction::MoveOrCopySection(id) => self.open_move_or_copy(MoveWhat::Section(id)),
            LibraryAction::MergeSection(id) => self.open_move_or_copy(MoveWhat::Merge(id)),
            LibraryAction::CopySectionLink(id) => {
                if self.find_section(&id).is_none() {
                    self.error(t!("error-section-gone"));
                    return;
                }
                self.pending_copy = Some(section_link(&id));
                self.toast(t!("toast-section-link-copied"));
            }
            LibraryAction::SectionColor(id, color) => {
                let Some((_, name, _)) = self.find_section(&id) else {
                    self.error(t!("error-section-gone"));
                    return;
                };
                match self.library.update_section(&id, &name, &color) {
                    Ok(()) => {
                        self.refresh_workspace();
                        if let Some(page) = self.page.as_mut().filter(|page| page.section_id == id)
                        {
                            page.section_color = color;
                        }
                    }
                    Err(error) => self.error(t!("error-rename-section", error = error.to_string())),
                }
            }
        }
    }

    fn handle_page_list_action(&mut self, action: PageListAction) {
        match action {
            PageListAction::Open(id) => self.open_page(&id, false),
            PageListAction::Trash(id) => {
                if !self.flush_save() {
                    return;
                }
                // The nearest row above that shows (not one folded away under a page).
                let hidden = if self.view == View::Section && self.page_sort == PageSort::None {
                    outline::hidden(
                        &self
                            .pages
                            .iter()
                            .map(|page| (page.level, page.collapsed))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    vec![false; self.pages.len()]
                };
                let above = self
                    .pages
                    .iter()
                    .position(|page| page.id == id)
                    .and_then(|index| (0..index).rev().find(|&above| !hidden[above]))
                    .map(|index| self.pages[index].id.clone());
                match self.library.trash_page(&id) {
                    Ok(trashed) => {
                        // A recording on a page sent to the trash stops there.
                        if self
                            .canvas
                            .recording_page()
                            .is_some_and(|page| trashed.iter().any(|id| id == page))
                        {
                            self.stop_recording(self.last_edit_at);
                            let _ = self.flush_save();
                        }
                        self.refresh_workspace();
                        self.refresh_page_list();
                        if self
                            .page
                            .as_ref()
                            .is_some_and(|page| trashed.contains(&page.id))
                        {
                            self.page = None;
                            self.editor.reset();
                            let next = above
                                .filter(|above| self.pages.iter().any(|page| &page.id == above))
                                .or_else(|| self.pages.first().map(|page| page.id.clone()));
                            if let Some(next) = next {
                                self.open_page(&next, false);
                            }
                        }
                        self.reload_tags_pane(); // M3: the Find Tags pane drops the trashed lines
                        self.toast(if trashed.len() > 1 {
                            t!("toast-trashed-with-subpages")
                        } else {
                            t!("toast-trashed")
                        });
                    }
                    Err(error) => self.error(t!("error-trash-page", error = error.to_string())),
                }
            }
            PageListAction::Restore(id) => {
                if let Err(error) = self.library.restore_page(&id) {
                    self.error(t!("error-restore-page", error = error.to_string()));
                } else {
                    self.refresh_workspace();
                    let was_current = self.page.as_ref().is_some_and(|page| page.id == id);
                    let section_id = self
                        .page
                        .as_ref()
                        .filter(|page| page.id == id)
                        .map(|page| page.section_id.clone());
                    if let Some(page) = self.page.as_mut() {
                        if page.id == id {
                            page.trashed = false;
                        }
                    }
                    if was_current {
                        if let Some(section) = section_id {
                            self.view = View::Section;
                            self.selected_section = Some(section);
                            self.sync_notebook();
                        }
                        self.scroll_list_to_open = true;
                    }
                    self.refresh_page_list();
                    self.reload_tags_pane();
                    self.toast(t!("toast-restored"));
                }
            }
            PageListAction::Delete(id) => {
                self.confirm(
                    &t!("dialog-delete-page-title"),
                    &t!("dialog-delete-page-body"),
                    &t!("common-delete-forever"),
                    ConfirmAction::DeletePage(id),
                );
            }
            PageListAction::PickTrashedSection(id) => self.trashed_section_picked = Some(id),
            PageListAction::RestoreSection(id) => {
                if let Err(error) = self.library.restore_section(&id) {
                    self.error(t!("error-restore-section", error = error.to_string()));
                }
                // An open page of the section (the page last open comes back at a start,
                // in the bin) is out of it too, unless it was deleted on its own before.
                if let Some(page) = self.page.as_mut().filter(|page| page.section_id == id) {
                    if let Ok(Some(stored)) = self.library.get_page(&page.id) {
                        page.trashed = stored.trashed;
                    }
                }
                self.refresh_workspace();
                self.refresh_page_list();
                self.reload_tags_pane();
            }
            PageListAction::DeleteTrashedSection(id) => self.confirm(
                &t!("dialog-delete-section-title"),
                &t!("dialog-delete-section-forever-body"),
                &t!("common-delete-forever"),
                ConfirmAction::DeleteTrashedSection(id),
            ),
            // the desktop notebook's Rename puts the caret in the page's title with all of it selected.
            PageListAction::Rename(id) => {
                self.open_page(&id, true);
                if self.page.as_ref().is_some_and(|page| page.id == id) {
                    self.editor.focus_title(true);
                }
            }
            PageListAction::CutPage(id) => self.page_clip = Some(PageClip { id, cut: true }),
            PageListAction::CopyPage(id) => self.page_clip = Some(PageClip { id, cut: false }),
            PageListAction::PastePage { after } => self.paste_page(after.as_deref()),
            PageListAction::MoveOrCopyPage(id) => self.open_move_or_copy(MoveWhat::Page(id)),
            PageListAction::Move(id, section_id) => {
                match self.library.move_page(&id, &section_id) {
                    Ok(true) => {
                        self.refresh_workspace();
                        self.sync_open_page_section();
                        self.refresh_page_list();
                        self.reload_tags_pane(); // M3: keep (the pane keeps its own scope)
                        self.toast(t!("toast-page-moved"));
                    }
                    // Already in that section: nothing moved, so nothing to say.
                    Ok(false) => {}
                    Err(error) => self.error(t!("error-move-page", error = error.to_string())),
                }
            }
            PageListAction::Level(id, delta) => match self.library.shift_page_level(&id, delta) {
                Ok(true) => {
                    self.follow_in_list(&id);
                    self.refresh_page_list();
                }
                Ok(false) if delta > 0 => self.toast(t!("toast-subpage-needs-page-above")),
                Ok(false) => {}
                Err(error) => self.error(t!("error-page-level", error = error.to_string())),
            },
            // As dragging: only in the section's own order, as the desktop notebook's sorted lists hold
            // still.
            PageListAction::Step(id, up) => {
                if self.view != View::Section || self.page_sort != PageSort::None {
                    return;
                }
                match self.library.step_page(&id, up) {
                    Ok(true) => {
                        self.follow_in_list(&id);
                        self.refresh_page_list();
                    }
                    Ok(false) => {}
                    Err(error) => self.error(t!("error-move-page", error = error.to_string())),
                }
            }
            PageListAction::Fold(id, collapsed) => {
                match self.library.set_page_collapsed(&id, collapsed) {
                    Ok(()) => self.refresh_page_list(),
                    Err(error) => self.error(t!("error-fold-page", error = error.to_string())),
                }
            }
            PageListAction::Place {
                id,
                section_id,
                before,
                level,
            } => {
                match self
                    .library
                    .place_page(&id, &section_id, before.as_deref(), level)
                {
                    Ok(_) => {
                        self.follow_in_list(&id);
                        self.refresh_workspace();
                        self.sync_open_page_section();
                        self.refresh_page_list();
                    }
                    Err(error) => self.error(t!("error-move-page", error = error.to_string())),
                }
            }
            PageListAction::NewBelow(id, subpage) => self.new_page_below(&id, subpage),
            // What History › Page Versions (EditorAction::Versions) does, for this page.
            PageListAction::Versions(id) => {
                self.open_page(&id, false);
                if self.page.as_ref().is_some_and(|page| page.id == id) {
                    self.open_versions_pane();
                }
            }
        }
    }

    /// Paste in the page list: the page on the page clipboard goes into the listed section,
    /// right after the row `after` and its subpages, at that row's level (the desktop notebook pastes
    /// after the selected page), or at the end of the section. A cut page moves there and
    /// the clipboard empties; a copied page is copied there and can be pasted again.
    fn paste_page(&mut self, after: Option<&str>) {
        let Some(clip) = self.page_clip.clone() else {
            return;
        };
        let Some(section) = self
            .selected_section
            .clone()
            .filter(|_| self.view == View::Section)
        else {
            return;
        };
        // A page deleted, or sent to the Recycle Bin, since it was cut or copied is gone.
        match self.library.page_title(&clip.id) {
            Ok(Some(_)) => {}
            Ok(None) => {
                self.page_clip = None;
                self.error(t!("error-page-gone"));
                return;
            }
            Err(error) => {
                self.error(t!("error-move-copy", error = error.to_string()));
                return;
            }
        }
        // A copy starts from what is on screen, so the open page is saved first.
        if !self.flush_save() {
            return;
        }
        let (before, level) = match after {
            Some(row) => {
                // A sorted list is not in the section's own order, which placing follows.
                let listed;
                let order: &[PageSummary] = if self.page_sort == PageSort::None {
                    &self.pages
                } else {
                    listed = self
                        .library
                        .page_list(PageFilter::Section(&section), "", 500)
                        .unwrap_or_default();
                    &listed
                };
                let levels: Vec<u8> = order.iter().map(|page| page.level).collect();
                let cut = clip
                    .cut
                    .then(|| order.iter().position(|page| page.id == clip.id))
                    .flatten();
                let Some(index) = order.iter().position(|page| page.id == row) else {
                    return;
                };
                // A cut page pasted on itself or its own subpages stays where it is.
                let Some((before, level)) = outline::paste_after(&levels, index, cut) else {
                    return;
                };
                (before.map(|at| order[at].id.clone()), level)
            }
            None => (None, 0),
        };
        if clip.cut {
            match self
                .library
                .place_page(&clip.id, &section, before.as_deref(), Some(level))
            {
                Ok(_) => {
                    self.page_clip = None;
                    // A cut page that was open stays open, in its new place.
                    self.follow_in_list(&clip.id);
                    self.after_move_or_copy();
                    self.toast(t!("toast-page-moved"));
                }
                Err(error) => self.error(t!("error-move-page", error = error.to_string())),
            }
            return;
        }
        match self.library.copy_page_to(&clip.id, &section) {
            Ok(Some(copy)) => {
                // The copy is already in the section (at its end) if placing it fails.
                let placed =
                    self.library
                        .place_page(&copy.id, &section, before.as_deref(), Some(level));
                self.after_move_or_copy();
                match placed {
                    Ok(_) => {
                        let name = self.section_name(&section);
                        self.toast(t!("toast-page-copied", section = name));
                    }
                    Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
                }
            }
            Ok(None) => {
                self.page_clip = None;
                self.error(t!("error-page-gone"));
            }
            Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
        }
    }

    /// Unfolds a notebook and the section group `group` with every group around it, so a
    /// section or group just put there shows in the sidebar.
    fn unfold_to(&mut self, notebook_id: &str, group: Option<&str>) {
        self.collapsed_notebooks.remove(notebook_id);
        let Some(notebook) = self
            .workspace
            .notebooks
            .iter()
            .find(|notebook| notebook.id == notebook_id)
        else {
            return;
        };
        let mut current = group.map(str::to_string);
        for _ in 0..=16 {
            let Some(id) = current.take() else {
                break;
            };
            current = notebook
                .groups
                .iter()
                .find(|candidate| candidate.id == id)
                .and_then(|candidate| candidate.parent_id.clone());
            self.collapsed_groups.remove(&id);
        }
    }

    /// After the open page moves within the list, its row scrolls back into view.
    fn follow_in_list(&mut self, id: &str) {
        if self.page.as_ref().is_some_and(|page| page.id == id) {
            self.scroll_list_to_open = true;
        }
    }

    fn find_section(&self, section_id: &str) -> Option<(String, String, String)> {
        self.workspace.notebooks.iter().find_map(|notebook| {
            notebook
                .sections
                .iter()
                .find(|section| section.id == section_id)
                .map(|section| {
                    (
                        notebook.id.clone(),
                        section.name.clone(),
                        section.color.clone(),
                    )
                })
        })
    }

    /// A section's name, or nothing for one that is gone.
    fn section_name(&self, section_id: &str) -> String {
        self.find_section(section_id)
            .map(|(_, name, _)| name)
            .unwrap_or_default()
    }

    /// After pages changed section, keeps the open page's section and notebook details
    /// current and shows its new section.
    fn sync_open_page_section(&mut self) {
        let Some(id) = self.page.as_ref().map(|page| page.id.clone()) else {
            return;
        };
        let Ok(Some(section_id)) = self.library.page_section_id(&id) else {
            return;
        };
        if self
            .page
            .as_ref()
            .is_some_and(|page| page.section_id == section_id)
        {
            return;
        }
        let Some((notebook_id, section_name, section_color)) = self.find_section(&section_id)
        else {
            return;
        };
        let notebook = self
            .workspace
            .notebooks
            .iter()
            .find(|notebook| notebook.id == notebook_id)
            .map(|notebook| (notebook.name.clone(), notebook.color.clone()));
        if let Some(page) = self.page.as_mut() {
            page.section_id = section_id.clone();
            page.notebook_id = notebook_id;
            page.section_name = section_name;
            page.section_color = section_color;
            if let Some((name, color)) = notebook {
                page.notebook_name = name;
                page.notebook_color = color;
            }
        }
        self.view = View::Section;
        self.selected_section = Some(section_id);
        self.sync_notebook();
    }

    fn apply_dialog_action(&mut self, action: DialogAction, ctx: &egui::Context) {
        match action {
            DialogAction::CreateNotebook { name, color } => {
                match self.library.create_notebook(&name, &color) {
                    Ok(notebook) => {
                        self.refresh_workspace();
                        if let Some(section) = notebook.sections.first() {
                            self.select_section(&section.id, true);
                        }
                        self.scroll_nav_to_selected = true;
                        self.toast(t!("toast-notebook-created"));
                    }
                    Err(error) => {
                        self.error(t!("error-create-notebook", error = error.to_string()))
                    }
                }
            }
            DialogAction::CreateSection {
                notebook_id,
                group_id,
                name,
                color,
            } => match self.library.create_section_in(
                &notebook_id,
                group_id.as_deref(),
                &name,
                &color,
            ) {
                Ok(section) => {
                    self.refresh_workspace();
                    self.unfold_to(&notebook_id, group_id.as_deref());
                    self.select_section(&section.id, true);
                    self.toast(t!("toast-section-created"));
                }
                Err(error) => self.error(t!("error-create-section", error = error.to_string())),
            },
            DialogAction::CreateGroup {
                notebook_id,
                parent_id,
                name,
                color,
            } => match self.library.create_section_group(
                &notebook_id,
                parent_id.as_deref(),
                &name,
                &color,
            ) {
                Ok(_) => {
                    self.refresh_workspace();
                    self.unfold_to(&notebook_id, parent_id.as_deref());
                    self.toast(t!("toast-group-created"));
                }
                Err(error) => self.error(t!("error-create-group", error = error.to_string())),
            },
            DialogAction::RenameNotebook { id, name, color } => {
                if let Err(error) = self.library.update_notebook(&id, &name, &color) {
                    self.error(t!("error-rename-notebook", error = error.to_string()));
                } else {
                    self.refresh_workspace();
                    self.refresh_page_list();
                    let _ = name;
                    let stored = self
                        .workspace
                        .notebooks
                        .iter()
                        .find(|notebook| notebook.id == id)
                        .map(|notebook| (notebook.name.clone(), notebook.color.clone()));
                    if let (Some(page), Some((name, color))) = (self.page.as_mut(), stored) {
                        if page.notebook_id == id {
                            page.notebook_name = name;
                            page.notebook_color = color;
                        }
                    }
                }
            }
            DialogAction::RenameSection { id, name, color } => {
                if let Err(error) = self.library.update_section(&id, &name, &color) {
                    self.error(t!("error-rename-section", error = error.to_string()));
                } else {
                    self.refresh_workspace();
                    self.refresh_page_list();
                    let _ = name;
                    if let (Some((_, name, color)), Some(page)) =
                        (self.find_section(&id), self.page.as_mut())
                    {
                        if page.section_id == id {
                            page.section_name = name;
                            page.section_color = color;
                        }
                    }
                }
            }
            DialogAction::RenameGroup { id, name, color } => {
                if let Err(error) = self.library.update_section_group(&id, &name, &color) {
                    self.error(t!("error-rename-group", error = error.to_string()));
                } else {
                    self.refresh_workspace();
                }
            }
            DialogAction::Confirm(action) => self.run_confirm(action),
            DialogAction::SetAppearance(appearance) => self.set_appearance(appearance, ctx),
            DialogAction::SetUiLanguage(choice) => {
                self.ui_language = choice;
                i18n::set_lang(choice.unwrap_or_else(|| i18n::startup_lang(None)));
                // Automatic is the default, stored as no value.
                let code = choice.map_or("", Lang::code);
                if let Err(error) = self.library.set_setting("ui_language", code) {
                    self.error(t!("error-save-language", error = error.to_string()));
                }
                // The page list and the Find Tags pane carry translated text ("Untitled"),
                // and the pane is sorted by it.
                self.refresh_page_list();
                self.reload_tags_pane();
                ctx.request_repaint();
            }
            DialogAction::SetSpellLanguage(code) => {
                self.editor.spell.set_language(&code);
                // Automatic is the default, stored as no value.
                let value = if code == "auto" { "" } else { code.as_str() };
                if let Err(error) = self.library.set_setting("spell_language", value) {
                    self.error(t!("error-save-spell-language", error = error.to_string()));
                }
            }
            DialogAction::ClearPersonalDictionary => {
                self.editor.spell.clear_personal();
                if let Err(error) = self.library.set_setting("spell_personal", "") {
                    self.error(t!("error-clear-dictionary", error = error.to_string()));
                }
            }
            DialogAction::Backup => self.backup(),
            DialogAction::OpenDataFolder => {
                let _ = open::that(self.library.data_dir());
            }
            DialogAction::ApplyLink(edit) => self.apply_link(edit, ctx),
            DialogAction::RestoreVersion {
                page_id,
                version_id,
            } => {
                self.restore_version(&page_id, version_id);
                self.reload_versions_pane();
            }
            DialogAction::DeleteVersions(page_id) => {
                self.delete_versions(VersionScope::Page, &page_id);
            }
            DialogAction::DeleteVersion {
                page_id,
                version_id,
            } => {
                match self.library.delete_version(&page_id, version_id) {
                    Ok(true) => self.toast(t!("toast-version-deleted")),
                    // Already gone (another window of the library deleted it).
                    Ok(false) => {}
                    Err(error) => {
                        self.error(t!("error-delete-versions", error = error.to_string()))
                    }
                }
                self.reload_versions_pane();
            }
            DialogAction::Run(cmd) => self.run_cmd(cmd, ctx, ctx.input(|i| i.time)),
            DialogAction::SetPageListLeft(on) => {
                self.page_list_left = on;
                self.save_flag("page_list_left", on);
            }
            DialogAction::SetNotebooksPinned(on) => self.set_notebooks_pinned(on),
            DialogAction::SetScrollbarLeft(on) => {
                self.scrollbar_left = on;
                self.save_flag("scrollbar_left", on);
            }
            DialogAction::SetSearchScope(scope) => {
                self.set_default_scope(scope);
            }
            DialogAction::SetPrintoutBackground(on) => {
                self.printout_background = on;
                self.save_flag("printout_background", on);
            }
            DialogAction::SetMiniToolbar(on) => {
                self.editor.mini_on_selection = on;
                self.save_flag("mini_toolbar_on_selection", on);
                if !on {
                    self.editor.mini_bar = None;
                }
            }
            DialogAction::SetAutoCalc(on) => {
                self.editor.auto_calc = on;
                self.save_flag("no_auto_calc", !on);
            }
            DialogAction::SaveTags(mut gallery) => {
                // A new revision, so the Tag menu measures its rows again.
                gallery.bump();
                if let Err(error) = gallery.save(&mut self.library) {
                    self.error(t!("error-save-setting", error = error.to_string()));
                }
                self.editor.tags = gallery;
            }
            DialogAction::MovePage { id, section } => match self.library.move_page(&id, &section) {
                Ok(moved) => {
                    self.after_move_or_copy();
                    if moved {
                        self.toast(t!("toast-page-moved"));
                    }
                }
                Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
            },
            // A copy starts from what is on screen, so the open page is saved first.
            DialogAction::CopyPage { .. } | DialogAction::CopySection { .. }
                if !self.flush_save() => {}
            DialogAction::CopyPage { id, section } => {
                match self.library.copy_page_to(&id, &section) {
                    Ok(Some(_)) => {
                        self.after_move_or_copy();
                        let name = self.section_name(&section);
                        self.toast(t!("toast-page-copied", section = name));
                    }
                    Ok(None) => self.error(t!("error-page-gone")),
                    Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
                }
            }
            DialogAction::MoveSection {
                id,
                notebook,
                group,
            } => match self
                .library
                .move_section_to(&id, &notebook, group.as_deref())
            {
                Ok(()) => {
                    self.after_move_or_copy();
                    self.section_moved(&id);
                    // The open page's section may now sit in another notebook.
                    let details = self
                        .workspace
                        .notebooks
                        .iter()
                        .find(|candidate| candidate.id == notebook)
                        .map(|candidate| (candidate.name.clone(), candidate.color.clone()));
                    if let (Some(page), Some((name, color))) = (
                        self.page.as_mut().filter(|page| page.section_id == id),
                        details,
                    ) {
                        page.notebook_id = notebook.clone();
                        page.notebook_name = name;
                        page.notebook_color = color;
                    }
                    self.unfold_to(&notebook, group.as_deref());
                    self.toast(t!("toast-section-moved"));
                }
                Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
            },
            DialogAction::CopySection {
                id,
                notebook,
                group,
            } => match self.library.copy_section(&id, &notebook, group.as_deref()) {
                Ok(_) => {
                    self.after_move_or_copy();
                    self.unfold_to(&notebook, group.as_deref());
                    self.toast(t!("toast-section-copied"));
                }
                Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
            },
            DialogAction::ImportPick { folder } => {
                let picked = if folder {
                    rfd::FileDialog::new().pick_folder()
                } else {
                    rfd::FileDialog::new()
                        .add_filter(
                            t!("import-notebook-file-filter"),
                            &["onepkg", "onetoc2", "one"],
                        )
                        .pick_file()
                };
                match picked {
                    Some(path) => self.start_notebook_import(path),
                    None => self.dialog = Some(Dialog::ImportNotebook(ImportDialog::Choose)),
                }
            }
            DialogAction::ImportCancel => {
                if let Some(job) = &self.import_job {
                    job.cancel();
                }
            }
            DialogAction::MergeSection { from, into } => {
                match self.library.merge_section(&from, &into) {
                    Ok(count) => {
                        self.after_move_or_copy();
                        self.select_section(&into, false);
                        let name = self.section_name(&into);
                        self.toast(t!("toast-section-merged", count = count, section = name));
                    }
                    Err(error) => self.error(t!("error-move-copy", error = error.to_string())),
                }
            }
        }
    }

    /// After the Move or Copy dialog changed the library: the navigation pane, the open
    /// page's section, the page list and the Find Tags pane show it.
    fn after_move_or_copy(&mut self) {
        self.refresh_workspace();
        self.sync_open_page_section();
        self.refresh_page_list();
        self.reload_tags_pane();
    }

    /// Insert › Symbol's recent row: `symbol` moves to the front of the symbols used
    /// lately, `RECENT_SYMBOLS` at most, and the list is saved.
    fn remember_symbol(&mut self, symbol: char) {
        if self.recent_symbols.first() == Some(&symbol) {
            return;
        }
        self.recent_symbols.retain(|kept| *kept != symbol);
        self.recent_symbols.insert(0, symbol);
        self.recent_symbols.truncate(RECENT_SYMBOLS);
        let value: String = self.recent_symbols.iter().collect();
        if let Err(error) = self.library.set_setting("recent_symbols", &value) {
            self.error(t!("error-save-setting", error = error.to_string()));
        }
    }

    /// Stores an on/off setting as `"true"` or `"false"`; a failed save says so.
    fn save_flag(&mut self, key: &str, on: bool) {
        let value = if on { "true" } else { "false" };
        if let Err(error) = self.library.set_setting(key, value) {
            self.error(t!("error-save-setting", error = error.to_string()));
        }
    }

    /// Deletes the kept versions of a page, or of every page in a section or notebook,
    /// and lists what is left in an open Versions pane.
    fn delete_versions(&mut self, scope: VersionScope, id: &str) {
        let deleted = match scope {
            VersionScope::Page => self.library.delete_page_versions(id),
            VersionScope::Section => self.library.delete_versions_in_section(id),
            VersionScope::Notebook => self.library.delete_versions_in_notebook(id),
        };
        match deleted {
            Ok(count) => self.toast(t!("toast-versions-deleted", count = count)),
            Err(error) => self.error(t!("error-delete-versions", error = error.to_string())),
        }
        self.reload_versions_pane();
    }

    /// History › Delete All Versions of This Page, in Section or in Notebook: asks first.
    fn confirm_delete_versions(&mut self, scope: VersionScope) {
        let (id, body) = match scope {
            VersionScope::Page => (self.versions_page(), t!("dialog-delete-versions-page-body")),
            VersionScope::Section => (
                self.selected_section.clone(),
                t!("dialog-delete-versions-section-body"),
            ),
            VersionScope::Notebook => (
                self.selected_notebook.clone(),
                t!("dialog-delete-versions-notebook-body"),
            ),
        };
        if let Some(id) = id {
            self.confirm(
                &t!("dialog-delete-versions-title"),
                &body,
                &t!("common-delete"),
                ConfirmAction::DeleteVersions(scope, id),
            );
        }
    }

    /// Empty Recycle Bin, from the bin's page list or History: asks first.
    fn confirm_empty_trash(&mut self) {
        self.confirm(
            &t!("dialog-empty-trash-title"),
            &t!("dialog-empty-trash-body"),
            &t!("app-empty-trash"),
            ConfirmAction::EmptyTrash,
        );
    }

    fn run_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::DeleteNotebook(id) => {
                self.stop_recording_within(&id);
                if !self.flush_save() {
                    return;
                }
                match self.library.delete_notebook(&id) {
                    Ok(()) => {
                        self.collapsed_notebooks.remove(&id);
                        self.refresh_workspace();
                        self.after_container_deleted();
                        self.reload_tags_pane();
                        self.toast(t!("toast-notebook-deleted"));
                    }
                    Err(error) => {
                        self.error(t!("error-delete-notebook", error = error.to_string()))
                    }
                }
            }
            ConfirmAction::DeleteSection(id) => {
                self.stop_recording_within(&id);
                if !self.flush_save() {
                    return;
                }
                match self.library.trash_section(&id) {
                    Ok(()) => {
                        self.refresh_workspace();
                        self.after_container_deleted();
                        self.reload_tags_pane();
                        self.toast(t!("toast-section-deleted"));
                    }
                    Err(error) => self.error(t!("error-delete-section", error = error.to_string())),
                }
            }
            ConfirmAction::DeleteTrashedSection(id) => {
                self.stop_recording_within(&id);
                if !self.flush_save() {
                    return;
                }
                if let Err(error) = self.library.delete_trashed_section(&id) {
                    self.error(t!("error-delete-section", error = error.to_string()));
                } else if self.page.as_ref().is_some_and(|page| page.section_id == id) {
                    // A page of it left open (the page last open, back at a start) goes
                    // with it.
                    self.page = None;
                    self.dirty = false;
                    self.editor.reset();
                }
                self.refresh_workspace();
                self.refresh_page_list();
                self.reload_tags_pane();
            }
            ConfirmAction::DeleteGroup(id) => {
                self.stop_recording_within(&id);
                if !self.flush_save() {
                    return;
                }
                match self.library.delete_section_group(&id) {
                    Ok(()) => {
                        self.collapsed_groups.remove(&id);
                        self.refresh_workspace();
                        self.after_container_deleted();
                        self.reload_tags_pane();
                        self.toast(t!("toast-group-deleted"));
                    }
                    Err(error) => self.error(t!("error-delete-group", error = error.to_string())),
                }
            }
            ConfirmAction::DeletePage(id) => {
                self.stop_recording_within(&id);
                // Save the open page first: it may show a picture that belongs to the
                // page being deleted, which is then kept for it.
                let _ = self.flush_save();
                if let Err(error) = self.library.delete_page_permanently(&id) {
                    self.error(t!("error-delete-page", error = error.to_string()));
                } else {
                    self.refresh_workspace();
                    self.refresh_page_list();
                    if self.page.as_ref().is_some_and(|page| page.id == id) {
                        self.page = None;
                        self.editor.reset();
                        if let Some(first) = self.pages.first().cloned() {
                            self.open_page(&first.id, false);
                        }
                    }
                    self.reload_tags_pane();
                    self.toast(t!("toast-page-deleted"));
                }
            }
            ConfirmAction::DeleteVersions(scope, id) => self.delete_versions(scope, &id),
            ConfirmAction::EmptyTrash => {
                let in_trash = self.canvas.recording_page().is_some_and(|page| {
                    self.library
                        .get_page(page)
                        .ok()
                        .flatten()
                        .is_none_or(|page| page.trashed)
                });
                if in_trash {
                    self.stop_recording(self.last_edit_at);
                }
                let _ = self.flush_save();
                if let Err(error) = self.library.empty_trash() {
                    self.error(t!("error-empty-trash", error = error.to_string()));
                } else {
                    self.refresh_workspace();
                    self.refresh_page_list();
                    if self.view == View::Trash
                        || self.page.as_ref().is_some_and(|page| page.trashed)
                    {
                        self.page = None;
                        self.dirty = false;
                        self.editor.reset();
                    }
                    self.toast(t!("toast-trash-emptied"));
                }
            }
        }
    }

    /// After a notebook or section is deleted, stay where we are if the open page and
    /// section still exist; otherwise fall back to the first section.
    fn after_container_deleted(&mut self) {
        let section_exists = |id: &str| self.find_section(id).is_some();
        let page_ok = self
            .page
            .as_ref()
            .is_none_or(|page| section_exists(&page.section_id));
        let section_ok = self.selected_section.as_deref().is_none_or(section_exists);
        if page_ok && section_ok {
            self.refresh_page_list();
        } else {
            self.page = None;
            self.dirty = false;
            self.editor.reset();
            self.pick_fallback_section();
        }
    }

    fn pick_fallback_section(&mut self) {
        if let Some(section) = self
            .workspace
            .notebooks
            .first()
            .and_then(|notebook| notebook.sections.first())
        {
            self.select_section(&section.id.clone(), true);
        } else {
            self.view = View::Section;
            self.selected_section = None;
            self.page = None;
            self.editor.reset();
            self.pages.clear();
        }
    }

    /// Sizes of the database and attachments. Reading them walks the attachments folder,
    /// so it is refreshed at most every few seconds rather than every frame.
    fn storage_info(&mut self, now: f64) -> Option<crate::model::StorageInfo> {
        let stale = self
            .storage
            .as_ref()
            .is_none_or(|(checked, _)| now - checked > 5.0 || now < *checked);
        if stale {
            self.storage = self.library.storage_info().ok().map(|info| (now, info));
        }
        self.storage.as_ref().map(|(_, info)| info.clone())
    }

    fn backup(&mut self) {
        let _ = self.flush_save();
        let default = format!(
            "folio-backup-{}.db",
            chrono::Local::now().format("%Y-%m-%d")
        );
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(default)
            .add_filter(t!("app-file-filter-database"), &["db"])
            .save_file()
        {
            match self.library.backup_to(&path) {
                Ok(media) => self.toast(t!(
                    "toast-backup-saved",
                    path = path.display().to_string(),
                    media = media.display().to_string()
                )),
                Err(error) => self.error(t!("error-backup", error = error.to_string())),
            }
        }
    }

    /// File › Open › Import the desktop notebook…: asks for a file or a folder.
    fn open_notebook_import(&mut self) {
        if self.import_job.is_none() {
            self.dialog = Some(Dialog::ImportNotebook(ImportDialog::Choose));
        }
    }

    /// Starts importing a the desktop notebook package, notebook folder or section. Tests call this
    /// directly, since they can't drive the system's file picker.
    pub(crate) fn start_notebook_import(&mut self, path: std::path::PathBuf) {
        if self.import_job.is_some() {
            return;
        }
        let Some(source) = Source::from_path(&path) else {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.dialog = Some(Dialog::ImportNotebook(ImportDialog::Failed(
                ImportError::Nothing { name },
            )));
            return;
        };
        let _ = self.flush_save();
        self.import_job = Some(ImportJob::start(source, self.library.data_dir()));
        self.dialog = Some(Dialog::ImportNotebook(ImportDialog::Running));
    }

    /// Checks the running import once a frame; writes its notebook when it is done.
    fn poll_notebook_import(&mut self, ctx: &egui::Context) {
        let Some(job) = self.import_job.as_mut() else {
            return;
        };
        let Some(outcome) = job.poll() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let name = job.name().to_string();
        self.import_job = None;
        self.finish_notebook_import(name, outcome);
        ctx.request_repaint();
    }

    fn finish_notebook_import(&mut self, name: String, outcome: Result<ImportPlan, ImportError>) {
        let step = match outcome.and_then(|plan| {
            self.library
                .import_plan(plan)
                .map_err(|error| ImportError::Failed {
                    name: name.clone(),
                    error: error.to_string(),
                })
        }) {
            Ok(summary) => {
                self.refresh_workspace();
                // A Find Tags pane open on all notebooks lists the imported tags too.
                self.reload_tags_pane();
                match summary.first_page.clone() {
                    Some(page) => self.open_page(&page, true),
                    // Only empty sections came in: show the new notebook's first one.
                    None => {
                        let first = self
                            .workspace
                            .notebooks
                            .iter()
                            .find(|notebook| notebook.id == summary.notebook_id)
                            .and_then(|notebook| notebook.sections.first())
                            .map(|section| section.id.clone());
                        if let Some(section) = first {
                            self.select_section(&section, true);
                        }
                    }
                }
                // The new notebook is the last in the pane: show where it went.
                self.scroll_nav_to_selected = true;
                ImportDialog::Done(Box::new(summary))
            }
            Err(ImportError::Cancelled) => {
                self.dialog = None;
                self.toast(t!("import-notebook-cancelled"));
                return;
            }
            Err(error) => ImportDialog::Failed(error),
        };
        self.dialog = Some(Dialog::ImportNotebook(step));
    }

    /// Saves the open page as a web page where the user picks. True when it was written.
    fn export_html(&mut self) -> bool {
        if self.page.is_none() {
            return false;
        }
        let _ = self.flush_save();
        let Some(page) = self.page.as_ref() else {
            return false;
        };
        let html = export::canvas_export_html(page, &self.library);
        let slug = util::file_slug(&display_title(&page.title));
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{slug}.html"))
            .add_filter(t!("app-file-filter-web-page"), &["html"])
            .save_file()
        else {
            return false;
        };
        match std::fs::write(&path, html) {
            Ok(()) => {
                self.toast(t!("toast-exported", path = path.display().to_string()));
                true
            }
            Err(error) => {
                self.error(t!("error-export", error = error.to_string()));
                false
            }
        }
    }

    /// Backstage › Export of `scope`: the open page as one web page, or the selected
    /// section's or notebook's pages as a folder of them with an index, in a folder the
    /// user picks. The pages come in the page list's order, a notebook's sections in the
    /// navigation pane's. True when it was written.
    fn export_scope(&mut self, scope: ExportScope) -> bool {
        let (name, sections) = match scope {
            ExportScope::Page => return self.export_html(),
            ExportScope::Section => {
                let Some(id) = self.selected_section.clone() else {
                    return false;
                };
                let Some((_, name, _)) = self.find_section(&id) else {
                    return false;
                };
                (name, vec![id])
            }
            ExportScope::Notebook => {
                let Some(notebook) = self.selected_notebook.as_deref().and_then(|id| {
                    self.workspace
                        .notebooks
                        .iter()
                        .find(|notebook| notebook.id == id)
                }) else {
                    return false;
                };
                let sections = section_choices(notebook)
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>();
                (notebook.name.clone(), sections)
            }
        };
        let _ = self.flush_save();
        let Some(dir) = rfd::FileDialog::new()
            .set_title(t!("app-backstage-export"))
            .pick_folder()
        else {
            return false;
        };
        let written = self.pages_in(&sections).and_then(|pages| {
            export::export_pages_to_folder(&dir, &name, &pages, &self.library)
                .map(|folder| (pages.len(), folder))
        });
        match written {
            Ok((count, folder)) => {
                self.toast(t!(
                    "toast-exported-folder",
                    count = count,
                    path = folder.display().to_string()
                ));
                true
            }
            Err(error) => {
                self.error(t!("error-export-folder", error = error));
                false
            }
        }
    }

    /// The pages of `sections`, in order, each in the page list's order. The Recycle Bin's
    /// pages are left out.
    fn pages_in(&self, sections: &[String]) -> Result<Vec<Page>, String> {
        let mut pages = Vec::new();
        for section in sections {
            let list = self
                .library
                .page_list(PageFilter::Section(section), "", i64::MAX)
                .map_err(|error| error.to_string())?;
            for summary in list {
                if let Some(page) = self
                    .library
                    .get_page(&summary.id)
                    .map_err(|error| error.to_string())?
                {
                    pages.push(page);
                }
            }
        }
        Ok(pages)
    }

    fn editor_action(&mut self, action: EditorAction) {
        let Some(page_id) = self.page.as_ref().map(|page| page.id.clone()) else {
            return;
        };
        match action {
            EditorAction::OpenReader => {
                let _ = self.flush_save();
                if let Some(page) = self.page.as_ref() {
                    self.reader.begin(page);
                }
            }
            EditorAction::PrintPage => {
                let _ = self.flush_save();
                if let Some(page) = self.page.as_ref() {
                    match export::print_page(page, &self.library) {
                        Ok(_) => self.toast(t!("toast-print-opened")),
                        Err(error) => self.error(t!("error-print-view", error = error.to_string())),
                    }
                }
            }
            EditorAction::Restore => self.handle_page_list_action(PageListAction::Restore(page_id)),
            EditorAction::Delete => self.handle_page_list_action(PageListAction::Delete(page_id)),
            EditorAction::Versions => self.open_versions_pane(),
        }
    }

    /// Acts on what the editor asked for this frame: the link dialog, or following a link.
    fn after_editor(&mut self) {
        if let Some(edit) = self.editor.link_edit.take() {
            self.dialog = Some(Dialog::Link {
                edit,
                query: String::new(),
                searched: None,
                choices: Vec::new(),
            });
            self.dialog_focus = true;
        }
        if let Some(url) = self.editor.link_request.take() {
            self.follow_link(&url);
        }
    }

    /// Another page opens in Folio; web, email and file links open in the system's app.
    fn follow_link(&mut self, url: &str) {
        // A paragraph link (Copy Link to Paragraph) also selects its paragraph, when it
        // is still there; else the page simply opens.
        if let Some((id, block)) = parse_page_link(url) {
            self.open_page(id, true);
            let Some(block) = block else {
                return;
            };
            let Some(page) = self.page.as_ref().filter(|page| page.id == id) else {
                return;
            };
            let item = page.canvas.items.iter().find(|item| {
                item.doc()
                    .is_some_and(|doc| doc.block_index(block).is_some())
            });
            if let Some(item) = item {
                self.canvas.reveal = Some(Reveal {
                    item: item.id,
                    block: Some(block),
                    focus: true,
                });
            }
            return;
        }
        // A link to a section (Copy Link to Section) opens it at its first page.
        if let Some(id) = url.strip_prefix(SECTION_LINK_PREFIX) {
            let id = id.trim();
            if self.find_section(id).is_some() {
                self.select_section(id, true);
            } else {
                self.error(t!("error-section-gone"));
            }
            return;
        }
        if !is_openable_link(url) {
            self.error(t!("error-link-kind"));
            return;
        }
        self.last_opened_url = Some(url.to_string());
        if let Err(error) = open_external(url) {
            self.error(t!("error-open-link", error = error.to_string()));
        }
    }

    fn apply_link(&mut self, edit: editor::LinkEdit, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let doc = edit
            .doc_id
            .rsplit_once(':')
            .and_then(|(_, item)| item.parse::<u64>().ok())
            .and_then(|item| page.canvas.item_mut(item))
            .and_then(CanvasItem::doc_mut);
        let changed = doc.is_some_and(|doc| editor::apply_link(doc, &mut self.editor, &edit, now));
        if changed {
            // New link text moves what find on page highlights.
            if let Some(find) = self.canvas.find.as_mut() {
                find.dirty = true;
            }
            self.mark_dirty(now);
        } else {
            // Nothing to link (an empty address at the caret): typing goes on where it was.
            self.editor.restore_focus();
        }
    }

    /// Pages to link to: recent ones, or those matching the search.
    fn link_choices(&self, query: &str) -> Vec<LinkChoice> {
        let query = query.trim();
        if query.is_empty() {
            return self
                .library
                .page_list(PageFilter::All, "", 6)
                .unwrap_or_default()
                .into_iter()
                .map(|page| LinkChoice {
                    title: display_title(&page.title).into_owned(),
                    place: format!("{} / {}", page.notebook_name, page.section_name),
                    id: page.id,
                })
                .collect();
        }
        self.library
            .search(query, 6)
            .unwrap_or_default()
            .into_iter()
            .map(|hit| LinkChoice {
                title: display_title(&hit.title).into_owned(),
                place: format!("{} / {}", hit.notebook_name, hit.section_name),
                id: hit.page_id,
            })
            .collect()
    }

    /// History › Page Versions: the open page's versions in the task pane, which then
    /// follows the page opened (see `follow_versions_pane`).
    fn open_versions_pane(&mut self) {
        // Save first, so the list (and a restore) starts from what is on screen.
        if !self.flush_save() {
            return;
        }
        let page_id = self.versions_page().unwrap_or_default();
        let versions = if page_id.is_empty() {
            Vec::new()
        } else {
            match self.library.page_versions(&page_id) {
                Ok(versions) => versions,
                Err(error) => {
                    self.error(t!("error-read-versions", error = error.to_string()));
                    return;
                }
            }
        };
        self.open_task_pane(TaskPane::Versions(VersionsPane {
            page_id,
            versions,
            selected: 0,
            confirm_delete: false,
        }));
    }

    /// The page the Versions pane lists: the open page, unless it is in the Recycle Bin.
    fn versions_page(&self) -> Option<String> {
        self.page
            .as_ref()
            .filter(|page| !page.trashed)
            .map(|page| page.id.clone())
    }

    /// Reads the Versions pane's list again (after a restore or a deletion), keeping the
    /// version previewed where it can.
    fn reload_versions_pane(&mut self) {
        let Some(TaskPane::Versions(pane)) = self.task_pane.as_mut() else {
            return;
        };
        pane.confirm_delete = false;
        if pane.page_id.is_empty() {
            return;
        }
        // The version previewed by its id: a version deleted above it, or the one a
        // restore adds at the top, leaves it previewed.
        let previewed = pane.versions.get(pane.selected).map(|version| version.id);
        match self.library.page_versions(&pane.page_id) {
            Ok(versions) => {
                pane.selected = previewed
                    .and_then(|id| versions.iter().position(|version| version.id == id))
                    .unwrap_or_else(|| pane.selected.min(versions.len().saturating_sub(1)));
                pane.versions = versions;
            }
            Err(error) => self.error(t!("error-read-versions", error = error.to_string())),
        }
    }

    /// The Versions pane follows the open page: another page opened lists its versions.
    fn follow_versions_pane(&mut self) {
        let Some(page_id) = self.versions_page() else {
            return;
        };
        let Some(TaskPane::Versions(pane)) = self.task_pane.as_mut() else {
            return;
        };
        if pane.page_id == page_id {
            return;
        }
        pane.page_id = page_id;
        pane.versions.clear();
        pane.selected = 0;
        self.reload_versions_pane();
    }

    /// History › Recent Edits: the pages edited in `range`, newest first, in the results
    /// pane.
    fn open_recent_edits(&mut self, range: EditsRange) {
        // The open page's latest edit counts.
        let _ = self.flush_save();
        match self.library.recent_pages(range.since(now_ms()), 200) {
            Ok(hits) => self.open_task_pane(TaskPane::Results(ResultsPane {
                kind: ResultsKind::Edits(range),
                hits,
            })),
            Err(error) => self.error(t!("error-search", error = error.to_string())),
        }
    }

    fn restore_version(&mut self, page_id: &str, version_id: i64) {
        if !self.flush_save() {
            return;
        }
        if let Err(error) = self.library.restore_version(page_id, version_id) {
            self.error(t!("error-restore-version", error = error.to_string()));
            return;
        }
        match self.library.get_page(page_id) {
            Ok(Some(page)) => {
                // Reloading resets the editor and canvas undo, which belonged to the old content.
                self.load_page(page, false);
                self.refresh_page_list();
                self.reload_tags_pane(); // M3: the Find Tags pane lists the restored content
                self.toast(t!("toast-version-restored"));
            }
            Ok(None) => self.error(t!("error-page-gone")),
            Err(error) => self.error(t!("error-open-page", error = error.to_string())),
        }
    }

    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.search.open {
            return;
        }
        // Esc leaves Backstage, as in Office; an open menu takes it first.
        if self.backstage.is_some()
            && !egui::Popup::is_any_open(ctx)
            && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.close_backstage();
            return;
        }
        // Alt+F opens File (Backstage), as in Office. Alt+Shift+F is left to the note,
        // where it inserts the date and time.
        if !pressed_with_shift(ctx, Key::F) && consume_alt_shortcut(ctx, Modifiers::ALT, Key::F) {
            self.open_backstage(BackstagePage::Info);
            return;
        }
        // Backstage covers the page and its panes: any other shortcut (Ctrl+N, F7…)
        // closes it, and runs on what shows again. Search (Ctrl+E) opens over it.
        let backstage = self.backstage.is_some();
        if self.shortcut_keys(ctx) && backstage {
            self.close_backstage();
        }
    }

    /// the desktop notebook's page, section and history keys. They work while typing too, except
    /// Alt+Shift+↑ / ↓, which move paragraphs in a note. Returns whether one ran.
    fn navigation_keys(&mut self, ctx: &egui::Context, typing: bool) -> bool {
        use crate::util::consume_exact as pressed;
        if self.reader.open {
            return false;
        }
        let ctrl = Modifiers::COMMAND;
        if pressed(ctx, ctrl, Key::PageUp) {
            self.open_listed(|_, at| Some(at.map_or(0, |at| at.saturating_sub(1))));
            return true;
        }
        if pressed(ctx, ctrl, Key::PageDown) {
            self.open_listed(|count, at| {
                Some(at.map_or(0, |at| (at + 1).min(count.saturating_sub(1))))
            });
            return true;
        }
        if pressed(ctx, Modifiers::ALT, Key::Home) {
            self.open_listed(|_, _| Some(0));
            return true;
        }
        if pressed(ctx, Modifiers::ALT, Key::End) {
            self.open_listed(|count, _| count.checked_sub(1));
            return true;
        }
        if pressed(ctx, Modifiers::ALT, Key::PageUp) {
            self.open_in_view(false);
            return true;
        }
        if pressed(ctx, Modifiers::ALT, Key::PageDown) {
            self.open_in_view(true);
            return true;
        }
        // Ctrl on every system: ⌘Tab switches applications on macOS.
        if pressed(ctx, Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
            self.step_section(false);
            return true;
        }
        if pressed(ctx, Modifiers::CTRL, Key::Tab) {
            self.step_section(true);
            return true;
        }
        // macOS keeps ⌥← / ⌥→ for moving by word.
        if cfg!(not(target_os = "macos")) {
            if pressed(ctx, Modifiers::ALT, Key::ArrowLeft) {
                self.walk_visited(true);
                return true;
            }
            if pressed(ctx, Modifiers::ALT, Key::ArrowRight) {
                self.walk_visited(false);
                return true;
            }
        }
        if !typing {
            for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
                if pressed(ctx, Modifiers::ALT | Modifiers::SHIFT, key) {
                    if let Some(id) = self
                        .page
                        .as_ref()
                        .filter(|page| !page.trashed)
                        .map(|page| page.id.clone())
                    {
                        self.handle_page_list_action(PageListAction::Step(id, up));
                    }
                    return true;
                }
            }
        }
        false
    }

    /// The app's keyboard shortcuts outside the note. Returns `true` when one ran that
    /// works on the page or its panes, which is every one but search.
    fn shortcut_keys(&mut self, ctx: &egui::Context) -> bool {
        let mods = Modifiers::COMMAND;
        let typing = self.editor.text_has_focus(ctx);
        // the desktop notebook's page shortcuts. They come first because a shorter shortcut such as
        // Ctrl+N would also match Ctrl+Alt+N.
        if let Some(id) = self
            .page
            .as_ref()
            .filter(|page| !page.trashed)
            .map(|page| page.id.clone())
        {
            let alt = mods | Modifiers::ALT;
            // Move or Copy the page (the desktop notebook's Ctrl+Alt+M).
            if consume_alt_shortcut(ctx, alt, Key::M) {
                self.open_move_or_copy(MoveWhat::Page(id));
                return true;
            }
            if consume_alt_shortcut(ctx, alt | Modifiers::SHIFT, Key::N) {
                self.handle_page_list_action(PageListAction::NewBelow(id, true));
                return true;
            }
            if consume_alt_shortcut(ctx, alt, Key::N) {
                self.handle_page_list_action(PageListAction::NewBelow(id, false));
                return true;
            }
            if consume_alt_shortcut(ctx, alt, Key::CloseBracket) {
                self.handle_page_list_action(PageListAction::Level(id, 1));
                return true;
            }
            if consume_alt_shortcut(ctx, alt, Key::OpenBracket) {
                self.handle_page_list_action(PageListAction::Level(id, -1));
                return true;
            }
        }
        if self.navigation_keys(ctx, typing) {
            return true;
        }
        // the desktop notebook searches with Ctrl+E. Ctrl+K inserts a link while typing in a note
        // (the editor handles it) and searches everywhere else. While typing, Ctrl+Alt+E
        // (which shortcut matching would take for Ctrl+E) is left to the note: in a table
        // it inserts a column on the left, and AltGr+E types € on many layouts.
        let ctrl_e = ctx.input_mut(|input| {
            let with_alt = input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { key: Key::E, pressed: true, modifiers, .. }
                        if modifiers.alt
                )
            });
            !(typing && with_alt) && input.consume_shortcut(&KeyboardShortcut::new(mods, Key::E))
        });
        if ctrl_e
            || (!typing
                && ctx.input_mut(|input| {
                    input.consume_shortcut(&KeyboardShortcut::new(mods, Key::K))
                }))
        {
            self.open_search();
            return false;
        }
        let shifted = |key: Key| pressed_with_shift(ctx, key);
        // While typing in a note, Ctrl+Shift+N clears formatting (the desktop notebook); the editor
        // handles it. Anywhere else (the page title, a filter box) it is "New notebook".
        if !(shifted(Key::N) && typing) {
            if ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(mods | Modifiers::SHIFT, Key::N))
            }) {
                self.open_new_notebook();
                return true;
            }
            if ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::N))) {
                self.new_page();
                return true;
            }
        }
        // Ctrl+Shift+T goes to the page title and selects it, as in the desktop notebook.
        if shifted(Key::T)
            && !pressed_with_alt(ctx, Key::T)
            && ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(mods | Modifiers::SHIFT, Key::T))
            })
        {
            if self
                .page
                .as_ref()
                .is_some_and(|page| !page.trashed && !page.title_hidden)
            {
                self.editor.focus_title(true);
            }
            return true;
        }
        // Ctrl+Shift+R turns the page's rule lines off, or back on as they were (College
        // when it never had any), as in the desktop notebook.
        if shifted(Key::R)
            && !pressed_with_alt(ctx, Key::R)
            && ctx.input_mut(|input| {
                input.consume_shortcut(&KeyboardShortcut::new(mods | Modifiers::SHIFT, Key::R))
            })
        {
            if let Some(page) = self.page.as_ref().filter(|page| !page.trashed) {
                let rule = if page.rule == RuleLines::None {
                    self.last_rule
                        .as_ref()
                        .filter(|(id, _)| *id == page.id)
                        .map(|(_, rule)| *rule)
                        .or(self.new_page_rule)
                        .unwrap_or(RuleLines::Lines(RuleSize::College))
                } else {
                    self.last_rule = Some((page.id.clone(), page.rule));
                    RuleLines::None
                };
                let now = ctx.input(|input| input.time);
                self.run_cmd(Cmd::RuleLines(rule), ctx, now);
            }
            return true;
        }
        // New section (Ctrl+T), as in the desktop notebook: in the selected notebook, beside the
        // selected section in its group. Ctrl+Shift+T goes to the title (above), and
        // Ctrl+Alt+T, which Windows reports for AltGr+T typing, is left alone.
        if !shifted(Key::T)
            && !pressed_with_alt(ctx, Key::T)
            && ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::T)))
        {
            let Some(notebook) = self.selected_notebook.clone() else {
                return false;
            };
            let group = self
                .workspace
                .notebooks
                .iter()
                .find(|candidate| candidate.id == notebook)
                .and_then(|notebook| {
                    notebook
                        .sections
                        .iter()
                        .find(|section| Some(&section.id) == self.selected_section.as_ref())
                })
                .and_then(|section| section.group_id.clone());
            self.handle_library_action(LibraryAction::NewSectionIn(notebook, group));
            return true;
        }
        // View › New Quick Note (Ctrl+Shift+M), as in the desktop notebook.
        if ctx.input_mut(|input| {
            input.consume_shortcut(&KeyboardShortcut::new(mods | Modifiers::SHIFT, Key::M))
        }) {
            self.new_quick_note();
            return true;
        }
        if ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::S))) {
            self.save_now();
            if !self.dirty {
                self.toast(t!("toast-page-saved"));
            }
            return true;
        }
        // File › Print (Ctrl+P), as in the desktop notebook: the open page's print view.
        if ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::P))) {
            self.editor_action(EditorAction::PrintPage);
            return true;
        }
        // Review › Spelling (F7), as in the desktop notebook.
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F7)) {
            self.spelling();
            return true;
        }
        // Full Page View (F11) and Show tabs only (Ctrl+F1), as in the desktop notebook.
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::F11)) {
            self.set_full_page(!self.full_page);
            return true;
        }
        if ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::F1))) {
            self.ribbon.collapsed = !self.ribbon.collapsed;
            self.ribbon.overlay = false;
            if let Err(error) = self.ribbon.save_collapsed(&mut self.library) {
                self.error(t!("error-save-ribbon", error = error.to_string()));
            }
            return true;
        }
        // Find on this page, as in the desktop notebook.
        if ctx.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::F))) {
            if !self.reader.open {
                if let Some(page) = self.page.as_ref() {
                    let prefill = canvas::selected_text(page, &self.canvas, &self.editor);
                    self.canvas.open_find(prefill);
                }
            }
            return true;
        }
        if !shifted(Key::Comma)
            && ctx
                .input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(mods, Key::Comma)))
        {
            self.dialog = Some(Dialog::Settings);
            return true;
        }
        // Esc leaves Full Page View; an open menu takes it first.
        if self.full_page
            && !egui::Popup::is_any_open(ctx)
            && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.set_full_page(false);
            return true;
        }
        false
    }

    fn draw(&mut self, root: &mut Ui, t: Tokens, now: f64) {
        let ctx = root.ctx().clone();
        // Who has the keyboard as the frame begins: a press elsewhere only takes it from
        // the text later in the frame, as the text is drawn.
        if ctx.memory(|memory| memory.focused().is_some()) {
            self.keyboard_in_note = self.editor.text_has_focus(&ctx);
        }
        // A menu open as the frame begins takes Esc for itself (see below).
        let menu_was_open = egui::Popup::is_any_open(&ctx);
        self.handle_global_keys(&ctx);
        self.tick_save(now);
        self.tick_spelling(&ctx);
        self.tick_recording(&ctx, now);
        if ctx.input(|input| input.viewport().close_requested())
            && !self.flush_save()
            && !self.close_anyway
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_anyway = true;
            self.error(t!("error-close-unsaved"));
        }
        if self.list_dirty {
            self.refresh_page_list();
        }

        egui::Panel::top("folio-header")
            .exact_size(48.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(t.bg))
            .show(root, |ui| self.header_strip(ui, t));
        self.ribbon.full_page = self.full_page;
        // File › Backstage covers the window below the header strip: the ribbon, the panes
        // and the page give way to its left column and its page.
        if self.backstage.is_some() {
            self.backstage_view(root, &ctx, t);
        } else {
            let host = ShellHost::new(
                self.page.as_ref(),
                &self.canvas,
                &self.editor,
                self.view == View::Trash,
                self.full_page,
                self.ribbon.layout,
                self.new_page_rule.is_some(),
            )
            .with_library(
                self.workspace.trash_count,
                self.selected_section.is_some(),
                self.selected_notebook.is_some(),
            )
            .with_view(self.tabs_layout, self.switch_background)
            .with_recent_symbols(&self.recent_symbols);
            let picked = egui::Panel::top("folio-ribbon")
                .exact_size(ribbon::panel_height(&self.ribbon))
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(t.bg))
                .show(root, |ui| ribbon::show(ui, t, &mut self.ribbon, &host))
                .inner;
            drop(host);
            // A click on the ribbon that ran nothing (a tab, empty band, a greyed control)
            // leaves the caret in the note, as in the desktop notebook: give back the keyboard egui took
            // from the text at the press.
            if self.ribbon.idle_click && self.keyboard_in_note && self.text_state().is_some() {
                self.editor.restore_focus();
            }
            if let Some(cmd) = picked {
                self.run_cmd(cmd, &ctx, now);
            }

            self.fit_task_pane(root.max_rect().width());
            // The task pane: a white card on the right, beside the page card, holding Find
            // Tags, the page's versions, spelling or a list of pages.
            if (self.tags_pane.is_some() || self.task_pane.is_some()) && !self.full_page {
                egui::Panel::right("folio-taskpane")
                    .default_size(TASKPANE_WIDTH)
                    .size_range(240.0..=480.0)
                    .resizable(true)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin {
                        left: 0,
                        right: 8,
                        top: 0,
                        bottom: 8,
                    }))
                    .show(root, |ui| {
                        widgets::card(t)
                            .inner_margin(Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_min_size(ui.available_size());
                                match self.task_pane {
                                    None => self.tags_panel(ui, t, now),
                                    Some(TaskPane::Results(_)) => self.results_panel(ui, t),
                                    Some(TaskPane::Versions(_)) => self.versions_panel(ui, t),
                                    Some(TaskPane::Spelling(_)) => self.spelling_panel(ui, t),
                                }
                            });
                    });
            }
            // View › Tabs Layout › Horizontal Tabs: the sections run as tabs over the page,
            // and the notebooks show as a pane only when pinned (D11).
            let horizontal = self.tabs_layout == TabsLayout::Horizontal;
            let nav_shown =
                self.show_library && !self.full_page && (!horizontal || self.notebooks_pinned);
            if nav_shown {
                let nav = egui::Panel::left("folio-nav")
                    .default_size(NAV_WIDTH)
                    .size_range(150.0..=360.0)
                    .resizable(true)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(t.panel))
                    .show(root, |ui| self.nav_pane(ui, t));
                self.nav_width = nav.response.rect.width();
            } else {
                self.nav_width = 0.0;
            }
            if horizontal && !self.full_page {
                // The selected tab reaches the panel's bottom edge, where the page starts.
                egui::Panel::top("folio-sectiontabs")
                    .exact_size(40.0)
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin {
                        left: 8,
                        right: 8,
                        top: 4,
                        bottom: 0,
                    }))
                    .show(root, |ui| self.section_tabs(ui, t));
            }
            let list_shown = self.show_pages && !self.full_page;
            // Horizontal Tabs puts the page list on the right of the page, unless Options
            // › Display moves it to the left.
            let list_right = list_shown && horizontal && !self.page_list_left;
            if list_shown {
                // The card keeps 8 px from the window's edges (and from the task pane), and
                // sits flush against the navigation pane when it shows. In Horizontal Tabs
                // it meets the page card on either side, as the desktop notebook's page tabs do (and
                // keeps the width "Add page" needs in French).
                let (panel, left, right) = if list_right {
                    (egui::Panel::right("folio-pagelist"), 0, 8)
                } else {
                    (
                        egui::Panel::left("folio-pagelist"),
                        if nav_shown { 0 } else { 8 },
                        if horizontal { 0 } else { 8 },
                    )
                };
                let tint = self.section_tint(t);
                let list = panel
                    .default_size(PAGE_LIST_WIDTH)
                    .size_range(140.0..=400.0)
                    .resizable(true)
                    .show_separator_line(false)
                    .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin {
                        left,
                        right,
                        top: 0,
                        bottom: 8,
                    }))
                    .show(root, |ui| self.page_list_card(ui, &ctx, t, tint));
                self.page_list_width = list.response.rect.width();
            } else {
                self.page_list_width = 0.0;
                // No rows in view: Alt+PgUp / Alt+PgDn go to the list's first or last.
                self.pages_in_view = None;
            }
            // Esc that closed a menu (the ribbon's, a pane's) only closes it, as in the desktop notebook:
            // the note keeps the caret, which egui dropped on Esc. The page's own menus,
            // still open here, take their Esc in the canvas.
            if menu_was_open
                && !egui::Popup::is_any_open(&ctx)
                && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
            {
                self.keep_caret();
            }
            // The page card keeps 8 px from the window's edges, and sits flush against the
            // page list or navigation pane beside it.
            let beside = (list_shown && !list_right) || nav_shown;
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin {
                    left: if beside { 0 } else { 8 },
                    right: if list_right { 0 } else { 8 },
                    top: 0,
                    bottom: 8,
                }))
                .show(root, |ui| {
                    self.page_card(ui, &ctx, t, now);
                });
            // A pick in the text menu's mini toolbar runs as its ribbon twin does.
            if let Some(cmd) = self.editor.menu_cmd.take() {
                self.run_cmd(Cmd::Text(cmd), &ctx, now);
            }
            // Its Font Color › More Colors… opens the ribbon's dialog.
            if std::mem::take(&mut self.editor.more_font_colors_request) {
                self.run_cmd(Cmd::MoreFontColors, &ctx, now);
            }
        }
        paint_drag_ghost(&ctx, t);
        // Find tags, from the ribbon button or a Tag menu.
        if std::mem::take(&mut self.canvas.open_find_tags)
            | std::mem::take(&mut self.editor.find_tags_request)
        {
            self.open_tags_pane();
            ctx.request_repaint();
        }
        // Customize Tags…, from the Tags gallery's ▾ or a Tag menu.
        if std::mem::take(&mut self.editor.customize_tags_request) {
            self.open_customize_tags();
        }

        self.search_dropdown(&ctx, t);
        self.poll_notebook_import(&ctx);
        self.dialog_modal(&ctx, t);
        if let Some(page) = self.page.as_ref() {
            if let reading::ReaderOutcome::Toast(message) =
                reading::show(&ctx, page, &mut self.reader, t)
            {
                self.toast(message);
            }
        }
        self.show_toast(&ctx, t);
        if let Some(text) = self.pending_copy.take() {
            ctx.copy_text(text);
        }

        if self.dirty {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        if self.toast.expires_at > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(path) = self.screenshot_path.clone() {
            self.frames += 1;
            let started = *self
                .screenshot_started
                .get_or_insert_with(std::time::Instant::now);
            let waited = started.elapsed().as_millis() as u64;
            if self.frames >= 4 && waited >= self.screenshot_wait_ms {
                self.screenshot_path = None;
                ctx.request_screenshot(move |image| {
                    let [width, height] = image.size;
                    let mut bytes = Vec::with_capacity(width * height * 4);
                    for pixel in &image.pixels {
                        let [r, g, b, a] = pixel.to_srgba_unmultiplied();
                        bytes.extend_from_slice(&[r, g, b, a]);
                    }
                    match image::save_buffer(
                        &path,
                        &bytes,
                        width as u32,
                        height as u32,
                        image::ColorType::Rgba8,
                    ) {
                        Ok(()) => eprintln!("screenshot saved to {}", path.display()),
                        Err(error) => eprintln!("screenshot failed: {error}"),
                    }
                    std::process::exit(0);
                });
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// File › Backstage, below the header strip: its left column in the desktop notebook's purple, as
    /// Office's colourful theme paints it, and the page picked there on a white card.
    fn backstage_view(&mut self, root: &mut Ui, ctx: &egui::Context, t: Tokens) {
        egui::Panel::left("folio-backstage-nav")
            .exact_size(BACKSTAGE_NAV_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme::backstage_fill(t)))
            .show(root, |ui| self.backstage_nav(ui, ctx, t));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin {
                left: 0,
                right: 8,
                top: 0,
                bottom: 8,
            }))
            .show(root, |ui| {
                widgets::card(t)
                    .inner_margin(Margin::symmetric(32, 24))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        // ← Back may have closed it this frame.
                        let Some(page) = self.backstage.as_ref().map(|backstage| backstage.page)
                        else {
                            return;
                        };
                        ScrollArea::vertical()
                            .id_salt("backstage-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| match page {
                                BackstagePage::Info => self.backstage_info(ui, t),
                                BackstagePage::New => self.backstage_new(ui, t),
                                BackstagePage::Open => self.backstage_open(ui, t),
                                BackstagePage::Print => self.backstage_print(ui, t),
                                BackstagePage::Export => self.backstage_export(ui, t),
                            });
                    });
            });
    }

    /// Backstage's left column: ← Back, its pages, then Options (which opens over
    /// Backstage, as in Office) and Close (which quits Folio).
    fn backstage_nav(&mut self, ui: &mut Ui, ctx: &egui::Context, t: Tokens) {
        let Some(current) = self.backstage.as_ref().map(|backstage| backstage.page) else {
            return;
        };
        // Office's colourful Backstage: white text on the app's colour, a darker fill on the
        // page shown, a lighter one under the pointer, white rules.
        let t = Tokens {
            ink: Color32::WHITE,
            muted: Color32::WHITE,
            hover: Color32::from_white_alpha(28),
            nav_selected: Color32::from_black_alpha(56),
            accent: Color32::WHITE,
            line: Color32::from_white_alpha(70),
            // ← Back's fill while pressed.
            sunken: Color32::from_black_alpha(56),
            ..t
        };
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
        let back = Rect::from_min_size(
            pos2(row.left() + 12.0, row.center().y - 18.0),
            egui::Vec2::splat(36.0),
        );
        let back = ui
            .scope_builder(egui::UiBuilder::new().max_rect(back), |ui| {
                widgets::icon_button_sized(
                    ui,
                    icons::ARROW_LEFT,
                    &t!("app-backstage-back"),
                    36.0,
                    18.0,
                    t,
                )
            })
            .inner;
        if back.clicked() {
            self.close_backstage();
            return;
        }
        let mut picked = None;
        for page in BackstagePage::ALL {
            if backstage_item(ui, &page.label(), Some(page == current), t).clicked() {
                picked = Some(page);
            }
        }
        ui.add_space(8.0);
        let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(
            Rect::from_min_max(
                pos2(rule.left() + 16.0, rule.top()),
                pos2(rule.right() - 16.0, rule.bottom()),
            ),
            CornerRadius::ZERO,
            t.line,
        );
        ui.add_space(8.0);
        if backstage_item(ui, &t!("app-backstage-options"), None, t).clicked() {
            self.dialog = Some(Dialog::Settings);
        }
        if backstage_item(ui, &t!("app-backstage-close"), None, t)
            .on_hover_text(t!("app-backstage-close-tip"))
            .clicked()
        {
            // `on_exit` saves the page on the way out.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(page) = picked.filter(|&page| page != current) {
            if let Some(backstage) = self.backstage.as_mut() {
                backstage.page = page;
                backstage.focus = page == BackstagePage::New;
                // Info counts the pages again when it shows.
                backstage.page_counts = None;
            }
        }
    }

    /// Backstage › Info: each notebook with what it holds and its Properties…, then Save &
    /// Backup, drawn by the same fn as Options › Save & Backup.
    fn backstage_info(&mut self, ui: &mut Ui, t: Tokens) {
        backstage_heading(ui, &BackstagePage::Info.label(), t);
        let width = ui.available_width().min(BACKSTAGE_COLUMN);
        let counts = self.backstage_page_counts();
        ui.label(
            RichText::new(t!("app-info-notebooks"))
                .font(theme::semibold(14.0))
                .color(t.ink),
        );
        ui.add_space(4.0);
        let properties = t!("app-notebook-properties");
        // A secondary button is as wide as a primary one with the same label.
        let button = widgets::primary_button_width(ui, None, &properties);
        let mut library_action = None;
        for notebook in &self.workspace.notebooks {
            let (row, _) = ui.allocate_exact_size(vec2(width, 44.0), Sense::hover());
            let painter = ui.painter();
            painter.text(
                pos2(row.left(), row.center().y),
                Align2::LEFT_CENTER,
                icons::NOTEBOOK_FILLED,
                theme::icon_filled(20.0),
                theme::color(&notebook.color),
            );
            let left = row.left() + 30.0;
            let room = row.right() - button - 16.0 - left;
            widgets::fitted_text(
                painter,
                pos2(left, row.center().y - 8.0),
                Align2::LEFT_CENTER,
                notebook.name.as_str(),
                theme::semibold(13.0),
                t.ink,
                room,
            );
            let pages = counts.get(&notebook.id).copied().unwrap_or(0);
            widgets::fitted_text(
                painter,
                pos2(left, row.center().y + 9.0),
                Align2::LEFT_CENTER,
                format!(
                    "{} · {}",
                    t!("app-section-count", count = notebook.sections.len()),
                    t!("app-page-count", count = pages)
                ),
                theme::ui(12.0),
                t.muted,
                room,
            );
            let clicked = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(row)
                        .layout(Layout::right_to_left(Align::Center)),
                    |ui| widgets::secondary_button(ui, None, &properties, t).clicked(),
                )
                .inner;
            if clicked {
                // Notebook Properties opens over Backstage.
                library_action = Some(LibraryAction::RenameNotebook(notebook.id.clone()));
            }
        }
        ui.add_space(24.0);
        ui.label(
            RichText::new(t!("app-options-save"))
                .font(theme::semibold(14.0))
                .color(t.ink),
        );
        ui.add_space(4.0);
        let mut action = None;
        ui.scope(|ui| {
            ui.set_max_width(width);
            self.options_save_contents(ui, t, &mut action);
        });
        if let Some(action) = library_action {
            self.handle_library_action(action);
        }
        if let Some(action) = action {
            let ctx = ui.ctx().clone();
            self.apply_dialog_action(action, &ctx);
        }
    }

    /// How many pages each notebook holds (by id), counted once while Info shows.
    fn backstage_page_counts(&mut self) -> HashMap<String, usize> {
        if let Some(counts) = self
            .backstage
            .as_ref()
            .and_then(|backstage| backstage.page_counts.clone())
        {
            return counts;
        }
        let mut counts = HashMap::new();
        for page in self
            .library
            .page_list(PageFilter::All, "", i64::MAX)
            .unwrap_or_default()
        {
            *counts.entry(page.notebook_id).or_insert(0) += 1;
        }
        if let Some(backstage) = self.backstage.as_mut() {
            backstage.page_counts = Some(counts.clone());
        }
        counts
    }

    /// Backstage › New: a notebook named and coloured as in the New notebook dialog. An
    /// empty name is fine: the library names it as the dialog's would be.
    fn backstage_new(&mut self, ui: &mut Ui, t: Tokens) {
        backstage_heading(ui, &t!("common-new-notebook"), t);
        let width = ui.available_width().min(BACKSTAGE_COLUMN);
        let Some(backstage) = self.backstage.as_mut() else {
            return;
        };
        let mut create = false;
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.label(
                RichText::new(t!("app-new-notebook-body"))
                    .font(theme::ui(13.0))
                    .color(t.ink),
            );
            ui.add_space(16.0);
            create = name_field(
                ui,
                &mut backstage.name,
                "backstage-notebook-name",
                &t!("dialog-notebook-name"),
                backstage.focus,
                t,
            );
            // The click that opened New can take the focus back as it lands: ask again
            // until the field has it.
            backstage.focus &= ui.memory(|memory| memory.focused().is_none());
            color_field(ui, &mut backstage.color, t);
            ui.add_space(16.0);
            create |= widgets::primary_button(ui, None, &t!("dialog-create-notebook"), t).clicked();
        });
        if create {
            let action = DialogAction::CreateNotebook {
                name: backstage.name.trim().to_owned(),
                color: backstage.color.clone(),
            };
            let ctx = ui.ctx().clone();
            // The new notebook's first section opens, as from the dialog.
            self.apply_dialog_action(action, &ctx);
            self.close_backstage();
        }
    }

    /// Backstage › Open: importing from the desktop notebook, then every notebook (a click opens its first
    /// section, as in the navigation pane), then the folder the notes are kept in.
    fn backstage_open(&mut self, ui: &mut Ui, t: Tokens) {
        backstage_heading(ui, &BackstagePage::Open.label(), t);
        let width = ui.available_width().min(BACKSTAGE_COLUMN);
        let mut picked = None;
        let mut open_folder = false;
        let mut import = false;
        ui.scope(|ui| {
            ui.set_max_width(width);
            egui::Frame::new()
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(t!("app-open-import-body"))
                            .font(theme::ui(13.0))
                            .color(t.muted),
                    );
                    ui.add_space(10.0);
                    import = widgets::secondary_button(
                        ui,
                        Some(icons::ARROW_IMPORT),
                        &t!("app-open-import"),
                        t,
                    )
                    .clicked();
                });
            ui.add_space(24.0);
            ui.label(
                RichText::new(t!("app-open-recent"))
                    .font(theme::semibold(14.0))
                    .color(t.ink),
            );
            ui.add_space(4.0);
            for notebook in &self.workspace.notebooks {
                let response = nav_row(ui, 36.0, &notebook.name, false, Sense::click(), t);
                let rect = response.rect;
                let painter = ui.painter();
                painter.text(
                    pos2(rect.left() + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    icons::NOTEBOOK_FILLED,
                    theme::icon_filled(18.0),
                    theme::color(&notebook.color),
                );
                widgets::fitted_text(
                    painter,
                    pos2(rect.left() + 42.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    notebook.name.as_str(),
                    theme::ui(13.0),
                    t.ink,
                    rect.width() - 50.0,
                );
                if response.clicked() {
                    picked = Some(notebook.id.clone());
                }
            }
            ui.add_space(16.0);
            open_folder =
                widgets::text_button(ui, Some(icons::FOLDER_OPEN), &t!("app-open-folder"), t)
                    .clicked();
        });
        if import {
            // Backstage closes first, so Esc then closes only the import dialog.
            self.close_backstage();
            self.open_notebook_import();
        }
        if open_folder {
            let ctx = ui.ctx().clone();
            self.apply_dialog_action(DialogAction::OpenDataFolder, &ctx);
        }
        if let Some(id) = picked {
            // Opening a notebook unfolds it in the navigation pane.
            self.collapsed_notebooks.remove(&id);
            let first = self
                .workspace
                .notebooks
                .iter()
                .find(|notebook| notebook.id == id)
                .and_then(|notebook| notebook.sections.first())
                .map(|section| section.id.clone());
            if let Some(section) = first {
                self.handle_library_action(LibraryAction::SelectSection(section));
            }
            self.close_backstage();
        }
    }

    /// Backstage › Print: the open page's print view, in the web browser.
    fn backstage_print(&mut self, ui: &mut Ui, t: Tokens) {
        backstage_heading(ui, &BackstagePage::Print.label(), t);
        let width = ui.available_width().min(BACKSTAGE_COLUMN);
        let open = self.page.is_some();
        let mut print = false;
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.label(
                RichText::new(t!("app-print-body"))
                    .font(theme::ui(13.0))
                    .color(t.ink),
            );
            ui.add_space(16.0);
            if !open {
                ui.label(
                    RichText::new(t!("app-print-no-page"))
                        .font(theme::ui(13.0))
                        .color(t.muted),
                );
                ui.add_space(8.0);
            }
            print = ui
                .add_enabled_ui(open, |ui| {
                    widgets::primary_button(ui, Some(icons::PRINTER), &t!("app-print-button"), t)
                })
                .inner
                .clicked();
        });
        if print {
            self.editor_action(EditorAction::PrintPage);
            self.close_backstage();
        }
    }

    /// Backstage › Export, in Office's two columns: what to export (the open page, its
    /// section or its notebook), then the format and Export….
    fn backstage_export(&mut self, ui: &mut Ui, t: Tokens) {
        backstage_heading(ui, &BackstagePage::Export.label(), t);
        let Some(scope) = self.backstage.as_ref().map(|backstage| backstage.export) else {
            return;
        };
        let (page, section, notebook) = (
            self.page.is_some(),
            self.selected_section.is_some(),
            self.selected_notebook.is_some(),
        );
        let enabled = |scope: ExportScope| match scope {
            ExportScope::Page => page,
            ExportScope::Section => section,
            ExportScope::Notebook => notebook,
        };
        let format_width = (ui.available_width() - EXPORT_SCOPE_WIDTH - 33.0).clamp(200.0, 360.0);
        let mut picked = None;
        let mut export = false;
        let row = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let scopes = ui.vertical(|ui| {
                ui.set_width(EXPORT_SCOPE_WIDTH);
                export_column_heading(ui, &t!("app-export-current"), t);
                for each in ExportScope::ALL {
                    if export_row(
                        ui,
                        each.glyph(),
                        &each.label(),
                        each == scope,
                        enabled(each),
                        t,
                    )
                    .clicked()
                    {
                        picked = Some(each);
                    }
                }
            });
            ui.add_space(33.0);
            ui.vertical(|ui| {
                ui.set_width(format_width);
                export_column_heading(ui, &t!("app-export-format"), t);
                // Web pages are the one format Folio writes.
                export_row(
                    ui,
                    icons::CODE,
                    &t!("app-export-html-format"),
                    true,
                    true,
                    t,
                );
                if scope != ExportScope::Page {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(t!("app-export-folder-body"))
                            .font(theme::ui(13.0))
                            .color(t.muted),
                    );
                }
                ui.add_space(20.0);
                export = ui
                    .add_enabled_ui(enabled(scope), |ui| {
                        widgets::primary_button(
                            ui,
                            Some(icons::ARROW_EXPORT),
                            &t!("app-export-button"),
                            t,
                        )
                    })
                    .inner
                    .clicked();
            });
            scopes.response.rect.right()
        });
        // Office's hairline between the two columns.
        let x = row.inner + 16.0;
        ui.painter()
            .vline(x, row.response.rect.y_range(), Stroke::new(1.0, t.line));
        if let Some(scope) = picked {
            if let Some(backstage) = self.backstage.as_mut() {
                backstage.export = scope;
            }
        }
        if export && self.export_scope(scope) {
            self.close_backstage();
        }
    }

    /// Whether the search box sits at the right end of the section tabs, which show in
    /// Horizontal Tabs (outside Full Page View and Backstage): the header then leaves it out.
    fn search_in_tab_row(&self) -> bool {
        self.tabs_layout == TabsLayout::Horizontal && !self.full_page && self.backstage.is_none()
    }

    /// the desktop notebook's strip above the ribbon: Folio's mark and the page title on the left, the
    /// search box in the middle, and Settings on the right (until File › Options). In
    /// Horizontal Tabs the search box sits at the right end of the section tabs instead
    /// (`section_tabs`), as the desktop notebook's does.
    fn header_strip(&mut self, ui: &mut Ui, t: Tokens) {
        let strip = ui.max_rect();
        let middle = strip.center().y;
        let gear = Rect::from_min_size(
            pos2(strip.right() - 8.0 - 32.0, middle - 16.0),
            egui::Vec2::splat(32.0),
        );
        let in_header = !self.search_in_tab_row();
        self.search_in_header = in_header;
        // The search box: 470 px, but never more than a third of the window, centred.
        let search = Rect::from_center_size(
            pos2(strip.center().x, middle),
            vec2(HEADER_SEARCH_WIDTH.min(strip.width() / 3.0), 32.0),
        );
        let painter = ui.painter().clone();
        let mark = Rect::from_min_size(
            pos2(strip.left() + 12.0, middle - 10.0),
            egui::Vec2::splat(20.0),
        );
        paint_mark(&painter, mark);
        let title = self
            .page
            .as_ref()
            .map_or(Cow::Borrowed(APP_NAME), |page| display_title(&page.title));
        let title_left = mark.right() + 10.0;
        let title_right = if in_header {
            search.left()
        } else {
            gear.left()
        };
        widgets::fitted_text(
            &painter,
            pos2(title_left, middle),
            Align2::LEFT_CENTER,
            title,
            theme::ui(13.0),
            t.ink,
            title_right - 16.0 - title_left,
        );

        if in_header {
            self.search.box_rect = Some(search);
            if self.search.open {
                self.search_field(ui, search, t);
            } else {
                self.search_box(ui, search, t);
            }
        }

        let settings = ui
            .scope_builder(egui::UiBuilder::new().max_rect(gear), |ui| {
                widgets::icon_button_sized(
                    ui,
                    icons::GEAR_SIX,
                    &t!("common-settings"),
                    32.0,
                    16.0,
                    t,
                )
            })
            .inner;
        if settings.clicked() {
            self.dialog = Some(Dialog::Settings);
        }
    }

    /// The header's search box while the search is open: the words being searched for,
    /// typed in place (the desktop notebook's search box), the scope chip at its right
    /// (`scope_chip`), and the dropdown under it (`search_dropdown`).
    fn search_field(&mut self, ui: &mut Ui, search: Rect, t: Tokens) {
        let painter = ui.painter().clone();
        let middle = search.center().y;
        painter.rect_filled(search, CornerRadius::same(6), t.raised);
        painter.rect_stroke(
            search,
            CornerRadius::same(6),
            Stroke::new(1.0, t.accent),
            StrokeKind::Inside,
        );
        painter.text(
            pos2(search.left() + 12.0, middle),
            Align2::LEFT_CENTER,
            icons::MAGNIFYING_GLASS,
            theme::icon(16.0),
            t.muted,
        );
        let chip = self.scope_chip(ui, search, t);
        let field = Rect::from_min_max(
            pos2(search.left() + 34.0, search.top() + 4.0),
            pos2(chip.left() - 6.0, search.bottom() - 4.0),
        );
        // Alt+O opens the results pane (`search_dropdown`): the "o" some platforms send
        // with it is not typed in the box.
        let alt_o = ui.ctx().input(|input| {
            input.events.iter().any(|event| {
                matches!(event, egui::Event::Key { key: Key::O, pressed: true, modifiers, .. }
                    if modifiers.matches_logically(Modifiers::ALT))
            })
        });
        if alt_o {
            ui.ctx().input_mut(|input| {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Text(_)));
            });
        }
        // ↓ / ↑ belong to the dropdown's rows: the box never sees them (↑ would move its
        // caret to the start). The scope menu, while open, keeps them.
        if !self.search.scope_menu {
            self.search.arrows = ui.ctx().input_mut(|input| {
                (
                    input.consume_key(Modifiers::NONE, Key::ArrowDown),
                    input.consume_key(Modifiers::NONE, Key::ArrowUp),
                )
            });
        }
        let response = ui.put(
            field,
            TextEdit::singleline(&mut self.search.query)
                .id(Id::unique("folio-search-field"))
                .frame(egui::Frame::NONE)
                .margin(Margin::ZERO)
                .desired_width(field.width())
                .align(Align2::LEFT_CENTER)
                .font(theme::ui(13.0))
                .text_color(t.ink)
                .hint_text(
                    RichText::new(t!("app-header-search"))
                        .font(theme::ui(13.0))
                        .color(t.faint),
                ),
        );
        if std::mem::take(&mut self.search.focus) {
            response.request_focus();
        }
    }

    /// The scope chip at the right of the open search box: the scope searched and a
    /// chevron (the desktop notebook's "Search notebooks ⌄"). It opens the scope menu (`scope_menu`).
    /// Returns its rect.
    fn scope_chip(&mut self, ui: &mut Ui, search: Rect, t: Tokens) -> Rect {
        let label = self.search.scope.label();
        let font = theme::ui(12.0);
        // Never more than half the box: French scope names are long.
        let text_width = widgets::text_width(ui.ctx(), &label, font.clone())
            .min((search.width() / 2.0 - 36.0).max(40.0));
        let chip = Rect::from_min_max(
            pos2(
                search.right() - 4.0 - (10.0 + text_width + 6.0 + 12.0 + 8.0),
                search.top() + 4.0,
            ),
            pos2(search.right() - 4.0, search.bottom() - 4.0),
        );
        let name = t!("app-search-scope");
        let response = ui
            .interact(chip, Id::unique("folio-search-scope"), Sense::click())
            .on_hover_text(name.as_ref());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        self.search.scope_menu = open;
        let painter = ui.painter();
        if open || response.hovered() {
            painter.rect_filled(chip, CornerRadius::same(4), t.hover);
        }
        let middle = chip.center().y;
        widgets::fitted_text(
            painter,
            pos2(chip.left() + 10.0, middle),
            Align2::LEFT_CENTER,
            label.as_ref(),
            font,
            t.muted,
            text_width + 1.0,
        );
        painter.text(
            pos2(chip.right() - 8.0, middle),
            Align2::RIGHT_CENTER,
            icons::CARET_DOWN,
            theme::icon(12.0),
            t.muted,
        );
        let pick = egui::Popup::menu(&response)
            .show(|ui| self.scope_menu(ui, t))
            .and_then(|shown| shown.inner);
        if let Some(pick) = pick {
            self.pick_scope(pick);
        }
        chip
    }

    /// The search's scope menu, as the desktop notebook's: This Page (find on page, Ctrl+F); This
    /// Section, This Section Group (greyed when the section has no group), This Notebook
    /// and All Notebooks, the current one checked; Set This Scope as Default.
    fn scope_menu(&self, ui: &mut Ui, t: Tokens) -> Option<ScopePick> {
        let page = t!("app-scope-page");
        let find = util::menu_shortcut(Modifiers::COMMAND, Key::F);
        let set_default = t!("app-scope-set-default");
        let scopes: Vec<(ScopeKind, Cow<'static, str>)> = ScopeKind::ALL
            .into_iter()
            .map(|scope| (scope, scope.label()))
            .collect();
        let hint = widgets::text_width(ui.ctx(), &find, theme::ui(11.5));
        let rows = scopes
            .iter()
            .map(|(_, label)| (None, label.as_ref()))
            .chain([(None, set_default.as_ref())]);
        ui.set_width(
            rows_width(ui, rows).max(rows_width(ui, [(None, page.as_ref())]) + hint + 24.0),
        );
        let mut pick = None;
        // A `menu_item` with its shortcut painted at the right, so its label lines up
        // with the scopes' below.
        let row = widgets::menu_item(ui, None, &page, t);
        ui.painter().text(
            row.rect.right_center() - vec2(10.0, 0.0),
            Align2::RIGHT_CENTER,
            &find,
            theme::ui(11.5),
            t.muted,
        );
        if row.clicked() {
            pick = Some(ScopePick::Page);
        }
        ui.separator();
        let grouped = self.search_group_sections().is_some();
        for (scope, label) in &scopes {
            let current = *scope == self.search.scope;
            let row = ui
                .add_enabled_ui(*scope != ScopeKind::Group || grouped, |ui| {
                    widgets::checked_menu_item(ui, None, label, current, t)
                })
                .inner;
            if row.clicked() {
                pick = Some(ScopePick::Scope(*scope));
            }
        }
        ui.separator();
        if widgets::menu_item(ui, None, &set_default, t).clicked() {
            pick = Some(ScopePick::SetDefault);
        }
        pick
    }

    /// The header's search box while the search is closed: a button that opens it, with
    /// its Ctrl+E hint.
    fn search_box(&mut self, ui: &mut Ui, search: Rect, t: Tokens) {
        let painter = ui.painter().clone();
        let middle = search.center().y;
        let label = t!("app-header-search");
        let response = ui
            .interact(search, Id::unique("folio-header-search"), Sense::click())
            .on_hover_text(t!("app-search-hint"));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        painter.rect_filled(search, CornerRadius::same(6), t.raised);
        painter.rect_stroke(
            search,
            CornerRadius::same(6),
            Stroke::new(
                1.0,
                if response.hovered() {
                    t.muted
                } else {
                    t.line_strong
                },
            ),
            StrokeKind::Inside,
        );
        painter.text(
            pos2(search.left() + 12.0, middle),
            Align2::LEFT_CENTER,
            icons::MAGNIFYING_GLASS,
            theme::icon(16.0),
            t.muted,
        );
        // The shortcut first, so the placeholder (longer in French) is cut beside it.
        let shortcut = painter.text(
            pos2(search.right() - 12.0, middle),
            Align2::RIGHT_CENTER,
            util::shortcut("E"),
            theme::ui(11.0),
            t.faint,
        );
        let text_left = search.left() + 36.0;
        widgets::fitted_text(
            &painter,
            pos2(text_left, middle),
            Align2::LEFT_CENTER,
            label.as_ref(),
            theme::ui(13.0),
            t.muted,
            shortcut.left() - 10.0 - text_left,
        );
        if response.clicked() {
            self.open_search();
        }
    }

    /// Runs what the ribbon gave this frame. The app's own commands run here; the page's
    /// go to the canvas, which runs them when it draws the page, later this frame.
    fn run_cmd(&mut self, cmd: Cmd, ctx: &egui::Context, now: f64) {
        match cmd {
            Cmd::PauseRecording => {
                if let Some((_, paused)) = self.canvas.recording_time() {
                    self.canvas.set_recording_paused(!paused);
                }
            }
            Cmd::StopRecording => self.stop_recording(now),
            Cmd::Reader => self.editor_action(EditorAction::OpenReader),
            Cmd::NormalView => self.set_full_page(false),
            Cmd::FullPageView => self.set_full_page(true),
            // Again closes the pane, as the desktop notebook's button does.
            Cmd::PageVersions => {
                if matches!(self.task_pane, Some(TaskPane::Versions(_))) {
                    self.close_task_pane();
                } else {
                    self.editor_action(EditorAction::Versions);
                }
            }
            Cmd::DeleteVersions(scope) => self.confirm_delete_versions(scope),
            Cmd::EmptyRecycleBin => self.confirm_empty_trash(),
            Cmd::RecentEdits(range) => self.open_recent_edits(range),
            Cmd::Spelling => {
                self.spelling();
                // With no misspelling to select, the caret stays in the note it was in.
                if !matches!(&self.task_pane, Some(TaskPane::Spelling(pane)) if pane.current.is_some())
                {
                    self.keep_caret();
                }
            }
            Cmd::RecycleBin => {
                if self.view == View::Trash {
                    // Again: back to the section of the open page, leaving a page from
                    // the bin for the section's own.
                    let page = self.page.as_ref();
                    let trashed = page.is_some_and(|page| page.trashed);
                    let section = page
                        .map(|page| page.section_id.clone())
                        .or_else(|| self.selected_section.clone());
                    if let Some(section) = section {
                        self.select_section(&section, trashed);
                    }
                } else {
                    self.select_view(View::Trash);
                }
            }
            Cmd::ProofingLanguage(code) => {
                self.apply_dialog_action(DialogAction::SetSpellLanguage(code), ctx)
            }
            Cmd::Shortcuts => self.dialog = Some(Dialog::Shortcuts),
            Cmd::About => self.dialog = Some(Dialog::About),
            Cmd::Backstage => self.open_backstage(BackstagePage::Info),
            Cmd::RibbonLayout(layout) => {
                self.ribbon.layout = layout;
                if let Err(error) = self.ribbon.save_layout(&mut self.library) {
                    self.error(t!("error-save-ribbon", error = error.to_string()));
                }
            }
            Cmd::TabsLayout(layout) => {
                self.tabs_layout = layout;
                if let Err(error) = self.library.set_setting("tabs_layout", layout.code()) {
                    self.error(t!("error-save-setting", error = error.to_string()));
                }
                // The panels around the page change: fit them to the window again.
                self.pane_fold.fitted_width = 0.0;
                // Only the panels move: the caret stays in the note.
                self.keep_caret();
            }
            Cmd::SwitchBackground => {
                self.switch_background = !self.switch_background;
                let value = if self.switch_background {
                    "true"
                } else {
                    "false"
                };
                if let Err(error) = self.library.set_setting("switch_background", value) {
                    self.error(t!("error-save-setting", error = error.to_string()));
                }
                // Only the page's colours change: the caret stays in the note.
                self.keep_caret();
            }
            Cmd::NewQuickNote => self.new_quick_note(),
            Cmd::CollapseRibbon(collapsed) => {
                self.ribbon.collapsed = collapsed;
                if let Err(error) = self.ribbon.save_collapsed(&mut self.library) {
                    self.error(t!("error-save-ribbon", error = error.to_string()));
                }
            }
            Cmd::PageColor(color) => {
                if let Some(page) = self.page.as_mut() {
                    page.color = color;
                    self.mark_dirty(now);
                }
                self.keep_caret();
            }
            // the desktop notebook's More Colors… opens a colour dialog, starting from the page's colour.
            Cmd::MorePageColors => {
                if let Some(page) = self.page.as_ref() {
                    let color = page
                        .color
                        .map_or(egui::Color32::WHITE, |color| color.light().to_color32());
                    let [r, g, b, _] = color.to_array();
                    let hex = Rgb(r, g, b).hex();
                    self.dialog = Some(Dialog::PageColor { color, hex });
                }
            }
            // Home › Font Color › More Colors…: from the colour of the text at the caret.
            Cmd::MoreFontColors => {
                let current = self
                    .page
                    .as_ref()
                    .and_then(|page| active_doc(page, &self.canvas))
                    .and_then(|doc| self.editor.shown_style(doc).color);
                self.open_more_colors(ColorTarget::Font, current.unwrap_or(Rgb(0, 0, 0)));
            }
            // Draw › pen ▾ › More Colors…: from the pen's colour.
            Cmd::MorePenColors => self.open_more_colors(ColorTarget::Pen, self.canvas.ink_color),
            Cmd::RuleLineColor(color) => {
                if let Some(page) = self.page.as_mut() {
                    page.rule_color = color;
                    self.mark_dirty(now);
                }
                self.keep_caret();
            }
            Cmd::RuleLines(rule) => {
                if let Some(page) = self.page.as_mut() {
                    page.rule = rule;
                    self.mark_dirty(now);
                }
                // With "always" on, new pages follow the latest choice of lines.
                if self.new_page_rule.is_some() && rule != RuleLines::None {
                    self.new_page_rule = Some(rule);
                    if let Err(error) = self.library.set_new_page_rule(Some(rule)) {
                        self.error(t!("error-save-setting", error = error.to_string()));
                    }
                }
                self.keep_caret();
            }
            Cmd::AlwaysRuleLines => {
                let next = match self.new_page_rule {
                    Some(_) => None,
                    None => Some(
                        self.page
                            .as_ref()
                            .map(|page| page.rule)
                            .filter(|rule| *rule != RuleLines::None)
                            .unwrap_or(RuleLines::Lines(RuleSize::College)),
                    ),
                };
                self.new_page_rule = next;
                if let Err(error) = self.library.set_new_page_rule(next) {
                    self.error(t!("error-save-setting", error = error.to_string()));
                }
                self.keep_caret();
            }
            Cmd::HidePageTitle => {
                if let Some(page) = self.page.as_mut() {
                    page.title_hidden = !page.title_hidden;
                    self.mark_dirty(now);
                }
                self.keep_caret();
            }
            // The page's canvas zooms, and the caret stays where it was.
            Cmd::ZoomOut | Cmd::ZoomIn | Cmd::Zoom100 | Cmd::Zoom(_) | Cmd::PageWidth => {
                self.run_page_cmd(cmd);
                self.keep_caret();
            }
            Cmd::PageTemplate(template) => self.new_page_from(template, now),
            // With nothing being typed, Find Tags still opens the pane.
            Cmd::Text(ToolbarCommand::Tag(TagCommand::FindTags)) if self.text_state().is_none() => {
                self.editor.find_tags_request = true;
            }
            Cmd::Text(ToolbarCommand::Tag(TagCommand::Customize))
                if self.text_state().is_none() =>
            {
                self.editor.customize_tags_request = true;
            }
            // The symbol joins Insert › Symbol's recent row, and goes in at the caret.
            Cmd::Symbol(symbol) => {
                self.remember_symbol(symbol);
                self.run_page_cmd(cmd);
            }
            Cmd::CustomizeNumbering => self.open_customize_numbering(),
            Cmd::ParagraphSpacing => {
                if let Some(spacing) = self.text_state().map(|text| text.spacing) {
                    self.dialog = Some(Dialog::ParagraphSpacing {
                        before: spacing.before,
                        after: spacing.after,
                        line: spacing.line_at_least,
                    });
                }
            }
            Cmd::MoreSymbols => {
                self.dialog = Some(Dialog::Symbols {
                    set: 0,
                    query: String::new(),
                    picked: None,
                });
                self.dialog_focus = true;
            }
            cmd => self.run_page_cmd(cmd),
        }
        self.ribbon.overlay = false;
    }

    /// After a ribbon command that leaves the text alone (the page's look, the zoom), the
    /// caret goes back into the note it was in: egui took the keyboard from the text at
    /// the press, and the desktop notebook keeps typing in the note.
    fn keep_caret(&mut self) {
        if self.keyboard_in_note && self.text_state().is_some() {
            self.editor.restore_focus();
        }
    }

    /// Customize Numbering…, filled from the paragraph at the caret: its numbering style
    /// (1. for a paragraph that is not numbered yet) and the number it shows.
    fn open_customize_numbering(&mut self) {
        let caret = self
            .page
            .as_ref()
            .and_then(|page| active_doc(page, &self.canvas))
            .and_then(|doc| {
                let index = self.editor.active_index(doc)?;
                let number = crate::model::list_numbers(&doc.blocks)[index];
                Some((doc.blocks[index].list_marker(), number))
            });
        let Some((marker, number)) = caret else {
            return;
        };
        self.dialog = Some(Dialog::CustomizeNumbering {
            marker: marker
                .filter(|marker| marker.numbered())
                .unwrap_or(ListMarker::Decimal),
            // Shown as it is, even past the field's highest: OK on it changes nothing.
            start: u32::try_from(number).unwrap_or(1).max(1),
        });
    }

    /// More Colors… for `target`, starting from `color`.
    fn open_more_colors(&mut self, target: ColorTarget, color: Rgb) {
        self.dialog = Some(Dialog::MoreColors {
            target,
            color: color.to_color32(),
            hex: color.hex(),
        });
    }

    /// A ribbon command for the open page: its canvas runs it.
    fn run_page_cmd(&mut self, cmd: Cmd) {
        if self.page.is_some() {
            self.canvas.ribbon_cmd = Some(cmd);
        }
    }

    /// The paragraph being typed in, as the ribbon shows it: in the note being edited.
    fn text_state(&self) -> Option<TextState> {
        let page = self.page.as_ref()?;
        editor::text_state(active_doc(page, &self.canvas)?, &self.editor)
    }

    /// the desktop notebook's navigation pane: the ≡ button, the notebooks with their sections and
    /// section groups, and Quick notes pinned at the bottom. A notebook's, group's or
    /// section's commands are on its right-click menu, as in the desktop notebook.
    fn nav_pane(&mut self, ui: &mut Ui, t: Tokens) {
        let mut action: Option<LibraryAction> = None;
        // Rows are measured to the pixel; the pane adds its own gaps.
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let quick = self.quick_section();
        let quick_active = self.view == View::Section
            && quick.is_some()
            && quick.as_deref() == self.selected_section.as_deref();
        // Quick notes is laid out first, from the bottom, so the notebooks' scroll area
        // stops above it.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            let label = t!("app-quick-notes");
            let row = nav_row(ui, 36.0, &label, quick_active, Sense::click(), t);
            let rect = row.rect;
            ui.painter().text(
                pos2(rect.left() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                icons::NOTE,
                theme::icon(16.0),
                t.text2,
            );
            widgets::fitted_text(
                ui.painter(),
                pos2(rect.left() + 38.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                theme::ui(13.0),
                t.ink,
                rect.width() - 46.0,
            );
            if row.clicked() {
                if let Some(section) = &quick {
                    action = Some(LibraryAction::SelectSection(section.clone()));
                }
            }
            ui.add_space(4.0);
            nav_divider(ui, t);
            ui.add_space(4.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 36.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_height(36.0);
                        ui.add_space(6.0);
                        // In Horizontal Tabs the pane is the notebook picker pinned to the
                        // side: its button unpins it.
                        if self.tabs_layout == TabsLayout::Horizontal {
                            if widgets::icon_button_sized(
                                ui,
                                icons::PIN_OFF,
                                &t!("app-unpin-notebooks"),
                                32.0,
                                16.0,
                                t,
                            )
                            .clicked()
                            {
                                self.set_notebooks_pinned(false);
                            }
                        } else if widgets::icon_button_sized(
                            ui,
                            icons::LIST,
                            &t!("app-hide-navigation"),
                            32.0,
                            16.0,
                            t,
                        )
                        .clicked()
                        {
                            self.show_library = false;
                            self.panels_toggled_by_hand(true);
                        }
                    },
                );
                ScrollArea::vertical()
                    .id_salt("nav-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let notebooks = self.workspace.notebooks.clone();
                        for (index, notebook) in notebooks.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(6.0);
                                nav_divider(ui, t);
                                ui.add_space(6.0);
                            }
                            let expanded = !self.collapsed_notebooks.contains(&notebook.id);
                            if self.notebook_row(ui, notebook, expanded, t, &mut action) {
                                // Opening a notebook unfolds it.
                                self.collapsed_notebooks.remove(&notebook.id);
                            }
                            if expanded {
                                self.section_tree(ui, notebook, None, 0, t, &mut action);
                                let label = t!("common-new-section");
                                let add = nav_row(ui, 28.0, &label, false, Sense::click(), t);
                                let rect = add.rect;
                                // The + sits on the sections' colour pills, the label
                                // under their names.
                                ui.painter().text(
                                    pos2(rect.left() + 27.0, rect.center().y),
                                    Align2::CENTER_CENTER,
                                    icons::PLUS,
                                    theme::icon(14.0),
                                    t.accent,
                                );
                                widgets::fitted_text(
                                    ui.painter(),
                                    pos2(rect.left() + 38.0, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    label,
                                    theme::ui(13.0),
                                    t.accent,
                                    rect.width() - 46.0,
                                );
                                if add.clicked() {
                                    action = Some(LibraryAction::NewSection(notebook.id.clone()));
                                }
                            }
                        }
                        // The empty area under the tree (at least the old 8 px under a
                        // long one) has the desktop notebook's menu for adding to the library.
                        let rest = ui.available_height().max(8.0);
                        let empty =
                            ui.allocate_response(vec2(ui.available_width(), rest), Sense::click());
                        empty.context_menu(|ui| {
                            let notebook = self.selected_notebook.as_deref();
                            if let Some(picked) = nav_area_menu(ui, notebook, t) {
                                action = Some(picked);
                            }
                        });
                    });
            });
        });
        self.scroll_nav_to_selected = false;
        if let Some(action) = action {
            self.handle_library_action(action);
        }
    }

    /// A notebook's row in the navigation pane: fold chevron, coloured book, name.
    /// Returns `true` when the row (not its chevron) was clicked, which opens the
    /// notebook's first section.
    fn notebook_row(
        &self,
        ui: &mut Ui,
        notebook: &crate::model::Notebook,
        expanded: bool,
        t: Tokens,
        action: &mut Option<LibraryAction>,
    ) -> bool {
        let response = nav_row(ui, 32.0, &notebook.name, false, Sense::click(), t);
        let rect = response.rect;
        // The chevron is its own hit area, made after the row so it wins the click.
        let fold = ui.interact(
            Rect::from_center_size(
                pos2(rect.left() + 14.0, rect.center().y),
                egui::Vec2::splat(20.0),
            ),
            Id::unique(("nav-fold-notebook", notebook.id.as_str())),
            Sense::click(),
        );
        fold.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, t!("app-fold-notebook"))
        });
        let empty = notebook.sections.is_empty() && notebook.groups.is_empty();
        let painter = ui.painter();
        painter.text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            if expanded && !empty {
                icons::CARET_DOWN
            } else {
                icons::CARET_RIGHT
            },
            theme::icon(12.0),
            if fold.hovered() { t.ink } else { t.muted },
        );
        painter.text(
            pos2(rect.left() + 24.0, rect.center().y),
            Align2::LEFT_CENTER,
            icons::NOTEBOOK_FILLED,
            theme::icon_filled(16.0),
            theme::color(&notebook.color),
        );
        widgets::fitted_text(
            painter,
            pos2(rect.left() + 46.0, rect.center().y),
            Align2::LEFT_CENTER,
            notebook.name.as_str(),
            theme::semibold(13.0),
            t.ink,
            rect.width() - 54.0,
        );
        let mut opened = false;
        if fold.clicked() {
            *action = Some(LibraryAction::ToggleNotebook(notebook.id.clone()));
        } else if response.clicked() {
            opened = true;
            if let Some(section) = notebook.sections.first() {
                *action = Some(LibraryAction::SelectSection(section.id.clone()));
            }
        }
        // A section dropped on its notebook moves out of its group.
        let same = |drag: &SectionDrag| drag.notebook_id == notebook.id;
        if response
            .dnd_hover_payload::<SectionDrag>()
            .is_some_and(|drag| same(&drag))
        {
            ui.painter().rect_filled(
                response.rect,
                CornerRadius::same(2),
                theme::with_alpha(t.accent, 46),
            );
        }
        if let Some(drag) = response
            .dnd_release_payload::<SectionDrag>()
            .filter(|drag| same(drag))
        {
            *action = Some(LibraryAction::MoveSection {
                id: drag.id.clone(),
                group_id: None,
                before: None,
            });
        }
        response.context_menu(|ui| {
            let notebooks = &self.workspace.notebooks;
            let first = notebooks
                .first()
                .is_some_and(|first| first.id == notebook.id);
            let last = notebooks.last().is_some_and(|last| last.id == notebook.id);
            if let Some(picked) = notebook_menu(ui, notebook, first, last, t) {
                *action = Some(picked);
            }
        });
        opened
    }

    /// Sections and section groups in `parent` (the notebook's top level for `None`):
    /// sections first, then groups, as the desktop notebook lists them.
    fn section_tree(
        &self,
        ui: &mut Ui,
        notebook: &crate::model::Notebook,
        parent: Option<&str>,
        depth: usize,
        t: Tokens,
        action: &mut Option<LibraryAction>,
    ) {
        let indent = depth as f32 * 14.0;
        let sections: Vec<&crate::model::Section> = notebook
            .sections
            .iter()
            .filter(|section| section.group_id.as_deref() == parent)
            .collect();
        for (index, section) in sections.iter().enumerate() {
            let next = sections.get(index + 1).map(|next| next.id.as_str());
            self.section_row(ui, notebook, section, next, indent, t, action);
        }
        for group in notebook
            .groups
            .iter()
            .filter(|group| group.parent_id.as_deref() == parent)
        {
            let expanded = !self.collapsed_groups.contains(&group.id);
            let response = nav_row(ui, 28.0, &group.name, false, Sense::click(), t);
            let rect = response.rect;
            let fold = ui.interact(
                Rect::from_center_size(
                    pos2(rect.left() + 14.0 + indent, rect.center().y),
                    egui::Vec2::splat(20.0),
                ),
                Id::unique(("nav-fold-group", group.id.as_str())),
                Sense::click(),
            );
            fold.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, t!("app-fold-section-group"))
            });
            let painter = ui.painter();
            painter.text(
                pos2(rect.left() + 8.0 + indent, rect.center().y),
                Align2::LEFT_CENTER,
                if expanded {
                    icons::CARET_DOWN
                } else {
                    icons::CARET_RIGHT
                },
                theme::icon(12.0),
                if fold.hovered() { t.ink } else { t.muted },
            );
            painter.text(
                pos2(rect.left() + 24.0 + indent, rect.center().y),
                Align2::LEFT_CENTER,
                icons::FOLDER_SIMPLE,
                theme::icon(14.0),
                theme::color(&group.color),
            );
            widgets::fitted_text(
                painter,
                pos2(rect.left() + 42.0 + indent, rect.center().y),
                Align2::LEFT_CENTER,
                group.name.as_str(),
                theme::ui(13.0),
                t.text2,
                rect.width() - 50.0 - indent,
            );
            if fold.clicked() || response.clicked() {
                *action = Some(LibraryAction::ToggleGroup(group.id.clone()));
            }
            // A section of this notebook dropped on a group goes inside it.
            let same = |drag: &SectionDrag| drag.notebook_id == notebook.id;
            if response
                .dnd_hover_payload::<SectionDrag>()
                .is_some_and(|drag| same(&drag))
            {
                ui.painter().rect_filled(
                    response.rect,
                    CornerRadius::same(2),
                    theme::with_alpha(t.accent, 46),
                );
            }
            if let Some(drag) = response
                .dnd_release_payload::<SectionDrag>()
                .filter(|drag| same(drag))
            {
                *action = Some(LibraryAction::MoveSection {
                    id: drag.id.clone(),
                    group_id: Some(group.id.clone()),
                    before: None,
                });
            }
            response.context_menu(|ui| {
                if let Some(picked) = group_menu(ui, notebook, group, t) {
                    *action = Some(picked);
                }
            });
            if expanded {
                self.section_tree(ui, notebook, Some(&group.id), depth + 1, t, action);
            }
        }
    }

    /// One section row in the navigation pane: colour pill and name, the selected
    /// section on the desktop notebook's grey fill. `next` is the following section in the same
    /// container (M8.9 drops a dragged section before it).
    #[allow(clippy::too_many_arguments)]
    fn section_row(
        &self,
        ui: &mut Ui,
        notebook: &crate::model::Notebook,
        section: &crate::model::Section,
        next: Option<&str>,
        indent: f32,
        t: Tokens,
        action: &mut Option<LibraryAction>,
    ) {
        let active = self.view == View::Section
            && self.selected_section.as_deref() == Some(section.id.as_str());
        let response = nav_row(ui, 28.0, &section.name, active, Sense::click_and_drag(), t);
        let rect = response.rect;
        if active && self.scroll_nav_to_selected {
            ui.scroll_to_rect(rect, None);
        }
        ui.painter().rect_filled(
            Rect::from_min_size(
                pos2(rect.left() + 24.0 + indent, rect.center().y - 8.0),
                vec2(6.0, 16.0),
            ),
            CornerRadius::same(3),
            theme::section_color(&section.color, t),
        );
        widgets::fitted_text(
            ui.painter(),
            pos2(rect.left() + 38.0 + indent, rect.center().y),
            Align2::LEFT_CENTER,
            section.name.as_str(),
            theme::ui(13.0),
            t.text2,
            rect.width() - 46.0 - indent,
        );
        response.dnd_set_drag_payload(SectionDrag {
            id: section.id.clone(),
            notebook_id: notebook.id.clone(),
            name: section.name.clone(),
        });
        // A page dropped on another section moves there (its own section is no
        // target). The release is only taken while a page hovers: egui's
        // `dnd_release_payload` drops a payload of any other type, which would lose a
        // section dropped here.
        if response
            .dnd_hover_payload::<PageDrag>()
            .is_some_and(|drag| drag.section_id != section.id)
        {
            ui.painter().rect_filled(
                response.rect,
                CornerRadius::same(2),
                theme::with_alpha(t.accent, 46),
            );
            if let Some(drag) = response.dnd_release_payload::<PageDrag>() {
                *action = Some(LibraryAction::MovePageTo(
                    drag.id.clone(),
                    section.id.clone(),
                ));
            }
        }
        // A section dropped on a section of the same notebook goes above or below it.
        let same = |drag: &SectionDrag| drag.notebook_id == notebook.id && drag.id != section.id;
        let above = || {
            ui.input(|i| i.pointer.hover_pos())
                .is_some_and(|p| p.y < response.rect.center().y)
        };
        if response
            .dnd_hover_payload::<SectionDrag>()
            .is_some_and(|drag| same(&drag))
        {
            let y = if above() {
                response.rect.top()
            } else {
                response.rect.bottom()
            };
            ui.painter()
                .hline(response.rect.x_range(), y, Stroke::new(2.0, t.accent));
        }
        if let Some(drag) = response
            .dnd_release_payload::<SectionDrag>()
            .filter(|drag| same(drag))
        {
            *action = Some(LibraryAction::MoveSection {
                id: drag.id.clone(),
                group_id: section.group_id.clone(),
                before: if above() {
                    Some(section.id.clone())
                } else {
                    next.map(str::to_string)
                },
            });
        }
        if response.clicked() {
            *action = Some(LibraryAction::SelectSection(section.id.clone()));
        }
        // the desktop notebook renames a section on double-click.
        if response.double_clicked() {
            *action = Some(LibraryAction::RenameSection(section.id.clone()));
        }
        response.context_menu(|ui| {
            if let Some(picked) = section_menu(ui, notebook, section, t) {
                *action = Some(picked);
            }
        });
    }

    /// Horizontal Tabs (D11), over the page: the notebook picker (unless the notebooks are
    /// pinned as a pane), then the current level's sections and section groups as
    /// the desktop notebook's pastel folder tabs, in the navigation pane's order. Inside a group a
    /// parent tab comes first; "+" adds a section at this level, and "⌄" lists the tabs
    /// that do not fit. The selected tab stands taller and reaches the panel's bottom
    /// edge, where the page starts.
    fn section_tabs(&mut self, ui: &mut Ui, t: Tokens) {
        // However the open section changed (a search hit, a link, start-up), the tabs show
        // its level.
        if self.tab_group_for != self.selected_section {
            match self.selected_section.clone() {
                Some(id) => self.show_tabs_of(&id),
                None => self.tab_group_for = None,
            }
        }
        let mut action: Option<LibraryAction> = None;
        let mut pin = false;
        // A new level for the tabs: `Some(None)` is the notebook's top level.
        let mut level: Option<Option<String>> = None;
        let area = ui.max_rect();
        let bottom = area.bottom();
        // The search box keeps the row's right end, as in the desktop notebook; the tabs fill the rest.
        let tabs_right = area.right() - TAB_SEARCH_WIDTH - 8.0;
        let mut x = area.left();
        let notebooks = &self.workspace.notebooks;
        let notebook = self
            .selected_notebook
            .as_deref()
            .and_then(|id| notebooks.iter().find(|notebook| notebook.id == id))
            .or_else(|| notebooks.first());
        if !(self.notebooks_pinned && self.show_library) {
            let (right, picked) =
                notebook_picker(ui, notebooks, notebook, pos2(x, bottom - 32.0), t);
            x = right + 8.0;
            match picked {
                Some(PickerPick::Pin) => pin = true,
                Some(PickerPick::Action(picked)) => action = Some(picked),
                None => {}
            }
        }
        if let Some(notebook) = notebook {
            // A group that is gone (deleted, or in another notebook) shows the top level.
            let group = self
                .tab_group
                .as_deref()
                .and_then(|id| notebook.groups.iter().find(|group| group.id == id));
            if self.tab_group.is_some() && group.is_none() {
                level = Some(None);
            }
            let parent = group.map(|group| group.id.as_str());
            let tabs: Vec<SectionTab> = notebook
                .sections
                .iter()
                .filter(|section| section.group_id.as_deref() == parent)
                .map(SectionTab::Section)
                .chain(
                    notebook
                        .groups
                        .iter()
                        .filter(|group| group.parent_id.as_deref() == parent)
                        .map(SectionTab::Group),
                )
                .collect();
            let widths: Vec<f32> = tabs.iter().map(|tab| tab.width(ui.ctx())).collect();
            let open = |section: &crate::model::Section| {
                self.view == View::Section
                    && self.selected_section.as_deref() == Some(section.id.as_str())
            };
            let selected = tabs
                .iter()
                .position(|tab| matches!(tab, SectionTab::Section(section) if open(section)));
            // The selected tab's top; the others start 6 px lower, and all end at `bottom`.
            let y = bottom - 34.0;
            if let Some(group) = group {
                let rect = Rect::from_min_size(pos2(x, y + 6.0), vec2(TAB_SMALL, 28.0));
                let label = t!("app-parent-group");
                let response = ui
                    .interact(rect, Id::unique("folio-parent-group-tab"), Sense::click())
                    .on_hover_text(label.as_ref());
                response
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                let amount = if response.hovered() {
                    theme::TAB_HOVER_TINT
                } else {
                    theme::TAB_TINT
                };
                let color = theme::section_color(&group.color, t);
                let painter = ui.painter();
                painter.rect_filled(rect, TAB_RADIUS, theme::pastel(color, t.raised, amount));
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icons::ARROW_TURN_UP_LEFT,
                    theme::icon(14.0),
                    t.ink,
                );
                if response.clicked() {
                    level = Some(group.parent_id.clone());
                }
                x = rect.right() + TAB_GAP;
            }
            // "+" follows the tabs shown; "⌄" keeps the row's right end.
            let room = tabs_right - x - (TAB_SMALL + TAB_GAP);
            let shown = fit_tabs(&widths, selected, room, TAB_SMALL + TAB_GAP);
            for &index in &shown {
                let tall = selected == Some(index);
                let rect = if tall {
                    Rect::from_min_size(pos2(x, y), vec2(widths[index], 34.0))
                } else {
                    Rect::from_min_size(pos2(x, y + 6.0), vec2(widths[index], 28.0))
                };
                let picked = match tabs[index] {
                    SectionTab::Section(section) => {
                        section_tab(ui, notebook, section, rect, tall, t)
                    }
                    SectionTab::Group(group) => {
                        let (open, picked) = group_tab(ui, notebook, group, rect, t);
                        if open {
                            level = Some(Some(group.id.clone()));
                        }
                        picked
                    }
                };
                if picked.is_some() {
                    action = picked;
                }
                x = rect.right() + TAB_GAP;
            }
            let rect = Rect::from_min_size(pos2(x, y + 6.0), vec2(TAB_SMALL, 28.0));
            let label = t!("common-new-section");
            let response = ui
                .interact(rect, Id::unique("folio-new-section-tab"), Sense::click())
                .on_hover_text(label.as_ref());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            let painter = ui.painter();
            painter.rect_filled(rect, TAB_RADIUS, t.raised);
            if response.hovered() {
                painter.rect_filled(rect, TAB_RADIUS, t.hover);
            }
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                icons::PLUS,
                theme::icon(14.0),
                if response.hovered() { t.ink } else { t.muted },
            );
            if response.clicked() {
                action = Some(LibraryAction::NewSectionIn(
                    notebook.id.clone(),
                    parent.map(str::to_string),
                ));
            }
            if shown.len() < tabs.len() {
                let rect = Rect::from_min_size(
                    pos2(tabs_right - TAB_SMALL, y + 6.0),
                    vec2(TAB_SMALL, 28.0),
                );
                let label = t!("app-more-sections");
                let response = ui
                    .interact(rect, Id::unique("folio-more-sections"), Sense::click())
                    .on_hover_text(label.as_ref());
                response
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                let menu_open =
                    egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
                let painter = ui.painter();
                if menu_open || response.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(4), t.hover);
                }
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icons::CARET_DOWN,
                    theme::icon(12.0),
                    if response.hovered() { t.ink } else { t.muted },
                );
                let hidden: Vec<SectionTab> = (0..tabs.len())
                    .filter(|index| !shown.contains(index))
                    .map(|index| tabs[index])
                    .collect();
                let picked = egui::Popup::menu(&response)
                    .show(|ui| more_sections_menu(ui, &hidden, t))
                    .and_then(|shown| shown.inner);
                match picked {
                    Some(SectionTab::Section(section)) => {
                        action = Some(LibraryAction::SelectSection(section.id.clone()));
                    }
                    Some(SectionTab::Group(group)) => level = Some(Some(group.id.clone())),
                    None => {}
                }
            }
        }
        if !self.search_in_header {
            let search = Rect::from_min_size(
                pos2(area.right() - TAB_SEARCH_WIDTH, bottom - 32.0),
                vec2(TAB_SEARCH_WIDTH, 28.0),
            );
            self.search.box_rect = Some(search);
            if self.search.open {
                self.search_field(ui, search, t);
            } else {
                self.search_box(ui, search, t);
            }
        }
        if let Some(level) = level {
            self.tab_group = level;
        }
        if pin {
            self.set_notebooks_pinned(true);
        }
        if let Some(action) = action {
            self.handle_library_action(action);
        }
    }

    /// The Find Tags pane: the desktop notebook's Tags Summary, docked on the right.
    fn tags_panel(&mut self, ui: &mut Ui, t: Tokens, now: f64) {
        let Some(pane) = self.tags_pane.as_ref() else {
            return;
        };
        let current = pane.scope;
        let mut scope = current;
        let mut refresh = false;
        let mut go: Option<TaggedLine> = None;
        // Refresh sits left of Close.
        let close = pane_header_with(ui, &t!("app-line-tags"), t, |ui| {
            refresh = widgets::icon_button_sized(
                ui,
                icons::ARROWS_CLOCKWISE,
                &t!("app-refresh-line-tags"),
                24.0,
                13.0,
                t,
            )
            .clicked();
        });
        ui.add_space(4.0);
        // Wraps rather than running past the pane when the labels are long (French).
        ui.horizontal_wrapped(|ui| {
            for (value, label) in [
                (TagsScope::Section, t!("app-tags-scope-section")),
                (TagsScope::Notebook, t!("app-tags-scope-notebook")),
                (TagsScope::All, t!("app-tags-scope-all")),
            ] {
                if widgets::toggle_chip(ui, scope == value, &label, t).clicked() {
                    scope = value;
                }
            }
        });
        // The pane keeps the section or notebook it listed while pages elsewhere open,
        // so it says which one that is.
        let place = match pane.scope {
            TagsScope::Section => pane
                .section_id
                .as_deref()
                .and_then(|id| self.find_section(id))
                .map(|(_, name, _)| name),
            TagsScope::Notebook => pane.notebook_id.as_deref().and_then(|id| {
                self.workspace
                    .notebooks
                    .iter()
                    .find(|notebook| notebook.id == id)
                    .map(|notebook| notebook.name.clone())
            }),
            TagsScope::All => None,
        };
        if let Some(place) = place {
            ui.add_space(2.0);
            ui.label(
                RichText::new(util::truncate(&place, 40))
                    .font(theme::ui(11.0))
                    .color(t.faint),
            );
        }
        ui.add_space(6.0);
        let mut group_by = pane.group_by;
        let mut unchecked_only = pane.unchecked_only;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(t!("app-tags-group-by"))
                    .font(theme::ui(12.0))
                    .color(t.ink),
            );
            let choices = TagsGroupBy::ALL.map(|by| (by, by.label().into_owned()));
            if let Some(by) = dropdown(ui, &group_by, &choices, 0.0, t) {
                group_by = by;
            }
        });
        ui.checkbox(
            &mut unchecked_only,
            RichText::new(t!("app-tags-unchecked-only"))
                .font(theme::ui(12.0))
                .color(t.ink),
        );
        ui.add_space(6.0);
        widgets::hairline(ui, t.line);
        // Each shown line with its group's number (a line pushed past `sort` has its own).
        let shown: Vec<(&TaggedLine, usize)> = pane
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !(pane.unchecked_only && line.checked))
            .map(|(index, line)| (line, pane.groups.get(index).copied().unwrap_or(!index)))
            .collect();
        // The lines fill the pane down to Create Summary Page, which stays at its foot.
        const FOOTER: f32 = 48.0;
        let body = vec2(
            ui.available_width(),
            (ui.available_height() - FOOTER).max(0.0),
        );
        ui.allocate_ui_with_layout(body, Layout::top_down(Align::Min), |ui| {
            ui.set_min_size(body);
            if pane.lines.is_empty() {
                widgets::empty_state(
                    ui,
                    icons::TAG,
                    &t!("app-no-tagged-lines"),
                    &t!(
                        "app-no-tagged-lines-body",
                        first = util::shortcut("1"),
                        last = util::shortcut("9")
                    ),
                    t,
                );
                return;
            }
            ScrollArea::vertical()
                .id_salt("tags-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // A heading wherever the group changes; Note text is one flat list.
                    let headings = pane.group_by != TagsGroupBy::NoteText;
                    for (index, &(line, group)) in shown.iter().enumerate() {
                        if headings && (index == 0 || shown[index - 1].1 != group) {
                            let count = shown[index..]
                                .iter()
                                .take_while(|(_, other)| *other == group)
                                .count();
                            ui.add_space(8.0);
                            tags_heading(ui, line, pane.group_by, count, t);
                            ui.add_space(2.0);
                        }
                        if tagged_line_row(ui, line, t).clicked() {
                            go = Some(line.clone());
                        }
                    }
                });
        });
        widgets::hairline(ui, t.line);
        ui.add_space(8.0);
        let create = ui
            .add_enabled_ui(!shown.is_empty(), |ui| {
                widgets::secondary_button(
                    ui,
                    Some(icons::NOTE_ADD),
                    &t!("app-tags-create-summary"),
                    t,
                )
                .clicked()
            })
            .inner;
        // As the pane groups them, whatever the dropdown says this frame.
        let summary = create.then(|| {
            let lines: Vec<TaggedLine> = shown.iter().map(|(line, _)| (*line).clone()).collect();
            (lines, pane.group_by)
        });
        if close {
            self.close_tags_pane();
            return;
        }
        if let Some((lines, by)) = summary {
            self.create_tags_summary_page(&lines, by, now);
        }
        if refresh {
            let _ = self.flush_save();
        }
        if let Some(pane) = self.tags_pane.as_mut() {
            pane.unchecked_only = unchecked_only;
            if pane.group_by != group_by {
                pane.group_by = group_by;
                pane.sort();
            }
        }
        if refresh || scope != current {
            self.load_tags(scope);
        }
        if let Some(line) = go {
            self.reveal_tagged_line(line);
        }
    }

    /// The results task pane: the pages a search found, or History › Recent Edits, one
    /// row each. A click opens the page and the pane stays open beside it.
    fn results_panel(&mut self, ui: &mut Ui, t: Tokens) {
        let Some(TaskPane::Results(pane)) = self.task_pane.as_ref() else {
            return;
        };
        let title = match &pane.kind {
            ResultsKind::Search { query, .. } => {
                t!("app-results-search", query = query.trim().to_string())
            }
            ResultsKind::Edits(range) => t!("app-results-edits", range = range.label()),
        };
        let close = pane_header(ui, &title, t);
        ui.label(
            RichText::new(t!("app-results-count", count = pane.hits.len()))
                .font(theme::ui(11.5))
                .color(t.muted),
        );
        ui.add_space(6.0);
        widgets::hairline(ui, t.line);
        // The page to open, and the words to find on it.
        let mut open: Option<(String, Option<String>)> = None;
        if pane.hits.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(t!("app-results-empty"))
                        .font(theme::ui(13.0))
                        .color(t.muted),
                );
            });
        } else {
            let current = self.page.as_ref().map(|page| page.id.as_str());
            ScrollArea::vertical()
                .id_salt("results-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    for hit in &pane.hits {
                        let (line3, find) = match &pane.kind {
                            ResultsKind::Search { query, .. } => {
                                (Cow::Borrowed(hit.excerpt.as_str()), Some(query.trim()))
                            }
                            // When the page was last edited.
                            ResultsKind::Edits(_) => {
                                (Cow::Owned(util::relative_time(hit.updated_at)), None)
                            }
                        };
                        let selected = current == Some(hit.page_id.as_str());
                        if hit_row(ui, hit, &line3, selected, t).clicked() {
                            open = Some((
                                hit.page_id.clone(),
                                find.filter(|query| !query.is_empty()).map(str::to_string),
                            ));
                        }
                    }
                });
        }
        if close {
            self.close_task_pane();
            return;
        }
        if let Some((id, find)) = open {
            self.open_page(&id, true);
            // As from the search box: the words searched for stay highlighted on the page.
            if let Some(query) = find {
                if self.page.as_ref().is_some_and(|page| page.id == id) {
                    self.canvas.open_find_from_search(&query);
                }
            }
        }
    }

    /// The Versions task pane (History › Page Versions): the open page's kept versions,
    /// newest first, a preview of the one picked, and Restore or Delete all. It follows
    /// the page opened; with none (or one in the Recycle Bin) it says to open one.
    fn versions_panel(&mut self, ui: &mut Ui, t: Tokens) {
        self.follow_versions_pane();
        let followed = self.versions_page().is_some();
        let Some(TaskPane::Versions(pane)) = self.task_pane.as_mut() else {
            return;
        };
        let close = pane_header(ui, &t!("common-page-versions"), t);
        ui.add(
            egui::Label::new(
                RichText::new(t!("app-versions-explain"))
                    .font(theme::ui(12.0))
                    .color(t.muted),
            )
            .wrap(),
        );
        ui.add_space(8.0);
        let mut action: Option<DialogAction> = None;
        if !followed {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(t!("app-versions-no-page"))
                            .font(theme::ui(13.0))
                            .color(t.muted),
                    )
                    .wrap(),
                );
            });
        } else if pane.versions.is_empty() {
            widgets::empty_state(
                ui,
                icons::CLOCK_COUNTER_CLOCKWISE,
                &t!("app-versions-empty-title"),
                &t!("app-versions-empty-body"),
                t,
            );
        } else {
            pane.selected = pane.selected.min(pane.versions.len() - 1);
            let mut picked = None;
            ScrollArea::vertical()
                .id_salt("versions-list")
                .max_height(260.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (index, version) in pane.versions.iter().enumerate() {
                        let label = format!(
                            "{} · {}",
                            util::relative_time(version.saved_at),
                            t!("common-word-count", count = version.word_count)
                        );
                        let current = index == pane.selected;
                        let width = ui.available_width();
                        let row =
                            left_row(ui, width, 30.0, &label, current, current, t, Sense::click())
                                .on_hover_text(util::full_date(version.saved_at));
                        if row.clicked() {
                            picked = Some(index);
                        }
                        // Its own menu: Restore Version and Delete Version, for this row.
                        row.context_menu(|ui| {
                            let (page_id, version_id) = (pane.page_id.clone(), version.id);
                            if widgets::menu_item(
                                ui,
                                Some(icons::ARROW_COUNTER_CLOCKWISE),
                                &t!("app-restore-version"),
                                t,
                            )
                            .clicked()
                            {
                                action = Some(DialogAction::RestoreVersion {
                                    page_id,
                                    version_id,
                                });
                                ui.close();
                            } else if widgets::menu_item(
                                ui,
                                Some(icons::TRASH),
                                &t!("app-delete-version"),
                                t,
                            )
                            .clicked()
                            {
                                action = Some(DialogAction::DeleteVersion {
                                    page_id,
                                    version_id,
                                });
                                ui.close();
                            }
                        });
                    }
                });
            if let Some(index) = picked {
                pane.selected = index;
                pane.confirm_delete = false;
            }
            // The version picked: its title, when it was kept, and its text.
            let version = &pane.versions[pane.selected];
            ui.add_space(8.0);
            widgets::hairline(ui, t.line);
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    RichText::new(display_title(&version.title))
                        .font(theme::semibold(13.0))
                        .color(t.ink),
                )
                .truncate(),
            );
            ui.label(
                RichText::new(util::full_date(version.saved_at))
                    .font(theme::ui(11.5))
                    .color(t.faint),
            );
            ui.add_space(6.0);
            ScrollArea::vertical()
                .id_salt("version-preview")
                .max_height(240.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let (text, font, color) = if version.plain_text.trim().is_empty() {
                        (t!("app-version-no-text"), theme::ui(12.0), t.faint)
                    } else {
                        (
                            crate::math::readable_text(&version.plain_text),
                            theme::body(13.0),
                            t.muted,
                        )
                    };
                    ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap());
                });
            ui.add_space(10.0);
            let version_id = version.id;
            let count = pane.versions.len();
            let mut confirm = pane.confirm_delete;
            // Side by side when they fit, else one under the other.
            ui.horizontal_wrapped(|ui| {
                if confirm {
                    let label = t!("app-delete-versions", count = count);
                    if widgets::danger_button(ui, &label, t).clicked() {
                        action = Some(DialogAction::DeleteVersions(pane.page_id.clone()));
                    }
                    if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                        confirm = false;
                    }
                } else {
                    if widgets::primary_button(
                        ui,
                        Some(icons::ARROW_COUNTER_CLOCKWISE),
                        &t!("app-restore-version"),
                        t,
                    )
                    .clicked()
                    {
                        action = Some(DialogAction::RestoreVersion {
                            page_id: pane.page_id.clone(),
                            version_id,
                        });
                    }
                    if widgets::text_button(ui, Some(icons::TRASH), &t!("app-delete-version"), t)
                        .clicked()
                    {
                        action = Some(DialogAction::DeleteVersion {
                            page_id: pane.page_id.clone(),
                            version_id,
                        });
                    }
                    if widgets::text_button(
                        ui,
                        Some(icons::TRASH),
                        &t!("app-delete-all-versions"),
                        t,
                    )
                    .clicked()
                    {
                        confirm = true;
                    }
                }
            });
            pane.confirm_delete = confirm;
        }
        if close {
            self.close_task_pane();
            return;
        }
        if let Some(action) = action {
            let ctx = ui.ctx().clone();
            self.apply_dialog_action(action, &ctx);
        }
    }

    /// The Spelling task pane (Review › Spelling, F7): the open page's misspellings one at
    /// a time, each selected in its note, with its suggestions and Change, Ignore, Ignore
    /// All and Add. The buttons work without taking the keyboard from the note.
    fn spelling_panel(&mut self, ui: &mut Ui, t: Tokens) {
        self.follow_spelling_pane();
        let close = pane_header(ui, &t!("app-spell-pane"), t);
        ui.add_space(8.0);
        let Some(TaskPane::Spelling(pane)) = self.task_pane.as_mut() else {
            return;
        };
        let spell = &mut self.editor.spell;
        let muted = |ui: &mut Ui, text: &str| {
            ui.add(
                egui::Label::new(RichText::new(text).font(theme::ui(13.0)).color(t.muted)).wrap(),
            );
        };
        let mut step: Option<SpellStep> = None;
        if pane.page_id.is_none() {
            muted(ui, &t!("app-spell-no-page"));
        } else if spell.setting() == "off" {
            muted(ui, &t!("app-spell-is-off"));
            ui.add_space(6.0);
            if widgets::text_button(ui, None, &t!("app-spell-open-options"), t).clicked() {
                step = Some(SpellStep::Options);
            }
        } else if !spell.is_ready() {
            muted(ui, &spell.status());
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        } else if pane.done {
            muted(ui, &t!("app-spell-done"));
            ui.add_space(6.0);
            if widgets::text_button(ui, None, &t!("app-spell-start-over"), t).clicked() {
                step = Some(SpellStep::StartOver);
            }
        } else if let Some(hit) = pane.current.as_ref() {
            ui.label(
                RichText::new(t!("app-spell-not-in-dictionary"))
                    .font(theme::ui(12.0))
                    .color(t.muted),
            );
            ui.add_space(4.0);
            // The word, in a sunken box.
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 32.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_size(vec2(ui.available_width(), 32.0));
                    ui.painter().rect(
                        ui.max_rect(),
                        CornerRadius::same(4),
                        t.sunken,
                        Stroke::new(1.0, t.line),
                        StrokeKind::Inside,
                    );
                    ui.add_space(10.0);
                    ui.set_max_width(ui.available_width() - 10.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(hit.word.as_str())
                                .font(theme::semibold(14.0))
                                .color(t.ink),
                        )
                        .truncate(),
                    );
                },
            );
            ui.add_space(12.0);
            ui.label(
                RichText::new(t!("app-spell-suggestions"))
                    .font(theme::semibold(12.0))
                    .color(t.ink),
            );
            ui.add_space(4.0);
            let suggestions: Option<Vec<String>> = spell
                .suggestions(&hit.word, hit.language)
                .map(|list| list.iter().take(8).cloned().collect());
            match suggestions.as_deref() {
                None => {
                    muted(ui, &t!("spell-looking"));
                    // The suggestions are looked up in the background.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                Some([]) => muted(ui, &t!("spell-no-suggestions")),
                Some(list) => {
                    pane.picked = pane.picked.min(list.len() - 1);
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (index, suggestion) in list.iter().enumerate() {
                            let width = ui.available_width();
                            let current = index == pane.picked;
                            let row = left_row(
                                ui,
                                width,
                                26.0,
                                suggestion,
                                current,
                                false,
                                t,
                                Sense::click(),
                            );
                            if row.double_clicked() {
                                step = Some(SpellStep::Change(suggestion.clone()));
                            } else if row.clicked() {
                                pane.picked = index;
                            }
                        }
                    });
                }
            }
            let picked = suggestions
                .as_ref()
                .and_then(|list| list.get(pane.picked))
                .cloned();
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                // A button that does not fit goes to the next line whole. The buttons
                // draw in a scope of their own, which the row cannot wrap, so each is
                // measured first.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let place = |ui: &mut Ui, label: &str| {
                    let width = widgets::primary_button_width(ui, None, label);
                    let room = ui.available_size_before_wrap().x;
                    if room < width && ui.cursor().left() > ui.max_rect().left() {
                        ui.end_row();
                    }
                };
                let label = t!("app-spell-change");
                place(ui, &label);
                let change = ui
                    .add_enabled_ui(picked.is_some(), |ui| {
                        widgets::primary_button(ui, None, &label, t)
                    })
                    .inner;
                if let Some(word) = picked.filter(|_| change.clicked()) {
                    step = Some(SpellStep::Change(word));
                }
                let others = [
                    (t!("app-spell-ignore"), SpellStep::Ignore),
                    (t!("spell-ignore-all"), SpellStep::IgnoreAll),
                    (t!("spell-add-to-dictionary"), SpellStep::Add),
                ];
                for (label, clicked) in others {
                    place(ui, &label);
                    if widgets::secondary_button(ui, None, &label, t).clicked() {
                        step = Some(clicked);
                    }
                }
            });
            // The language the word was checked in: the paragraph's, when the speller
            // can tell, else the setting's.
            let name = hit
                .language
                .and_then(|index| {
                    spell
                        .active_codes()
                        .get(index)
                        .map(|code| crate::spell::language_name(code))
                })
                .unwrap_or_else(|| match spell.setting() {
                    "auto" => spell.automatic_name(),
                    code => crate::spell::language_name(code),
                });
            ui.add_space(12.0);
            widgets::hairline(ui, t.line);
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new(t!("app-spell-language", name = name))
                        .font(theme::ui(12.0))
                        .color(t.muted),
                )
                .truncate(),
            );
        }
        let current = pane.current.clone();
        if close {
            self.close_task_pane();
            return;
        }
        let Some(step) = step else {
            return;
        };
        let at = current
            .as_ref()
            .map(|hit| (hit.item, hit.block, hit.range.start));
        match step {
            SpellStep::Change(word) => {
                let Some(hit) = current else {
                    return;
                };
                let changed = self.page.as_mut().is_some_and(|page| {
                    canvas::replace_in_note(
                        page,
                        &mut self.canvas,
                        hit.item,
                        hit.block,
                        hit.range.clone(),
                        &hit.word,
                        &word,
                    )
                });
                if changed {
                    self.mark_dirty(ui.input(|input| input.time));
                    let end = hit.range.start + word.chars().count();
                    self.spell_next(Some((hit.item, hit.block, end)));
                } else {
                    // The note was edited since the word was found: check the page again.
                    self.spell_next(None);
                }
            }
            SpellStep::Ignore => {
                if let (Some(TaskPane::Spelling(pane)), Some(at)) = (self.task_pane.as_mut(), at) {
                    pane.skipped.insert(at);
                }
                self.spell_next(at);
            }
            SpellStep::IgnoreAll | SpellStep::Add => {
                if let Some(hit) = &current {
                    if matches!(step, SpellStep::Add) {
                        // `tick_spelling` saves the personal dictionary.
                        self.editor.spell.add_personal(&hit.word);
                    } else {
                        self.editor.spell.ignore_all(&hit.word);
                    }
                }
                self.spell_next(at);
            }
            SpellStep::StartOver => {
                if let Some(TaskPane::Spelling(pane)) = self.task_pane.as_mut() {
                    pane.skipped.clear();
                }
                self.spell_next(None);
            }
            SpellStep::Options => {
                self.options_page = OptionsPage::Proofing;
                self.dialog = Some(Dialog::Settings);
            }
        }
    }

    /// The page list: a white card holding Add page and the sort button (or the Recycle
    /// Bin's heading), then the section's pages, one title per row, as the desktop notebook lists them.
    /// Horizontal Tabs passes the open section's colour as `tint` (D11): the card takes a
    /// light wash of it, the header a deeper band, and the open page a white row.
    fn page_list_card(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        t: Tokens,
        tint: Option<Color32>,
    ) {
        let mut action: Option<PageListAction> = None;
        // The empty area's New Page.
        let mut add_page = false;
        let trash = self.view == View::Trash;
        let card = match tint {
            Some(color) => widgets::card(t).fill(theme::pastel(color, t.raised, theme::LIST_TINT)),
            None => widgets::card(t),
        };
        // Without the notebooks pane (Horizontal Tabs, unpinned) there is nothing to show.
        let nav_hidden = !self.show_library && self.nav_pane_can_show();
        let mut in_view: Option<(usize, usize)> = None;
        card.inner_margin(Margin::symmetric(6, 8)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if let Some(color) = tint {
                // The header band spans the card's width, from its top edge to the
                // first row.
                let inner = ui.max_rect();
                let header = Rect::from_min_max(
                    inner.left_top() - vec2(6.0, 8.0),
                    pos2(inner.right() + 6.0, inner.top() + 40.0),
                );
                ui.painter().rect_filled(
                    header,
                    CARD_TOP_RADIUS,
                    theme::pastel(color, t.raised, theme::LIST_HEADER_TINT),
                );
            }
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 36.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_height(36.0);
                    ui.spacing_mut().item_spacing.x = HEADER_GAP;
                    if nav_hidden
                        && widgets::icon_button_sized(
                            ui,
                            icons::LIST,
                            &t!("app-show-navigation"),
                            32.0,
                            16.0,
                            t,
                        )
                        .clicked()
                    {
                        self.show_library = true;
                        self.panels_toggled_by_hand(true);
                    }
                    if trash {
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.view_title())
                                    .font(theme::semibold(13.0))
                                    .color(t.ink),
                            )
                            .truncate(),
                        );
                        return;
                    }
                    // Add page gives way to the sort button in a narrow list.
                    let room = ui.available_width() - SORT_BUTTON - HEADER_GAP;
                    if add_page_button(ui, room, tint.is_some(), t).clicked() {
                        self.new_page();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(sort) = self.sort_button(ui, t) {
                            self.set_page_sort(sort);
                        }
                    });
                },
            );
            if trash && !(self.pages.is_empty() && self.trashed_sections.is_empty()) {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    if widgets::text_button(ui, Some(icons::TRASH), &t!("app-empty-trash"), t)
                        .clicked()
                    {
                        self.confirm_empty_trash();
                    }
                });
            }
            ui.add_space(4.0);
            let pages = self.pages.clone();
            let trashed_sections = self.trashed_sections.clone();
            let picked_section = self.trashed_section_picked.clone();
            let opened_id = self.page.as_ref().map(|page| page.id.clone());
            let clip = self.page_clip.is_some();
            ScrollArea::vertical()
                .id_salt("pages-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    // The empty area below the rows (at least the old 28 px under a long
                    // list) has the desktop notebook's menu: New Page and Paste. The Recycle Bin has
                    // none.
                    let mut empty_area = |ui: &mut Ui, action: &mut Option<PageListAction>| {
                        let rest = ui.available_height().max(28.0);
                        let empty =
                            ui.allocate_response(vec2(ui.available_width(), rest), Sense::click());
                        if trash {
                            return;
                        }
                        empty.context_menu(|ui| match page_area_menu(ui, clip, t) {
                            Some(PageMenuPick::NewPage) => add_page = true,
                            Some(PageMenuPick::Action(picked)) => *action = Some(picked),
                            Some(PageMenuPick::CopyLink) | None => {}
                        });
                    };
                    // The bin's deleted sections, above its pages.
                    if !trashed_sections.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(t!("app-trash-sections"))
                                    .font(theme::semibold(12.0))
                                    .color(t.muted),
                            );
                        });
                        ui.add_space(4.0);
                        for section in &trashed_sections {
                            let picked = picked_section.as_deref() == Some(section.id.as_str());
                            if let Some(picked) = trashed_section_row(ui, section, picked, t) {
                                action = Some(picked);
                            }
                        }
                        ui.add_space(8.0);
                    }
                    if pages.is_empty() && !trashed_sections.is_empty() {
                        empty_area(ui, &mut action);
                        return;
                    }
                    if pages.is_empty() {
                        let (title, body) = match &self.view {
                            View::Trash => (
                                t!("app-list-empty-trash-title"),
                                t!("app-list-empty-trash-body"),
                            ),
                            _ => (
                                t!("app-list-empty-section-title"),
                                t!("app-list-empty-section-body"),
                            ),
                        };
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.0);
                            for text in [title, body] {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text).font(theme::ui(12.0)).color(t.muted),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(4.0);
                            }
                        });
                        empty_area(ui, &mut action);
                        return;
                    }
                    // A sorted list is flat: no folds, no nesting, no dragging.
                    let outline_view =
                        self.view == View::Section && self.page_sort == PageSort::None;
                    let levels: Vec<u8> = pages.iter().map(|page| page.level).collect();
                    let hidden = if outline_view {
                        outline::hidden(
                            &pages
                                .iter()
                                .map(|page| (page.level, page.collapsed))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        vec![false; pages.len()]
                    };
                    let visible: Vec<usize> = (0..pages.len()).filter(|&i| !hidden[i]).collect();
                    for (slot, &index) in visible.iter().enumerate() {
                        let page = &pages[index];
                        // A page dropped below this row lands before the next row shown,
                        // at that row's level.
                        let below = visible.get(slot + 1).map(|&next| &pages[next]);
                        let row_outline = outline_view.then(|| RowOutline {
                            level: page.level,
                            has_children: outline::has_children(&levels, index),
                            collapsed: page.collapsed,
                            can_demote: index > 0
                                && page.level < outline::MAX_LEVEL
                                && page.level <= levels[index - 1],
                            below_level: below.map_or(page.level, |next| next.level),
                        });
                        let selected = opened_id.as_deref() == Some(page.id.as_str());
                        let row = page_row(ui, page, selected, t, clip, row_outline, tint);
                        if ui.clip_rect().contains_rect(row.rect) {
                            in_view =
                                Some(in_view.map_or((index, index), |(first, _)| (first, index)));
                        }
                        if selected && self.scroll_list_to_open {
                            ui.scroll_to_rect(row.rect, None);
                        }
                        if row.clicked {
                            action = Some(PageListAction::Open(page.id.clone()));
                        }
                        if let Some(menu_action) = row.context_action {
                            action = Some(menu_action);
                        }
                        if row.copy_link_clicked {
                            ctx.copy_text(page_link(&page.id));
                            self.toast(t!("toast-link-copied"));
                        }
                        if let Some((dragged, above)) = row.dropped {
                            // The page takes the level of the row it lands next to.
                            let (before, level) = if above {
                                (Some(page.id.clone()), page.level)
                            } else if let Some(next) = below {
                                (Some(next.id.clone()), next.level)
                            } else {
                                (None, page.level)
                            };
                            action = Some(PageListAction::Place {
                                id: dragged,
                                section_id: page.section_id.clone(),
                                before,
                                level: Some(level),
                            });
                        }
                    }
                    empty_area(ui, &mut action);
                });
        });
        // Spent even when the open page has no row here (folded away, or in another view).
        self.scroll_list_to_open = false;
        self.pages_in_view = in_view;
        if add_page {
            self.new_page();
        }
        if let Some(action) = action {
            self.handle_page_list_action(action);
        }
    }

    /// The page list's sort button and its menu: None, Alphabetical, Date created and
    /// Date modified, the current one checked with an arrow for its direction.
    fn sort_button(&self, ui: &mut Ui, t: Tokens) -> Option<PageSort> {
        let response = widgets::icon_button_sized(
            ui,
            icons::ARROW_SORT,
            &t!("app-sort-pages"),
            SORT_BUTTON,
            16.0,
            t,
        );
        egui::Popup::menu(&response)
            .show(|ui| {
                ui.set_min_width(200.0);
                let mut picked = None;
                for sort in PageSort::ALL {
                    let current = sort == self.page_sort;
                    let label = sort.label();
                    // As the desktop notebook's menu reads: the current order is checked at the left,
                    // a sort by its direction's arrow on a highlighted square.
                    let arrow =
                        (current && sort != PageSort::None).then_some(if self.page_sort_desc {
                            icons::ARROW_DOWN
                        } else {
                            icons::ARROW_UP
                        });
                    let row = widgets::checked_menu_item(ui, arrow, &label, current, t);
                    // Named without the arrow, and checked when current.
                    row.widget_info(|| {
                        egui::WidgetInfo::selected(egui::Role::Button, true, current, &label)
                    });
                    if row.clicked() {
                        picked = Some(sort);
                        ui.close();
                    }
                }
                picked
            })
            .and_then(|shown| shown.inner)
    }

    /// The page card, the desktop notebook's white page area: the open page (or the empty state), the
    /// Recycle Bin banner over a deleted page, and the Full Page View button in the
    /// top-right corner.
    fn page_card(&mut self, ui: &mut Ui, ctx: &egui::Context, t: Tokens, now: f64) {
        let mut banner_action = None;
        // With both side panels folded away (the Find Tags pane needed the room), a strip
        // at the card's left offers to show the navigation again (D7).
        let nav_strip =
            !self.show_library && !self.show_pages && !self.full_page && self.nav_pane_can_show();
        // Horizontal Tabs: a band in the open section's colour runs along the card's top,
        // where the selected tab meets it, and the page starts below it.
        let band = self.section_tint(t);
        // the desktop notebook's View › Switch Background: the page in the other theme's colours, the rest
        // of the window unchanged.
        let page_t = if self.switch_background {
            Tokens::for_dark(!t.dark)
        } else {
            t
        };
        let page_style = if self.switch_background {
            let fresh = self
                .switched_style
                .as_ref()
                .filter(|(dark, _)| *dark == page_t.dark);
            if fresh.is_none() {
                let style = std::sync::Arc::new(theme::style(page_t));
                self.switched_style = Some((page_t.dark, style));
            }
            self.switched_style.as_ref().map(|(_, style)| style.clone())
        } else {
            None
        };
        widgets::card(page_t)
            .inner_margin(Margin::ZERO)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let card = ui.max_rect();
                // The menus, tooltips and dialogs the page opens are drawn in the context's
                // style, not the card's: they take the page's colours too while it draws, so
                // their text (in the page's tokens) shows on them.
                let window_style = page_style.as_ref().map(|style| {
                    let window = ctx.global_style();
                    ctx.set_global_style(style.clone());
                    window
                });
                ui.scope(|ui| {
                    // egui's own widgets in the page (text edits, the selection, the caret)
                    // follow the page's theme.
                    if let Some(style) = &page_style {
                        ui.set_style(style.clone());
                    }
                    let mut content = card;
                    if let Some(color) = band {
                        // In the window's tokens, as the tab it continues.
                        let strip =
                            Rect::from_min_size(card.left_top(), vec2(card.width(), BAND_HEIGHT));
                        ui.painter().rect_filled(
                            strip,
                            CARD_TOP_RADIUS,
                            theme::pastel(color, t.raised, theme::TAB_TINT),
                        );
                        content.min.y += BAND_HEIGHT;
                    }
                    if nav_strip {
                        content.min.x += 40.0;
                        let button = Rect::from_min_size(
                            pos2(card.left(), content.top()) + vec2(4.0, 6.0),
                            vec2(32.0, 32.0),
                        );
                        if ui
                            .put(button, |ui: &mut Ui| {
                                widgets::icon_button_sized(
                                    ui,
                                    icons::LIST,
                                    &t!("app-show-navigation"),
                                    32.0,
                                    16.0,
                                    page_t,
                                )
                            })
                            .clicked()
                        {
                            self.show_library = true;
                            self.panels_toggled_by_hand(true);
                        }
                    }
                    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                        if self.page.as_ref().is_some_and(|page| page.trashed) {
                            banner_action = trash_banner(ui, !nav_strip, page_t);
                        }
                        self.editor_panel(ui, ctx, page_t, now);
                    });
                });
                if let Some(window) = window_style {
                    ctx.set_global_style(window);
                }
                // In the card's corner, over the page (which is drawn in its own layer and fills
                // the card), without taking layout space. Menus and dialogs still cover it.
                let (glyph, label) = if self.full_page {
                    (icons::FULL_SCREEN_MINIMIZE, t!("app-normal-view"))
                } else {
                    (icons::FULL_SCREEN_MAXIMIZE, t!("app-full-page-view"))
                };
                let corner = card.right_top() + vec2(-36.0, 8.0);
                let clicked = egui::Area::new(Id::unique("folio-full-page-corner"))
                    .order(egui::Order::Middle)
                    .fixed_pos(corner)
                    .show(ui.ctx(), |ui| {
                        widgets::icon_button_sized(ui, glyph, &label, 28.0, 14.0, t).clicked()
                    })
                    .inner;
                if clicked {
                    self.set_full_page(!self.full_page);
                }
            });
        if let Some(action) = banner_action {
            self.editor_action(action);
        }
    }

    fn editor_panel(&mut self, ui: &mut Ui, ctx: &egui::Context, t: Tokens, now: f64) {
        if self.page.is_none() {
            self.empty_editor(ui, ctx, t);
            return;
        }
        let mut page = self.page.take().unwrap();
        self.canvas.covered = self.dialog.is_some() || self.search.open || self.reader.open;
        self.canvas.scrollbar_left = self.scrollbar_left;
        let result = canvas::show(
            ui,
            &mut page,
            &mut self.canvas,
            &mut self.editor,
            &mut self.library,
            &mut self.textures,
            t,
        );
        let changed = result.changed;
        let open_versions = std::mem::take(&mut self.canvas.open_versions);
        if let Some((message, is_error)) = result.toast {
            if is_error {
                self.error(message);
            } else {
                self.toast(message);
            }
        }

        self.page = Some(page);
        if changed {
            self.mark_dirty(now);
        }
        self.after_editor();
        if open_versions {
            self.editor_action(EditorAction::Versions);
        }
    }

    fn empty_editor(&mut self, ui: &mut Ui, _ctx: &egui::Context, t: Tokens) {
        let (title, body) = match &self.view {
            View::Trash if self.pages.is_empty() && self.trashed_sections.is_empty() => (
                t!("app-editor-empty-trash-title"),
                t!("app-editor-empty-trash-body"),
            ),
            _ => (t!("app-editor-empty-title"), t!("app-editor-empty-body")),
        };
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.26);
            ui.label(RichText::new(title).font(theme::light(26.0)).color(t.ink));
            ui.add_space(6.0);
            ui.label(RichText::new(body).font(theme::ui(13.0)).color(t.muted));
            ui.add_space(20.0);
            if self.view != View::Trash
                && widgets::primary_button(ui, Some(icons::PLUS), &t!("common-new-page"), t)
                    .clicked()
            {
                self.new_page();
            }
            ui.add_space(26.0);
            widgets::caption(
                ui,
                &t!("app-editor-empty-hint", shortcut = util::shortcut("E")),
                t.muted,
            );
        });
    }

    /// The dropdown under the header's search box (the desktop notebook's search): the recent pages
    /// while the box is empty, else the pages found in the search's scope and, under
    /// them, "Open in the search results pane" (Alt+O, `open_results_pane`). Up and Down
    /// pick a row and Enter opens it; Escape, or a press outside the box and the
    /// dropdown, closes the search.
    fn search_dropdown(&mut self, ctx: &egui::Context, t: Tokens) {
        if !self.search.open {
            // However the search closed, the page its previews left showing is kept.
            self.settle_preview();
            return;
        }
        let Some(field) = self.search.box_rect else {
            return;
        };
        // The hits follow the words typed and the scope.
        let query = self.search.query.trim().to_string();
        if query != self.search.last_query || self.search.scope != self.search.last_scope {
            let filter = self.search_filter(self.search.scope);
            self.search.results = if query.is_empty() {
                Vec::new()
            } else {
                match self.library.search_in(&query, filter.as_db(), 9) {
                    Ok(hits) => hits,
                    Err(error) => {
                        self.error(t!("error-search", error = error.to_string()));
                        Vec::new()
                    }
                }
            };
            self.search.last_query = query.clone();
            self.search.last_scope = self.search.scope;
            self.search.selected = 0;
        }
        let recent = query.is_empty();
        let count = if recent {
            self.search.recent.len()
        } else {
            self.search.results.len()
        };
        // The scope menu, while open, has the keys (Escape closes it alone). The box took
        // ↓ / ↑ before it drew (`search_field`).
        let (taken_down, taken_up) = std::mem::take(&mut self.search.arrows);
        let (down, up, enter, escape, mut open_pane) = if self.search.scope_menu {
            (false, false, false, false, false)
        } else {
            ctx.input_mut(|input| {
                (
                    input.consume_key(Modifiers::NONE, Key::ArrowDown) || taken_down,
                    input.consume_key(Modifiers::NONE, Key::ArrowUp) || taken_up,
                    input.consume_key(Modifiers::NONE, Key::Enter),
                    input.consume_key(Modifiers::NONE, Key::Escape),
                    // Alt+O: the results in the docked results pane, as in the desktop notebook.
                    input.consume_key(Modifiers::ALT, Key::O),
                )
            })
        };
        let moved = count > 0 && (down || up);
        if moved {
            let selected = self.search.selected.min(count - 1);
            self.search.selected = if down {
                (selected + 1) % count
            } else {
                (selected + count - 1) % count
            };
        }
        // ↓ / ↑ show the result's page behind the list, as the desktop notebook previews it; Enter keeps
        // it (and marks the words), Esc leaves it open.
        if moved {
            let hits = if recent {
                &self.search.recent
            } else {
                &self.search.results
            };
            if let Some(id) = hits
                .get(self.search.selected)
                .map(|hit| hit.page_id.clone())
            {
                self.preview_page(&id);
            }
        }

        let mut choose: Option<String> = None;
        if enter {
            let hits = if recent {
                &self.search.recent
            } else {
                &self.search.results
            };
            match hits.get(self.search.selected) {
                Some(hit) => choose = Some(hit.page_id.clone()),
                // Nothing to open: the words stay, ready to be changed.
                None => self.search.focus = true,
            }
        }

        let screen = ctx.content_rect();
        let width = field.width().max(420.0);
        let left = field
            .left()
            .min(screen.right() - 8.0 - width)
            .max(screen.left() + 8.0);
        let top = field.bottom() + 4.0;
        // With words typed, the footer's "Open in the search results pane" under the rows.
        let footer = if recent { 0.0 } else { 34.0 };
        let alt_o = if cfg!(target_os = "macos") {
            "⌥O".to_string()
        } else {
            format!("{} O", i18n::modifier_names().alt)
        };
        let search = &self.search;
        let area = egui::Area::new(Id::unique("folio-search-dropdown"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(left, top))
            .show(ctx, |ui| {
                widgets::card(t)
                    .stroke(Stroke::new(1.0, t.line))
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(width - 12.0);
                        let hits = if recent {
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(t!("app-search-recent"))
                                        .font(theme::ui(11.5))
                                        .color(t.muted),
                                );
                            });
                            ui.add_space(2.0);
                            &search.recent
                        } else {
                            &search.results
                        };
                        if hits.is_empty() {
                            ui.add_space(16.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(if recent {
                                        t!("app-search-recent-empty")
                                    } else {
                                        t!("app-search-no-matches")
                                    })
                                    .font(theme::ui(13.0))
                                    .color(t.faint),
                                );
                            });
                            ui.add_space(16.0);
                        } else {
                            // A short window scrolls the rows rather than cut them off.
                            ScrollArea::vertical()
                                .id_salt("search-dropdown-scroll")
                                .max_height((screen.bottom() - top - 60.0 - footer).max(120.0))
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for (index, hit) in hits.iter().enumerate() {
                                        let selected = index == search.selected;
                                        let row = hit_row(ui, hit, &hit.excerpt, selected, t);
                                        if selected && moved {
                                            row.scroll_to_me(None);
                                        }
                                        if row.clicked() {
                                            choose = Some(hit.page_id.clone());
                                        }
                                    }
                                });
                        }
                        if !recent {
                            ui.add_space(4.0);
                            widgets::hairline(ui, t.line);
                            ui.add_space(2.0);
                            // Its glyph under the rows' text.
                            let button = ui
                                .horizontal(|ui| {
                                    ui.add_space(4.0);
                                    widgets::text_button(
                                        ui,
                                        Some(icons::DOCUMENT_SEARCH),
                                        &t!("app-search-open-pane"),
                                        t,
                                    )
                                    .on_hover_text(alt_o.as_str())
                                })
                                .inner;
                            if button.clicked() {
                                open_pane = true;
                            }
                        }
                    });
            });
        let dropdown = area.response.rect;
        self.search.dropdown_rect = Some(dropdown);

        // A press anywhere else closes the search, as a menu closes (a menu opened from
        // the box keeps it open).
        let pressed_outside = ctx.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .press_origin()
                    .is_some_and(|pos| !field.contains(pos) && !dropdown.contains(pos))
        });
        if escape || (pressed_outside && !egui::Popup::is_any_open(ctx)) {
            self.search.open = false;
        }
        if open_pane && !query.is_empty() {
            self.open_results_pane();
        } else if let Some(id) = choose {
            self.search.open = false;
            // A result opens over Backstage, then shows its page: the search's one visit,
            // however many pages its previews showed.
            self.preview_page(&id);
            self.settle_preview();
            // Like the desktop notebook, the words searched for stay highlighted on the page.
            if !query.is_empty() && self.page.as_ref().is_some_and(|page| page.id == id) {
                self.canvas.open_find_from_search(&query);
            }
        }
    }

    /// The settings Options can change, as they are now.
    fn options_snapshot(&self) -> OptionsSnapshot {
        OptionsSnapshot {
            appearance: self.appearance,
            ribbon_layout: self.ribbon.layout,
            tabs_layout: self.tabs_layout,
            page_list_left: self.page_list_left,
            notebooks_pinned: self.notebooks_pinned,
            scrollbar_left: self.scrollbar_left,
            spell_language: self.editor.spell.setting().to_string(),
            ui_language: self.ui_language,
            search_scope: self.search.default_scope,
            printout_background: self.printout_background,
            mini_on_selection: self.editor.mini_on_selection,
            auto_calc: self.editor.auto_calc,
            show_library: self.show_library,
        }
    }

    /// Options › Cancel: puts back each setting that differs from `before`, the way the
    /// dialog set it, so it is stored again too.
    fn restore_options(&mut self, before: OptionsSnapshot, ctx: &egui::Context) {
        let now = self.options_snapshot();
        let mut actions = Vec::new();
        if before.appearance != now.appearance {
            actions.push(DialogAction::SetAppearance(before.appearance));
        }
        if before.ribbon_layout != now.ribbon_layout {
            actions.push(DialogAction::Run(Cmd::RibbonLayout(before.ribbon_layout)));
        }
        if before.tabs_layout != now.tabs_layout {
            actions.push(DialogAction::Run(Cmd::TabsLayout(before.tabs_layout)));
        }
        if before.page_list_left != now.page_list_left {
            actions.push(DialogAction::SetPageListLeft(before.page_list_left));
        }
        if before.notebooks_pinned != now.notebooks_pinned {
            actions.push(DialogAction::SetNotebooksPinned(before.notebooks_pinned));
        }
        if before.scrollbar_left != now.scrollbar_left {
            actions.push(DialogAction::SetScrollbarLeft(before.scrollbar_left));
        }
        if before.spell_language != now.spell_language {
            actions.push(DialogAction::SetSpellLanguage(before.spell_language));
        }
        if before.ui_language != now.ui_language {
            actions.push(DialogAction::SetUiLanguage(before.ui_language));
        }
        if before.search_scope != now.search_scope {
            actions.push(DialogAction::SetSearchScope(before.search_scope));
        }
        if before.printout_background != now.printout_background {
            actions.push(DialogAction::SetPrintoutBackground(
                before.printout_background,
            ));
        }
        if before.mini_on_selection != now.mini_on_selection {
            actions.push(DialogAction::SetMiniToolbar(before.mini_on_selection));
        }
        if before.auto_calc != now.auto_calc {
            actions.push(DialogAction::SetAutoCalc(before.auto_calc));
        }
        for action in actions {
            self.apply_dialog_action(action, ctx);
        }
        if self.show_library != before.show_library {
            self.show_library = before.show_library;
            self.pane_fold.fitted_width = 0.0;
        }
    }

    /// Options › General: the theme and the ribbon's layout, then whether the Mini Toolbar
    /// shows on a selection.
    fn options_general(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        options_group(ui, &t!("app-options-personalize"), t, |ui| {
            options_row(ui, &t!("app-options-theme"), t, |ui| {
                let choices = [Appearance::System, Appearance::Light, Appearance::Dark]
                    .map(|option| (option, option.label().into_owned()));
                if let Some(chosen) =
                    dropdown(ui, &self.appearance, &choices, OPTIONS_COMBO_WIDTH, t)
                {
                    *action = Some(DialogAction::SetAppearance(chosen));
                }
            });
            ui.add_space(6.0);
            options_row(ui, &t!("app-options-ribbon"), t, |ui| {
                let label = |layout: RibbonLayout| match layout {
                    RibbonLayout::Classic => t!("ribbon-classic"),
                    RibbonLayout::Simplified => t!("ribbon-simplified"),
                };
                let choices = [RibbonLayout::Classic, RibbonLayout::Simplified]
                    .map(|layout| (layout, label(layout).into_owned()));
                if let Some(chosen) =
                    dropdown(ui, &self.ribbon.layout, &choices, OPTIONS_COMBO_WIDTH, t)
                {
                    *action = Some(DialogAction::Run(Cmd::RibbonLayout(chosen)));
                }
            });
        });
        options_group(ui, &t!("app-options-ui"), t, |ui| {
            let mut on = self.editor.mini_on_selection;
            if options_check(ui, true, &mut on, &t!("app-options-mini-toolbar"), t).changed() {
                *action = Some(DialogAction::SetMiniToolbar(on));
            }
        });
    }

    /// Options › Display: how the notebooks, sections and pages are laid out around the
    /// page. As in the desktop notebook, the page list and the notebook list only move in Horizontal
    /// Tabs.
    fn options_display(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        options_group(ui, &t!("app-options-navigation"), t, |ui| {
            options_row(ui, &t!("app-options-tabs-layout"), t, |ui| {
                let choices = TabsLayout::ALL.map(|layout| (layout, layout.label().into_owned()));
                if let Some(chosen) =
                    dropdown(ui, &self.tabs_layout, &choices, OPTIONS_COMBO_WIDTH, t)
                {
                    *action = Some(DialogAction::Run(Cmd::TabsLayout(chosen)));
                }
            });
            ui.add_space(8.0);
            let horizontal = self.tabs_layout == TabsLayout::Horizontal;
            let only = t!("app-options-horizontal-only");
            let mut page_list_left = self.page_list_left;
            if options_check(
                ui,
                horizontal,
                &mut page_list_left,
                &t!("app-options-page-list-left"),
                t,
            )
            .on_hover_text(only.as_ref())
            .on_disabled_hover_text(only.as_ref())
            .changed()
            {
                *action = Some(DialogAction::SetPageListLeft(page_list_left));
            }
            ui.add_space(4.0);
            let mut pinned = self.notebooks_pinned;
            if options_check(
                ui,
                horizontal,
                &mut pinned,
                &t!("app-options-pin-notebooks"),
                t,
            )
            .on_hover_text(only.as_ref())
            .on_disabled_hover_text(only.as_ref())
            .changed()
            {
                *action = Some(DialogAction::SetNotebooksPinned(pinned));
            }
            ui.add_space(4.0);
            let mut scrollbar_left = self.scrollbar_left;
            if options_check(
                ui,
                true,
                &mut scrollbar_left,
                &t!("app-options-scrollbar-left"),
                t,
            )
            .changed()
            {
                *action = Some(DialogAction::SetScrollbarLeft(scrollbar_left));
            }
        });
    }

    /// Options › Proofing: the spelling language and the personal dictionary.
    fn options_proofing(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        options_group(ui, &t!("app-spelling"), t, |ui| {
            let current = self.editor.spell.setting().to_string();
            let automatic = t!(
                "app-spell-automatic",
                names = self.editor.spell.automatic_name()
            )
            .into_owned();
            let off = t!("app-spell-off").into_owned();
            let mut choices = vec![("auto".to_string(), automatic), ("off".to_string(), off)];
            choices.extend(
                self.editor
                    .spell
                    .dictionaries()
                    .iter()
                    .map(|dictionary| (dictionary.code.clone(), dictionary.name.clone())),
            );
            // A language kept from before whose dictionary is gone still reads as itself.
            if !choices.iter().any(|(code, _)| *code == current) {
                choices.push((current.clone(), crate::spell::language_name(&current)));
            }
            let mut chosen = current.clone();
            ui.horizontal(|ui| {
                if let Some(code) = dropdown(ui, &current, &choices, 260.0, t) {
                    chosen = code;
                }
                let words = self.editor.spell.personal_count();
                if words > 0
                    && widgets::text_button(
                        ui,
                        None,
                        &t!("app-clear-personal-words", count = words),
                        t,
                    )
                    .clicked()
                {
                    *action = Some(DialogAction::ClearPersonalDictionary);
                }
            });
            if chosen != current {
                *action = Some(DialogAction::SetSpellLanguage(chosen));
            }
            ui.label(
                RichText::new(self.editor.spell.status())
                    .font(theme::ui(11.5))
                    .color(t.faint),
            );
        });
    }

    /// Options › Save & Backup: where the notes are kept, and backing them up.
    fn options_save(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        options_group(ui, &t!("app-your-files"), t, |ui| {
            self.options_save_contents(ui, t, action);
        });
    }

    /// The data folder, what it holds, Back up and Open folder. Options › Save & Backup
    /// shows it, and so does Backstage › Info, so the two never drift apart.
    fn options_save_contents(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        let now = ui.input(|input| input.time);
        if let Some(info) = self.storage_info(now) {
            ui.label(
                RichText::new(info.database_path.clone())
                    .font(theme::ui(11.5))
                    .color(t.muted),
            );
            ui.label(
                RichText::new(t!(
                    "app-storage-stats",
                    pages = t!("app-page-count", count = info.page_count).into_owned(),
                    notebooks = t!("app-notebook-count", count = info.notebook_count).into_owned(),
                    database = util::file_size(info.database_bytes),
                    media = util::file_size(info.attachment_bytes)
                ))
                .font(theme::ui(11.5))
                .color(t.faint),
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, Some(icons::EXPORT), &t!("app-back-up"), t).clicked() {
                *action = Some(DialogAction::Backup);
            }
            if widgets::secondary_button(ui, Some(icons::FOLDER_OPEN), &t!("app-open-folder"), t)
                .clicked()
            {
                *action = Some(DialogAction::OpenDataFolder);
            }
        });
    }

    /// Options › Language: the language Folio's interface speaks.
    fn options_language(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        options_group(ui, &t!("app-options-display-language"), t, |ui| {
            ui.label(
                RichText::new(t!("app-options-display-language-body"))
                    .font(theme::ui(12.0))
                    .color(t.muted),
            );
            ui.add_space(6.0);
            let automatic = t!(
                "dialog-settings-language-automatic",
                language = i18n::startup_lang(None).endonym()
            );
            let mut choices = vec![(None, automatic.into_owned())];
            choices.extend(Lang::ALL.map(|lang| (Some(lang), lang.endonym().to_string())));
            if let Some(chosen) = dropdown(ui, &self.ui_language, &choices, 260.0, t) {
                *action = Some(DialogAction::SetUiLanguage(chosen));
            }
        });
    }

    /// Options › Advanced: the scope the header search starts in, inline maths, and how
    /// printouts are inserted.
    fn options_advanced(&mut self, ui: &mut Ui, t: Tokens, action: &mut Option<DialogAction>) {
        options_group(ui, &t!("app-options-search"), t, |ui| {
            options_row(ui, &t!("app-options-default-scope"), t, |ui| {
                let choices = ScopeKind::ALL.map(|scope| (scope, scope.label().into_owned()));
                if let Some(chosen) = dropdown(
                    ui,
                    &self.search.default_scope,
                    &choices,
                    OPTIONS_COMBO_WIDTH,
                    t,
                ) {
                    *action = Some(DialogAction::SetSearchScope(chosen));
                }
            });
        });
        // the desktop notebook's "95+83+416=" then Space, which a pupil may want off for exercises.
        options_group(ui, &t!("app-options-editing"), t, |ui| {
            let mut on = self.editor.auto_calc;
            if options_check(ui, true, &mut on, &t!("app-options-auto-calc"), t).changed() {
                *action = Some(DialogAction::SetAutoCalc(on));
            }
        });
        options_group(ui, &t!("printout-options-group"), t, |ui| {
            let mut background = self.printout_background;
            if options_check(
                ui,
                true,
                &mut background,
                &t!("printout-option-background"),
                t,
            )
            .changed()
            {
                *action = Some(DialogAction::SetPrintoutBackground(background));
            }
        });
    }

    fn dialog_modal(&mut self, ctx: &egui::Context, t: Tokens) {
        // Options' Cancel puts back the settings as they were when it opened: taken on its
        // first frame (it opens by being set, from menus and tests alike), dropped once it
        // is gone, however it went.
        if matches!(self.dialog, Some(Dialog::Settings)) {
            if self.options_before.is_none() {
                self.options_before = Some(self.options_snapshot());
            }
        } else {
            self.options_before = None;
        }
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut action: Option<DialogAction> = None;
        let mut close = false;
        // Options' Cancel, its title's × or Esc: its settings go back.
        let mut cancel_options = false;
        let response = Modal::new(Id::unique("folio-dialog"))
            .backdrop_color(Color32::from_black_alpha(if t.dark { 150 } else { 76 }))
            .frame(
                egui::Frame::new()
                    .fill(t.raised)
                    .stroke(Stroke::new(1.0, t.line_strong))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| match &mut dialog {
                Dialog::NewNotebook { name, color } => {
                    ui.set_width(420.0);
                    dialog_heading(
                        ui,
                        &t!("common-new-notebook"),
                        &t!("dialog-new-notebook-body"),
                        t,
                    );
                    let submit = name_field(
                        ui,
                        name,
                        "notebook-name",
                        &t!("dialog-notebook-name"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    color_field(ui, color, t);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, None, &t!("dialog-create-notebook"), t)
                            .clicked()
                            || submit
                        {
                            action = Some(DialogAction::CreateNotebook {
                                name: name.clone(),
                                color: color.clone(),
                            });
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::NewSection {
                    notebook_id,
                    group_id,
                    name,
                    color,
                } => {
                    ui.set_width(420.0);
                    dialog_heading(
                        ui,
                        &t!("common-new-section"),
                        &t!("dialog-new-section-body"),
                        t,
                    );
                    let submit = name_field(
                        ui,
                        name,
                        "section-name",
                        &t!("dialog-section-name"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    color_field(ui, color, t);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, None, &t!("dialog-create-section"), t)
                            .clicked()
                            || submit
                        {
                            action = Some(DialogAction::CreateSection {
                                notebook_id: notebook_id.clone(),
                                group_id: group_id.clone(),
                                name: name.clone(),
                                color: color.clone(),
                            });
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::NewGroup {
                    notebook_id,
                    parent_id,
                    name,
                    color,
                } => {
                    ui.set_width(420.0);
                    dialog_heading(
                        ui,
                        &t!("common-new-section-group"),
                        &t!("dialog-new-group-body"),
                        t,
                    );
                    let submit = name_field(
                        ui,
                        name,
                        "group-name",
                        &t!("dialog-group-name"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    color_field(ui, color, t);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, None, &t!("dialog-create-group"), t)
                            .clicked()
                            || submit
                        {
                            action = Some(DialogAction::CreateGroup {
                                notebook_id: notebook_id.clone(),
                                parent_id: parent_id.clone(),
                                name: name.clone(),
                                color: color.clone(),
                            });
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                // the desktop notebook's Notebook Properties: the display name and the colour.
                Dialog::RenameNotebook { id, name, color } => {
                    ui.set_width(420.0);
                    dialog_heading(
                        ui,
                        &t!("dialog-notebook-properties"),
                        &t!("dialog-rename-body"),
                        t,
                    );
                    // A label over the field: it always holds the name, so a hint never
                    // shows.
                    let display_name = t!("dialog-display-name");
                    widgets::caption(ui, &display_name, t.muted);
                    ui.add_space(4.0);
                    let submit = name_field(
                        ui,
                        name,
                        "notebook-name",
                        &display_name,
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    color_field(ui, color, t);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, None, &t!("dialog-save-changes"), t)
                            .clicked()
                            || submit
                        {
                            action = Some(DialogAction::RenameNotebook {
                                id: id.clone(),
                                name: name.clone(),
                                color: color.clone(),
                            });
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::RenameSection { id, name, color } => {
                    ui.set_width(420.0);
                    dialog_heading(
                        ui,
                        &t!("dialog-rename-section"),
                        &t!("dialog-rename-body"),
                        t,
                    );
                    let submit = name_field(
                        ui,
                        name,
                        "section-name",
                        &t!("dialog-section-name"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    color_field(ui, color, t);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, None, &t!("dialog-save-changes"), t)
                            .clicked()
                            || submit
                        {
                            action = Some(DialogAction::RenameSection {
                                id: id.clone(),
                                name: name.clone(),
                                color: color.clone(),
                            });
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::RenameGroup { id, name, color } => {
                    ui.set_width(420.0);
                    dialog_heading(ui, &t!("dialog-rename-group"), &t!("dialog-rename-body"), t);
                    let submit = name_field(
                        ui,
                        name,
                        "group-name",
                        &t!("dialog-group-name"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    color_field(ui, color, t);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, None, &t!("dialog-save-changes"), t)
                            .clicked()
                            || submit
                        {
                            action = Some(DialogAction::RenameGroup {
                                id: id.clone(),
                                name: name.clone(),
                                color: color.clone(),
                            });
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::ImportNotebook(step) => {
                    ui.set_width(460.0);
                    match step {
                        ImportDialog::Choose => {
                            dialog_heading(
                                ui,
                                &t!("import-notebook-title"),
                                &t!("import-notebook-body"),
                                t,
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if widgets::primary_button(
                                    ui,
                                    None,
                                    &t!("import-notebook-choose-file"),
                                    t,
                                )
                                .clicked()
                                {
                                    action = Some(DialogAction::ImportPick { folder: false });
                                    close = true;
                                }
                                if widgets::text_button(
                                    ui,
                                    None,
                                    &t!("import-notebook-choose-folder"),
                                    t,
                                )
                                .clicked()
                                {
                                    action = Some(DialogAction::ImportPick { folder: true });
                                    close = true;
                                }
                                if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked()
                                {
                                    close = true;
                                }
                            });
                        }
                        ImportDialog::Running => {
                            let (name, (done, total)) = self
                                .import_job
                                .as_ref()
                                .map_or((String::new(), (0, 0)), |job| {
                                    (job.name().to_string(), job.progress())
                                });
                            dialog_heading(
                                ui,
                                &t!("import-notebook-title"),
                                &t!("import-notebook-reading", name = name),
                                t,
                            );
                            let label = if total == 0 {
                                t!("import-notebook-opening")
                            } else {
                                t!(
                                    "import-notebook-progress",
                                    done = done as i64,
                                    total = total as i64
                                )
                            };
                            ui.label(RichText::new(label).font(theme::ui(12.0)).color(t.ink));
                            ui.add_space(6.0);
                            import_progress_bar(ui, done, total, t);
                            ui.add_space(14.0);
                            if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                                action = Some(DialogAction::ImportCancel);
                            }
                        }
                        ImportDialog::Done(summary) => {
                            let body = t!(
                                "import-notebook-done-body",
                                name = summary.name.clone(),
                                sections = summary.report.sections as i64,
                                pages = summary.report.pages as i64
                            );
                            dialog_heading(ui, &t!("import-notebook-done-title"), &body, t);
                            let (kept, skipped) = import_report_lines(&summary.report);
                            if !kept.is_empty() {
                                ui.label(t!("import-notebook-also", list = kept.join(", ")));
                            }
                            if !skipped.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(t!("import-notebook-skipped"))
                                        .font(theme::semibold(13.0))
                                        .color(t.ink),
                                );
                                ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                    for line in &skipped {
                                        ui.label(format!("• {line}"));
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            if widgets::primary_button(ui, None, &t!("common-close"), t).clicked() {
                                close = true;
                            }
                        }
                        ImportDialog::Failed(error) => {
                            dialog_heading(
                                ui,
                                &t!("import-notebook-failed-title"),
                                &import_error_text(error),
                                t,
                            );
                            ui.add_space(6.0);
                            if widgets::primary_button(ui, None, &t!("common-close"), t).clicked() {
                                close = true;
                            }
                        }
                    }
                }
                Dialog::Confirm {
                    title,
                    body,
                    confirm_label,
                    action: confirm,
                } => {
                    ui.set_width(430.0);
                    dialog_heading(ui, title, body, t);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if widgets::danger_button(ui, confirm_label, t).clicked() {
                            action = Some(DialogAction::Confirm(confirm.clone()));
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                // the desktop notebook's Move or Copy Pages / Section and Merge Section: a search field,
                // the tree to pick from, then Move and Copy (or Merge, which asks again).
                Dialog::MoveOrCopy {
                    what,
                    filter,
                    target,
                    confirm,
                } => {
                    ui.set_width(MOVE_DIALOG_WIDTH);
                    // A page's title and own section; a section's name.
                    let page = match &*what {
                        MoveWhat::Page(id) => Some(self.move_page_place(id)),
                        _ => None,
                    };
                    let name = match &*what {
                        MoveWhat::Section(id) | MoveWhat::Merge(id) => self.section_name(id),
                        MoveWhat::Page(_) => String::new(),
                    };
                    let (title, body) = match &*what {
                        MoveWhat::Page(_) => (
                            t!("dialog-move-copy-pages"),
                            t!(
                                "dialog-move-copy-pages-body",
                                title = display_title(page.as_ref().map_or("", |(title, _)| title))
                                    .into_owned()
                            ),
                        ),
                        MoveWhat::Section(_) => (
                            t!("dialog-move-copy-section"),
                            t!("dialog-move-copy-section-body", name = name.clone()),
                        ),
                        MoveWhat::Merge(_) => (
                            t!("dialog-merge-section"),
                            t!("dialog-merge-body", name = name.clone()),
                        ),
                    };
                    dialog_heading(ui, &title, &body, t);
                    let _ = name_field(
                        ui,
                        filter,
                        "move-filter",
                        &t!("dialog-move-copy-search"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    let picked = target.clone();
                    move_tree(ui, &self.workspace, what, filter, target, t);
                    // Merge asks again for the section it now names.
                    if *target != picked {
                        *confirm = false;
                    }
                    // Pages and merges go into a section; a section into a notebook or group.
                    let into = match &*target {
                        Some(MoveTarget::Section(id)) => Some(id.clone()),
                        _ => None,
                    };
                    ui.add_space(12.0);
                    if let (true, MoveWhat::Merge(_), Some(into)) = (*confirm, &*what, &into) {
                        ui.label(
                            RichText::new(t!(
                                "dialog-merge-confirm",
                                name = name.clone(),
                                target = self.section_name(into)
                            ))
                            .font(theme::ui(12.0))
                            .color(t.danger),
                        );
                        ui.add_space(8.0);
                    }
                    ui.horizontal(|ui| {
                        // Laid out right to left: Cancel at the far right, then Copy, then
                        // Move (or Merge alone).
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                                close = true;
                            }
                            match &*what {
                                MoveWhat::Page(id) => {
                                    let own =
                                        page.as_ref().and_then(|(_, section)| section.as_deref());
                                    let copy = ui
                                        .add_enabled_ui(into.is_some(), |ui| {
                                            widgets::secondary_button(
                                                ui,
                                                None,
                                                &t!("dialog-copy"),
                                                t,
                                            )
                                        })
                                        .inner
                                        .clicked();
                                    // A page already in that section has nowhere to move.
                                    let movable = into.is_some() && into.as_deref() != own;
                                    let moved = ui
                                        .add_enabled_ui(movable, |ui| {
                                            widgets::primary_button(ui, None, &t!("dialog-move"), t)
                                        })
                                        .inner
                                        .clicked();
                                    if let Some(section) = into.clone() {
                                        if moved {
                                            action = Some(DialogAction::MovePage {
                                                id: id.clone(),
                                                section,
                                            });
                                            close = true;
                                        } else if copy {
                                            action = Some(DialogAction::CopyPage {
                                                id: id.clone(),
                                                section,
                                            });
                                            close = true;
                                        }
                                    }
                                }
                                MoveWhat::Section(id) => {
                                    let place = match &*target {
                                        Some(MoveTarget::Notebook(notebook)) => {
                                            Some((notebook.clone(), None))
                                        }
                                        Some(MoveTarget::Group { notebook, id }) => {
                                            Some((notebook.clone(), Some(id.clone())))
                                        }
                                        _ => None,
                                    };
                                    let copy = ui
                                        .add_enabled_ui(place.is_some(), |ui| {
                                            widgets::secondary_button(
                                                ui,
                                                None,
                                                &t!("dialog-copy"),
                                                t,
                                            )
                                        })
                                        .inner
                                        .clicked();
                                    let moved = ui
                                        .add_enabled_ui(place.is_some(), |ui| {
                                            widgets::primary_button(ui, None, &t!("dialog-move"), t)
                                        })
                                        .inner
                                        .clicked();
                                    if let Some((notebook, group)) = place {
                                        if moved {
                                            action = Some(DialogAction::MoveSection {
                                                id: id.clone(),
                                                notebook,
                                                group,
                                            });
                                            close = true;
                                        } else if copy {
                                            action = Some(DialogAction::CopySection {
                                                id: id.clone(),
                                                notebook,
                                                group,
                                            });
                                            close = true;
                                        }
                                    }
                                }
                                MoveWhat::Merge(id) => {
                                    let label = t!("dialog-merge");
                                    let merge = ui
                                        .add_enabled_ui(into.is_some(), |ui| {
                                            if *confirm {
                                                widgets::danger_button(ui, &label, t)
                                            } else {
                                                widgets::primary_button(ui, None, &label, t)
                                            }
                                        })
                                        .inner
                                        .clicked();
                                    if let Some(into) = into.clone().filter(|_| merge) {
                                        if *confirm {
                                            action = Some(DialogAction::MergeSection {
                                                from: id.clone(),
                                                into,
                                            });
                                            close = true;
                                        } else {
                                            *confirm = true;
                                        }
                                    }
                                }
                            }
                        });
                    });
                }
                Dialog::Link {
                    edit,
                    query,
                    searched,
                    choices,
                } => {
                    ui.set_width(440.0);
                    dialog_heading(
                        ui,
                        &if edit.existing {
                            t!("dialog-edit-link")
                        } else {
                            t!("dialog-insert-link")
                        },
                        &t!("dialog-link-body"),
                        t,
                    );
                    widgets::caption(ui, &t!("dialog-link-text"), t.muted);
                    ui.add_space(3.0);
                    let _ = name_field(
                        ui,
                        &mut edit.text,
                        "link-text",
                        &t!("dialog-link-text"),
                        false,
                        t,
                    );
                    widgets::caption(ui, &t!("dialog-link-address"), t.muted);
                    ui.add_space(3.0);
                    let submit = name_field(
                        ui,
                        &mut edit.url,
                        "link-url",
                        &t!("dialog-link-address-hint"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's very first frame is an invisible sizing pass that can't
                    // take focus; ask again on the next one.
                    self.dialog_focus = ui.is_sizing_pass();
                    widgets::caption(ui, &t!("dialog-link-page"), t.muted);
                    ui.add_space(3.0);
                    let _ = name_field(
                        ui,
                        query,
                        "link-search",
                        &t!("dialog-link-search"),
                        false,
                        t,
                    );
                    if searched.as_deref() != Some(query.as_str()) {
                        *choices = self.link_choices(query);
                        *searched = Some(query.clone());
                    }
                    for choice in choices.iter() {
                        // Title on the left, its notebook and section on the right.
                        let row = egui::Button::new(
                            RichText::new(format!("{}    {}", icons::FILE_TEXT, choice.title))
                                .font(theme::ui(12.5))
                                .color(t.ink),
                        )
                        .shortcut_text(
                            RichText::new(&choice.place)
                                .font(theme::ui(11.0))
                                .color(t.faint),
                        )
                        .frame(false)
                        .min_size(vec2(ui.available_width(), 28.0));
                        if ui.add(row).clicked() {
                            edit.url = page_link(&choice.id);
                            if edit.text.trim().is_empty() {
                                edit.text = choice.title.clone();
                            }
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let label = if edit.existing {
                            t!("dialog-save-link")
                        } else {
                            t!("dialog-insert-link")
                        };
                        if widgets::primary_button(ui, Some(icons::LINK), &label, t).clicked()
                            || submit
                        {
                            action = Some(DialogAction::ApplyLink(edit.clone()));
                            close = true;
                        }
                        if edit.existing
                            && widgets::secondary_button(
                                ui,
                                Some(icons::LINK_BREAK),
                                &t!("dialog-remove-link"),
                                t,
                            )
                            .clicked()
                        {
                            let mut removed = edit.clone();
                            removed.url.clear();
                            action = Some(DialogAction::ApplyLink(removed));
                            close = true;
                        }
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Settings => {
                    ui.set_width(OPTIONS_WIDTH);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(t!("app-options-title"))
                                .font(theme::ui(16.0))
                                .color(t.ink),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::icon_button_sized(
                                ui,
                                icons::X,
                                &t!("common-close"),
                                28.0,
                                14.0,
                                t,
                            )
                            .clicked()
                            {
                                close = true;
                                cancel_options = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.horizontal_top(|ui| {
                        // the desktop notebook boxes its list of pages: a hairline frame on the window grey.
                        egui::Frame::new()
                            .fill(t.panel)
                            .stroke(Stroke::new(1.0, t.line))
                            .inner_margin(egui::Margin::symmetric(4, 6))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(OPTIONS_LIST_WIDTH);
                                    ui.set_min_height(OPTIONS_HEIGHT - 12.0);
                                    for page in OptionsPage::ALL {
                                        if options_tab(ui, page, self.options_page == page, t)
                                            .clicked()
                                        {
                                            self.options_page = page;
                                        }
                                    }
                                });
                            });
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.set_width(OPTIONS_PAGE_WIDTH);
                            let page = self.options_page;
                            ScrollArea::vertical()
                                .id_salt("options-page")
                                .max_height(OPTIONS_HEIGHT)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(page.glyph())
                                                .font(theme::icon(20.0))
                                                .color(t.muted),
                                        );
                                        ui.label(
                                            RichText::new(page.caption())
                                                .font(theme::ui(13.0))
                                                .color(t.ink),
                                        );
                                    });
                                    ui.add_space(14.0);
                                    match page {
                                        OptionsPage::General => {
                                            self.options_general(ui, t, &mut action)
                                        }
                                        OptionsPage::Display => {
                                            self.options_display(ui, t, &mut action)
                                        }
                                        OptionsPage::Proofing => {
                                            self.options_proofing(ui, t, &mut action)
                                        }
                                        OptionsPage::Save => self.options_save(ui, t, &mut action),
                                        OptionsPage::Language => {
                                            self.options_language(ui, t, &mut action)
                                        }
                                        OptionsPage::Advanced => {
                                            self.options_advanced(ui, t, &mut action)
                                        }
                                    }
                                });
                        });
                    });
                    ui.add_space(10.0);
                    widgets::hairline(ui, t.line);
                    ui.add_space(10.0);
                    // Options apply as soon as they change, so the result shows at once;
                    // Cancel puts back what this opening of the dialog changed.
                    dialog_buttons(ui, |ui| {
                        // Right to left: OK then Cancel, the desktop notebook's order.
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                            cancel_options = true;
                        }
                        if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::PageColor { color, hex } => {
                    ui.set_width(320.0);
                    ui.label(
                        RichText::new(t!("ribbon-page-color"))
                            .font(theme::ui(16.0))
                            .color(t.ink),
                    );
                    color_dialog_body(ui, color, hex, t);
                    dialog_buttons(ui, |ui| {
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                        if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                            let [r, g, b, _] = color.to_array();
                            let color = PageColor::Custom(Rgb(r, g, b));
                            action = Some(DialogAction::Run(Cmd::PageColor(Some(color))));
                            close = true;
                        }
                    });
                }
                Dialog::MoreColors { target, color, hex } => {
                    ui.set_width(320.0);
                    // the desktop notebook titles it after its button, Font Color.
                    let title = match target {
                        ColorTarget::Font => t!("app-tag-font-color"),
                        ColorTarget::Pen => t!("ribbon-more-colors-title"),
                    };
                    ui.label(RichText::new(title).font(theme::ui(16.0)).color(t.ink));
                    color_dialog_body(ui, color, hex, t);
                    dialog_buttons(ui, |ui| {
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                        if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                            let [r, g, b, _] = color.to_array();
                            let rgb = Rgb(r, g, b);
                            action = Some(DialogAction::Run(match target {
                                ColorTarget::Font => Cmd::Text(ToolbarCommand::Color(Some(rgb))),
                                ColorTarget::Pen => Cmd::Ink(rgb),
                            }));
                            close = true;
                        }
                    });
                }
                Dialog::Symbols { set, query, picked } => {
                    ui.set_width(SYMBOLS_DIALOG_WIDTH);
                    ui.label(
                        RichText::new(t!("ribbon-symbol"))
                            .font(theme::ui(16.0))
                            .color(t.ink),
                    );
                    ui.add_space(10.0);
                    let _ = name_field(
                        ui,
                        query,
                        "symbol-search",
                        &t!("editor-symbol-search"),
                        self.dialog_focus,
                        t,
                    );
                    // The dialog's first frame is a sizing pass that can't take focus.
                    self.dialog_focus = ui.is_sizing_pass();
                    // The sets: picking one shows it whole, whatever was searched.
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        for (index, (name, _)) in editor::SYMBOL_SETS.iter().enumerate() {
                            let on = query.is_empty() && *set == index;
                            if widgets::toggle_chip(ui, on, &i18n::tr(name), t).clicked() {
                                *set = index;
                                query.clear();
                            }
                        }
                    });
                    ui.add_space(10.0);
                    let mut insert = None;
                    ScrollArea::vertical()
                        .max_height(SYMBOL_ROWS * 30.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                            let shown = symbols_shown(*set, query);
                            if shown.is_empty() {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(t!("editor-no-symbols"))
                                        .font(theme::ui(12.0))
                                        .color(t.muted),
                                );
                            }
                            for row in shown.chunks(SYMBOL_COLUMNS) {
                                ui.horizontal(|ui| {
                                    for &(symbol, names) in row {
                                        let text = symbol.to_string();
                                        let current = *picked == Some(symbol);
                                        let response = editor::glyph_button(
                                            ui,
                                            &text,
                                            theme::body(17.0),
                                            current,
                                            t,
                                        )
                                        .on_hover_text(names);
                                        // A click picks it; a double-click puts it in.
                                        if response.clicked() {
                                            *picked = Some(symbol);
                                        }
                                        if response.double_clicked() {
                                            insert = Some(symbol);
                                        }
                                    }
                                });
                            }
                        });
                    ui.add_space(10.0);
                    dialog_buttons(ui, |ui| {
                        if widgets::text_button(ui, None, &t!("common-close"), t).clicked() {
                            close = true;
                        }
                        let chosen = *picked;
                        ui.add_enabled_ui(chosen.is_some(), |ui| {
                            if widgets::primary_button(ui, None, &t!("editor-insert"), t).clicked()
                            {
                                insert = chosen;
                            }
                        });
                    });
                    // It goes in at the caret, the note's or the title's, as from the
                    // ribbon, and the dialog stays up for the next one, as Office's does.
                    if let Some(symbol) = insert {
                        if !self.canvas.title_active {
                            self.editor.restore_focus();
                        }
                        action = Some(DialogAction::Run(Cmd::Symbol(symbol)));
                    }
                }
                Dialog::CustomizeNumbering { marker, start } => {
                    ui.set_width(320.0);
                    ui.label(
                        RichText::new(t!("ribbon-customize-numbering-title"))
                            .font(theme::ui(16.0))
                            .color(t.ink),
                    );
                    ui.add_space(10.0);
                    // The formats, each showing the number the paragraph starts at.
                    let gap = 4.0;
                    let size = vec2((320.0 - 3.0 * gap) / 4.0, 36.0);
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing = vec2(gap, gap);
                        for row in ListMarker::NUMBERS.chunks(4) {
                            ui.horizontal(|ui| {
                                for &format in row {
                                    let text = format.text(*start as usize);
                                    let current = *marker == format;
                                    if editor::glyph_tile(
                                        ui,
                                        size,
                                        &text,
                                        theme::body(15.0),
                                        current,
                                        t,
                                    )
                                    .clicked()
                                    {
                                        *marker = format;
                                    }
                                }
                            });
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let label = ui.label(
                            RichText::new(t!("ribbon-numbering-start-at"))
                                .font(theme::ui(13.0))
                                .color(t.ink),
                        );
                        ui.add(
                            egui::DragValue::new(start)
                                .range(1..=MAX_LIST_START)
                                .clamp_existing_to_range(false),
                        )
                        .labelled_by(label.id);
                    });
                    ui.add_space(14.0);
                    dialog_buttons(ui, |ui| {
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                        if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                            // The style, which makes the paragraph a numbered item, and its
                            // start: one command, so one Ctrl+Z takes both back.
                            action = Some(DialogAction::Run(Cmd::Text(ToolbarCommand::Numbering(
                                *marker,
                                ListStart::At(*start),
                            ))));
                            close = true;
                        }
                    });
                }
                Dialog::ParagraphSpacing {
                    before,
                    after,
                    line,
                } => {
                    ui.set_width(300.0);
                    ui.label(
                        RichText::new(t!("editor-paragraph-spacing-title"))
                            .font(theme::ui(16.0))
                            .color(t.ink),
                    );
                    ui.add_space(10.0);
                    let font = theme::ui(13.0);
                    let rows = [
                        (t!("editor-spacing-before"), &mut *before),
                        (t!("editor-spacing-after"), &mut *after),
                        (t!("editor-spacing-line"), &mut *line),
                    ];
                    // The labels make one column and the fields another, as the desktop notebook's do.
                    let column = rows
                        .iter()
                        .map(|(label, _)| widgets::text_width(ui.ctx(), label, font.clone()))
                        .fold(0.0, f32::max)
                        + 12.0;
                    for (label, value) in rows {
                        ui.horizontal(|ui| {
                            let label = ui
                                .allocate_ui_with_layout(
                                    vec2(column, 24.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_min_size(vec2(column, 24.0));
                                        ui.label(
                                            RichText::new(label).font(font.clone()).color(t.ink),
                                        )
                                    },
                                )
                                .inner;
                            ui.add(
                                egui::DragValue::new(value)
                                    .range(0.0..=MAX_PARA_SPACING)
                                    .speed(1.0)
                                    .max_decimals(1)
                                    .suffix(" pt")
                                    .clamp_existing_to_range(false),
                            )
                            .labelled_by(label.id);
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(10.0);
                    dialog_buttons(ui, |ui| {
                        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
                            close = true;
                        }
                        if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                            action = Some(DialogAction::Run(Cmd::Text(ToolbarCommand::Spacing(
                                ParaSpacing {
                                    before: *before,
                                    after: *after,
                                    line_at_least: *line,
                                },
                            ))));
                            close = true;
                        }
                    });
                }
                Dialog::CustomizeTags(state) => {
                    ui.set_width(460.0);
                    if state.editing.is_some() {
                        tag_form(ui, state, &mut self.dialog_focus, t);
                    } else if let Some(ok) =
                        customize_tags_list(ui, state, &mut self.dialog_focus, t)
                    {
                        if ok {
                            action = Some(DialogAction::SaveTags(state.gallery.clone()));
                        }
                        close = true;
                    }
                }
                Dialog::About => {
                    ui.set_width(420.0);
                    // The title row, as the Options dialog draws it.
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(t!("dialog-about-title"))
                                .font(theme::ui(16.0))
                                .color(t.ink),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::icon_button_sized(
                                ui,
                                icons::X,
                                &t!("common-close"),
                                28.0,
                                14.0,
                                t,
                            )
                            .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let (mark, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(40.0), Sense::hover());
                        paint_mark(ui.painter(), mark);
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(APP_NAME)
                                .font(theme::light(theme::TITLE_SIZE))
                                .color(t.ink),
                        );
                    });
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(t!(
                            "app-settings-footer",
                            version = env!("CARGO_PKG_VERSION")
                        ))
                        .font(theme::ui(12.0))
                        .color(t.ink),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(t!("dialog-about-body"))
                            .font(theme::ui(13.0))
                            .color(t.ink),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(t!("dialog-about-licences"))
                            .font(theme::ui(12.0))
                            .color(t.muted),
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
                                close = true;
                            }
                        });
                    });
                }
                Dialog::Shortcuts => {
                    ui.set_width(540.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(t!("common-keyboard-shortcuts"))
                                .font(theme::semibold(20.0))
                                .color(t.ink),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::icon_button_sized(
                                ui,
                                icons::X,
                                &t!("common-close"),
                                28.0,
                                14.0,
                                t,
                            )
                            .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    widgets::hairline(ui, t.line);
                    ui.add_space(8.0);
                    let mac = cfg!(target_os = "macos");
                    let tab = t!("key-tab");
                    let enter = if mac {
                        "↵".to_string()
                    } else {
                        t!("key-enter").into_owned()
                    };
                    let alt = if mac {
                        "⌥".to_string()
                    } else {
                        crate::i18n::modifier_names().alt.to_string()
                    };
                    let names = crate::i18n::modifier_names();
                    let (home, end) = (t!("key-home"), t!("key-end"));
                    let (page_up, page_down) = (t!("key-page-up"), t!("key-page-down"));
                    let line_break = util::shift_key(&enter);
                    let mut rows = vec![
                        (t!("common-search-everything"), util::shortcut("E")),
                        (t!("common-new-page"), util::shortcut("N")),
                        (t!("common-new-section"), util::shortcut("T")),
                        (t!("common-new-notebook"), util::shift_shortcut("N")),
                        (t!("ribbon-new-quick-note"), util::shift_shortcut("M")),
                        (t!("app-shortcut-save"), util::shortcut("S")),
                        (t!("app-shortcut-print"), util::shortcut("P")),
                        (t!("app-shortcut-file-tab"), format!("{alt} F")),
                        (t!("app-shortcut-spelling"), "F7".to_string()),
                        (t!("app-shortcut-title"), util::shift_shortcut("T")),
                        (t!("app-shortcut-rule-lines"), util::shift_shortcut("R")),
                        (t!("app-full-page-view"), "F11".to_string()),
                        (t!("app-shortcut-ribbon"), util::shortcut("F1")),
                        (t!("app-shortcut-find"), util::shortcut("F")),
                        (t!("app-backstage-options"), util::shortcut(",")),
                        (
                            t!("app-shortcut-format"),
                            format!(
                                "{} · {} · {}",
                                util::shortcut("B"),
                                util::shortcut("I"),
                                util::shortcut("U")
                            ),
                        ),
                        (t!("app-shortcut-link"), util::shortcut("K")),
                        (
                            t!("app-shortcut-open-link"),
                            util::shortcut(&t!("app-key-click")),
                        ),
                        (
                            t!("app-shortcut-text-size"),
                            format!(
                                "{} · {}",
                                util::shift_shortcut(">"),
                                util::shift_shortcut("<")
                            ),
                        ),
                        (
                            t!("app-shortcut-scripts"),
                            format!("{} · {}", util::shift_shortcut("="), util::shortcut("=")),
                        ),
                        (
                            t!("app-shortcut-align"),
                            format!("{} · {}", util::shortcut("L"), util::shortcut("R")),
                        ),
                        (t!("app-shortcut-clear-format"), util::shift_shortcut("N")),
                        (t!("app-shortcut-new-block"), enter),
                        (
                            t!("app-shortcut-indent"),
                            format!(
                                "{tab} / {} · {} / {}",
                                util::shift_key(&tab),
                                util::alt_shift_key("→"),
                                util::alt_shift_key("←")
                            ),
                        ),
                        (
                            t!("app-shortcut-undo"),
                            format!("{} · {}", util::shortcut("Z"), util::shift_shortcut("Z")),
                        ),
                        (
                            t!("app-shortcut-headings"),
                            format!("{} … {}", util::alt_shortcut("1"), util::alt_shortcut("6")),
                        ),
                        (
                            t!("app-shortcut-move-paragraph"),
                            format!(
                                "{} · {}",
                                util::alt_shift_key("↑"),
                                util::alt_shift_key("↓")
                            ),
                        ),
                        (
                            t!("app-shortcut-show-levels"),
                            format!(
                                "{} … {}",
                                util::alt_shift_key("0"),
                                util::alt_shift_key("9")
                            ),
                        ),
                        (
                            t!("app-shortcut-expand-collapse"),
                            format!(
                                "{} · {}",
                                util::alt_shift_key("+"),
                                util::alt_shift_key("-")
                            ),
                        ),
                        (t!("app-shortcut-select-all"), util::shortcut("A")),
                        (
                            t!("app-shortcut-pan"),
                            format!("{} + {}", t!("key-space"), t!("app-key-drag")),
                        ),
                        (t!("app-shortcut-equation"), format!("{alt} =")),
                        (
                            t!("app-shortcut-date-time"),
                            format!(
                                "{} · {} · {}",
                                util::alt_shift_key("D"),
                                util::alt_shift_key("T"),
                                util::alt_shift_key("F")
                            ),
                        ),
                        (
                            t!("app-shortcut-page-below"),
                            format!(
                                "{} · {}",
                                util::alt_shortcut("N"),
                                util::alt_shift_shortcut("N")
                            ),
                        ),
                        (
                            t!("app-shortcut-page-level"),
                            format!("{} · {}", util::alt_shortcut("]"), util::alt_shortcut("[")),
                        ),
                        (t!("app-shortcut-move-page"), util::alt_shortcut("M")),
                        (
                            t!("app-shortcut-tags"),
                            format!("{} … {}", util::shortcut("1"), util::shortcut("9")),
                        ),
                        (t!("app-shortcut-remove-tags"), util::shortcut("0")),
                        (t!("app-shortcut-line-break"), line_break),
                        (
                            t!("app-shortcut-paragraphs"),
                            format!("{} · {}", util::shortcut("↑"), util::shortcut("↓")),
                        ),
                        (
                            t!("app-shortcut-page-ends"),
                            format!("{} · {}", util::shortcut(&home), util::shortcut(&end)),
                        ),
                        (
                            t!("app-shortcut-scroll"),
                            format!("{page_up} · {page_down}"),
                        ),
                        (
                            t!("app-shortcut-pages"),
                            format!(
                                "{} · {}",
                                util::shortcut(&page_up),
                                util::shortcut(&page_down)
                            ),
                        ),
                        (
                            t!("app-shortcut-first-last-page"),
                            format!("{alt} {home} · {alt} {end}"),
                        ),
                        (
                            t!("app-shortcut-pages-in-view"),
                            format!("{alt} {page_up} · {alt} {page_down}"),
                        ),
                        (
                            t!("app-shortcut-move-page-order"),
                            format!(
                                "{} · {}",
                                util::alt_shift_key("↑"),
                                util::alt_shift_key("↓")
                            ),
                        ),
                        (
                            t!("app-shortcut-sections"),
                            format!(
                                "{} {tab} · {} {} {tab}",
                                names.ctrl, names.ctrl, names.shift
                            ),
                        ),
                        (
                            t!("app-shortcut-zoom"),
                            format!("{} · {}", util::shortcut("+"), util::shortcut("-")),
                        ),
                    ];
                    // Back and Forward: not on macOS, where ⌥← / ⌥→ move by word.
                    if !mac {
                        rows.push((
                            t!("app-shortcut-back-forward"),
                            format!("{alt} ← · {alt} →"),
                        ));
                    }
                    // The list scrolls rather than pushing the dialog's title off a short
                    // window. Its height is set, not taken from the room left: the dialogs
                    // share one modal, which starts at the last one's size (About's, say).
                    let height = (ui.ctx().content_rect().height() - 220.0).max(240.0);
                    ScrollArea::vertical()
                        .max_height(height)
                        .min_scrolled_height(height)
                        .show(ui, |ui| {
                            for (label, key) in rows {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(label).font(theme::ui(12.0)).color(t.muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        widgets::keycap(ui, &key, t);
                                    });
                                });
                                ui.add_space(2.0);
                            }
                        });
                }
            });
        // A running import's dialog stays up until the import ends: closed, the import would
        // carry on and land unseen. Only its Cancel stops it.
        if response.should_close()
            && !matches!(dialog, Dialog::ImportNotebook(ImportDialog::Running))
        {
            match &mut dialog {
                // Escape in New Tag or Modify Tag leaves that form alone, as the desktop notebook's own
                // dialog over Customize Tags closes: the list keeps its changes.
                Dialog::CustomizeTags(state) if state.editing.is_some() => state.editing = None,
                Dialog::Settings => {
                    close = true;
                    cancel_options = true;
                }
                _ => close = true,
            }
        }
        let keep_open = matches!(
            action,
            Some(DialogAction::SetAppearance(_))
                | Some(DialogAction::Backup)
                | Some(DialogAction::OpenDataFolder)
                | Some(DialogAction::SetSpellLanguage(_))
                | Some(DialogAction::SetUiLanguage(_))
                | Some(DialogAction::ClearPersonalDictionary)
                | Some(DialogAction::Run(_))
                | Some(DialogAction::SetPageListLeft(_))
                | Some(DialogAction::SetNotebooksPinned(_))
                | Some(DialogAction::SetScrollbarLeft(_))
                | Some(DialogAction::SetSearchScope(_))
                | Some(DialogAction::SetPrintoutBackground(_))
                | Some(DialogAction::SetMiniToolbar(_))
                | Some(DialogAction::SetAutoCalc(_))
                | Some(DialogAction::ImportCancel)
        );
        if close && action.is_none() && matches!(dialog, Dialog::Link { .. }) {
            self.editor.restore_focus();
        }
        // The paragraph being typed in gets the keyboard back, whichever button closed it,
        // or the title, when More Symbols typed into it. The font colour lands on the
        // selection it kept while the dialog was up.
        if close && matches!(dialog, Dialog::Symbols { .. }) && self.canvas.title_active {
            self.canvas.refocus_title = true;
        } else if close
            && matches!(
                dialog,
                Dialog::CustomizeTags(_)
                    | Dialog::Symbols { .. }
                    | Dialog::CustomizeNumbering { .. }
                    | Dialog::ParagraphSpacing { .. }
                    | Dialog::MoreColors {
                        target: ColorTarget::Font,
                        ..
                    }
            )
        {
            self.editor.restore_focus();
        }
        let before = if close && matches!(dialog, Dialog::Settings) {
            self.options_before.take()
        } else {
            None
        };
        if close || (action.is_some() && !keep_open) {
            self.dialog = None;
        } else {
            self.dialog = Some(dialog);
        }
        if let Some(action) = action {
            self.apply_dialog_action(action, ctx);
        }
        if let Some(before) = before.filter(|_| cancel_options) {
            self.restore_options(before, ctx);
        }
    }

    fn show_toast(&mut self, ctx: &egui::Context, t: Tokens) {
        if self.toast.message.is_empty() {
            return;
        }
        let now = now_ms() as f64 / 1000.0;
        if now > self.toast.expires_at {
            self.toast.message.clear();
            self.toast.expires_at = 0.0;
            return;
        }
        // Office's notification: white text on dark grey, low over the window's bottom edge.
        egui::Area::new(Id::unique("folio-toast"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.ink)
                    .corner_radius(CornerRadius::same(6))
                    .shadow(egui::Shadow {
                        offset: [0, 2],
                        blur: 8,
                        spread: 0,
                        color: t.card_shadow,
                    })
                    .inner_margin(Margin::symmetric(14, 9))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let glyph = if self.toast.error {
                                icons::WARNING
                            } else {
                                icons::CHECK_CIRCLE
                            };
                            ui.label(RichText::new(glyph).font(theme::icon(13.0)).color(t.raised));
                            ui.label(
                                RichText::new(&self.toast.message)
                                    .font(theme::ui(12.5))
                                    .color(t.raised),
                            );
                        });
                    });
            });
    }
}

impl eframe::App for FolioApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let t = theme::tokens(ui.ctx());
        let now = ui.input(|input| input.time);
        self.draw(ui, t, now);
    }

    fn on_exit(&mut self) {
        // Finish a recording in progress so its file is complete and its clip has a length.
        match self
            .canvas
            .finish_recording(self.page.as_mut(), &mut self.editor, &mut self.library)
        {
            Ok(true) => self.dirty = true,
            Ok(false) => {}
            Err(error) => eprintln!("folio: the recording could not be finished: {error}"),
        }
        // A printout still being inserted leaves nothing behind.
        self.canvas.abandon_printout(&mut self.library);
        if !self.flush_save() {
            eprintln!("folio: the last edits to the open page could not be saved");
        }
    }
}

/// A task pane's header: its title, and Close at the right. Returns true when Close was
/// clicked.
fn pane_header(ui: &mut Ui, title: &str, t: Tokens) -> bool {
    pane_header_with(ui, title, t, |_| {})
}

/// `pane_header` with more buttons left of Close: `buttons` adds them right to left. A
/// title too long for the pane is cut short.
fn pane_header_with(ui: &mut Ui, title: &str, t: Tokens, buttons: impl FnOnce(&mut Ui)) -> bool {
    let mut close = false;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 26.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            close = widgets::icon_button_sized(ui, icons::X, &t!("common-close"), 24.0, 12.0, t)
                .clicked();
            buttons(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(theme::semibold(14.0))
                            .color(t.ink),
                    )
                    .truncate(),
                );
            });
        },
    );
    close
}

/// One page in a list of results (the results pane, and the search box's dropdown): its
/// title, "Notebook › Section", and `line3` (the excerpt, or when it was edited).
/// `selected` marks the open page, or the row the keyboard is on.
fn hit_row(ui: &mut Ui, hit: &SearchHit, line3: &str, selected: bool, t: Tokens) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let title = display_title(&hit.title);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &title));
    // A long list repaints with the pane: rows scrolled out of view cost nothing.
    if !ui.is_rect_visible(rect) {
        return response;
    }
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(3),
            if selected { t.page_selected } else { t.hover },
        );
    }
    let left = rect.left() + 8.0;
    let width = rect.width() - 16.0;
    widgets::fitted_text(
        ui.painter(),
        pos2(left, rect.top() + 13.0),
        Align2::LEFT_CENTER,
        title.to_string(),
        theme::semibold(13.0),
        t.ink,
        width,
    );
    widgets::fitted_text(
        ui.painter(),
        pos2(left, rect.top() + 27.0),
        Align2::LEFT_CENTER,
        format!("{} › {}", hit.notebook_name, hit.section_name),
        theme::ui(11.5),
        t.muted,
        width,
    );
    widgets::fitted_text(
        ui.painter(),
        pos2(left, rect.top() + 41.0),
        Align2::LEFT_CENTER,
        line3,
        theme::ui(11.5),
        t.faint,
        width,
    );
    response
}

/// A choice in a framed box with a chevron, as Folio draws its dropdowns (the ribbon's
/// Font box, the Mini Toolbar's), and a menu of the `choices` with the current one ticked:
/// Tags Summary › Group tags by, and Options' boxes. The box fits the longest choice and is
/// at least `min_width` wide. Returns the one picked, when it is not the current one.
fn dropdown<T: Clone + PartialEq>(
    ui: &mut Ui,
    current: &T,
    choices: &[(T, String)],
    min_width: f32,
    t: Tokens,
) -> Option<T> {
    let font = theme::ui(12.0);
    let widest = choices
        .iter()
        .map(|(_, label)| widgets::text_width(ui.ctx(), label, font.clone()))
        .fold(0.0, f32::max);
    let size = vec2((widest + 6.0 + 22.0).max(min_width), 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let value = choices
        .iter()
        .find(|(choice, _)| choice == current)
        .map_or("", |(_, label)| label.as_str());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, value);
        info.current_text_value = Some(value.to_string());
        info
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let border = if response.hovered() {
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
        widgets::fitted_text(
            painter,
            pos2(rect.left() + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            value,
            font,
            t.ink,
            rect.width() - 6.0 - 18.0,
        );
        painter.text(
            pos2(rect.right() - 9.0, rect.center().y),
            Align2::CENTER_CENTER,
            icons::CARET_DOWN,
            theme::icon(10.0),
            t.muted,
        );
    }
    egui::Popup::menu(&response)
        .show(|ui| {
            ui.set_min_width(rect.width());
            let mut picked = None;
            for (choice, label) in choices {
                let row =
                    widgets::checked_menu_row(ui, None, label, "", true, choice == current, t);
                if row.clicked() {
                    picked = Some(choice.clone());
                    ui.close();
                }
            }
            picked
        })
        .and_then(|inner| inner.inner)
        .filter(|picked| picked != current)
}

/// A Tags Summary heading over a group of lines, read from `line`, the group's first.
/// Grouped by tag, it shows the tag's icon in its colour and the group's count, as before.
fn tags_heading(ui: &mut Ui, line: &TaggedLine, by: TagsGroupBy, count: usize, t: Tokens) {
    let Some(text) = tags_heading_text(line, by) else {
        return;
    };
    ui.horizontal(|ui| {
        let text = if by == TagsGroupBy::TagName {
            ui.label(
                RichText::new(line.tag_icon())
                    .font(theme::icon(13.0))
                    .color(editor::tag_glyph_color(line.tag_color(), t)),
            );
            format!("{text}  ·  {count}")
        } else {
            text
        };
        ui.label(RichText::new(text).font(theme::semibold(12.0)).color(t.ink));
    });
}

/// A Tags Summary row's glyph: a box (To Do's, or a custom tag's) shows whether it is
/// ticked, as on the page; other tags show their icon.
fn tagged_line_glyph(line: &TaggedLine) -> &'static str {
    let boxed = match line.mark {
        TagMark::Todo => true,
        TagMark::Line(_) => false,
        TagMark::Custom => line
            .custom
            .as_ref()
            .is_some_and(|tag| tag.symbol == TagSymbol::Checkbox),
    };
    match (boxed, line.checked) {
        (true, true) => icons::CHECK_SQUARE,
        (true, false) => icons::SQUARE,
        (false, _) => line.tag_icon(),
    }
}

/// A built-in tag that can be ticked, as a Tags Summary row draws it with the tag
/// column's own painting, so its tick shows; `None` for the rest (`tagged_line_glyph`).
fn ticked_symbol(line: &TaggedLine) -> Option<TagSymbol> {
    let symbol = match line.mark {
        TagMark::Line(tag) => TagSymbol::Tag(tag),
        TagMark::Custom => line.custom.as_ref()?.symbol,
        TagMark::Todo => return None,
    };
    matches!(symbol, TagSymbol::Tag(tag) if tag.checkable()).then_some(symbol)
}

/// One line of the Find Tags pane: its tag (or to-do box), its text, and where it is.
fn tagged_line_row(ui: &mut Ui, line: &TaggedLine, t: Tokens) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            t!(
                "app-tagged-line-label",
                tag = line.tag_label().into_owned(),
                text = line.text.as_str()
            ),
        )
    });
    // A long summary repaints while typing: rows scrolled out of view cost nothing.
    if !ui.is_rect_visible(rect) {
        return response;
    }
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), t.hover);
    }
    let at = pos2(rect.left() + 10.0, rect.top() + 13.0);
    match ticked_symbol(line) {
        // Call back, To Do priority 1…: drawn as on the page, ticked or not.
        Some(symbol) => editor::paint_symbol(
            ui.painter(),
            Rect::from_center_size(at + vec2(0.0, -1.0), vec2(editor::TAG_SLOT, 24.0)),
            symbol,
            line.checked,
            t,
        ),
        None => {
            ui.painter().text(
                at,
                Align2::CENTER_CENTER,
                tagged_line_glyph(line),
                theme::icon(12.0),
                editor::tag_glyph_color(line.tag_color(), t),
            );
        }
    }
    let text_width = rect.width() - 24.0 - 8.0;
    widgets::fitted_text(
        ui.painter(),
        pos2(rect.left() + 24.0, rect.top() + 13.0),
        Align2::LEFT_CENTER,
        line.text.as_str(),
        theme::ui(12.0),
        if line.checked { t.faint } else { t.ink },
        text_width,
    );
    widgets::fitted_text(
        ui.painter(),
        pos2(rect.left() + 24.0, rect.top() + 29.0),
        Align2::LEFT_CENTER,
        format!(
            "{} · {}",
            display_title(&line.page_title),
            line.section_name
        ),
        theme::ui(10.5),
        t.faint,
        text_width,
    );
    response.on_hover_text(line.text.as_str())
}

/// The banner over a page in the Notebook Recycle Bin: what the page is, with Restore and
/// Delete forever at the right. It runs along the card's top edge,
/// rounded like it (`at_left`: also at the card's left edge), and its right end keeps room
/// for the card's Full Page View button.
fn trash_banner(ui: &mut Ui, at_left: bool, t: Tokens) -> Option<EditorAction> {
    let mut action = None;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: if at_left { 8 } else { 0 },
            ne: 8,
            sw: 0,
            se: 0,
        },
        theme::with_alpha(t.danger, 14),
    );
    let left = rect.left() + 12.0;
    let inner = Rect::from_x_y_ranges(left..=(rect.right() - 44.0).max(left), rect.y_range());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            if widgets::text_button(ui, Some(icons::TRASH), &t!("common-delete-forever"), t)
                .clicked()
            {
                action = Some(EditorAction::Delete);
            }
            if widgets::text_button(
                ui,
                Some(icons::ARROW_COUNTER_CLOCKWISE),
                &t!("common-restore"),
                t,
            )
            .clicked()
            {
                action = Some(EditorAction::Restore);
            }
            // The message takes the room the buttons leave, cut if it must be.
            let rest = ui.available_rect_before_wrap();
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rest)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.label(
                        RichText::new(icons::WARNING)
                            .font(theme::icon(16.0))
                            .color(t.danger),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(t!("app-page-in-trash"))
                                .font(theme::ui(13.0))
                                .color(t.ink),
                        )
                        .truncate(),
                    );
                },
            );
        },
    );
    action
}

/// The document whose paragraph is being typed in: the note being edited.
fn active_doc<'a>(page: &'a Page, canvas: &CanvasState) -> Option<&'a Document> {
    canvas
        .active_text
        .and_then(|id| page.canvas.item(id))
        .and_then(CanvasItem::doc)
}

/// What the ribbon sees of the app this frame. It is built from the fields it reads, not
/// from the whole app, so the ribbon's own state can be borrowed mutably beside it.
struct ShellHost<'a> {
    page: Option<&'a Page>,
    editor: &'a EditorState,
    /// The paragraph being typed in (see `active_doc`).
    text: Option<TextState>,
    /// The note's paragraph the Tags menu acts on.
    block: Option<&'a Block>,
    /// The table holding the caret: the Table tab shows while there is one.
    table: Option<editor::TableInfo>,
    /// The selected page picture, else the picture clicked in the note being edited: the
    /// Picture tab shows while there is one.
    picture: Option<canvas::PictureInfo>,
    /// The canvas's view of the open page; `None` with no page open.
    snap: Option<canvas::RibbonSnapshot>,
    /// The shape the Shapes menu marks as current.
    shape_kind: ShapeKind,
    /// The recording under way: its length so far (ms) and whether it is paused.
    recording: Option<(u64, bool)>,
    trash_view: bool,
    full_page: bool,
    layout: RibbonLayout,
    /// Always Create Pages with Rule Lines is on.
    always_rule: bool,
    /// An equation block's source is being typed in a note.
    equation_block: bool,
    /// A section's pages are listed (not the Recycle Bin): Page Templates can add one.
    section_open: bool,
    /// Draw's pen gallery, shown (greyed) even with no page open.
    pens: &'a [canvas::PenPreset],
    /// The Format Painter while it is on: `Some(sticky)`.
    painter: Option<bool>,
    /// Pages in the Recycle Bin: Empty Recycle Bin needs one.
    trash_count: i64,
    /// A section, and a notebook, are selected: History can delete their versions.
    section_selected: bool,
    notebook_selected: bool,
    /// View › Tabs Layout and Switch Background, as the app has them.
    tabs_layout: TabsLayout,
    switch_background: bool,
    /// A printout is being inserted, or a dropped PDF awaits its question: File Printout
    /// waits for it.
    printout_busy: bool,
    /// Insert › Symbol's recent row, newest first.
    recent_symbols: &'a [char],
}

impl<'a> ShellHost<'a> {
    /// The page's zoom as the Zoom Level box shows it, in per cent.
    fn zoom_percent(&self) -> u16 {
        self.snap
            .as_ref()
            .map_or(100, |snap| (snap.zoom * 100.0).round() as u16)
    }

    fn new(
        page: Option<&'a Page>,
        canvas: &'a CanvasState,
        editor: &'a EditorState,
        trash_view: bool,
        full_page: bool,
        layout: RibbonLayout,
        always_rule: bool,
    ) -> Self {
        let doc = page.and_then(|page| active_doc(page, canvas));
        Self {
            page,
            editor,
            text: doc.and_then(|doc| editor::text_state(doc, editor)),
            block: doc.and_then(|doc| editor.active_block(doc)),
            table: doc.and_then(|doc| editor::table_info(doc, editor)),
            picture: page.and_then(|page| canvas::picture_info(page, canvas, editor)),
            snap: page.map(|page| canvas::ribbon_snapshot(page, canvas)),
            shape_kind: canvas.shape_kind,
            recording: canvas.recording_time(),
            trash_view,
            full_page,
            layout,
            always_rule,
            equation_block: editor.equation_block.is_some(),
            section_open: !trash_view,
            pens: &canvas.pens,
            painter: editor.painter.as_ref().map(|painter| painter.sticky),
            trash_count: 0,
            section_selected: false,
            notebook_selected: false,
            tabs_layout: TabsLayout::Vertical,
            switch_background: false,
            printout_busy: canvas.printout_busy(),
            recent_symbols: &[],
        }
    }

    /// The symbols Insert › Symbol ▾ shows as recently used.
    fn with_recent_symbols(mut self, recent: &'a [char]) -> Self {
        self.recent_symbols = recent;
        self
    }

    /// What History's commands need beyond the open page: the Recycle Bin's size and the
    /// selected section and notebook.
    fn with_library(mut self, trash_count: i64, section: bool, notebook: bool) -> Self {
        self.trash_count = trash_count;
        self.section_selected = section;
        self.notebook_selected = notebook;
        self
    }

    /// What View's checked controls show: the tabs layout and Switch Background.
    fn with_view(mut self, tabs_layout: TabsLayout, switch_background: bool) -> Self {
        self.tabs_layout = tabs_layout;
        self.switch_background = switch_background;
        self
    }

    fn single_kind(&self) -> Option<SingleKind> {
        self.snap.as_ref().and_then(|snap| snap.single_kind)
    }

    /// The text being typed in, when Undo and Redo are its own: not with a picture picked
    /// in the note (as `canvas::run_ribbon_cmd` routes them).
    fn text_history(&self) -> Option<&TextState> {
        self.text
            .as_ref()
            .filter(|text| !matches!(text.kind, BlockKind::Image { .. }))
    }

    /// The Picture tab acts on a page picture (not one in a note).
    fn page_picture(&self) -> bool {
        self.picture
            .is_some_and(|picture| matches!(picture.target, canvas::PictureTarget::Item(_)))
    }

    /// Draw › Rotate applies: the selection holds ink, a shape or a picture, or a picture
    /// in a note was clicked.
    fn rotatable(&self) -> bool {
        self.snap.as_ref().is_some_and(|snap| snap.rotatable) || self.picture.is_some()
    }

    /// A page is open and can be changed: not one in the Recycle Bin.
    fn editable_page(&self) -> bool {
        self.snap.is_some() && self.page.is_some_and(|page| !page.trashed)
    }

    /// Review › Proofing Language: the spelling languages as the Settings dialog lists
    /// them (automatic, off, then each dictionary), the current one checked.
    fn proofing_menu(&self, ui: &mut Ui, t: Tokens) -> Option<Cmd> {
        ui.set_min_width(220.0);
        let spell = &self.editor.spell;
        let current = spell.setting();
        let mut choices = vec![
            (
                "auto".to_owned(),
                t!("app-spell-automatic", names = spell.automatic_name()).into_owned(),
            ),
            ("off".to_owned(), t!("app-spell-off").into_owned()),
        ];
        choices.extend(
            spell
                .dictionaries()
                .iter()
                .map(|dictionary| (dictionary.code.clone(), dictionary.name.clone())),
        );
        let mut picked = None;
        for (code, label) in choices {
            if widgets::checked_menu_item(ui, None, &label, code == current, t).clicked() {
                picked = Some(Cmd::ProofingLanguage(code));
            }
        }
        picked
    }

    /// Rows of commands as `widgets::menu_row`s, each greyed when it can't run, as wide
    /// as the menu (see `menu_width`); returns the one clicked.
    fn menu_rows(
        &self,
        ui: &mut Ui,
        rows: Vec<(Option<&str>, Cow<'static, str>, Cmd)>,
        t: Tokens,
    ) -> Option<Cmd> {
        let mut picked = None;
        for (glyph, label, cmd) in rows {
            if widgets::menu_row(ui, glyph, &label, "", self.enabled(&cmd), t).clicked() {
                picked = Some(cmd);
            }
        }
        picked
    }

    /// Rows of commands, each checked when the host says so; returns the one clicked.
    fn command_rows(
        &self,
        ui: &mut Ui,
        rows: Vec<(Option<&str>, Cow<'static, str>, Cmd)>,
        t: Tokens,
    ) -> Option<Cmd> {
        ui.set_min_width(180.0);
        let mut picked = None;
        for (glyph, label, cmd) in rows {
            if widgets::checked_menu_item(ui, glyph, &label, self.checked(&cmd), t).clicked() {
                picked = Some(cmd);
            }
        }
        picked
    }
}

/// A menu of `widgets::menu_row`s as wide as its widest glyph and label, with room after
/// it: a row is otherwise as wide as the room the popup offers.
fn menu_width(ui: &Ui, rows: &[(Option<&str>, Cow<'static, str>, Cmd)]) -> f32 {
    rows_width(
        ui,
        rows.iter()
            .map(|(glyph, label, _)| (*glyph, label.as_ref())),
    )
}

/// The width of a menu of `widgets::menu_row`s with these glyphs and labels, with room
/// for a check mark, and never under 160 px.
fn rows_width<'a>(ui: &Ui, rows: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) -> f32 {
    let widest = rows
        .into_iter()
        .map(|(glyph, label)| {
            let text = match glyph {
                Some(glyph) => format!("{glyph}    {label}"),
                None => label.to_string(),
            };
            widgets::text_width(ui.ctx(), &text, theme::ui(12.5))
        })
        .fold(0.0, f32::max);
    (widest + 2.0 * ui.spacing().button_padding.x + 32.0).max(160.0)
}

impl RibbonHost for ShellHost<'_> {
    fn enabled(&self, cmd: &Cmd) -> bool {
        let typing = self.text.is_some();
        let canvas_page = self.snap.is_some();
        let editable_canvas = self.editable_page();
        let selected = self.snap.as_ref().map_or(0, |snap| snap.selection_count);
        let single = self.single_kind();
        match cmd {
            // Find Tags lists the tags of the section even when nothing is being typed.
            Cmd::Text(ToolbarCommand::Tag(TagCommand::FindTags)) => self.page.is_some(),
            Cmd::Text(_)
            | Cmd::MoreFontColors
            | Cmd::CustomizeNumbering
            | Cmd::ParagraphSpacing => typing,
            Cmd::MorePenColors => canvas_page,
            // While typing, the text's own history; otherwise the page's, as with a picture
            // picked in the note (its text has no caret, and its commands are page steps).
            Cmd::Undo => match (self.text_history(), &self.snap) {
                (Some(text), _) => text.can_undo,
                (None, Some(snap)) => snap.can_undo,
                (None, None) => false,
            },
            Cmd::Redo => match (self.text_history(), &self.snap) {
                (Some(text), _) => text.can_redo,
                (None, Some(snap)) => snap.can_redo,
                (None, None) => false,
            },
            Cmd::Delete => typing || selected > 0,
            Cmd::Attachment
            | Cmd::RecordAudio
            | Cmd::Timeline
            | Cmd::Equation
            | Cmd::ReadyEquation(_)
            | Cmd::Date
            | Cmd::Time
            | Cmd::DateTime
            | Cmd::Symbol(_)
            | Cmd::MoreSymbols
            | Cmd::InsertTable { .. }
            | Cmd::Link
            | Cmd::Tool(_)
            | Cmd::Eraser(_)
            | Cmd::InkToShape
            | Cmd::Ink(_)
            | Cmd::PenWidth(_)
            | Cmd::Shape(_)
            | Cmd::PenPreset(_)
            | Cmd::AddPen { .. }
            | Cmd::LockDrawing
            | Cmd::PageColor(_)
            | Cmd::MorePageColors
            | Cmd::RuleLines(_)
            | Cmd::HidePageTitle
            | Cmd::ZoomOut
            | Cmd::ZoomIn
            | Cmd::Zoom100
            | Cmd::Zoom(_)
            | Cmd::PageWidth => editable_canvas,
            // A setting for new pages, offered beside the open page's rule lines.
            Cmd::AlwaysRuleLines => self.page.is_some(),
            // Only lines have a colour to change.
            Cmd::RuleLineColor(_) => {
                editable_canvas && self.page.is_some_and(|page| page.rule != RuleLines::None)
            }
            Cmd::Picture => canvas_page,
            Cmd::Fill(_) => self.snap.as_ref().is_some_and(|snap| snap.fillable),
            Cmd::BringToFront | Cmd::SendToBack | Cmd::BringForward | Cmd::SendBackward => {
                selected > 0
            }
            // The gallery keeps at least one pen.
            Cmd::RemovePen => self
                .snap
                .as_ref()
                .is_some_and(|snap| snap.pen.is_some() && snap.pens.len() > 1),
            // Ink, lines and arrows turn by 45°; a picture or a boxed shape has no angle.
            Cmd::Rotate(Rotation::Right45 | Rotation::Left45) => {
                self.picture.is_none() && self.snap.as_ref().is_some_and(|snap| snap.turns_45)
            }
            // Ink, shapes and pictures turn; so does the picture clicked in a note.
            Cmd::Rotate(_) => self.rotatable(),
            Cmd::PageTemplate(_) => self.section_open,
            Cmd::TableEdit(_) => self.table.is_some(),
            // Only a page picture can be the page's background.
            Cmd::PictureEdit(PictureCmd::Background) => self.page_picture(),
            // Only a page picture is cropped: not a printout page, nor the background.
            Cmd::PictureEdit(PictureCmd::Crop) => {
                self.page_picture()
                    && self.picture.is_some_and(|picture| {
                        picture.cropping || !(picture.printout || picture.background)
                    })
            }
            Cmd::PictureEdit(_) => self.picture.is_some(),
            // An equation block being typed in a note is already being edited; its LaTeX
            // can still be copied.
            Cmd::EditEquation => single == Some(SingleKind::Equation) && !self.equation_block,
            Cmd::CopyLatex => single == Some(SingleKind::Equation) || self.equation_block,
            Cmd::PlayPause | Cmd::SaveAs => single == Some(SingleKind::Audio),
            Cmd::PauseRecording | Cmd::StopRecording => self.recording.is_some(),
            Cmd::Reader => self.page.is_some(),
            // A page in the Recycle Bin keeps no versions of its own to look at.
            Cmd::PageVersions | Cmd::DeleteVersions(VersionScope::Page) => {
                self.page.is_some_and(|page| !page.trashed)
            }
            Cmd::DeleteVersions(VersionScope::Section) => self.section_selected,
            Cmd::DeleteVersions(VersionScope::Notebook) => self.notebook_selected,
            Cmd::EmptyRecycleBin => self.trash_count > 0,
            Cmd::RecycleBin
            | Cmd::RecentEdits(_)
            | Cmd::Spelling
            | Cmd::ProofingLanguage(_)
            | Cmd::NormalView
            | Cmd::FullPageView
            | Cmd::TabsLayout(_)
            | Cmd::SwitchBackground
            | Cmd::NewQuickNote
            | Cmd::Shortcuts
            | Cmd::About
            | Cmd::Backstage
            | Cmd::RibbonLayout(_)
            | Cmd::CollapseRibbon(_) => true,
            // The text selected while typing, else the selected items.
            Cmd::Cut | Cmd::Copy => self
                .text
                .as_ref()
                .map_or(selected > 0, |text| text.has_selection),
            // Into the note being typed in, else onto the page.
            Cmd::Paste(_) => editable_canvas,
            // It picks up the formatting at the caret; once on, a click puts it away.
            Cmd::FormatPainter => self.text.is_some() || self.painter.is_some(),
            // One printout at a time, on a page that can change.
            Cmd::FilePrintout => editable_canvas && !self.printout_busy,
        }
    }

    fn checked(&self, cmd: &Cmd) -> bool {
        let text = self.text.as_ref();
        let snap = self.snap.as_ref();
        match cmd {
            Cmd::Text(ToolbarCommand::Toggle(mark)) => text.is_some_and(|text| text.mark(*mark)),
            Cmd::Text(ToolbarCommand::Align(align)) => {
                text.is_some_and(|text| text.align == *align)
            }
            Cmd::Text(ToolbarCommand::SetKind(kind)) => {
                text.is_some_and(|text| editor::same_kind(&text.kind, kind))
            }
            // The list's own marker, picked or its level's default (`None` off a list).
            Cmd::Text(ToolbarCommand::ListMarker(marker)) => {
                text.is_some_and(|text| text.marker == *marker)
            }
            Cmd::Tool(tool) => snap.is_some_and(|snap| snap.tool == *tool),
            Cmd::Eraser(mode) => snap.is_some_and(|snap| snap.eraser == *mode),
            Cmd::InkToShape => snap.is_some_and(|snap| snap.ink_to_shape),
            Cmd::Ink(color) => snap.is_some_and(|snap| snap.ink_color == *color),
            Cmd::PenWidth(width) => snap.is_some_and(|snap| (snap.ink_width - width).abs() < 0.01),
            Cmd::Shape(kind) => snap.is_some_and(|snap| snap.tool == CanvasTool::Shape(*kind)),
            // The pen last picked, while its tool is on.
            Cmd::PenPreset(index) => snap.is_some_and(|snap| {
                matches!(snap.tool, CanvasTool::Pen | CanvasTool::Highlighter)
                    && snap.pen == Some(*index)
            }),
            Cmd::LockDrawing => snap.is_some_and(|snap| snap.lock_drawing),
            Cmd::RecordAudio => self.recording.is_some(),
            Cmd::PauseRecording => self.recording.is_some_and(|(_, paused)| paused),
            Cmd::NormalView => !self.full_page,
            Cmd::FullPageView => self.full_page,
            Cmd::RecycleBin => self.trash_view,
            Cmd::TabsLayout(layout) => self.tabs_layout == *layout,
            Cmd::SwitchBackground => self.switch_background,
            Cmd::RibbonLayout(layout) => self.layout == *layout,
            Cmd::PageColor(color) => self.page.is_some_and(|page| page.color == *color),
            Cmd::MorePageColors => self
                .page
                .is_some_and(|page| matches!(page.color, Some(PageColor::Custom(_)))),
            Cmd::RuleLineColor(color) => self.page.is_some_and(|page| page.rule_color == *color),
            Cmd::RuleLines(rule) => self.page.is_some_and(|page| page.rule == *rule),
            Cmd::HidePageTitle => self.page.is_some_and(|page| page.title_hidden),
            Cmd::AlwaysRuleLines => self.always_rule,
            Cmd::FormatPainter => self.painter.is_some(),
            Cmd::TableEdit(TableCmd::HideBorders) => self.table.is_some_and(|t| t.hide_borders),
            Cmd::TableEdit(TableCmd::Align(align)) => self.table.is_some_and(|t| t.align == *align),
            Cmd::TableEdit(TableCmd::HeaderRow) => self.table.is_some_and(|t| t.header),
            Cmd::PictureEdit(PictureCmd::Background) => {
                self.picture.is_some_and(|picture| picture.background)
            }
            Cmd::PictureEdit(PictureCmd::Crop) => {
                self.picture.is_some_and(|picture| picture.cropping)
            }
            Cmd::Zoom(percent) => self.snap.is_some() && self.zoom_percent() == *percent,
            _ => false,
        }
    }

    fn menu_enabled(&self, menu: MenuId) -> bool {
        match menu {
            MenuId::Fonts
            | MenuId::Sizes
            | MenuId::Script
            | MenuId::Highlight
            | MenuId::FontColor
            | MenuId::Align
            | MenuId::Styles => self.text.is_some(),
            MenuId::Tags => self.page.is_some(),
            MenuId::Eraser | MenuId::Shapes => self.snap.is_some(),
            MenuId::Fill => self.snap.as_ref().is_some_and(|snap| snap.fillable),
            MenuId::Arrange => self
                .snap
                .as_ref()
                .is_some_and(|snap| snap.selection_count > 0),
            MenuId::Proofing
            | MenuId::PageVersions
            | MenuId::RecycleBin
            | MenuId::RecentEdits
            | MenuId::TabsLayout => true,
            MenuId::PageColor | MenuId::RuleLines => self.page.is_some_and(|page| !page.trashed),
            MenuId::Zoom => self.enabled(&Cmd::ZoomIn),
            MenuId::Bullets | MenuId::Numbering => self.text.is_some(),
            MenuId::Paste
            | MenuId::Symbols
            | MenuId::TableGrid
            | MenuId::PenSettings
            | MenuId::AddPen => self.editable_page(),
            MenuId::PageTemplates => self.section_open,
            MenuId::EquationGallery => self.enabled(&Cmd::Equation),
            MenuId::Rotate => self.rotatable(),
            MenuId::Shading | MenuId::TableSort => self.table.is_some(),
            // A picture in a note flows with its text: it has no order on the page.
            MenuId::PictureOrder => self.page_picture(),
            MenuId::PictureRotate => self.picture.is_some(),
        }
    }

    fn combo_text(&self, menu: MenuId) -> String {
        match menu {
            MenuId::Fonts => self
                .text
                .as_ref()
                .map_or(FontFace::Body, |text| text.face)
                .label()
                .to_owned(),
            MenuId::Sizes => self
                .text
                .as_ref()
                .map_or_else(|| "11".to_owned(), |text| format!("{}", text.pt.round())),
            MenuId::Zoom => t!("ribbon-zoom-percent", percent = self.zoom_percent()).into_owned(),
            _ => String::new(),
        }
    }

    fn contextual(&self) -> Vec<RibbonTab> {
        let single = self.single_kind();
        let mut tabs = Vec::new();
        // The caret in a table, as the desktop notebook shows its Table tab.
        if self.table.is_some() {
            tabs.push(RibbonTab::Table);
        }
        // A selected page picture, or a picture clicked in a note.
        if self.picture.is_some() {
            tabs.push(RibbonTab::Picture);
        }
        // A selected equation, or an equation block being typed in a note.
        if single == Some(SingleKind::Equation) || self.equation_block {
            tabs.push(RibbonTab::Equation);
        }
        if self.recording.is_some() || single == Some(SingleKind::Audio) {
            tabs.push(RibbonTab::Recording);
        }
        tabs
    }

    fn menu(&self, ui: &mut Ui, menu: MenuId, t: Tokens) -> Option<Cmd> {
        use ToolbarCommand as Do;
        let text = self.text.as_ref();
        match menu {
            MenuId::Fonts => editor::font_list(ui, text.map_or(FontFace::Body, |text| text.face))
                .map(|face| Cmd::Text(Do::Face(face))),
            MenuId::Sizes => editor::size_list(ui, text.map_or(11.0, |text| text.pt))
                .map(|size| Cmd::Text(Do::Size(size))),
            MenuId::Script => self.command_rows(
                ui,
                vec![
                    (
                        Some(icons::TEXT_SUBSCRIPT),
                        t!("editor-subscript"),
                        Cmd::Text(Do::Toggle(Mark::Subscript)),
                    ),
                    (
                        Some(icons::TEXT_SUPERSCRIPT),
                        t!("editor-superscript"),
                        Cmd::Text(Do::Toggle(Mark::Superscript)),
                    ),
                ],
                t,
            ),
            MenuId::Highlight => {
                editor::highlight_menu(ui, t).map(|color| Cmd::Text(Do::Highlight(color)))
            }
            MenuId::FontColor => editor::text_color_menu(ui, t).map(|pick| match pick {
                editor::ColorPick::Color(color) => Cmd::Text(Do::Color(color)),
                editor::ColorPick::More => Cmd::MoreFontColors,
            }),
            MenuId::Align => {
                editor::align_list(ui, text.map_or(ParaAlign::Left, |text| text.align)).map(
                    |pick| match pick {
                        AlignPick::Align(align) => Cmd::Text(Do::Align(align)),
                        AlignPick::Spacing => Cmd::ParagraphSpacing,
                    },
                )
            }
            // the desktop notebook's eleven styles, as rows, in its order, the current one checked; then
            // Clear Formatting. The text menu's mini toolbar shows the same rows.
            MenuId::Styles => {
                editor::styles_menu(ui, text.map(|text| &text.kind), t).map(Cmd::Text)
            }
            MenuId::Tags => editor::tag_menu_contents(ui, self.block, &self.editor.tags, t)
                .map(|command| Cmd::Text(Do::Tag(command))),
            MenuId::Eraser => canvas::eraser_list(
                ui,
                self.snap
                    .as_ref()
                    .map_or(EraserMode::Stroke, |snap| snap.eraser),
            )
            .map(Cmd::Eraser),
            MenuId::Shapes => canvas::shape_list(ui, self.shape_kind).map(Cmd::Shape),
            MenuId::Fill => canvas::fill_list(ui, t).map(Cmd::Fill),
            // the desktop notebook's four: to the front, one forward, one backward, to the back.
            MenuId::Arrange => {
                let rows = vec![
                    (
                        Some(icons::POSITION_TO_FRONT),
                        t!("ribbon-front-tip"),
                        Cmd::BringToFront,
                    ),
                    (
                        Some(icons::ARROW_UP),
                        t!("ribbon-bring-forward"),
                        Cmd::BringForward,
                    ),
                    (
                        Some(icons::ARROW_DOWN),
                        t!("ribbon-send-backward"),
                        Cmd::SendBackward,
                    ),
                    (
                        Some(icons::POSITION_TO_BACK),
                        t!("ribbon-back-tip"),
                        Cmd::SendToBack,
                    ),
                ];
                self.command_rows(ui, rows, t)
            }
            MenuId::Proofing => self.proofing_menu(ui, t),
            // The pane, then deleting the versions of the page, its section or notebook.
            MenuId::PageVersions => {
                let pane = vec![(
                    Some(icons::CLOCK_COUNTER_CLOCKWISE),
                    t!("common-page-versions"),
                    Cmd::PageVersions,
                )];
                let delete: Vec<_> = [
                    (VersionScope::Page, t!("ribbon-delete-versions-page")),
                    (VersionScope::Section, t!("ribbon-delete-versions-section")),
                    (
                        VersionScope::Notebook,
                        t!("ribbon-delete-versions-notebook"),
                    ),
                ]
                .into_iter()
                .map(|(scope, label)| (Some(icons::TRASH), label, Cmd::DeleteVersions(scope)))
                .collect();
                ui.set_width(menu_width(ui, &pane).max(menu_width(ui, &delete)));
                let pane = self.menu_rows(ui, pane, t);
                ui.separator();
                let delete = self.menu_rows(ui, delete, t);
                pane.or(delete)
            }
            MenuId::RecycleBin => {
                let rows = vec![
                    (Some(icons::TRASH), t!("app-recycle-bin"), Cmd::RecycleBin),
                    (
                        Some(icons::DELETE_DISMISS),
                        t!("app-empty-trash"),
                        Cmd::EmptyRecycleBin,
                    ),
                ];
                ui.set_width(menu_width(ui, &rows));
                self.menu_rows(ui, rows, t)
            }
            MenuId::RecentEdits => {
                let rows: Vec<_> = EditsRange::ALL
                    .into_iter()
                    .map(|range| (None, range.label(), Cmd::RecentEdits(range)))
                    .collect();
                ui.set_width(menu_width(ui, &rows));
                self.menu_rows(ui, rows, t)
            }
            // Vertical Tabs and Horizontal Tabs, the layout in use checked.
            MenuId::TabsLayout => {
                let rows: Vec<_> = TabsLayout::ALL
                    .into_iter()
                    .map(|layout| (None, layout.label(), Cmd::TabsLayout(layout)))
                    .collect();
                ui.set_width(menu_width(ui, &rows));
                let mut picked = None;
                for (glyph, label, cmd) in rows {
                    let enabled = self.enabled(&cmd);
                    let checked = self.checked(&cmd);
                    let row = widgets::checked_menu_row(ui, glyph, &label, "", enabled, checked, t);
                    if row.clicked() {
                        picked = Some(cmd);
                    }
                }
                picked
            }
            // View › Zoom Level: the desktop notebook's steps, the current one ticked, then a box to type
            // a percentage (Enter applies it).
            MenuId::Zoom => {
                let rows: Vec<_> = ribbon::ZOOM_STEPS
                    .into_iter()
                    .map(|percent| {
                        (
                            None,
                            t!("ribbon-zoom-percent", percent = percent),
                            Cmd::Zoom(percent),
                        )
                    })
                    .collect();
                ui.set_width(menu_width(ui, &rows));
                let mut picked = None;
                for (glyph, label, cmd) in rows {
                    let enabled = self.enabled(&cmd);
                    let checked = self.checked(&cmd);
                    let row = widgets::checked_menu_row(ui, glyph, &label, "", enabled, checked, t);
                    if row.clicked() {
                        picked = Some(cmd);
                    }
                }
                ui.separator();
                let id = ui.scope_id().with("zoom-typed");
                let mut typed = ui
                    .data_mut(|data| data.get_temp::<String>(id))
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(t!("ribbon-zoom-typed"))
                            .font(theme::ui(12.5))
                            .color(t.ink),
                    );
                    // A menu draws its widgets frameless: the box keeps the app's frame.
                    let widgets = ui.ctx().global_style().visuals.widgets.clone();
                    ui.visuals_mut().widgets = widgets;
                    let field = ui.add(
                        TextEdit::singleline(&mut typed)
                            .desired_width(48.0)
                            .align(Align2::LEFT_CENTER)
                            .font(theme::ui(12.5)),
                    );
                    if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        picked = ribbon::zoom_percent(&typed).map(Cmd::Zoom);
                    }
                });
                ui.data_mut(|data| data.insert_temp(id, typed));
                picked
            }
            MenuId::PageColor => ribbon::page_color_menu(ui, self, t),
            MenuId::RuleLines => ribbon::rule_lines_menu(ui, self, t),
            MenuId::Paste => editor::paste_menu(ui, None, t).map(Cmd::Paste),
            MenuId::Bullets | MenuId::Numbering => editor::marker_library(
                ui,
                menu == MenuId::Numbering,
                text.and_then(|text| text.marker),
                t,
            )
            .map(|pick| match pick {
                MarkerPick::Marker(marker) => Cmd::Text(Do::ListMarker(marker)),
                MarkerPick::Customize => Cmd::CustomizeNumbering,
            }),
            MenuId::EquationGallery => editor::equation_gallery(ui, t),
            MenuId::Symbols => {
                editor::symbol_grid(ui, self.recent_symbols, t).map(|pick| match pick {
                    SymbolPick::Symbol(symbol) => Cmd::Symbol(symbol),
                    SymbolPick::More => Cmd::MoreSymbols,
                })
            }
            MenuId::TableGrid => canvas::table_grid(ui, t),
            MenuId::PageTemplates => templates::template_list(ui, t).map(Cmd::PageTemplate),
            MenuId::PenSettings => self
                .snap
                .as_ref()
                .and_then(|snap| canvas::pen_settings(ui, snap, t)),
            MenuId::AddPen => canvas::add_pen_menu(ui, t),
            // A picture, even one clicked in a note, keeps quarter turns.
            MenuId::Rotate => canvas::rotate_list(
                ui,
                self.picture.is_none() && self.snap.as_ref().is_some_and(|snap| snap.turns_45),
                t,
            ),
            MenuId::Shading => {
                editor::shading_menu(ui, t).map(|color| Cmd::TableEdit(TableCmd::Shading(color)))
            }
            // Sort Ascending and Descending, then Header Row, checked while it is on.
            MenuId::TableSort => {
                let sorts = self.command_rows(
                    ui,
                    vec![
                        (
                            Some(icons::TEXT_SORT_ASCENDING),
                            t!("table-sort-ascending"),
                            Cmd::TableEdit(TableCmd::SortAscending),
                        ),
                        (
                            Some(icons::TEXT_SORT_DESCENDING),
                            t!("table-sort-descending"),
                            Cmd::TableEdit(TableCmd::SortDescending),
                        ),
                    ],
                    t,
                );
                ui.separator();
                let header = self.command_rows(
                    ui,
                    vec![(
                        Some(icons::TABLE_FREEZE_ROW),
                        t!("table-header-row"),
                        Cmd::TableEdit(TableCmd::HeaderRow),
                    )],
                    t,
                );
                sorts.or(header)
            }
            // the desktop notebook's Picture › Order has the same four rows as Draw › Arrange.
            MenuId::PictureOrder => {
                let rows = vec![
                    (
                        Some(icons::ARROW_UP),
                        t!("ribbon-bring-forward"),
                        Cmd::BringForward,
                    ),
                    (
                        Some(icons::POSITION_TO_FRONT),
                        t!("picture-bring-to-front"),
                        Cmd::BringToFront,
                    ),
                    (
                        Some(icons::ARROW_DOWN),
                        t!("ribbon-send-backward"),
                        Cmd::SendBackward,
                    ),
                    (
                        Some(icons::POSITION_TO_BACK),
                        t!("picture-send-to-back"),
                        Cmd::SendToBack,
                    ),
                ];
                self.command_rows(ui, rows, t)
            }
            MenuId::PictureRotate => editor::rotate_rows(ui, t).map(Cmd::PictureEdit),
        }
    }

    fn recording(&self) -> Option<(String, bool)> {
        self.recording
            .map(|(elapsed, paused)| (crate::audio::format_time(elapsed), paused))
    }

    fn audio_selected(&self) -> bool {
        self.single_kind() == Some(SingleKind::Audio)
    }

    fn pens(&self) -> Vec<canvas::PenPreset> {
        self.pens.to_vec()
    }

    fn tag_gallery(&self) -> TagGallery {
        self.editor.tags.clone()
    }
}

/// Puts a section's pages in the sort button's order. Ties fall back to the page id, so
/// the order is total and descending is ascending turned around.
fn sort_pages(pages: &mut [PageSummary], sort: PageSort, desc: bool) {
    match sort {
        PageSort::None => return,
        PageSort::Alphabetical => pages.sort_by_cached_key(|page| {
            (display_title(&page.title).to_lowercase(), page.id.clone())
        }),
        PageSort::Created => pages.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        }),
        PageSort::Modified => pages.sort_by(|a, b| {
            a.updated_at
                .cmp(&b.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
    if desc {
        pages.reverse();
    }
}

/// Where a page row sits in its section's outline (a section in its own order only; a
/// sorted list is flat).
#[derive(Debug, Clone, Copy)]
struct RowOutline {
    level: u8,
    has_children: bool,
    collapsed: bool,
    can_demote: bool,
    /// The level a page dropped below this row takes (that of the next row shown).
    below_level: u8,
}

/// A page row being dragged: onto another row to reorder, onto a section to move.
#[derive(Debug, Clone)]
struct PageDrag {
    id: String,
    title: String,
    section_id: String,
}

/// A section row being dragged in the sidebar.
#[derive(Debug, Clone)]
struct SectionDrag {
    id: String,
    notebook_id: String,
    name: String,
}

struct PageRowResult {
    clicked: bool,
    copy_link_clicked: bool,
    context_action: Option<PageListAction>,
    /// A dragged page dropped on this row, and whether it landed above it.
    dropped: Option<(String, bool)>,
    rect: Rect,
}

/// The page list's Add page button: outlined, with the glyph and label in the accent
/// colour, at most `max_width` wide (the label is cut to fit). the desktop notebook's metrics: a 12 px
/// label and 8 px of padding, so "Ajouter une page" and the sort button share the
/// default 180 px list. On the `tinted` header of Horizontal Tabs it is the desktop notebook's "+ Add
/// Page": a plus and the label in ink, without the outline.
fn add_page_button(ui: &mut Ui, max_width: f32, tinted: bool, t: Tokens) -> egui::Response {
    const PAD: f32 = 8.0;
    const GLYPH: f32 = 16.0;
    const GAP: f32 = 4.0;
    let label = t!("app-add-page");
    let font = theme::ui(12.0);
    let text = widgets::text_width(ui.ctx(), &label, font.clone());
    let width = (PAD + GLYPH + GAP + text + PAD).min(max_width.max(PAD + GLYPH + PAD));
    let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let response = response.on_hover_text(t!("app-add-page-tip"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let painter = ui.painter();
    // The translucent hover reads on the tinted header too; the opaque pressed fill would
    // not.
    let fill = if response.is_pointer_button_down_on() && !tinted {
        t.sunken
    } else if response.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, CornerRadius::same(4), fill);
    }
    let border = if response.has_focus() {
        Some(t.accent)
    } else {
        (!tinted).then_some(t.line_strong)
    };
    if let Some(border) = border {
        painter.rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
    }
    let (glyph, color) = if tinted {
        (icons::PLUS, t.ink)
    } else {
        (icons::DOCUMENT_ADD, t.accent)
    };
    painter.text(
        pos2(rect.left() + PAD, rect.center().y),
        Align2::LEFT_CENTER,
        glyph,
        theme::icon(GLYPH),
        color,
    );
    let left = rect.left() + PAD + GLYPH + GAP;
    widgets::fitted_text(
        painter,
        pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        color,
        rect.right() - PAD - left,
    );
    response
}

/// A notebook's sections in the navigation pane's order (`FolioApp::section_tree`): a
/// level's sections, then its section groups, each with theirs.
fn sections_in_order<'a>(
    notebook: &'a crate::model::Notebook,
    parent: Option<&str>,
) -> Vec<&'a crate::model::Section> {
    let mut sections: Vec<&crate::model::Section> = notebook
        .sections
        .iter()
        .filter(|section| section.group_id.as_deref() == parent)
        .collect();
    for group in notebook
        .groups
        .iter()
        .filter(|group| group.parent_id.as_deref() == parent)
    {
        sections.extend(sections_in_order(notebook, Some(&group.id)));
    }
    sections
}

/// How far a page row dragged back onto itself went sideways: one level right (`1`) or
/// left (`-1`) once past 24 px, as dragging a page tab to the right makes a subpage in
/// the desktop notebook.
fn sideways(ui: &Ui) -> Option<i8> {
    const SIDEWAYS: f32 = 24.0;
    // egui forgets where the press began on the frame the button comes up: the drop
    // measures from where the drag began, as last seen.
    let id = Id::unique("page-drag-origin");
    let origin = match ui.input(|input| input.pointer.press_origin()) {
        Some(origin) => {
            ui.data_mut(|data| data.insert_temp(id, origin.x));
            origin.x
        }
        None => ui.data(|data| data.get_temp::<f32>(id))?,
    };
    let dx = ui.input(|input| input.pointer.interact_pos())?.x - origin;
    (dx.abs() >= SIDEWAYS).then_some(if dx > 0.0 { 1 } else { -1 })
}

/// One page in the page list: its title on a 30 px row, indented by its level, with
/// the desktop notebook's neutral selection, the fold chevron of a page with subpages, dragging, and
/// the right-click menu (`page_menu`; its Paste is greyed without a `clip`). In a list
/// tinted with a section's colour (Horizontal Tabs) the open page is a white row with a
/// thin border, and a hovered one a deeper wash of the colour.
fn page_row(
    ui: &mut Ui,
    page: &PageSummary,
    selected: bool,
    t: Tokens,
    clip: bool,
    outline: Option<RowOutline>,
    tint: Option<Color32>,
) -> PageRowResult {
    let mut result = PageRowResult {
        clicked: false,
        copy_link_clicked: false,
        context_action: None,
        dropped: None,
        rect: Rect::NOTHING,
    };
    let width = ui.available_width();
    let sense = if page.trashed {
        Sense::click()
    } else {
        Sense::click_and_drag()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), sense);
    result.rect = rect;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::ListItem, true, display_title(&page.title))
    });
    if !page.trashed {
        response.dnd_set_drag_payload(PageDrag {
            id: page.id.clone(),
            title: display_title(&page.title).to_string(),
            section_id: page.section_id.clone(),
        });
    }
    let indent = outline.map_or(0.0, |o| o.level as f32 * 16.0);
    let left = rect.left() + 10.0 + indent;
    let right = rect.right() - 10.0;
    // A long list repaints while typing: rows scrolled out of view build no text.
    if ui.is_rect_visible(rect) {
        let fill = match (selected, tint) {
            (true, Some(_)) => t.raised,
            (true, None) => t.page_selected,
            (false, Some(color)) if response.hovered() => theme::pastel(color, t.raised, 0.75),
            (false, None) if response.hovered() => t.hover,
            _ => Color32::TRANSPARENT,
        };
        let row = rect.shrink2(vec2(0.0, 2.0));
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(row, CornerRadius::same(4), fill);
        }
        if selected && tint.is_some() {
            ui.painter().rect_stroke(
                row,
                CornerRadius::same(4),
                Stroke::new(1.0, t.line_strong),
                StrokeKind::Inside,
            );
        }
        let (font, color) = if selected {
            (theme::semibold(13.0), t.ink)
        } else {
            (theme::ui(13.0), t.text2)
        };
        widgets::fitted_text(
            ui.painter(),
            pos2(left, rect.center().y),
            Align2::LEFT_CENTER,
            display_title(&page.title),
            font,
            color,
            right - left,
        );
    }
    // The fold chevron, in the 12 px before the title, is created after the row, so it
    // wins the click.
    let mut fold_clicked = false;
    if let Some(o) = outline.filter(|o| o.has_children) {
        let fold_rect = Rect::from_min_max(
            pos2(left - 12.0, rect.center().y - 8.0),
            pos2(left, rect.center().y + 8.0),
        );
        let fold = ui.interact(
            fold_rect,
            Id::unique(("page-fold", page.id.as_str())),
            Sense::click(),
        );
        let label = if o.collapsed {
            t!("app-expand-subpages")
        } else {
            t!("app-collapse-subpages")
        };
        fold.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        ui.painter().text(
            fold_rect.center(),
            Align2::CENTER_CENTER,
            if o.collapsed {
                icons::CARET_RIGHT
            } else {
                icons::CARET_DOWN
            },
            theme::icon(10.0),
            if fold.hovered() { t.ink } else { t.muted },
        );
        if fold.clicked() {
            fold_clicked = true;
            result.context_action = Some(PageListAction::Fold(page.id.clone(), !o.collapsed));
        }
    }
    if let Some(o) = outline {
        let pointer_above = || {
            ui.input(|i| i.pointer.hover_pos())
                .is_some_and(|p| p.y < rect.center().y)
        };
        if let Some(drag) = response.dnd_hover_payload::<PageDrag>() {
            if drag.id != page.id {
                // The line starts at the indent of the level the page will take.
                let (y, level) = if pointer_above() {
                    (rect.top(), o.level)
                } else {
                    (rect.bottom(), o.below_level)
                };
                let x = rect.left() + 10.0 + level as f32 * 16.0;
                ui.painter()
                    .hline(x - 6.0..=right, y, Stroke::new(2.0, t.accent));
            } else if let Some(delta) = sideways(ui) {
                // Dragged sideways over its own row: the line shows the level it takes.
                let level = (o.level as i8 + delta).clamp(0, outline::MAX_LEVEL as i8);
                let x = rect.left() + 10.0 + level as f32 * 16.0;
                ui.painter()
                    .hline(x - 6.0..=right, rect.bottom(), Stroke::new(2.0, t.accent));
            }
        }
        if let Some(drag) = response.dnd_release_payload::<PageDrag>() {
            if drag.id != page.id {
                result.dropped = Some((drag.id.clone(), pointer_above()));
            } else if let Some(delta) = sideways(ui) {
                // Dropped back on its own row, further right or left: a subpage, or one level
                // up (Ctrl+Alt+] / [ do the same).
                result.context_action = Some(PageListAction::Level(page.id.clone(), delta));
            }
        }
    }
    if response.clicked() && !fold_clicked {
        result.clicked = true;
    }
    response.context_menu(|ui| match page_menu(ui, page, outline, clip, t) {
        Some(PageMenuPick::Action(picked)) => result.context_action = Some(picked),
        Some(PageMenuPick::CopyLink) => result.copy_link_clicked = true,
        Some(PageMenuPick::NewPage) | None => {}
    });
    result
}

/// A section in the Recycle Bin, as the bin lists it: its colour bar, its name, and its
/// notebook and pages in muted text. A click picks the row; its menu restores the section
/// or deletes it forever.
fn trashed_section_row(
    ui: &mut Ui,
    section: &TrashedSection,
    picked: bool,
    t: Tokens,
) -> Option<PageListAction> {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &section.name));
    if ui.is_rect_visible(rect) {
        let row = rect.shrink2(vec2(0.0, 2.0));
        let fill = if picked {
            t.page_selected
        } else if response.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(row, CornerRadius::same(4), fill);
        ui.painter().rect_filled(
            Rect::from_min_size(
                pos2(rect.left() + 10.0, rect.center().y - 12.0),
                vec2(6.0, 24.0),
            ),
            CornerRadius::same(3),
            theme::section_color(&section.color, t),
        );
        let left = rect.left() + 24.0;
        let width = rect.right() - 10.0 - left;
        widgets::fitted_text(
            ui.painter(),
            pos2(left, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            section.name.as_str(),
            if picked {
                theme::semibold(13.0)
            } else {
                theme::ui(13.0)
            },
            t.ink,
            width,
        );
        widgets::fitted_text(
            ui.painter(),
            pos2(left, rect.center().y + 1.0),
            Align2::LEFT_TOP,
            t!(
                "app-trash-section-detail",
                notebook = section.notebook_name.as_str(),
                count = section.page_count
            ),
            theme::ui(11.5),
            t.muted,
            width,
        );
    }
    let mut action = response
        .clicked()
        .then(|| PageListAction::PickTrashedSection(section.id.clone()));
    response.context_menu(|ui| {
        let id = &section.id;
        let rows = vec![
            Some((
                icons::ARROW_COUNTER_CLOCKWISE,
                t!("common-restore"),
                String::new(),
                true,
                PageMenuPick::Action(PageListAction::RestoreSection(id.clone())),
            )),
            Some((
                icons::TRASH,
                t!("common-delete-forever"),
                String::new(),
                true,
                PageMenuPick::Action(PageListAction::DeleteTrashedSection(id.clone())),
            )),
        ];
        if let Some(PageMenuPick::Action(picked)) = page_menu_rows(ui, rows, t) {
            action = Some(picked);
        }
    });
    action
}

/// What a page-list menu row does: a page-list action, Copy Link to Page (which needs the
/// egui context), or the empty area's New Page.
enum PageMenuPick {
    Action(PageListAction),
    CopyLink,
    NewPage,
}

/// A row of a page-list menu: glyph, label, shortcut, whether it can run, and what it
/// does. `None` is a separator.
type PageMenuRow = Option<(&'static str, Cow<'static, str>, String, bool, PageMenuPick)>;

/// Draws a page-list menu's rows as `widgets::menu_row`s, as wide as the widest label and
/// shortcut; a click closes the menu and returns that row's pick.
fn page_menu_rows(ui: &mut Ui, rows: Vec<PageMenuRow>, t: Tokens) -> Option<PageMenuPick> {
    let labels = rows
        .iter()
        .flatten()
        .map(|(glyph, label, ..)| (Some(*glyph), label.as_ref()));
    let hint = rows
        .iter()
        .flatten()
        .map(|(_, _, hint, ..)| widgets::text_width(ui.ctx(), hint, theme::ui(11.5)))
        .fold(0.0, f32::max);
    let gap = if hint > 0.0 { 24.0 } else { 0.0 };
    ui.set_width(rows_width(ui, labels) + hint + gap);
    let mut picked = None;
    for row in rows {
        let Some((glyph, label, hint, enabled, pick)) = row else {
            ui.separator();
            continue;
        };
        if widgets::menu_row(ui, Some(glyph), &label, &hint, enabled, t).clicked() {
            picked = Some(pick);
            ui.close();
        }
    }
    picked
}

/// A page row's right-click menu, in the desktop notebook's order (reference §5, "Page tab"): Rename,
/// Delete, Cut, Copy, Paste (greyed without a `clip`), Copy Link to Page, Move or Copy…,
/// New Page, the subpage rows (in the section's own order only, `outline`), and Show Page
/// Versions. A page in the Recycle Bin offers Restore and Delete forever.
fn page_menu(
    ui: &mut Ui,
    page: &PageSummary,
    outline: Option<RowOutline>,
    clip: bool,
    t: Tokens,
) -> Option<PageMenuPick> {
    let id = &page.id;
    let action = |action: PageListAction| PageMenuPick::Action(action);
    if page.trashed {
        let rows = vec![
            Some((
                icons::ARROW_COUNTER_CLOCKWISE,
                t!("common-restore"),
                String::new(),
                true,
                action(PageListAction::Restore(id.clone())),
            )),
            Some((
                icons::TRASH,
                t!("common-delete-forever"),
                String::new(),
                true,
                action(PageListAction::Delete(id.clone())),
            )),
        ];
        return page_menu_rows(ui, rows, t);
    }
    let mut rows = vec![
        Some((
            icons::RENAME,
            t!("common-rename"),
            String::new(),
            true,
            action(PageListAction::Rename(id.clone())),
        )),
        // the desktop notebook's Delete sends the page to the Recycle Bin.
        Some((
            icons::TRASH,
            t!("common-delete"),
            String::new(),
            true,
            action(PageListAction::Trash(id.clone())),
        )),
        Some((
            icons::SCISSORS,
            t!("common-cut"),
            String::new(),
            true,
            action(PageListAction::CutPage(id.clone())),
        )),
        Some((
            icons::COPY,
            t!("common-copy"),
            String::new(),
            true,
            action(PageListAction::CopyPage(id.clone())),
        )),
        Some((
            icons::CLIPBOARD_TEXT,
            t!("common-paste"),
            String::new(),
            clip,
            action(PageListAction::PastePage {
                after: Some(id.clone()),
            }),
        )),
        None,
        Some((
            icons::LINK,
            t!("app-menu-copy-link-to-page"),
            String::new(),
            true,
            PageMenuPick::CopyLink,
        )),
        Some((
            icons::ARROW_SWAP,
            t!("app-move-or-copy"),
            util::menu_shortcut(Modifiers::COMMAND | Modifiers::ALT, Key::M),
            true,
            action(PageListAction::MoveOrCopyPage(id.clone())),
        )),
        None,
        Some((
            icons::DOCUMENT_ADD,
            t!("app-menu-new-page"),
            String::new(),
            true,
            action(PageListAction::NewBelow(id.clone(), false)),
        )),
    ];
    // A sorted list is flat: subpages are made and folded in the section's own order.
    if let Some(o) = outline {
        rows.push(Some((
            icons::TEXT_INDENT_INCREASE_LTR,
            t!("app-make-subpage"),
            String::new(),
            o.can_demote,
            action(PageListAction::Level(id.clone(), 1)),
        )));
        rows.push(Some((
            icons::TEXT_INDENT_DECREASE_LTR,
            t!("app-promote-subpage"),
            String::new(),
            o.level > 0,
            action(PageListAction::Level(id.clone(), -1)),
        )));
        if o.has_children {
            let (glyph, label) = if o.collapsed {
                (icons::CARET_DOWN, t!("app-expand-subpages"))
            } else {
                (icons::CARET_RIGHT, t!("app-collapse-subpages"))
            };
            rows.push(Some((
                glyph,
                label,
                String::new(),
                true,
                action(PageListAction::Fold(id.clone(), !o.collapsed)),
            )));
        }
    }
    rows.push(None);
    rows.push(Some((
        icons::CLOCK_COUNTER_CLOCKWISE,
        t!("app-show-page-versions"),
        String::new(),
        true,
        action(PageListAction::Versions(id.clone())),
    )));
    page_menu_rows(ui, rows, t)
}

/// The menu of the page list's empty area (the desktop notebook's ContextMenuPageArea): New Page, and
/// Paste at the end of the section (greyed without a `clip`).
fn page_area_menu(ui: &mut Ui, clip: bool, t: Tokens) -> Option<PageMenuPick> {
    let rows = vec![
        Some((
            icons::DOCUMENT_ADD,
            t!("app-menu-new-page"),
            String::new(),
            true,
            PageMenuPick::NewPage,
        )),
        Some((
            icons::CLIPBOARD_TEXT,
            t!("common-paste"),
            String::new(),
            clip,
            PageMenuPick::Action(PageListAction::PastePage { after: None }),
        )),
    ];
    page_menu_rows(ui, rows, t)
}

/// Shows what is being dragged next to the pointer.
fn paint_drag_ghost(ctx: &egui::Context, t: Tokens) {
    let Some(pos) = ctx.pointer_interact_pos() else {
        return;
    };
    let Some(label) = egui::DragAndDrop::payload::<PageDrag>(ctx)
        .map(|drag| drag.title.clone())
        .or_else(|| egui::DragAndDrop::payload::<SectionDrag>(ctx).map(|drag| drag.name.clone()))
    else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        Id::unique("folio-drag-ghost"),
    ));
    let galley = painter.layout_no_wrap(util::truncate(&label, 34), theme::semibold(12.0), t.ink);
    let rect = Rect::from_min_size(pos + vec2(14.0, 8.0), galley.size() + vec2(18.0, 10.0));
    painter.rect(
        rect,
        CornerRadius::same(3),
        t.raised,
        Stroke::new(1.0, t.line_strong),
        StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(9.0, 5.0), galley, t.ink);
}

/// `key` was pressed with Shift this frame. Shift is read from the key press itself: the
/// frame's modifier state may already be released by the time the press is handled (and
/// kittest always releases it).
fn pressed_with_shift(ctx: &egui::Context, key: Key) -> bool {
    ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: pressed_key, pressed: true, modifiers, .. }
                if *pressed_key == key && modifiers.shift)
        })
    })
}

fn pressed_with_alt(ctx: &egui::Context, key: Key) -> bool {
    ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: pressed_key, pressed: true, modifiers, .. }
                if *pressed_key == key && modifiers.alt)
        })
    })
}

/// The Ctrl+Alt page shortcuts, matched by key position as well, so Ctrl+Alt+] works on
/// keyboards where brackets need AltGr. A frame that typed text is typing (Windows
/// reports AltGr as Ctrl+Alt, so AltGr+N typing "ń" must not add a page), not a shortcut.
fn consume_alt_shortcut(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|input| {
        if input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Text(_)))
        {
            return false;
        }
        let before = input.events.len();
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key { key: logical, physical_key, pressed: true, modifiers: held, .. }
                    if (*logical == key || *physical_key == Some(key))
                        && held.matches_logically(modifiers)
            )
        });
        input.events.len() != before
    })
}

/// "Term 1 / Science": a group's name with its parents', for menus.
fn group_path(notebook: &crate::model::Notebook, id: &str) -> String {
    let mut names = Vec::new();
    let mut current = Some(id.to_string());
    while let Some(group) = current
        .as_deref()
        .and_then(|id| notebook.groups.iter().find(|group| group.id == id))
    {
        names.push(group.name.clone());
        current = group.parent_id.clone();
        if names.len() > 16 {
            break; // a cycle can't happen, but never loop forever
        }
    }
    names.reverse();
    names.join(" / ")
}

/// A notebook's sections in sidebar order (sections first, then groups, at every level),
/// each as `(id, label)` where the label carries its group path: "Term 1 / Biology".
fn section_choices(notebook: &crate::model::Notebook) -> Vec<(String, String)> {
    fn walk(
        notebook: &crate::model::Notebook,
        parent: Option<&str>,
        depth: usize,
        out: &mut Vec<(String, String)>,
    ) {
        for section in notebook
            .sections
            .iter()
            .filter(|section| section.group_id.as_deref() == parent)
        {
            let label = match parent {
                Some(group) => format!("{} / {}", group_path(notebook, group), section.name),
                None => section.name.clone(),
            };
            out.push((section.id.clone(), label));
        }
        if depth > 16 {
            return; // a cycle can't happen, but never recurse forever
        }
        for group in notebook
            .groups
            .iter()
            .filter(|group| group.parent_id.as_deref() == parent)
        {
            walk(notebook, Some(&group.id), depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(notebook, None, 0, &mut out);
    // A section whose group is missing still gets a choice.
    for section in &notebook.sections {
        if !out.iter().any(|(id, _)| *id == section.id) {
            out.push((section.id.clone(), section.name.clone()));
        }
    }
    out
}

/// A row of Backstage's left column, as Office draws it: the label in 14 px, the selected
/// fill with a bar in the accent on the page shown, the hover fill on the others (in the
/// column's own tokens: white on purple). `selected` is `Some` for a page (a tab) and
/// `None` for a command (Options, Close).
fn backstage_item(ui: &mut Ui, label: &str, selected: Option<bool>, t: Tokens) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    response.widget_info(|| match selected {
        Some(selected) => {
            let mut info = egui::WidgetInfo::labeled(egui::Role::Tab, true, label);
            info.selected = Some(selected);
            info
        }
        None => egui::WidgetInfo::labeled(egui::Role::Button, true, label),
    });
    let fill = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.top() + 2.0),
        pos2(rect.right() - 8.0, rect.bottom() - 2.0),
    );
    let painter = ui.painter();
    if selected == Some(true) {
        painter.rect_filled(fill, CornerRadius::same(4), t.nav_selected);
        painter.rect_filled(
            Rect::from_min_size(pos2(fill.left(), fill.center().y - 10.0), vec2(3.0, 20.0)),
            CornerRadius::same(2),
            t.accent,
        );
    } else if response.hovered() {
        painter.rect_filled(fill, CornerRadius::same(4), t.hover);
    }
    widgets::fitted_text(
        painter,
        pos2(rect.left() + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::ui(14.0),
        t.ink,
        rect.width() - 32.0,
    );
    response
}

/// A choice in Backstage › Export ("Export current" and "Select format"): grey with a
/// purple bar when chosen, the hover fill on the others, greyed when there is nothing to
/// export.
fn export_row(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    selected: bool,
    enabled: bool,
    t: Tokens,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), sense);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, selected, label)
    });
    let fill = rect.shrink2(vec2(0.0, 2.0));
    let painter = ui.painter();
    if selected {
        painter.rect_filled(fill, CornerRadius::same(4), t.nav_selected);
        painter.rect_filled(
            Rect::from_min_size(pos2(fill.left(), fill.center().y - 10.0), vec2(3.0, 20.0)),
            CornerRadius::same(2),
            t.accent,
        );
    } else if enabled && response.hovered() {
        painter.rect_filled(fill, CornerRadius::same(4), t.hover);
    }
    let ink = if enabled { t.ink } else { t.faint };
    painter.text(
        pos2(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        glyph,
        theme::icon(20.0),
        ink,
    );
    widgets::fitted_text(
        painter,
        pos2(rect.left() + 48.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::ui(14.0),
        ink,
        rect.width() - 56.0,
    );
    response
}

/// The heading over a column of Backstage › Export, as Info heads its parts.
fn export_column_heading(ui: &mut Ui, text: &str, t: Tokens) {
    ui.label(RichText::new(text).font(theme::semibold(14.0)).color(t.ink));
    ui.add_space(8.0);
}

/// A Backstage page's title, as Office sets it: large and light, then 20 px of room. It
/// is painted, not a label: the selected tab in the left column already names the page.
fn backstage_heading(ui: &mut Ui, title: &str, t: Tokens) {
    let galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), theme::light(theme::TITLE_SIZE), t.text2);
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, t.text2);
    ui.add_space(20.0);
}

/// A row of a navigation menu: glyph, label, whether it can run, and what it does.
/// `None` is a separator.
type NavMenuRow = Option<(&'static str, Cow<'static, str>, bool, LibraryAction)>;

/// Draws a navigation menu's rows as `widgets::menu_row`s, as wide as the widest; a
/// click closes the menu and returns that row's action.
fn nav_menu(ui: &mut Ui, rows: Vec<NavMenuRow>, t: Tokens) -> Option<LibraryAction> {
    let labels = rows
        .iter()
        .flatten()
        .map(|(glyph, label, _, _)| (Some(*glyph), label.as_ref()));
    ui.set_width(rows_width(ui, labels));
    let mut picked = None;
    for row in rows {
        let Some((glyph, label, enabled, action)) = row else {
            ui.separator();
            continue;
        };
        if widgets::menu_row(ui, Some(glyph), &label, "", enabled, t).clicked() {
            picked = Some(action);
            ui.close();
        }
    }
    picked
}

/// A notebook's right-click menu, in the desktop notebook's order (reference §5): new sections and
/// groups, its place among the notebooks (Move Up greyed for the `first`, Move Down for
/// the `last`), the Recycle Bin, Properties… and Delete. The navigation pane and the
/// Horizontal Tabs notebook picker share it.
fn notebook_menu(
    ui: &mut Ui,
    notebook: &crate::model::Notebook,
    first: bool,
    last: bool,
    t: Tokens,
) -> Option<LibraryAction> {
    let id = &notebook.id;
    let rows = vec![
        Some((
            icons::PLUS,
            t!("app-menu-new-section"),
            true,
            LibraryAction::NewSection(id.clone()),
        )),
        Some((
            icons::FOLDER_SIMPLE_PLUS,
            t!("app-menu-new-section-group"),
            true,
            LibraryAction::NewGroup(id.clone(), None),
        )),
        None,
        Some((
            icons::ARROW_UP,
            t!("app-move-up"),
            !first,
            LibraryAction::MoveNotebook(id.clone(), true),
        )),
        Some((
            icons::ARROW_DOWN,
            t!("app-move-down"),
            !last,
            LibraryAction::MoveNotebook(id.clone(), false),
        )),
        None,
        Some((
            icons::TRASH,
            t!("app-recycle-bin"),
            true,
            LibraryAction::OpenRecycleBin,
        )),
        Some((
            icons::GEAR_SIX,
            t!("app-notebook-properties"),
            true,
            LibraryAction::RenameNotebook(id.clone()),
        )),
        None,
        Some((
            icons::TRASH,
            t!("common-delete"),
            true,
            LibraryAction::DeleteNotebook(id.clone()),
        )),
    ];
    nav_menu(ui, rows, t)
}

/// A section group's right-click menu: Rename and Delete, then new sections and groups
/// inside it.
fn group_menu(
    ui: &mut Ui,
    notebook: &crate::model::Notebook,
    group: &crate::model::SectionGroup,
    t: Tokens,
) -> Option<LibraryAction> {
    let rows = vec![
        Some((
            icons::RENAME,
            t!("common-rename"),
            true,
            LibraryAction::RenameGroup(group.id.clone()),
        )),
        Some((
            icons::TRASH,
            t!("common-delete"),
            true,
            LibraryAction::DeleteGroup(group.id.clone()),
        )),
        None,
        Some((
            icons::PLUS,
            t!("app-menu-new-section"),
            true,
            LibraryAction::NewSectionIn(notebook.id.clone(), Some(group.id.clone())),
        )),
        Some((
            icons::FOLDER_SIMPLE_PLUS,
            t!("app-menu-new-section-group"),
            true,
            LibraryAction::NewGroup(notebook.id.clone(), Some(group.id.clone())),
        )),
    ];
    nav_menu(ui, rows, t)
}

/// A section's right-click menu, in the desktop notebook's order (reference §5, and the screenshots
/// `context-menu-section-and-simplified-home.png` and `fr-menu-onglet-section-2016.png`):
/// Rename, Export…, Delete, Move or Copy…, Merge into Another Section…, Copy Link to
/// Section, new sections and groups beside it, then Section Color ›. The navigation pane
/// and the Horizontal Tabs section tabs share it.
fn section_menu(
    ui: &mut Ui,
    notebook: &crate::model::Notebook,
    section: &crate::model::Section,
    t: Tokens,
) -> Option<LibraryAction> {
    let id = &section.id;
    let rows = vec![
        Some((
            icons::RENAME,
            t!("common-rename"),
            true,
            LibraryAction::RenameSection(id.clone()),
        )),
        Some((
            icons::ARROW_EXPORT,
            t!("app-export-ellipsis"),
            true,
            LibraryAction::ExportSection(id.clone()),
        )),
        Some((
            icons::TRASH,
            t!("common-delete"),
            true,
            LibraryAction::DeleteSection(id.clone()),
        )),
        Some((
            icons::ARROW_SWAP,
            t!("app-move-or-copy"),
            true,
            LibraryAction::MoveOrCopySection(id.clone()),
        )),
        Some((
            icons::MERGE,
            t!("app-merge-section"),
            true,
            LibraryAction::MergeSection(id.clone()),
        )),
        None,
        Some((
            icons::LINK,
            t!("app-copy-link-section"),
            true,
            LibraryAction::CopySectionLink(id.clone()),
        )),
        None,
        // New sections and groups go beside this one, in its group.
        Some((
            icons::PLUS,
            t!("app-menu-new-section"),
            true,
            LibraryAction::NewSectionIn(notebook.id.clone(), section.group_id.clone()),
        )),
        Some((
            icons::FOLDER_SIMPLE_PLUS,
            t!("app-menu-new-section-group"),
            true,
            LibraryAction::NewGroup(notebook.id.clone(), section.group_id.clone()),
        )),
    ];
    let color = t!("app-section-color");
    // The submenu's row is measured with the others, so the menu fits it too.
    ui.set_min_width(rows_width(ui, [(Some(icons::PALETTE), color.as_ref())]));
    let mut picked = nav_menu(ui, rows, t);
    ui.separator();
    ui.menu_button(
        RichText::new(format!("{}    {color}", icons::PALETTE))
            .font(theme::ui(12.5))
            .color(t.ink),
        |ui| {
            let names: Vec<Cow<'static, str>> = theme::SECTION_COLORS
                .iter()
                .map(|hex| theme::color_name(hex))
                .chain([t!("app-color-none")])
                .collect();
            ui.set_width(rows_width(
                ui,
                names
                    .iter()
                    .map(|name| (Some(icons::PALETTE), name.as_ref())),
            ));
            for (&hex, name) in theme::SECTION_COLORS.iter().zip(&names) {
                let current = section.color.eq_ignore_ascii_case(hex);
                if swatch_menu_row(ui, Some(theme::color(hex)), name, current, t).clicked() {
                    picked = Some(LibraryAction::SectionColor(id.clone(), hex.to_string()));
                }
            }
            ui.separator();
            let none = section.color == theme::NO_COLOR;
            let label = names.last().map_or("", |name| name.as_ref());
            if swatch_menu_row(ui, None, label, none, t).clicked() {
                picked = Some(LibraryAction::SectionColor(
                    id.clone(),
                    theme::NO_COLOR.to_string(),
                ));
            }
        },
    );
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// A row of Section Color ›: a 14 px swatch of `fill` where a menu row's glyph goes
/// (nothing for None), the colour's name in the label column, and a 2 px ring around the
/// `current` colour's swatch, as the desktop notebook frames it. It is a button named after the colour.
fn swatch_menu_row(
    ui: &mut Ui,
    fill: Option<Color32>,
    label: &str,
    current: bool,
    t: Tokens,
) -> egui::Response {
    let font = theme::ui(12.5);
    let left = ui.spacing().button_padding.x;
    // Every Fluent glyph has the same advance: the label lines up with glyph rows.
    let label_x =
        left + widgets::text_width(ui.ctx(), &format!("{}    ", icons::PALETTE), font.clone());
    let width = label_x + widgets::text_width(ui.ctx(), label, font.clone()) + 32.0;
    let (rect, response) = ui.allocate_at_least(vec2(width, 28.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() || response.has_focus() {
            let hovered = &ui.visuals().widgets.hovered;
            painter.rect_filled(rect, hovered.corner_radius, hovered.weak_bg_fill);
        }
        let y = rect.center().y;
        let swatch = Rect::from_min_size(pos2(rect.left() + left, y - 7.0), vec2(14.0, 14.0));
        if let Some(fill) = fill {
            painter.rect_filled(swatch, CornerRadius::same(3), fill);
            painter.rect_stroke(
                swatch,
                CornerRadius::same(3),
                Stroke::new(1.0, t.line_strong),
                StrokeKind::Inside,
            );
        }
        if current {
            painter.rect_stroke(
                swatch.expand(2.0),
                CornerRadius::same(4),
                Stroke::new(2.0, t.ink),
                StrokeKind::Inside,
            );
        }
        painter.text(
            pos2(rect.left() + label_x, y),
            Align2::LEFT_CENTER,
            label,
            font,
            t.ink,
        );
    }
    response
}

/// The menu of the navigation pane's empty area: a section or group at the top level of
/// the selected `notebook` (greyed when none is), or a new notebook.
fn nav_area_menu(ui: &mut Ui, notebook: Option<&str>, t: Tokens) -> Option<LibraryAction> {
    let in_notebook = notebook.is_some();
    // A greyed row is never clicked, so its empty id never reaches the library.
    let notebook = notebook.unwrap_or_default().to_string();
    let rows = vec![
        Some((
            icons::PLUS,
            t!("app-menu-new-section"),
            in_notebook,
            LibraryAction::NewSectionIn(notebook.clone(), None),
        )),
        Some((
            icons::FOLDER_SIMPLE_PLUS,
            t!("app-menu-new-section-group"),
            in_notebook,
            LibraryAction::NewGroup(notebook, None),
        )),
        Some((
            icons::NOTEBOOK_ADD,
            t!("app-menu-new-notebook"),
            true,
            LibraryAction::NewNotebook,
        )),
    ];
    nav_menu(ui, rows, t)
}

/// One row of the navigation pane: a full-width hit area with the desktop notebook's hover and
/// selection fills, labelled for screen readers and tests. The caller paints the row's
/// glyphs and text over it.
fn nav_row(
    ui: &mut Ui,
    height: f32,
    label: &str,
    selected: bool,
    sense: Sense,
    t: Tokens,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let fill = if selected {
        t.nav_selected
    } else if response.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(6.0, 1.0)), CornerRadius::same(4), fill);
    }
    response
}

/// The navigation pane's divider between notebooks and above Quick notes: a hairline
/// inset 12 px on both sides.
fn nav_divider(ui: &mut Ui, t: Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(rect.left() + 12.0, rect.top()),
            pos2(rect.right() - 12.0, rect.bottom()),
        ),
        CornerRadius::ZERO,
        t.line,
    );
}

/// Horizontal Tabs: a tab's widest, the room between two tabs, the width of the parent,
/// "+" and "⌄" tabs, and the notebook picker's widest name.
const TAB_MAX: f32 = 200.0;
const TAB_GAP: f32 = 4.0;
const TAB_SMALL: f32 = 30.0;
/// The search box at the right end of the section tabs (Horizontal Tabs).
const TAB_SEARCH_WIDTH: f32 = 220.0;
const PICKER_NAME_MAX: f32 = 180.0;
/// the desktop notebook's folder tabs: rounded at the top, square where they meet the page.
const TAB_RADIUS: CornerRadius = CornerRadius {
    nw: 6,
    ne: 6,
    sw: 0,
    se: 0,
};
/// A card's top corners (`widgets::card`'s radius), for what Horizontal Tabs paints along
/// a card's top edge: the band over the page and the page list's header.
const CARD_TOP_RADIUS: CornerRadius = CornerRadius {
    nw: 8,
    ne: 8,
    sw: 0,
    se: 0,
};
/// The band across the page card's top that the selected section tab joins.
const BAND_HEIGHT: f32 = 6.0;

/// One tab of Horizontal Tabs: a section, or a section group that opens its level.
#[derive(Clone, Copy)]
enum SectionTab<'a> {
    Section(&'a crate::model::Section),
    Group(&'a crate::model::SectionGroup),
}

impl SectionTab<'_> {
    /// The name's width and 14 px on each side (a group's folder glyph too), at most
    /// `TAB_MAX`. A section is measured in the selected tab's semibold, so its name fits
    /// selected or not.
    fn width(&self, ctx: &egui::Context) -> f32 {
        let width = match self {
            SectionTab::Section(section) => {
                widgets::text_width(ctx, &section.name, theme::semibold(13.0)) + 28.0
            }
            SectionTab::Group(group) => {
                widgets::text_width(ctx, &group.name, theme::ui(13.0)) + 28.0 + 20.0
            }
        };
        width.min(TAB_MAX)
    }
}

/// Which of the tabs, `widths` wide and each followed by `TAB_GAP`, fit in `room`. When
/// some do not, `more` is kept for the "⌄" button and the tabs past the room go into it;
/// the `selected` tab is never among them, it takes the last place shown. Returns the
/// indices shown, in the order they are drawn.
fn fit_tabs(widths: &[f32], selected: Option<usize>, room: f32, more: f32) -> Vec<usize> {
    let total: f32 = widths.iter().map(|width| width + TAB_GAP).sum();
    if total <= room {
        return (0..widths.len()).collect();
    }
    let room = room - more;
    let mut shown = Vec::new();
    let mut used = 0.0;
    for (index, width) in widths.iter().enumerate() {
        if used + width + TAB_GAP > room {
            break;
        }
        shown.push(index);
        used += width + TAB_GAP;
    }
    if let Some(selected) = selected.filter(|selected| !shown.contains(selected)) {
        let width = widths[selected] + TAB_GAP;
        while used + width > room {
            let Some(last) = shown.pop() else {
                break;
            };
            used -= widths[last] + TAB_GAP;
        }
        shown.push(selected);
    }
    shown
}

/// A section's tab in `rect`: its colour's pastel and its name (in semibold when
/// `selected`; the selected tab is also taller, joining the band under the tabs). A click
/// opens the section, a double-click renames it, a right-click is its menu, and a page
/// dropped on it moves there.
fn section_tab(
    ui: &mut Ui,
    notebook: &crate::model::Notebook,
    section: &crate::model::Section,
    rect: Rect,
    selected: bool,
    t: Tokens,
) -> Option<LibraryAction> {
    let response = ui.interact(
        rect,
        Id::unique(("folio-section-tab", section.id.as_str())),
        Sense::click(),
    );
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, &section.name));
    let amount = if response.hovered() && !selected {
        theme::TAB_HOVER_TINT
    } else {
        theme::TAB_TINT
    };
    let color = theme::section_color(&section.color, t);
    let painter = ui.painter();
    painter.rect_filled(rect, TAB_RADIUS, theme::pastel(color, t.raised, amount));
    widgets::fitted_text(
        painter,
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        section.name.as_str(),
        if selected {
            theme::semibold(13.0)
        } else {
            theme::ui(13.0)
        },
        t.ink,
        rect.width() - 28.0,
    );
    let mut picked = None;
    // A page dropped on another section's tab moves there, as on the section's row in
    // the navigation pane (the release is only taken while a page hovers).
    if response
        .dnd_hover_payload::<PageDrag>()
        .is_some_and(|drag| drag.section_id != section.id)
    {
        ui.painter()
            .rect_filled(rect, TAB_RADIUS, theme::with_alpha(t.accent, 46));
        if let Some(drag) = response.dnd_release_payload::<PageDrag>() {
            picked = Some(LibraryAction::MovePageTo(
                drag.id.clone(),
                section.id.clone(),
            ));
        }
    }
    if response.clicked() {
        picked = Some(LibraryAction::SelectSection(section.id.clone()));
    }
    // the desktop notebook renames a section on double-click, on its tab as on its row.
    if response.double_clicked() {
        picked = Some(LibraryAction::RenameSection(section.id.clone()));
    }
    response.context_menu(|ui| {
        if let Some(menu) = section_menu(ui, notebook, section, t) {
            picked = Some(menu);
        }
    });
    picked
}

/// A section group's tab in `rect`: its colour's pastel, a folder and its name. Returns
/// whether it was clicked (the tabs then show its contents) and what its right-click
/// menu picked.
fn group_tab(
    ui: &mut Ui,
    notebook: &crate::model::Notebook,
    group: &crate::model::SectionGroup,
    rect: Rect,
    t: Tokens,
) -> (bool, Option<LibraryAction>) {
    let response = ui.interact(
        rect,
        Id::unique(("folio-group-tab", group.id.as_str())),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Tab, true, &group.name));
    let amount = if response.hovered() {
        theme::TAB_HOVER_TINT
    } else {
        theme::TAB_TINT
    };
    let color = theme::section_color(&group.color, t);
    let painter = ui.painter();
    painter.rect_filled(rect, TAB_RADIUS, theme::pastel(color, t.raised, amount));
    let y = rect.center().y;
    painter.text(
        pos2(rect.left() + 14.0, y),
        Align2::LEFT_CENTER,
        icons::FOLDER_SIMPLE,
        theme::icon(14.0),
        t.ink,
    );
    widgets::fitted_text(
        painter,
        pos2(rect.left() + 34.0, y),
        Align2::LEFT_CENTER,
        group.name.as_str(),
        theme::ui(13.0),
        t.ink,
        rect.width() - 48.0,
    );
    let mut picked = None;
    response.context_menu(|ui| {
        if let Some(menu) = group_menu(ui, notebook, group, t) {
            picked = Some(menu);
        }
    });
    (response.clicked(), picked)
}

/// The "⌄" menu of Horizontal Tabs: the tabs that did not fit, a section with its colour
/// pill where the glyph goes, a group with its folder.
fn more_sections_menu<'a>(
    ui: &mut Ui,
    hidden: &[SectionTab<'a>],
    t: Tokens,
) -> Option<SectionTab<'a>> {
    // Every Fluent glyph has the same advance: any one measures the pill's column.
    ui.set_width(rows_width(
        ui,
        hidden.iter().map(|tab| match tab {
            SectionTab::Section(section) => (Some(icons::FOLDER_SIMPLE), section.name.as_str()),
            SectionTab::Group(group) => (Some(icons::FOLDER_SIMPLE), group.name.as_str()),
        }),
    ));
    let mut picked = None;
    for &tab in hidden {
        let row = match tab {
            SectionTab::Section(section) => pill_menu_row(
                ui,
                theme::section_color(&section.color, t),
                &section.name,
                t,
            ),
            SectionTab::Group(group) => widgets::tinted_menu_item(
                ui,
                Some(icons::FOLDER_SIMPLE),
                Some(theme::section_color(&group.color, t)),
                &group.name,
                t,
            ),
        };
        if row.clicked() {
            picked = Some(tab);
            ui.close();
        }
    }
    picked
}

/// A row of the "⌄" menu for a section: its colour pill (6 × 16, as in the navigation
/// pane) where a menu row's glyph goes, and its name in the label column. It is a
/// button named after the section.
fn pill_menu_row(ui: &mut Ui, fill: Color32, label: &str, t: Tokens) -> egui::Response {
    let font = theme::ui(12.5);
    let left = ui.spacing().button_padding.x;
    // Every Fluent glyph has the same advance: the label lines up with glyph rows.
    let glyph = widgets::text_width(ui.ctx(), icons::FOLDER_SIMPLE, font.clone());
    let label_x = left
        + widgets::text_width(
            ui.ctx(),
            &format!("{}    ", icons::FOLDER_SIMPLE),
            font.clone(),
        );
    let width = label_x + widgets::text_width(ui.ctx(), label, font.clone()) + 32.0;
    let (rect, response) = ui.allocate_at_least(vec2(width, 28.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() || response.has_focus() {
            let hovered = &ui.visuals().widgets.hovered;
            painter.rect_filled(rect, hovered.corner_radius, hovered.weak_bg_fill);
        }
        let y = rect.center().y;
        painter.rect_filled(
            Rect::from_center_size(pos2(rect.left() + left + glyph / 2.0, y), vec2(6.0, 16.0)),
            CornerRadius::same(3),
            fill,
        );
        painter.text(
            pos2(rect.left() + label_x, y),
            Align2::LEFT_CENTER,
            label,
            font,
            t.ink,
        );
    }
    response
}

/// What the notebook picker of Horizontal Tabs asked for.
enum PickerPick {
    /// The pin in its list's header: the notebooks become a pane on the left.
    Pin,
    Action(LibraryAction),
}

/// The notebook picker of Horizontal Tabs, with its top-left at `at`: the open
/// notebook's book in its colour, its name, and ⌄. A click drops the notebooks down; a
/// right-click is the notebook's own menu, as on its row in the navigation pane. Returns
/// its right edge and what was picked.
fn notebook_picker(
    ui: &mut Ui,
    notebooks: &[crate::model::Notebook],
    current: Option<&crate::model::Notebook>,
    at: egui::Pos2,
    t: Tokens,
) -> (f32, Option<PickerPick>) {
    let label = t!("app-notebook-picker");
    let name = current.map_or(label.as_ref(), |notebook| notebook.name.as_str());
    let font = theme::semibold(13.0);
    let name_width = widgets::text_width(ui.ctx(), name, font.clone()).min(PICKER_NAME_MAX);
    let rect = Rect::from_min_size(
        at,
        vec2(8.0 + 16.0 + 6.0 + name_width + 6.0 + 12.0 + 8.0, 30.0),
    );
    let response = ui
        .interact(rect, Id::unique("folio-notebook-picker"), Sense::click())
        .on_hover_text(label.as_ref());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let painter = ui.painter();
    if open || response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    let y = rect.center().y;
    painter.text(
        pos2(rect.left() + 8.0, y),
        Align2::LEFT_CENTER,
        icons::NOTEBOOK_FILLED,
        theme::icon_filled(16.0),
        current.map_or(t.muted, |notebook| theme::color(&notebook.color)),
    );
    widgets::fitted_text(
        painter,
        pos2(rect.left() + 30.0, y),
        Align2::LEFT_CENTER,
        name,
        font,
        t.ink,
        name_width + 1.0,
    );
    painter.text(
        pos2(rect.right() - 8.0, y),
        Align2::RIGHT_CENTER,
        icons::CARET_DOWN,
        theme::icon(12.0),
        t.muted,
    );
    let mut picked = egui::Popup::menu(&response)
        .show(|ui| notebook_picker_menu(ui, notebooks, current, t))
        .and_then(|shown| shown.inner);
    if let Some(notebook) = current {
        // Its own popup: the click's is the list of notebooks.
        egui::Popup::context_menu(&response)
            .id(response.id.with("notebook-menu"))
            .show(|ui| {
                let first = notebooks
                    .first()
                    .is_some_and(|first| first.id == notebook.id);
                let last = notebooks.last().is_some_and(|last| last.id == notebook.id);
                if let Some(menu) = notebook_menu(ui, notebook, first, last, t) {
                    picked = Some(PickerPick::Action(menu));
                }
            });
    }
    (rect.right(), picked)
}

/// The notebook picker's list: a "Notebooks" header with the pin that keeps the list as
/// a pane, every notebook (the open one checked; a click opens its first section), then
/// New Notebook.
fn notebook_picker_menu(
    ui: &mut Ui,
    notebooks: &[crate::model::Notebook],
    current: Option<&crate::model::Notebook>,
    t: Tokens,
) -> Option<PickerPick> {
    let heading = t!("app-notebook-picker");
    let new = t!("app-menu-new-notebook");
    let padding = ui.spacing().button_padding.x;
    let header =
        widgets::text_width(ui.ctx(), &heading, theme::semibold(13.0)) + 2.0 * padding + 8.0 + 26.0;
    let rows = notebooks
        .iter()
        .map(|notebook| (Some(icons::NOTEBOOK), notebook.name.as_str()))
        .chain([(Some(icons::NOTEBOOK_ADD), new.as_ref())]);
    ui.set_width(rows_width(ui, rows).max(header));
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.add_space(padding);
        ui.label(
            RichText::new(heading.as_ref())
                .font(theme::semibold(13.0))
                .color(t.ink),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button_sized(
                ui,
                icons::PUSH_PIN,
                &t!("app-pin-notebooks"),
                26.0,
                14.0,
                t,
            )
            .clicked()
            {
                picked = Some(PickerPick::Pin);
            }
        });
    });
    for notebook in notebooks {
        let row = widgets::tinted_checked_menu_item(
            ui,
            Some(icons::NOTEBOOK),
            Some(theme::color(&notebook.color)),
            &notebook.name,
            current.is_some_and(|current| current.id == notebook.id),
            t,
        );
        if row.clicked() {
            // Its first section opens, at the notebook's top level when one is there, so
            // the tabs show that level.
            let first = notebook
                .sections
                .iter()
                .find(|section| section.group_id.is_none())
                .or_else(|| notebook.sections.first());
            picked = Some(PickerPick::Action(match first {
                Some(section) => LibraryAction::SelectSection(section.id.clone()),
                // A notebook without sections has nothing to show: it gets its first.
                None => LibraryAction::NewSection(notebook.id.clone()),
            }));
        }
    }
    ui.separator();
    if widgets::menu_row(ui, Some(icons::NOTEBOOK_ADD), &new, "", true, t).clicked() {
        picked = Some(PickerPick::Action(LibraryAction::NewNotebook));
    }
    if picked.is_some() {
        ui.close();
    }
    picked
}

#[allow(clippy::too_many_arguments)]
fn left_row(
    ui: &mut Ui,
    width: f32,
    height: f32,
    label: &str,
    active: bool,
    strong: bool,
    t: Tokens,
    sense: Sense,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if active {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(2),
            theme::with_alpha(t.accent, if t.dark { 30 } else { 18 }),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(2), t.hover);
    }
    let font = if strong {
        theme::semibold(12.0)
    } else {
        theme::ui(12.0)
    };
    widgets::fitted_text(
        ui.painter(),
        pos2(rect.left() + 3.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        if active { t.ink } else { t.muted },
        rect.width() - 6.0,
    );
    response
}

/// One page in the Options dialog's list: the desktop notebook's grey row with a purple bar when
/// selected, labelled as a tab for screen readers and tests.
fn options_tab(ui: &mut Ui, page: OptionsPage, selected: bool, t: Tokens) -> egui::Response {
    let label = page.label();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Tab, true, label.as_ref());
        info.selected = Some(selected);
        info
    });
    let painter = ui.painter();
    let row = Rect::from_min_max(
        pos2(rect.left(), rect.top() + 1.0),
        pos2(rect.right() - 6.0, rect.bottom() - 1.0),
    );
    if selected {
        painter.rect_filled(row, CornerRadius::same(4), t.nav_selected);
        painter.rect_filled(
            Rect::from_center_size(pos2(row.left() + 1.5, row.center().y), vec2(3.0, 16.0)),
            CornerRadius::same(2),
            t.accent,
        );
    } else if response.hovered() {
        painter.rect_filled(row, CornerRadius::same(4), t.hover);
    }
    widgets::fitted_text(
        painter,
        pos2(row.left() + 12.0, row.center().y),
        Align2::LEFT_CENTER,
        label.as_ref(),
        theme::ui(13.0),
        t.ink,
        row.width() - 16.0,
    );
    response
}

/// A titled group of options, as the desktop notebook's Options pages have: a semibold title over a
/// hairline, then its controls.
fn options_group(ui: &mut Ui, title: &str, t: Tokens, add: impl FnOnce(&mut Ui)) {
    ui.label(
        RichText::new(title)
            .font(theme::semibold(13.0))
            .color(t.ink),
    );
    ui.add_space(2.0);
    widgets::hairline(ui, t.line);
    ui.add_space(8.0);
    ui.indent(title, add);
    ui.add_space(14.0);
}

/// A checkbox of an Options page, greyed (and left as it is) when `enabled` is false.
fn options_check(
    ui: &mut Ui,
    enabled: bool,
    on: &mut bool,
    label: &str,
    t: Tokens,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Checkbox::new(on, RichText::new(label).font(theme::ui(13.0)).color(t.ink)),
    )
}

/// A labelled row of an Options page: the label in a 200 px column, then its control.
fn options_row(ui: &mut Ui, label: &str, t: Tokens, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(OPTIONS_LABEL_WIDTH, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(OPTIONS_LABEL_WIDTH);
                ui.label(RichText::new(label).font(theme::ui(13.0)).color(t.ink));
            },
        );
        add(ui);
    });
}

/// Folio's mark: its own app icon (`assets/icon.png`), scaled to `rect` (20 px in the
/// header strip, 40 in About). Never a letter on a coloured tile, which reads as
/// Microsoft's product icons. Decoded once per process at twice the About size.
fn paint_mark(painter: &egui::Painter, rect: Rect) {
    const SIDE: u32 = 80;
    static IMAGE: std::sync::OnceLock<egui::ColorImage> = std::sync::OnceLock::new();
    let ctx = painter.ctx();
    let id = Id::unique("folio-mark");
    let texture = ctx
        .data(|data| data.get_temp::<egui::TextureHandle>(id))
        .unwrap_or_else(|| {
            let icon = IMAGE.get_or_init(|| {
                image::load_from_memory(include_bytes!("../assets/icon.png")).map_or_else(
                    |_| egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0; 4]),
                    |icon| {
                        let rgba = icon
                            .resize_exact(SIDE, SIDE, image::imageops::FilterType::Lanczos3)
                            .to_rgba8();
                        egui::ColorImage::from_rgba_unmultiplied([SIDE as usize; 2], &rgba)
                    },
                )
            });
            let texture = ctx.load_texture("folio-mark", icon.clone(), Default::default());
            ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(texture.id(), rect, uv, Color32::WHITE);
}

/// What a finished import brought in, and what it left out: report lines in the
/// interface language.
fn import_report_lines(report: &ImportReport) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    for (count, line) in [
        (
            report.pictures,
            t!("import-notebook-pictures", count = report.pictures as i64),
        ),
        (
            report.files,
            t!("import-notebook-files", count = report.files as i64),
        ),
        (
            report.tables,
            t!("import-notebook-tables", count = report.tables as i64),
        ),
        (
            report.ink_strokes,
            t!("import-notebook-ink", count = report.ink_strokes as i64),
        ),
    ] {
        if count > 0 {
            kept.push(line.into_owned());
        }
    }
    let mut skipped = Vec::new();
    if !report.password_sections.is_empty() {
        let names = report.password_sections.join(", ");
        skipped.push(
            t!(
                "import-notebook-skipped-password",
                count = report.password_sections.len() as i64,
                names = names
            )
            .into_owned(),
        );
    }
    if !report.unreadable_sections.is_empty() {
        let names: Vec<&str> = report
            .unreadable_sections
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        skipped.push(
            t!(
                "import-notebook-skipped-unreadable",
                names = names.join(", ")
            )
            .into_owned(),
        );
    }
    if !report.too_big.is_empty() {
        let max = util::file_size(crate::db::MAX_ATTACHMENT_BYTES);
        skipped.push(
            t!(
                "import-notebook-skipped-too-big",
                max = max,
                names = report.too_big.join(", ")
            )
            .into_owned(),
        );
    }
    if !report.unknown_tags.is_empty() {
        let names: Vec<&str> = report.unknown_tags.iter().map(String::as_str).collect();
        skipped.push(t!("import-notebook-skipped-tags", names = names.join(", ")).into_owned());
    }
    for (count, line) in [
        (
            report.missing_files,
            t!(
                "import-notebook-skipped-missing",
                count = report.missing_files as i64
            ),
        ),
        (
            report.unsupported_pictures,
            t!(
                "import-notebook-skipped-pictures",
                count = report.unsupported_pictures as i64
            ),
        ),
        (
            report.office_printout_pages,
            t!(
                "import-notebook-skipped-printouts",
                count = report.office_printout_pages as i64
            ),
        ),
        (
            report.inline_ink,
            t!(
                "import-notebook-skipped-inline-ink",
                count = report.inline_ink as i64
            ),
        ),
        (
            report.math,
            t!("import-notebook-skipped-math", count = report.math as i64),
        ),
        (
            report.outside_links,
            t!(
                "import-notebook-skipped-links",
                count = report.outside_links as i64
            ),
        ),
    ] {
        if count > 0 {
            skipped.push(line.into_owned());
        }
    }
    (kept, skipped)
}

fn import_error_text(error: &ImportError) -> String {
    match error {
        ImportError::PasswordOrDamaged { name } => {
            t!("import-notebook-failed-password", name = name.clone())
        }
        ImportError::Nothing { name } => t!("import-notebook-failed-nothing", name = name.clone()),
        ImportError::TooLarge { name } => t!(
            "import-notebook-failed-too-large",
            name = name.clone(),
            max = util::file_size(crate::import_notebook::PACKAGE_LIMIT)
        ),
        ImportError::Failed { name, error } => t!(
            "import-notebook-failed",
            name = name.clone(),
            error = error.clone()
        ),
        ImportError::Crashed { name } => t!("import-notebook-failed-crashed", name = name.clone()),
        ImportError::Cancelled => t!("import-notebook-cancelled"),
    }
    .into_owned()
}

/// The import's progress: a thin Fluent bar, its done part in the accent, drawn by hand as
/// the printout dialog's is (egui's `ProgressBar` paints its track in the dialog's own
/// colour). While the files are still being opened (`total` 0) a short segment sweeps
/// along it.
fn import_progress_bar(ui: &mut Ui, done: usize, total: usize, t: Tokens) {
    let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(track, CornerRadius::same(3), t.line);
    let part = if total == 0 {
        let phase = (ui.input(|input| input.time) * 0.8).fract() as f32;
        let width = track.width() * 0.3;
        let left = track.left() - width + (track.width() + width) * phase;
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(30));
        (left.max(track.left()), (left + width).min(track.right()))
    } else {
        let fraction = (done as f32 / total as f32).clamp(0.0, 1.0);
        (
            track.left(),
            track.left() + (track.width() * fraction).max(6.0),
        )
    };
    if part.1 > part.0 && (total == 0 || done > 0) {
        let rect = Rect::from_x_y_ranges(part.0..=part.1, track.y_range());
        painter.rect_filled(rect, CornerRadius::same(3), t.accent);
    }
}

/// A dialog's buttons, in a row at its right as the desktop notebook has them. `add` lays them out right
/// to left: the last one (Cancel, Close) first.
fn dialog_buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

/// The colour dialogs' body (Page Color's and More Colors…'s): a plane of colours, then
/// the colour picked beside its "#rrggbb", which can be typed.
fn color_dialog_body(ui: &mut Ui, color: &mut Color32, hex: &mut String, t: Tokens) {
    ui.add_space(10.0);
    if widgets::color_plane(ui, color, vec2(320.0, 160.0), t) {
        let [r, g, b, _] = color.to_array();
        *hex = Rgb(r, g, b).hex();
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(vec2(44.0, 24.0), Sense::hover());
        ui.painter()
            .rect_filled(swatch, CornerRadius::same(3), *color);
        ui.painter().rect_stroke(
            swatch,
            CornerRadius::same(3),
            Stroke::new(1.0, t.line_strong),
            StrokeKind::Inside,
        );
        let field = ui.add(
            TextEdit::singleline(hex)
                .desired_width(90.0)
                .font(theme::mono(13.0))
                .text_color(t.ink),
        );
        if field.changed() {
            if let Some(rgb) = Rgb::from_hex(hex.trim()) {
                *color = rgb.to_color32();
            }
        }
    });
    ui.add_space(14.0);
}

fn dialog_heading(ui: &mut Ui, title: &str, body: &str, t: Tokens) {
    ui.label(
        RichText::new(title)
            .font(theme::semibold(20.0))
            .color(t.ink),
    );
    ui.add_space(3.0);
    ui.label(RichText::new(body).font(theme::ui(12.0)).color(t.muted));
    ui.add_space(14.0);
}

/// More Symbols…'s grid: the symbols of set `set`, or, with a query, those of every set
/// that are the query or have it in a name (in any case), each once: those whose name
/// starts with it first, so "pi" lists π before "pilcrow".
fn symbols_shown(set: usize, query: &str) -> Vec<(char, &'static str)> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return editor::SYMBOL_SETS
            .get(set)
            .map_or_else(Vec::new, |(_, symbols)| symbols.to_vec());
    }
    let mut shown: Vec<(char, &'static str)> = Vec::new();
    let mut later: Vec<(char, &'static str)> = Vec::new();
    for (_, symbols) in editor::SYMBOL_SETS {
        for &(symbol, names) in symbols {
            let names_folded = names.to_lowercase();
            let start = symbol.to_lowercase().eq(query.chars())
                || names_folded.split(' ').any(|name| name.starts_with(&query));
            let inside = names_folded.contains(&query);
            let listed = |list: &[(char, &str)]| list.iter().any(|(kept, _)| *kept == symbol);
            if start && !listed(&shown) {
                later.retain(|(kept, _)| *kept != symbol);
                shown.push((symbol, names));
            } else if inside && !listed(&shown) && !listed(&later) {
                later.push((symbol, names));
            }
        }
    }
    shown.extend(later);
    shown
}

/// A dialog's title row, as About and Options draw it: the title, and a close button at
/// the right. Returns whether the close button was clicked.
fn dialog_title_row(ui: &mut Ui, title: &str, t: Tokens) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::ui(16.0)).color(t.ink));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::icon_button_sized(ui, icons::X, &t!("common-close"), 28.0, 14.0, t).clicked()
        })
        .inner
    })
    .inner
}

/// Where Customize Tags paints a tag's symbol: To Do's box, a built-in's icon, or a custom
/// tag's own symbol (the tag glyph for one with none).
fn gallery_symbol(gallery: &TagGallery, key: TagKey) -> TagSymbol {
    match key {
        TagKey::Todo => TagSymbol::Checkbox,
        TagKey::Line(tag) => TagSymbol::Tag(tag),
        TagKey::Custom(index) => gallery
            .custom(index)
            .map_or(TagSymbol::None, |tag| tag.symbol),
    }
}

/// Paints `symbol` centred on `center`, as the tag column does; a symbol of `None` is the
/// tag glyph, muted.
fn paint_tag_symbol(painter: &egui::Painter, center: egui::Pos2, symbol: TagSymbol, t: Tokens) {
    if symbol == TagSymbol::None {
        painter.text(
            center,
            Align2::CENTER_CENTER,
            icons::TAG,
            theme::icon(13.0),
            t.muted,
        );
    } else {
        let rect = Rect::from_center_size(center, vec2(editor::TAG_SLOT, 24.0));
        editor::paint_symbol(painter, rect, symbol, false, t);
    }
}

/// One row of Customize Tags' list: the tag's symbol, its name, and its Ctrl shortcut
/// for the first nine. The selected row is shaded.
fn customize_tags_row(
    ui: &mut Ui,
    gallery: &TagGallery,
    key: TagKey,
    selected: bool,
    t: Tokens,
) -> egui::Response {
    let label = gallery.label(key);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, selected, label.as_ref())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, t.nav_selected);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, t.hover);
    }
    let y = rect.center().y;
    paint_tag_symbol(
        painter,
        pos2(rect.left() + 16.0, y),
        gallery_symbol(gallery, key),
        t,
    );
    let hint = gallery.digit(key).map(editor::tag_hint).unwrap_or_default();
    let mut room = rect.width() - 32.0 - 10.0;
    if !hint.is_empty() {
        let hint = painter.text(
            pos2(rect.right() - 10.0, y),
            Align2::RIGHT_CENTER,
            hint,
            theme::ui(11.0),
            t.muted,
        );
        room -= hint.width() + 12.0;
    }
    widgets::fitted_text(
        painter,
        pos2(rect.left() + 32.0, y),
        Align2::LEFT_CENTER,
        label,
        theme::ui(12.5),
        t.ink,
        room,
    );
    response
}

/// Customize Tags' list: every tag in the gallery's order, with New Tag…, Modify Tag…,
/// Delete and ↑ / ↓ beside it, then OK and Cancel. Returns `Some(true)` for OK and
/// `Some(false)` for Cancel or the close button.
fn customize_tags_list(
    ui: &mut Ui,
    state: &mut CustomizeTags,
    focus: &mut bool,
    t: Tokens,
) -> Option<bool> {
    const LIST_HEIGHT: f32 = 300.0;
    let mut done = dialog_title_row(ui, &t!("app-customize-tags-title"), t).then_some(false);
    let (new_label, modify_label, delete_label) =
        (t!("app-new-tag"), t!("app-modify-tag"), t!("common-delete"));
    // The buttons beside the list are as wide as each other, and as their longest label.
    let side = [&new_label, &modify_label, &delete_label]
        .into_iter()
        .map(|label| widgets::primary_button_width(ui, None, label))
        .fold(130.0, f32::max);
    ui.add_space(12.0);
    state.selected = state
        .selected
        .min(state.gallery.order.len().saturating_sub(1));
    let custom = state.selected_custom();
    let gap = ui.spacing().item_spacing.x;
    let list_width = ui.available_width() - side - gap;
    let mut picked = None;
    // What a button beside the list asks for: run once the list is drawn.
    let (mut new, mut modify, mut delete, mut up, mut down) = (false, false, false, false, false);
    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .stroke(Stroke::new(1.0, t.line_strong))
            .show(ui, |ui| {
                ui.set_width(list_width - 2.0);
                ScrollArea::vertical()
                    .id_salt("customize-tags-list")
                    .max_height(LIST_HEIGHT)
                    .min_scrolled_height(LIST_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // The rows stack: the scroll area takes the row's horizontal layout.
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (index, key) in state.gallery.order.iter().enumerate() {
                                let selected = index == state.selected;
                                let row = customize_tags_row(ui, &state.gallery, *key, selected, t);
                                if selected && state.reveal {
                                    row.scroll_to_me(None);
                                }
                                if row.clicked() {
                                    picked = Some(index);
                                }
                            }
                        });
                    });
            });
        let layout = Layout::top_down_justified(Align::Min);
        ui.allocate_ui_with_layout(vec2(side, LIST_HEIGHT), layout, |ui| {
            new = widgets::secondary_button(ui, None, &new_label, t).clicked();
            ui.add_enabled_ui(custom.is_some(), |ui| {
                modify = widgets::secondary_button(ui, None, &modify_label, t).clicked();
                delete = widgets::secondary_button(ui, None, &delete_label, t).clicked();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let last = state.gallery.order.len().saturating_sub(1);
                ui.add_enabled_ui(state.selected > 0, |ui| {
                    up = widgets::icon_button_sized(
                        ui,
                        icons::ARROW_UP,
                        &t!("app-move-tag-up"),
                        28.0,
                        14.0,
                        t,
                    )
                    .clicked();
                });
                ui.add_enabled_ui(state.selected < last, |ui| {
                    down = widgets::icon_button_sized(
                        ui,
                        icons::ARROW_DOWN,
                        &t!("app-move-tag-down"),
                        28.0,
                        14.0,
                        t,
                    )
                    .clicked();
                });
            });
        });
    });
    state.reveal = false;
    ui.add_space(8.0);
    ui.label(
        RichText::new(t!("app-customize-tags-note"))
            .font(theme::ui(11.5))
            .color(t.muted),
    );
    ui.add_space(14.0);
    dialog_buttons(ui, |ui| {
        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
            done = Some(false);
        }
        if widgets::primary_button(ui, None, &t!("common-ok"), t).clicked() {
            done = Some(true);
        }
    });
    if let Some(index) = picked {
        state.selected = index;
    }
    if up || down {
        state.move_selected(down);
    }
    if delete {
        state.delete_selected();
    }
    if new || modify {
        let changed = custom
            .filter(|_| modify)
            .and_then(|index| Some((index, state.gallery.custom(index)?)));
        state.editing = Some(match changed {
            Some((index, tag)) => TagForm {
                index: Some(index),
                name: tag.name.clone(),
                symbol: tag.symbol,
                font_color: tag.font_color,
                highlight: tag.highlight,
            },
            None => TagForm {
                index: None,
                name: String::new(),
                symbol: TagSymbol::Checkbox,
                font_color: None,
                highlight: None,
            },
        });
        // The name field takes the keyboard when the form opens.
        *focus = true;
    }
    done
}

/// One of the tag form's symbol buttons, shaded when it is the current one. No symbol is
/// an empty frame, larger and fainter than To Do's box.
fn symbol_button(ui: &mut Ui, symbol: TagSymbol, current: bool, t: Tokens) -> egui::Response {
    let label = match symbol {
        TagSymbol::None => t!("app-tag-no-symbol"),
        TagSymbol::Checkbox => TagMark::Todo.label(),
        TagSymbol::Tag(tag) => tag.label(),
    };
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(28.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, current, label.as_ref())
    });
    let painter = ui.painter();
    let radius = CornerRadius::same(3);
    if current {
        painter.rect_filled(rect, radius, t.nav_selected);
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, t.container_border),
            StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, radius, t.hover);
    }
    if symbol == TagSymbol::None {
        painter.rect_stroke(
            Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0)),
            CornerRadius::same(2),
            Stroke::new(1.0, t.line_strong),
            StrokeKind::Inside,
        );
    } else {
        paint_tag_symbol(painter, rect.center(), symbol, t);
    }
    response.on_hover_text(label)
}

/// Customize Tags' New Tag / Modify Tag form: the name, a symbol, a font colour and a
/// highlight colour, with a preview. OK puts the tag in the list, Cancel goes back to it.
fn tag_form(ui: &mut Ui, state: &mut CustomizeTags, focus: &mut bool, t: Tokens) {
    let Some(mut form) = state.editing.take() else {
        return;
    };
    let title = if form.index.is_some() {
        t!("app-modify-tag")
    } else {
        t!("app-new-tag")
    };
    // The buttons' "New Tag…" and "Modify Tag…" name the form without their "…".
    let mut cancel = dialog_title_row(ui, title.trim_end_matches('…'), t);
    ui.add_space(12.0);
    widgets::caption(ui, &t!("app-tag-name"), t.muted);
    ui.add_space(4.0);
    let submit = name_field(ui, &mut form.name, "tag-name", "", *focus, t);
    // The dialog's first frame is a sizing pass that can't take focus.
    *focus = ui.is_sizing_pass();
    let taken = state.name_taken(&form.name, form.index);
    let valid = !form.name.trim().is_empty() && !taken;
    if taken {
        ui.label(
            RichText::new(t!("app-tag-name-taken"))
                .font(theme::ui(12.0))
                .color(t.danger),
        );
        ui.add_space(6.0);
    }
    widgets::caption(ui, &t!("app-tag-symbol"), t.muted);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for symbol in TagSymbol::choices() {
            if symbol_button(ui, symbol, form.symbol == symbol, t).clicked() {
                form.symbol = symbol;
            }
        }
    });
    ui.add_space(10.0);
    widgets::caption(ui, &t!("app-tag-font-color"), t.muted);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let automatic = t!("app-tag-automatic");
        if widgets::toggle_chip(ui, form.font_color.is_none(), &automatic, t).clicked() {
            form.font_color = None;
        }
        for (color, _) in canvas::PEN_COLORS {
            let current = form.font_color == Some(color);
            if editor::color_swatch(ui, color, 18.0, current, t).clicked() {
                form.font_color = Some(color);
            }
        }
    });
    ui.add_space(10.0);
    widgets::caption(ui, &t!("app-tag-highlight-color"), t.muted);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let none = t!("app-tag-no-color");
        if widgets::toggle_chip(ui, form.highlight.is_none(), &none, t).clicked() {
            form.highlight = None;
        }
        for color in Rgb::HIGHLIGHTERS {
            let current = form.highlight == Some(color);
            if editor::color_swatch(ui, color, 18.0, current, t).clicked() {
                form.highlight = Some(color);
            }
        }
    });
    ui.add_space(10.0);
    // The tag as a tagged paragraph shows it: its symbol in the margin, then the name in
    // its colours. Highlighted text stays dark in dark mode, as on the page.
    widgets::caption(ui, &t!("app-tag-preview"), t.muted);
    ui.add_space(4.0);
    egui::Frame::new()
        .stroke(Stroke::new(1.0, t.line_strong))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (slot, _) =
                    ui.allocate_exact_size(vec2(editor::TAG_SLOT, 24.0), Sense::hover());
                editor::paint_symbol(ui.painter(), slot, form.symbol, false, t);
                let ink = match form.font_color {
                    Some(color) => color.to_color32(),
                    None if t.dark && form.highlight.is_some() => Rgb::INKS[0].to_color32(),
                    None => t.ink,
                };
                let mut text = RichText::new(form.name.trim())
                    .font(theme::ui(14.0))
                    .color(ink);
                if let Some(highlight) = form.highlight {
                    text = text.background_color(highlight.to_color32());
                }
                ui.label(text);
            });
        });
    ui.add_space(14.0);
    let mut ok = submit && valid;
    dialog_buttons(ui, |ui| {
        if widgets::text_button(ui, None, &t!("common-cancel"), t).clicked() {
            cancel = true;
        }
        ui.add_enabled_ui(valid, |ui| {
            ok |= widgets::primary_button(ui, None, &t!("common-ok"), t).clicked();
        });
    });
    if ok {
        state.save_form(form);
    } else if !cancel {
        state.editing = Some(form);
    }
}

/// A dialog's name input. Returns `true` when Enter is pressed in it, to submit.
#[cfg(not(test))]
fn open_external(url: &str) -> std::io::Result<()> {
    open::that(url)
}

/// Tests never launch a browser.
#[cfg(test)]
fn open_external(_url: &str) -> std::io::Result<()> {
    Ok(())
}

#[derive(Debug, Clone)]
struct LinkChoice {
    id: String,
    title: String,
    place: String,
}

fn name_field(
    ui: &mut Ui,
    name: &mut String,
    key: &'static str,
    hint: &str,
    focus: bool,
    t: Tokens,
) -> bool {
    // The id comes from `key`, never from the (translated) hint.
    let id = Id::unique(("folio-name-field", key));
    if focus {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let response = egui::Frame::new()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.line_strong))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::symmetric(9, 5))
        .show(ui, |ui| {
            ui.set_width(360.0);
            ui.add(
                TextEdit::singleline(name)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(Margin::ZERO)
                    .desired_width(f32::INFINITY)
                    .font(theme::ui(14.0))
                    .text_color(t.ink)
                    .hint_text(RichText::new(hint).font(theme::ui(14.0)).color(t.faint)),
            )
        })
        .inner;
    ui.add_space(10.0);
    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
}

fn color_field(ui: &mut Ui, color: &mut String, t: Tokens) {
    widgets::caption(ui, &t!("dialog-color"), t.muted);
    ui.add_space(4.0);
    widgets::swatch_grid(ui, color, false, t);
}

/// The Move or Copy dialog's width, and its tree's height.
const MOVE_DIALOG_WIDTH: f32 = 480.0;
const MOVE_TREE_HEIGHT: f32 = 300.0;

/// What a row of the Move or Copy tree shows before its name.
#[derive(Debug, Clone, Copy)]
enum TreeMark {
    Notebook,
    Group,
    Section,
}

/// How a row of the Move or Copy tree answers a click.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TreeRow {
    /// Shown for where it sits; not a place to put this in.
    Heading,
    /// A place it can't go: where the section already is, or the section itself.
    Disabled,
    /// A place to pick; `true` once picked.
    Target(bool),
}

/// The Move or Copy dialog's tree: every notebook, with its sections and section groups
/// under it in the navigation pane's order. Only the rows `what` can go into answer a
/// click, which sets `target`. With a `filter`, a row shows when its name, or a name
/// below it, contains the filter, ignoring case and accents.
fn move_tree(
    ui: &mut Ui,
    workspace: &crate::model::Workspace,
    what: &MoveWhat,
    filter: &str,
    target: &mut Option<MoveTarget>,
    t: Tokens,
) {
    let needle = crate::table::fold(filter);
    // The notebooks, groups and sections whose own name matches; `None` without a
    // filter, when every row shows.
    let hits: Option<HashSet<&str>> = (!needle.is_empty()).then(|| {
        workspace
            .notebooks
            .iter()
            .flat_map(|notebook| {
                std::iter::once((notebook.id.as_str(), notebook.name.as_str()))
                    .chain(
                        notebook
                            .groups
                            .iter()
                            .map(|group| (group.id.as_str(), group.name.as_str())),
                    )
                    .chain(
                        notebook
                            .sections
                            .iter()
                            .map(|section| (section.id.as_str(), section.name.as_str())),
                    )
            })
            .filter(|(_, name)| crate::table::fold(name).contains(&needle))
            .map(|(id, _)| id)
            .collect()
    });
    // The section being moved or merged.
    let moving = match what {
        MoveWhat::Section(id) | MoveWhat::Merge(id) => workspace
            .notebooks
            .iter()
            .flat_map(|notebook| notebook.sections.iter())
            .find(|section| section.id == *id),
        MoveWhat::Page(_) => None,
    };
    let tree = MoveTree { what, moving, hits };
    egui::Frame::new()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.line_strong))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(Margin::same(4))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt("move-tree")
                .max_height(MOVE_TREE_HEIGHT)
                .min_scrolled_height(MOVE_TREE_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for notebook in &workspace.notebooks {
                        let shown = tree.hits.as_ref().is_none_or(|hits| {
                            hits.contains(notebook.id.as_str())
                                || notebook
                                    .groups
                                    .iter()
                                    .any(|group| hits.contains(group.id.as_str()))
                                || notebook
                                    .sections
                                    .iter()
                                    .any(|section| hits.contains(section.id.as_str()))
                        });
                        if !shown {
                            continue;
                        }
                        let here = MoveTarget::Notebook(notebook.id.clone());
                        // A section goes to a notebook's top level, except the one it is in.
                        let row = match tree.what {
                            MoveWhat::Section(_)
                                if tree.moving.is_some_and(|section| {
                                    section.notebook_id == notebook.id && section.group_id.is_none()
                                }) =>
                            {
                                TreeRow::Disabled
                            }
                            MoveWhat::Section(_) => TreeRow::Target(target.as_ref() == Some(&here)),
                            _ => TreeRow::Heading,
                        };
                        let color = theme::color(&notebook.color);
                        if move_row(ui, 0, TreeMark::Notebook, color, &notebook.name, row, t)
                            .clicked()
                        {
                            *target = Some(here);
                        }
                        tree.level(ui, notebook, None, 1, target, t);
                    }
                });
        });
    ui.add_space(4.0);
}

/// What the Move or Copy tree draws by: what moves, the section that does (when one
/// does), and the ids whose names match the filter (`None` without one).
struct MoveTree<'a> {
    what: &'a MoveWhat,
    moving: Option<&'a crate::model::Section>,
    hits: Option<HashSet<&'a str>>,
}

impl MoveTree<'_> {
    /// The sections, then the section groups, in `parent` (the notebook's top level for
    /// `None`), as the navigation pane lists them.
    fn level(
        &self,
        ui: &mut Ui,
        notebook: &crate::model::Notebook,
        parent: Option<&str>,
        depth: usize,
        target: &mut Option<MoveTarget>,
        t: Tokens,
    ) {
        for section in notebook
            .sections
            .iter()
            .filter(|section| section.group_id.as_deref() == parent)
        {
            if self
                .hits
                .as_ref()
                .is_some_and(|hits| !hits.contains(section.id.as_str()))
            {
                continue;
            }
            let here = MoveTarget::Section(section.id.clone());
            let picked = target.as_ref() == Some(&here);
            let row = match self.what {
                MoveWhat::Page(_) => TreeRow::Target(picked),
                // A section never merges into itself.
                MoveWhat::Merge(id) if *id == section.id => TreeRow::Disabled,
                MoveWhat::Merge(_) => TreeRow::Target(picked),
                MoveWhat::Section(_) => TreeRow::Heading,
            };
            let color = theme::section_color(&section.color, t);
            if move_row(ui, depth, TreeMark::Section, color, &section.name, row, t).clicked() {
                *target = Some(here);
            }
        }
        for group in notebook
            .groups
            .iter()
            .filter(|group| group.parent_id.as_deref() == parent)
        {
            if !self.group_shows(notebook, &group.id, 0) {
                continue;
            }
            let here = MoveTarget::Group {
                notebook: notebook.id.clone(),
                id: group.id.clone(),
            };
            let row = match self.what {
                MoveWhat::Section(_)
                    if self
                        .moving
                        .is_some_and(|section| section.group_id.as_deref() == Some(&group.id)) =>
                {
                    TreeRow::Disabled
                }
                MoveWhat::Section(_) => TreeRow::Target(target.as_ref() == Some(&here)),
                _ => TreeRow::Heading,
            };
            let color = theme::color(&group.color);
            if move_row(ui, depth, TreeMark::Group, color, &group.name, row, t).clicked() {
                *target = Some(here);
            }
            if depth < 16 {
                self.level(ui, notebook, Some(&group.id), depth + 1, target, t);
            }
        }
    }

    /// Whether a group shows: its name, or the name of a section or group inside it,
    /// matches the filter.
    fn group_shows(&self, notebook: &crate::model::Notebook, id: &str, depth: usize) -> bool {
        let Some(hits) = self.hits.as_ref() else {
            return true;
        };
        hits.contains(id)
            || notebook.sections.iter().any(|section| {
                section.group_id.as_deref() == Some(id) && hits.contains(section.id.as_str())
            })
            || (depth < 16
                && notebook
                    .groups
                    .iter()
                    .filter(|group| group.parent_id.as_deref() == Some(id))
                    .any(|group| self.group_shows(notebook, &group.id, depth + 1)))
    }
}

/// One row of the Move or Copy tree: the notebook's book, the group's folder or the
/// section's pill in its colour, then the name; the picked row on the navigation pane's
/// grey.
fn move_row(
    ui: &mut Ui,
    depth: usize,
    mark: TreeMark,
    color: Color32,
    name: &str,
    row: TreeRow,
    t: Tokens,
) -> egui::Response {
    let live = matches!(row, TreeRow::Target(_));
    let sense = if live { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), sense);
    let role = if row == TreeRow::Heading {
        egui::Role::Label
    } else {
        egui::Role::Button
    };
    response.widget_info(|| egui::WidgetInfo::labeled(role, live, name));
    let fill = match row {
        TreeRow::Target(true) => t.nav_selected,
        TreeRow::Target(false) if response.hovered() => t.hover,
        _ => Color32::TRANSPARENT,
    };
    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect.shrink2(vec2(2.0, 1.0)), CornerRadius::same(4), fill);
    }
    let x = rect.left() + 8.0 + depth as f32 * 16.0;
    let y = rect.center().y;
    let text_x = match mark {
        TreeMark::Notebook => {
            painter.text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                icons::NOTEBOOK_FILLED,
                theme::icon_filled(16.0),
                color,
            );
            x + 22.0
        }
        TreeMark::Group => {
            painter.text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                icons::FOLDER_SIMPLE,
                theme::icon(14.0),
                color,
            );
            x + 20.0
        }
        TreeMark::Section => {
            painter.rect_filled(
                Rect::from_min_size(pos2(x + 2.0, y - 8.0), vec2(6.0, 16.0)),
                CornerRadius::same(3),
                color,
            );
            x + 16.0
        }
    };
    let (font, ink) = match mark {
        TreeMark::Notebook => (theme::semibold(13.0), t.ink),
        TreeMark::Group | TreeMark::Section => (theme::ui(13.0), t.text2),
    };
    let ink = if row == TreeRow::Disabled {
        t.faint
    } else {
        ink
    };
    widgets::fitted_text(
        painter,
        pos2(text_x, y),
        Align2::LEFT_CENTER,
        name,
        font,
        ink,
        rect.right() - text_x - 8.0,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_symbols_finds_a_symbol_by_name_or_by_itself() {
        let chars = |query: &str| -> Vec<char> {
            symbols_shown(0, query)
                .into_iter()
                .map(|(symbol, _)| symbol)
                .collect()
        };
        assert_eq!(chars("euro"), ['€']);
        assert_eq!(chars(" EUR "), ['€'], "any case, spaces trimmed");
        assert_eq!(chars("€"), ['€'], "the symbol itself");
        let pi = chars("pi");
        assert!(pi.contains(&'π') && pi.contains(&'Π'), "{pi:?}");
        // Any of a symbol's names: ∪ is also "union".
        assert_eq!(chars("union"), ['∪']);
        // ⇒ is in two sets, and listed once.
        assert_eq!(chars("⇒"), ['⇒']);
        assert!(chars("zzz").is_empty());
        // Anywhere in a name: "arrow" finds the arrows, ⇒ once although its other name
        // (implies) does not hold it.
        let arrows = chars("Arrow");
        for arrow in ['←', '→', '↔', '⇐', '⇒', '↻'] {
            assert!(arrows.contains(&arrow), "{arrow} in {arrows:?}");
        }
        assert_eq!(arrows.iter().filter(|&&found| found == '⇒').count(), 1);
        // A name that starts with the query comes before one that only holds it: η (eta)
        // before β (beta), which comes first in its set.
        let eta = chars("eta");
        assert_eq!(eta.first(), Some(&'η'), "{eta:?}");
        assert!(eta.contains(&'β') && eta.contains(&'θ'), "{eta:?}");
        // No query: the set chosen, whole.
        assert_eq!(symbols_shown(1, "").len(), editor::SYMBOL_SETS[1].1.len());
        assert!(symbols_shown(99, "").is_empty());
    }

    #[test]
    fn backstage_column_reads_white_on_purple() {
        for t in [Tokens::light(), Tokens::dark()] {
            assert!(theme::contrast(Color32::WHITE, theme::backstage_fill(t)) >= 4.5);
        }
    }

    /// Horizontal Tabs: tabs past the room go into "⌄" (whose room is kept then), and the
    /// selected tab never does: it takes the last place shown.
    #[test]
    fn section_tabs_overflow_but_keep_the_selected_one() {
        let widths = [100.0, 100.0, 100.0, 100.0];
        let row = |count: f32| count * (100.0 + TAB_GAP);
        assert_eq!(fit_tabs(&widths, Some(3), row(4.0), 30.0), [0, 1, 2, 3]);
        assert_eq!(fit_tabs(&widths, Some(0), row(3.0) + 30.0, 30.0), [0, 1, 2]);
        assert_eq!(fit_tabs(&widths, Some(3), row(3.0) + 30.0, 30.0), [0, 1, 3]);
        assert_eq!(fit_tabs(&widths, None, row(2.0) + 30.0, 30.0), [0, 1]);
        // No room at all: the selected tab still shows.
        assert_eq!(fit_tabs(&widths, Some(2), 10.0, 30.0), [2]);
    }

    #[test]
    fn customize_tags_adds_changes_moves_and_deletes_custom_tags() {
        crate::i18n::pin_thread_lang(Lang::En);
        let form = |index: Option<u16>, name: &str| TagForm {
            index,
            name: name.to_string(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: Some(Rgb::HIGHLIGHTERS[1]),
        };
        let mut dialog = CustomizeTags {
            gallery: TagGallery::default(),
            selected: 1,
            editing: None,
            reveal: false,
        };
        // A new tag goes in after the selected row (Important) and is selected.
        dialog.save_form(form(None, "  Homework "));
        assert_eq!(dialog.gallery.order[2], TagKey::Custom(0));
        assert_eq!(dialog.selected, 2);
        assert_eq!(dialog.gallery.custom[0].name, "Homework");
        assert_eq!(dialog.selected_custom(), Some(0));
        dialog.save_form(form(None, "Reading"));
        assert_eq!(dialog.gallery.order[3], TagKey::Custom(1));
        // Names are unique among every tag's, ignoring case; a tag may keep its own.
        assert!(dialog.name_taken("homework", None));
        assert!(dialog.name_taken("IMPORTANT", None));
        assert!(!dialog.name_taken("Homework", Some(0)));
        assert!(!dialog.name_taken("Chores", None));
        // A changed tag keeps its key and place.
        dialog.save_form(form(Some(0), "Devoirs"));
        assert_eq!(dialog.gallery.order[2], TagKey::Custom(0));
        assert_eq!(dialog.gallery.custom[0].name, "Devoirs");
        // ↑ twice takes Homework to the top; ↑ at the top does nothing.
        dialog.selected = 2;
        dialog.move_selected(false);
        dialog.move_selected(false);
        dialog.move_selected(false);
        assert_eq!(dialog.gallery.order[0], TagKey::Custom(0));
        assert_eq!(dialog.gallery.order[1], TagKey::Todo);
        assert_eq!(dialog.selected, 0);
        // Deleting it renumbers the custom tags after it.
        let revision = dialog.gallery.revision;
        dialog.delete_selected();
        assert_eq!(dialog.gallery.custom.len(), 1);
        assert_eq!(dialog.gallery.custom[0].name, "Reading");
        assert_eq!(dialog.gallery.order[0], TagKey::Todo);
        assert_eq!(dialog.gallery.order[2], TagKey::Custom(0));
        assert_eq!(dialog.gallery.order.len(), 30);
        assert!(dialog.gallery.revision > revision);
        // Built-in tags are never deleted.
        dialog.selected = 1;
        dialog.delete_selected();
        assert_eq!(dialog.gallery.order.len(), 30);
    }

    #[test]
    fn the_tags_summary_groups_by_each_choice() {
        crate::i18n::pin_thread_lang(Lang::En);
        const DAY: i64 = 86_400_000;
        let line = |title: &str, section: &str, mark: TagMark, text: &str, day: i64| TaggedLine {
            page_id: title.to_lowercase(),
            page_title: title.to_string(),
            section_id: section.to_lowercase(),
            section_name: section.to_string(),
            section_color: String::new(),
            notebook_id: "nb".into(),
            notebook_name: "Notebook".into(),
            item: 1,
            block: 1,
            mark,
            text: text.to_string(),
            checked: false,
            order: 0,
            custom: None,
            updated_at: 1_790_000_000_000 + day * DAY,
        };
        let important = TagMark::Line(crate::model::LineTag::Important);
        let lines = vec![
            line(
                "Maths",
                "Science",
                TagMark::Line(crate::model::LineTag::Question),
                "Pythagoras",
                0,
            ),
            line("Biology", "Science", important, "Cells", 2),
            line("Biology", "Science", TagMark::Todo, "Read chapter 2", 2),
            line("Essay", "English", important, "Argument", 1),
        ];
        let mut lines = lines;
        lines[2].order = 1;
        let order = |by: TagsGroupBy| {
            let mut lines = lines.clone();
            sort_tagged(&mut lines, by);
            lines.into_iter().map(|line| line.text).collect::<Vec<_>>()
        };
        // Tag name: the tags' names in alphabetical order, whatever their case, a custom
        // tag among the built-ins, as the desktop notebook lists them; within a group, by page title.
        let mut by_name = lines.clone();
        by_name.push(line(
            "Reading",
            "English",
            TagMark::Line(crate::model::LineTag::Book),
            "Dune",
            3,
        ));
        let mut homework = line("Essay", "English", TagMark::Custom, "Outline", 1);
        homework.custom = Some(CustomTag {
            name: "homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: false,
        });
        by_name.push(homework);
        let tag_order = |lines: &[TaggedLine]| {
            let mut lines = lines.to_vec();
            sort_tagged(&mut lines, TagsGroupBy::TagName);
            lines.into_iter().map(|line| line.text).collect::<Vec<_>>()
        };
        // Book to read, homework, Important, Question, To Do.
        assert_eq!(
            tag_order(&by_name),
            [
                "Dune",
                "Outline",
                "Cells",
                "Argument",
                "Pythagoras",
                "Read chapter 2"
            ]
        );
        assert_eq!(
            order(TagsGroupBy::Title),
            ["Cells", "Read chapter 2", "Argument", "Pythagoras"]
        );
        // The pane numbers the groups once, when it sorts, and draws from the numbers.
        let mut by_title = lines.clone();
        sort_tagged(&mut by_title, TagsGroupBy::Title);
        assert_eq!(group_ids(&by_title, TagsGroupBy::Title), [0, 0, 1, 2]);
        assert_eq!(
            order(TagsGroupBy::Section),
            ["Argument", "Cells", "Read chapter 2", "Pythagoras"]
        );
        // Newest day first.
        assert_eq!(
            order(TagsGroupBy::Date),
            ["Cells", "Read chapter 2", "Argument", "Pythagoras"]
        );
        assert_eq!(
            order(TagsGroupBy::NoteText),
            ["Argument", "Cells", "Pythagoras", "Read chapter 2"]
        );
        // Headings name the group; Note text has none.
        let cells = &lines[1];
        assert_eq!(
            tags_heading_text(cells, TagsGroupBy::TagName).as_deref(),
            Some("Important")
        );
        assert_eq!(
            tags_heading_text(cells, TagsGroupBy::Title).as_deref(),
            Some("Biology")
        );
        assert_eq!(
            tags_heading_text(cells, TagsGroupBy::Section).as_deref(),
            Some("Science")
        );
        let day = local_time(cells.updated_at).map(util::long_date);
        assert!(day.is_some());
        assert_eq!(tags_heading_text(cells, TagsGroupBy::Date), day);
        assert_eq!(tags_heading_text(cells, TagsGroupBy::NoteText), None);
        assert_eq!(
            group_key(&lines[1], TagsGroupBy::Date),
            group_key(&lines[2], TagsGroupBy::Date)
        );
        // In French the names are French and "É" sorts with "e": À faire, Étude,
        // Important, Livre à lire, Question.
        crate::i18n::pin_thread_lang(Lang::Fr);
        if let Some(custom) = by_name[5].custom.as_mut() {
            custom.name = "Étude".into();
        }
        assert_eq!(
            tag_order(&by_name),
            [
                "Read chapter 2",
                "Outline",
                "Cells",
                "Argument",
                "Dune",
                "Pythagoras"
            ]
        );
    }

    #[test]
    fn a_summary_page_lists_the_lines_tagged_and_linked_under_their_groups() {
        crate::i18n::pin_thread_lang(Lang::En);
        use crate::model::LineTag;
        let line = |page: &str, title: &str, mark: TagMark, text: &str, checked: bool| TaggedLine {
            page_id: page.to_string(),
            page_title: title.to_string(),
            section_id: "s".into(),
            section_name: "This week".into(),
            section_color: String::new(),
            notebook_id: "nb".into(),
            notebook_name: "Notebook".into(),
            item: 1,
            block: 1,
            mark,
            text: text.to_string(),
            checked,
            order: 0,
            custom: None,
            updated_at: 1_790_000_000_000,
        };
        let mut homework = line("p2", "", TagMark::Custom, "Essay", true);
        homework.custom = Some(CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: true,
        });
        let lines = [
            line("p1", "Biology", TagMark::Todo, "Read chapter 2", true),
            line(
                "p1",
                "Biology",
                TagMark::Line(LineTag::Important),
                "Cells",
                false,
            ),
            line(
                "p1",
                "Biology",
                TagMark::Line(LineTag::CallBack),
                "Ring",
                true,
            ),
            homework,
        ];
        // The pane shortens a long line; the page gets the paragraph whole, formatting
        // and all. A line whose paragraph is gone keeps the pane's text.
        let long = format!("Cells divide{}", " and divide".repeat(20));
        let mut whole = StyledText::new(long.clone());
        whole.set_bold(0..5, true);
        let texts = [None, Some(whole), None];
        let canvas = tags_summary_canvas(&lines, &texts, TagsGroupBy::Title);
        assert_eq!(canvas.items.len(), 1);
        let item = &canvas.items[0];
        let blocks = &item.doc().unwrap().blocks;
        let texts: Vec<&str> = blocks
            .iter()
            .map(|block| block.text.text.as_str())
            .collect();
        assert_eq!(
            texts,
            [
                "Biology",
                "Read chapter 2 (Biology)",
                &format!("{long} (Biology)"),
                "Ring (Biology)",
                "Untitled page",
                "Essay (Untitled page)",
            ]
        );
        assert!(blocks[2].text.style_at(0).bold && !blocks[2].text.style_at(6).bold);
        assert_eq!(blocks[0].kind, BlockKind::heading(2));
        assert_eq!(blocks[1].kind, BlockKind::Todo { checked: true });
        assert_eq!(blocks[2].tags, [LineTag::Important]);
        assert!(blocks[2].checked_tags.is_empty());
        assert_eq!(blocks[3].checked_tags, [LineTag::CallBack]);
        assert_eq!(blocks[5].custom_tags[0].name, "Homework");
        assert!(blocks[5].custom_tags[0].checked);
        // "(Biology)" links back to its page; the line's own text does not.
        let ring = &blocks[3].text;
        let link = |at: usize| ring.style_at(at).link.as_deref().map(str::to_string);
        assert_eq!(link(0), None);
        assert_eq!(link(4), None, "the space before the brackets");
        assert_eq!(link(5), Some(page_link("p1")));
        assert_eq!(link(13), Some(page_link("p1")));
        let cells = &blocks[2].text;
        let end = cells.text.len() - 1;
        assert_eq!(
            cells.style_at(end).link.as_deref(),
            Some(page_link("p1").as_str())
        );
        assert_eq!(cells.style_at(long.len()).link, None);
        // The note's text starts under the title; its tags hang in the margin.
        let lead = editor::tag_lead(item.doc().unwrap());
        assert_eq!(item.x, PAGE_LEFT - canvas::NOTE_PAD_X - lead);
        // Note text: no headings.
        let flat = tags_summary_canvas(&lines, &[], TagsGroupBy::NoteText);
        assert_eq!(flat.items[0].doc().unwrap().blocks.len(), 4);
        // The pane's rows: a box shows its tick, a custom one too; other tags their icon.
        assert_eq!(tagged_line_glyph(&lines[0]), icons::CHECK_SQUARE);
        assert_eq!(tagged_line_glyph(&lines[1]), LineTag::Important.icon());
        assert_eq!(tagged_line_glyph(&lines[3]), icons::CHECK_SQUARE);
        let mut unticked = lines[3].clone();
        unticked.checked = false;
        if let Some(tag) = unticked.custom.as_mut() {
            tag.checked = false;
        }
        assert_eq!(tagged_line_glyph(&unticked), icons::SQUARE);
        // Call back is drawn as on the page, so its tick shows; Important has none.
        assert_eq!(
            ticked_symbol(&lines[2]),
            Some(TagSymbol::Tag(LineTag::CallBack))
        );
        assert_eq!(ticked_symbol(&lines[1]), None);
        assert_eq!(ticked_symbol(&lines[3]), None, "a custom box is a glyph");
    }

    #[test]
    fn search_scopes_keep_their_codes() {
        for scope in ScopeKind::ALL {
            assert_eq!(ScopeKind::from_code(scope.code()), scope);
            assert!(!scope.label().is_empty());
        }
        assert_eq!(ScopeKind::from_code("page"), ScopeKind::All);
        assert_eq!(ScopeKind::from_code(""), ScopeKind::All);
    }

    #[test]
    fn app_counts_use_each_languages_plural_rules() {
        use crate::i18n::{pin_thread_lang, Lang};
        pin_thread_lang(Lang::Fr);
        assert_eq!(t!("app-page-count", count = 0), "0 page");
        assert_eq!(t!("app-page-count", count = 1), "1 page");
        assert_eq!(t!("app-page-count", count = 2), "2 pages");
        assert_eq!(t!("app-notebook-count", count = 3), "3 blocs-notes");
        assert_eq!(t!("app-results-count", count = 0), "0 page");
        assert_eq!(t!("app-results-count", count = 4), "4 pages");
        assert_eq!(
            t!("dialog-delete-notebook-title"),
            "Supprimer ce bloc-notes\u{a0}?"
        );
        pin_thread_lang(Lang::En);
        assert_eq!(t!("app-page-count", count = 0), "0 pages");
        assert_eq!(t!("app-page-count", count = 1), "1 page");
        assert_eq!(t!("app-results-count", count = 1), "1 page");
        assert_eq!(t!("app-results-count", count = 0), "0 pages");
        assert_eq!(t!("app-row-words", count = 12), "12w");
    }

    #[test]
    fn create_and_edit_page_end_to_end() {
        let mut ui = super::ui_tests::Ui::start();
        let harness = &mut ui.h;
        harness.step();
        let before = harness.state().workspace.page_count;
        harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
        harness.step();
        harness.step();
        assert_eq!(harness.state().workspace.page_count, before + 1);

        harness.event(egui::Event::Text("A test page".into()));
        harness.step();
        harness.key_press(Key::Enter);
        harness.step();
        harness.event(egui::Event::Text("First paragraph.".into()));
        harness.step();
        harness.key_press(Key::Enter);
        harness.step();
        harness.event(egui::Event::Text("Second paragraph.".into()));
        harness.step();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::B);
        harness.step();
        harness.event(egui::Event::Text(" Bold.".into()));
        harness.step();

        let (page_id, title) = {
            let page = harness.state().page.as_ref().expect("open page");
            assert_eq!(page.title, "A test page");
            let doc = page
                .canvas
                .items
                .iter()
                .find_map(|item| item.doc())
                .expect("canvas text box");
            assert!(doc.blocks.len() >= 2);
            assert!(doc.plain_text().contains("First paragraph."));
            assert!(doc.plain_text().contains("Second paragraph."));
            let bold = doc
                .blocks
                .iter()
                .find(|block| block.text.text.contains("Bold."))
                .expect("bold block");
            let byte = bold.text.text.find("Bold.").expect("bold text");
            assert!(bold.text.style_at(byte).bold, "typed text should be bold");
            (page.id.clone(), page.title.clone())
        };
        harness.state_mut().save_now();
        let saved = harness
            .state()
            .library
            .get_page(&page_id)
            .unwrap()
            .expect("saved page");
        assert_eq!(saved.title, title);

        let blocks_before = harness
            .state()
            .page
            .as_ref()
            .unwrap()
            .canvas
            .items
            .iter()
            .find_map(|item| item.doc())
            .map(|doc| doc.blocks.len())
            .unwrap_or(0);
        harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
        harness.step();
        harness.key_press(Key::Enter);
        harness.step();
        let blocks_after_split = harness
            .state()
            .page
            .as_ref()
            .unwrap()
            .canvas
            .items
            .iter()
            .find_map(|item| item.doc())
            .map(|doc| doc.blocks.len())
            .unwrap_or(0);
        assert_eq!(blocks_after_split, blocks_before + 1);
        harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        harness.step();
        let restored = harness
            .state()
            .page
            .as_ref()
            .unwrap()
            .canvas
            .items
            .iter()
            .find_map(|item| item.doc())
            .unwrap();
        assert!(
            restored.plain_text().contains("Bold."),
            "undo should restore selected text"
        );
    }

    /// A canvas page holding one note per text, each at its y, as the Spelling pane tests
    /// need. Returns the page and each note's id and paragraph id.
    fn spelling_page(notes: &[(&str, f32)]) -> (Page, Vec<(u64, u64)>) {
        let mut page = Page {
            id: "p".into(),
            section_id: String::new(),
            section_name: String::new(),
            section_color: String::new(),
            notebook_id: String::new(),
            notebook_name: String::new(),
            notebook_color: String::new(),
            title: String::new(),
            rule: RuleLines::None,
            color: None,
            rule_color: None,
            title_hidden: false,
            pinned: false,
            trashed: false,
            word_count: 0,
            created_at: 0,
            updated_at: 0,
            canvas: crate::model::CanvasDoc::default(),
            tags: Vec::new(),
            notice: None,
        };
        let ids = notes
            .iter()
            .map(|(text, y)| {
                let block = Block::paragraph(*text);
                let block_id = block.id;
                let item = page.canvas.add(CanvasItem::new(
                    crate::model::CanvasKind::Text {
                        doc: Document {
                            blocks: vec![block],
                        },
                    },
                    48.0,
                    *y,
                    300.0,
                    40.0,
                ));
                (item, block_id)
            })
            .collect();
        (page, ids)
    }

    fn ready_speller() -> crate::spell::Speller {
        let mut speller = crate::spell::Speller::ready("en_US");
        speller.wait_until_ready();
        speller
    }

    fn at(hit: &SpellHit) -> (u64, u64, usize) {
        (hit.item, hit.block, hit.range.start)
    }

    #[test]
    fn the_spelling_walk_reads_the_page_top_note_first() {
        let (page, ids) = spelling_page(&[("teh cat", 100.0), ("a dgo", 20.0)]);
        let (a, b) = (ids[0], ids[1]);
        let mut speller = ready_speller();
        let hits = page_misspellings(&page, &mut speller);
        let found: Vec<(&str, u64, u64)> = hits
            .iter()
            .map(|hit| (hit.word.as_str(), hit.item, hit.block))
            .collect();
        assert_eq!(found, [("dgo", b.0, b.1), ("teh", a.0, a.1)]);
        assert_eq!(hits[0].range, 2..5);
        assert_eq!(hits[1].range, 0..3);

        let none = HashSet::new();
        let first = next_misspelling(&hits, None, &none).expect("a first misspelling");
        assert_eq!(first.word, "dgo");
        let second = next_misspelling(&hits, Some(at(&first)), &none).expect("a second one");
        assert_eq!(second.word, "teh");
        assert_eq!(next_misspelling(&hits, Some(at(&second)), &none), None);

        let skipped = HashSet::from([at(&first)]);
        let first = next_misspelling(&hits, None, &skipped).expect("one not ignored");
        assert_eq!(first.word, "teh");
    }

    #[test]
    fn after_a_change_the_spelling_walk_carries_on_from_the_changed_word() {
        let (mut page, ids) =
            spelling_page(&[("a dgo", 20.0), ("teh cat", 100.0), ("a brwn fox", 180.0)]);
        let mut speller = ready_speller();
        let mut state = CanvasState::default();
        // "dgo" was passed over (F7 moves on without ignoring it), then "teh" changed.
        let (item, block) = ids[1];
        assert!(canvas::replace_in_note(
            &mut page,
            &mut state,
            item,
            block,
            0..3,
            "teh",
            "the"
        ));
        let hits = page_misspellings(&page, &mut speller);
        let words: Vec<&str> = hits.iter().map(|hit| hit.word.as_str()).collect();
        assert_eq!(words, ["dgo", "brwn"]);
        let after = Some((item, block, 3));
        let next = next_misspelling(&hits, walk_position(&page, &hits, after), &HashSet::new());
        assert_eq!(
            next.map(|hit| hit.word),
            Some("brwn".to_string()),
            "the walk goes on past the changed word, not back to the top"
        );
    }

    #[test]
    fn spelling_changes_a_word_in_its_note_as_one_undo_step() {
        let (mut page, ids) = spelling_page(&[("teh cat", 100.0), ("a dgo", 20.0)]);
        let (item, block) = ids[0];
        let mut state = CanvasState::default();
        let text = |page: &Page| {
            page.canvas
                .item(item)
                .and_then(CanvasItem::doc)
                .map(Document::plain_text)
                .unwrap_or_default()
        };
        assert!(canvas::replace_in_note(
            &mut page,
            &mut state,
            item,
            block,
            0..3,
            "teh",
            "the"
        ));
        assert_eq!(text(&page), "the cat");

        // The page's Undo (the ribbon's, nothing being typed in) takes it back.
        let ctx = egui::Context::default();
        let mut editor = EditorState::default();
        let mut library = Library::open_in_memory().unwrap();
        let mut undone = false;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut toast = None;
            undone = canvas::run_ribbon_cmd(
                ui,
                Cmd::Undo,
                &mut page,
                &mut state,
                &mut editor,
                &mut library,
                &mut toast,
            );
        })
        .drop_without_applying_deltas();
        assert!(undone);
        assert_eq!(text(&page), "teh cat");

        // A stale word (the note was edited since) changes nothing.
        assert!(!canvas::replace_in_note(
            &mut page,
            &mut state,
            item,
            block,
            0..3,
            "tah",
            "the"
        ));
        assert!(!canvas::replace_in_note(
            &mut page,
            &mut state,
            item,
            block,
            5..10,
            "teh",
            "the"
        ));
        assert_eq!(text(&page), "teh cat");
    }
}

#[cfg(test)]
mod overlay_shots {
    use super::*;
    use crate::model::{
        Block, CanvasDoc, CanvasKind, Document, InkStroke, Milestone, Rgb, ShapeKind,
    };

    #[test]
    #[ignore]
    fn render_overlays() {
        let mut ui = super::ui_tests::Ui::start();
        let harness = &mut ui.h;
        harness.step();
        harness.step();

        {
            let app = harness.state_mut();
            let page = app.page.as_ref().unwrap();
            app.reader.begin(page);
        }
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-reader-school.png")
            .unwrap();
        harness.state_mut().reader.open = false;
        harness.step();

        harness.state_mut().open_search();
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-search.png")
            .unwrap();

        harness.state_mut().search.open = false;
        harness.state_mut().dialog = Some(Dialog::Settings);
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-settings.png")
            .unwrap();

        harness.state_mut().dialog = Some(Dialog::NewNotebook {
            name: "Studio notes".into(),
            color: SECTION_COLORS[0].into(),
        });
        harness.state_mut().dialog_focus = true;
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-new-notebook.png")
            .unwrap();

        harness.state_mut().dialog = None;
        {
            let app = harness.state_mut();
            let page = app.page.as_mut().unwrap();
            let mut canvas = CanvasDoc::default();
            let notes = Document {
                blocks: vec![
                    Block::paragraph("Field trip on Friday: meet at the gate at 8:30."),
                    Block::paragraph("Pack lunch, a notebook and a pencil."),
                ],
            };
            canvas.add(CanvasItem::new(
                CanvasKind::Text { doc: notes },
                56.0,
                72.0,
                560.0,
                320.0,
            ));
            let reminder = Document {
                blocks: vec![Block::paragraph("Bring the blue folder on Thursday.")],
            };
            canvas.add(CanvasItem::new(
                CanvasKind::Text { doc: reminder },
                680.0,
                84.0,
                240.0,
                150.0,
            ));
            canvas.add(CanvasItem::new(
                CanvasKind::Shape {
                    shape: ShapeKind::RoundRect,
                    stroke: Rgb(0x5b, 0x4b, 0xdb),
                    fill: Some(Rgb(0xe8, 0xe5, 0xfb)),
                    label: "School".into(),
                    flip_x: false,
                    flip_y: false,
                    thickness: None,
                },
                680.0,
                300.0,
                220.0,
                110.0,
            ));
            canvas.add(CanvasItem::new(
                CanvasKind::Timeline {
                    color: Rgb(0x5b, 0x4b, 0xdb),
                    milestones: vec![
                        Milestone {
                            title: "September".into(),
                            date: "Week 1".into(),
                            note: "New class".into(),
                        },
                        Milestone {
                            title: "October".into(),
                            date: "Week 4".into(),
                            note: "First project".into(),
                        },
                        Milestone {
                            title: "December".into(),
                            date: "Week 12".into(),
                            note: "Presentation".into(),
                        },
                    ],
                },
                56.0,
                420.0,
                760.0,
                210.0,
            ));
            canvas.ink.push(InkStroke {
                points: vec![
                    [820.0, 470.0],
                    [870.0, 440.0],
                    [920.0, 485.0],
                    [970.0, 445.0],
                ],
                color: Rgb(0xe8, 0x6a, 0x5a),
                width: 4.0,
                highlighter: false,
            });
            page.canvas = canvas;
            app.canvas.reset(&page.id);
            app.editor.reset();
        }
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-canvas.png")
            .unwrap();

        {
            let app = harness.state_mut();
            let page = app.page.as_ref().unwrap();
            app.reader.begin(page);
        }
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-reader.png")
            .unwrap();

        let app = harness.state();
        if let Some(page) = app.page.as_ref() {
            let html = export::canvas_print_html(page, &app.library);
            std::fs::write("/tmp/folio-print.html", html).unwrap();
        }

        harness.state_mut().reader.open = false;
        harness.state_mut().ribbon.tab = crate::ribbon::RibbonTab::Insert;
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-insert-tab.png")
            .unwrap();
        harness.state_mut().ribbon.tab = crate::ribbon::RibbonTab::Draw;
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-draw-tab.png")
            .unwrap();
        harness.state_mut().ribbon.tab = crate::ribbon::RibbonTab::View;
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-view-tab.png")
            .unwrap();

        {
            let app = harness.state_mut();
            let id = app
                .page
                .as_ref()
                .unwrap()
                .canvas
                .items
                .iter()
                .find(|item| item.doc().is_some())
                .map(|item| item.id);
            app.ribbon.tab = crate::ribbon::RibbonTab::Home;
            app.canvas.tool = canvas::CanvasTool::Select;
            app.canvas.active_text = id;
            app.canvas.selection = id.map(canvas::Selection::one).unwrap_or_default();
            app.canvas.scene_rect = None;
        }
        harness.step();
        harness.step();
        harness
            .render()
            .unwrap()
            .save("/tmp/folio-canvas-editing.png")
            .unwrap();
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod ui_tests;
