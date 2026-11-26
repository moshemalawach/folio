use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::i18n::t;

static NEXT_BLOCK_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_block_id() -> u64 {
    NEXT_BLOCK_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn seed_block_ids(min: u64) {
    let _ = NEXT_BLOCK_ID.try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.max(min))
    });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub id: String,
    pub notebook_id: String,
    pub name: String,
    pub color: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// The section group it sits in, or `None` at the notebook's top level.
    #[serde(default)]
    pub group_id: Option<String>,
}

/// A section waiting in the Notebook Recycle Bin, as the bin lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrashedSection {
    pub id: String,
    pub notebook_id: String,
    pub notebook_name: String,
    pub name: String,
    pub color: String,
    /// The pages deleted with it (not those already in the bin on their own).
    pub page_count: i64,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionGroup {
    pub id: String,
    pub notebook_id: String,
    /// The group it is nested in, or `None` at the notebook's top level.
    pub parent_id: Option<String>,
    pub name: String,
    pub color: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notebook {
    pub id: String,
    pub name: String,
    pub color: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub sections: Vec<Section>,
    #[serde(default)]
    pub groups: Vec<SectionGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub notebooks: Vec<Notebook>,
    pub tags: Vec<Tag>,
    pub quick_section_id: Option<String>,
    pub page_count: i64,
    pub trash_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSummary {
    pub id: String,
    pub section_id: String,
    pub section_name: String,
    pub section_color: String,
    pub notebook_id: String,
    pub notebook_name: String,
    pub notebook_color: String,
    pub title: String,
    pub excerpt: String,
    pub pinned: bool,
    pub trashed: bool,
    pub word_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<Tag>,
    /// 0 = page, 1 = subpage, 2 = sub-subpage (section views only).
    #[serde(default)]
    pub level: u8,
    /// Its subpages are folded away in the page list.
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub position: i64,
}

/// Where a page's own parts sit, in page units at 100 %, as the desktop notebook lays a page out: the
/// title starts 48 in and 40 down, and notes start below the date. The page has no area
/// above or left of (0, 0).
pub const PAGE_LEFT: f32 = 48.0;
pub const TITLE_TOP: f32 = 40.0;
pub const PAGE_CONTENT_TOP: f32 = 136.0;
/// The highest an item may sit: its grab bar (14 px, 2 px above it) is drawn above its top
/// and must stay on the page, where it can be grabbed. Ink has no bar and may reach y = 0.
pub const ITEM_TOP: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Rect,
    RoundRect,
    Ellipse,
    Triangle,
    Diamond,
    Line,
    Arrow,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 7] = [
        ShapeKind::Rect,
        ShapeKind::RoundRect,
        ShapeKind::Ellipse,
        ShapeKind::Triangle,
        ShapeKind::Diamond,
        ShapeKind::Line,
        ShapeKind::Arrow,
    ];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            ShapeKind::Rect => t!("shape-rectangle"),
            ShapeKind::RoundRect => t!("shape-rounded-rectangle"),
            ShapeKind::Ellipse => t!("shape-ellipse"),
            ShapeKind::Triangle => t!("shape-triangle"),
            ShapeKind::Diamond => t!("shape-diamond"),
            ShapeKind::Line => t!("shape-line"),
            ShapeKind::Arrow => t!("shape-arrow"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkStroke {
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    #[serde(default = "default_ink")]
    pub color: Rgb,
    #[serde(default = "default_ink_width")]
    pub width: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub highlighter: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub note: String,
}

/// A page of a file printout (Insert › File Printout): which page of which PDF the
/// picture shows, and that page's text, so search finds it and "Copy text" gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintoutPage {
    /// The attachment holding the PDF itself (shown above the pages).
    pub source: String,
    /// 1-based.
    pub page: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// The size it was inserted at, in page units: the page's printed size (a poster
    /// scaled down), which Restore Original Size gives back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanvasKind {
    /// A note container. Old sticky notes (`"type":"sticky"`) load as one, keeping their
    /// text, position and size; their colour is dropped, as the desktop notebook has no coloured
    /// containers.
    #[serde(alias = "sticky")]
    Text { doc: Document },
    Image {
        attachment: String,
        #[serde(default, skip_serializing_if = "PictureMeta::is_plain")]
        meta: PictureMeta,
        /// Set on the pages of a file printout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        printout: Option<Box<PrintoutPage>>,
    },
    /// Any other file, kept in the notebook and opened with the system's own app.
    File {
        attachment: String,
        /// The original file name, shown on the card and used when opening or saving it.
        #[serde(default)]
        name: String,
        #[serde(default)]
        size: u64,
    },
    /// A sound recording, played on the page.
    Audio {
        attachment: String,
        /// Its length; 0 while it is being recorded (the file's header has the truth).
        #[serde(default)]
        duration_ms: u64,
    },
    Shape {
        shape: ShapeKind,
        #[serde(default = "default_ink")]
        stroke: Rgb,
        #[serde(default)]
        fill: Option<Rgb>,
        #[serde(default)]
        label: String,
        #[serde(default)]
        flip_x: bool,
        #[serde(default)]
        flip_y: bool,
        /// The outline's width in page points. `None` is the shape tool's thin line;
        /// Ink to Shape keeps the width of the pen that drew it, as the desktop notebook does.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thickness: Option<f32>,
    },
    Timeline {
        #[serde(default = "default_frise")]
        color: Rgb,
        #[serde(default)]
        milestones: Vec<Milestone>,
    },
    /// Only read from old pages: `CanvasDoc::repair` turns it into a note holding a table.
    Table {
        #[serde(default)]
        rows: Vec<Vec<String>>,
        #[serde(default)]
        header: bool,
    },
    Equation {
        #[serde(default)]
        source: String,
        /// Font size in page units; dragging the corner changes it.
        #[serde(default = "default_equation_size")]
        size: f32,
    },
}

fn default_ink() -> Rgb {
    Rgb::INKS[0]
}

/// The width of a shape's outline when it has no `thickness` of its own.
pub const SHAPE_LINE: f32 = 1.5;

fn default_ink_width() -> f32 {
    3.0
}

fn default_frise() -> Rgb {
    Rgb(0x5b, 0x4b, 0xdb)
}

pub fn default_equation_size() -> f32 {
    22.0
}

impl CanvasKind {
    /// A picture on the page, as inserted: no alt text, upright.
    pub fn image(attachment: impl Into<String>) -> Self {
        CanvasKind::Image {
            attachment: attachment.into(),
            meta: PictureMeta::default(),
            printout: None,
        }
    }

    /// The printout page a picture shows, if it is one.
    pub fn printout(&self) -> Option<&PrintoutPage> {
        match self {
            CanvasKind::Image {
                printout: Some(printout),
                ..
            } => Some(printout),
            _ => None,
        }
    }

    pub fn printout_mut(&mut self) -> Option<&mut PrintoutPage> {
        match self {
            CanvasKind::Image {
                printout: Some(printout),
                ..
            } => Some(printout),
            _ => None,
        }
    }

    pub fn label(&self) -> Cow<'static, str> {
        match self {
            CanvasKind::Text { .. } => t!("item-text"),
            CanvasKind::Image { .. } => t!("item-picture"),
            CanvasKind::File { .. } => t!("item-file"),
            CanvasKind::Audio { .. } => t!("item-recording"),
            CanvasKind::Shape { .. } => t!("item-shape"),
            CanvasKind::Timeline { .. } => t!("item-timeline"),
            CanvasKind::Table { .. } => t!("item-table"),
            CanvasKind::Equation { .. } => t!("item-equation"),
        }
    }

    pub fn plain_text(&self) -> String {
        match self {
            CanvasKind::Text { doc } => doc.plain_text(),
            CanvasKind::Shape { label, .. } => label.clone(),
            CanvasKind::Timeline { milestones, .. } => milestones
                .iter()
                .map(|milestone| {
                    format!("{} {} {}", milestone.title, milestone.date, milestone.note)
                })
                .collect::<Vec<_>>()
                .join(
                    "
",
                ),
            CanvasKind::Table { rows, .. } => {
                rows.iter().flatten().cloned().collect::<Vec<_>>().join(" ")
            }
            CanvasKind::File { name, .. } => name.clone(),
            CanvasKind::Equation { source, .. } => source.clone(),
            CanvasKind::Image {
                printout: Some(printout),
                ..
            } => printout.text.clone(),
            CanvasKind::Image { .. } | CanvasKind::Audio { .. } => String::new(),
        }
    }

    /// The attachment a picture, file or recording shows.
    pub fn attachment(&self) -> Option<&str> {
        match self {
            CanvasKind::Image { attachment, .. }
            | CanvasKind::File { attachment, .. }
            | CanvasKind::Audio { attachment, .. } => Some(attachment),
            _ => None,
        }
    }

    pub fn attachment_mut(&mut self) -> Option<&mut String> {
        match self {
            CanvasKind::Image { attachment, .. }
            | CanvasKind::File { attachment, .. }
            | CanvasKind::Audio { attachment, .. } => Some(attachment),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasItem {
    #[serde(default = "next_block_id")]
    pub id: u64,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    #[serde(default)]
    pub z: i32,
    /// A note that sizes itself to its text, as the desktop notebook's new containers do, until its width
    /// handle is dragged. Notes saved before this field keep the width they had.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_width: bool,
    pub kind: CanvasKind,
}

impl CanvasItem {
    pub fn new(kind: CanvasKind, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            id: next_block_id(),
            x,
            y,
            width,
            height,
            z: 0,
            auto_width: false,
            kind,
        }
    }

    /// A picture set as the page's background: drawn first, never picked.
    pub fn is_background(&self) -> bool {
        matches!(&self.kind, CanvasKind::Image { meta, .. } if meta.background)
    }

    pub fn rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(self.x, self.y),
            egui::vec2(self.width, self.height),
        )
    }

    /// Lines and arrows are drawn corner to corner, so one side of their box may be zero.
    pub fn is_connector(&self) -> bool {
        matches!(
            self.kind,
            CanvasKind::Shape {
                shape: ShapeKind::Line | ShapeKind::Arrow,
                ..
            }
        )
    }

    pub fn doc(&self) -> Option<&Document> {
        match &self.kind {
            CanvasKind::Text { doc } => Some(doc),
            _ => None,
        }
    }

    pub fn doc_mut(&mut self) -> Option<&mut Document> {
        match &mut self.kind {
            CanvasKind::Text { doc } => Some(doc),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasDoc {
    #[serde(default)]
    pub items: Vec<CanvasItem>,
    #[serde(default)]
    pub ink: Vec<InkStroke>,
}

impl CanvasDoc {
    pub fn add(&mut self, mut item: CanvasItem) -> u64 {
        let z = self.items.iter().map(|item| item.z).max().unwrap_or(0) + 1;
        item.z = z;
        // The page starts at (0, 0): nothing is added above or left of it, and an item
        // keeps room above it for its grab bar.
        item.x = item.x.max(0.0);
        item.y = item.y.max(ITEM_TOP);
        let id = item.id;
        self.items.push(item);
        id
    }

    pub fn item(&self, id: u64) -> Option<&CanvasItem> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn item_mut(&mut self, id: u64) -> Option<&mut CanvasItem> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    pub fn remove(&mut self, id: u64) -> Option<CanvasItem> {
        let index = self.items.iter().position(|item| item.id == id)?;
        Some(self.items.remove(index))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Parses a stored canvas. Items or strokes that cannot be read (for example a kind
    /// written by a newer build) are skipped instead of discarding the whole page; the
    /// second value is `true` when anything was skipped.
    pub fn from_json_lossy(json: &str) -> (Self, bool) {
        if json.trim().is_empty() {
            return (Self::default(), false);
        }
        if let Ok(doc) = serde_json::from_str::<CanvasDoc>(json) {
            return (doc, false);
        }
        let Ok(serde_json::Value::Object(mut value)) = serde_json::from_str(json) else {
            return (Self::default(), true);
        };
        let items = parse_each(value.remove("items"));
        let ink = parse_each(value.remove("ink"));
        (Self { items, ink }, true)
    }

    pub fn plain_text(&self) -> String {
        let mut items: Vec<&CanvasItem> = self.items.iter().collect();
        // Reading order: top to bottom, then left to right.
        items.sort_by_key(|item| (item.y as i32, item.x as i32));
        items
            .into_iter()
            .map(|item| item.kind.plain_text())
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Gives recording `attachment` its length wherever it is on the page: a recording item
    /// or a recording in a note. Returns whether the page holds it.
    pub fn set_clip_length(&mut self, attachment: &str, length_ms: u64) -> bool {
        let mut found = false;
        for item in &mut self.items {
            match &mut item.kind {
                CanvasKind::Audio {
                    attachment: clip,
                    duration_ms,
                } if clip == attachment => {
                    *duration_ms = length_ms;
                    found = true;
                }
                CanvasKind::Text { doc } => found |= doc.set_clip_length(attachment, length_ms),
                _ => {}
            }
        }
        found
    }

    pub fn repair(&mut self) {
        for item in &mut self.items {
            // Tables used to be page items; in the desktop notebook (and Folio now) they live in a
            // note. An old one becomes a note at the same place holding the same cells.
            if let CanvasKind::Table { rows, header } = &item.kind {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
                let width = ((item.width - 12.0) / cols as f32)
                    .round()
                    .max(TABLE_MIN_COLUMN);
                let table = TableBlock::from_strings(rows, *header, width);
                item.kind = CanvasKind::Text {
                    doc: Document {
                        blocks: vec![Block::new(BlockKind::Table(table))],
                    },
                };
                // Its width is the table's: the note must not shrink to its (empty) text.
                item.auto_width = false;
            }
            if let Some(doc) = item.doc_mut() {
                doc.repair();
            }
            if let CanvasKind::Equation { size, .. } = &mut item.kind {
                *size = finite_or(*size, default_equation_size()).clamp(8.0, 160.0);
            }
            if !item.x.is_finite() {
                item.x = 0.0;
            }
            if !item.y.is_finite() {
                item.y = 0.0;
            }
            item.x = item.x.max(0.0);
            item.y = item.y.max(ITEM_TOP);
            // The same lower limits the canvas allows when drawing and resizing.
            let (min_width, min_height) = if item.is_connector() {
                (0.0, 0.0)
            } else if item.doc().is_some() {
                (80.0, 24.0)
            } else {
                (24.0, 24.0)
            };
            item.width = finite_or(item.width, min_width).clamp(min_width, 2400.0);
            item.height = finite_or(item.height, min_height).max(min_height);
            if let CanvasKind::Shape {
                thickness: Some(thickness),
                ..
            } = &mut item.kind
            {
                *thickness = finite_or(*thickness, SHAPE_LINE).clamp(0.5, 48.0);
            }
        }
        self.ink.retain(|stroke| !stroke.points.is_empty());
        // A stroke that strayed above or left of the page comes back whole, keeping its shape.
        for stroke in &mut self.ink {
            let mut low = [0.0f32; 2];
            for point in &stroke.points {
                for axis in 0..2 {
                    if point[axis].is_finite() {
                        low[axis] = low[axis].min(point[axis]);
                    }
                }
            }
            if low != [0.0, 0.0] {
                for point in &mut stroke.points {
                    point[0] -= low[0];
                    point[1] -= low[1];
                }
            }
        }
        let max = self.items.iter().map(|item| item.id).max().unwrap_or(0);
        seed_block_ids(max.saturating_add(1));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub section_id: String,
    pub section_name: String,
    pub section_color: String,
    pub notebook_id: String,
    pub notebook_name: String,
    pub notebook_color: String,
    pub title: String,
    /// Rule lines or a grid behind the notes (View ▸ Rule Lines).
    pub rule: RuleLines,
    /// The page colour (View ▸ Page Color); `None` is the desktop notebook's plain white.
    pub color: Option<PageColor>,
    /// View ▸ Rule Lines ▸ Rule Line Color; `None` is the desktop notebook's blue (Automatic).
    pub rule_color: Option<Rgb>,
    /// View ▸ Hide Page Title: the title, date and time are not shown.
    pub title_hidden: bool,
    pub pinned: bool,
    pub trashed: bool,
    pub word_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub canvas: CanvasDoc,
    pub tags: Vec<Tag>,
    /// A one-off message from loading, e.g. that damaged JSON was set aside.
    #[serde(skip)]
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub page_id: String,
    pub file_name: String,
    pub stored_path: String,
    pub mime: String,
    pub byte_size: u64,
    pub created_at: i64,
}

impl Page {
    pub fn plain_text(&self) -> String {
        self.canvas.plain_text()
    }

    /// The words of `plain_text`, counted item by item without building it: it joins the
    /// items' text with line breaks, so no word spans two items. A long printout's text
    /// is counted where it is, never copied.
    pub fn word_count(&self) -> i64 {
        let words = |text: &str| text.split_whitespace().count();
        self.canvas
            .items
            .iter()
            .map(|item| match item.kind.printout() {
                Some(printout) => words(&printout.text),
                None => words(&item.kind.plain_text()),
            })
            .sum::<usize>() as i64
    }

    pub fn tagged_lines(&self) -> Vec<TaggedLine> {
        let page = TaggedPage {
            id: &self.id,
            title: &self.title,
            section_id: &self.section_id,
            section_name: &self.section_name,
            section_color: &self.section_color,
            notebook_id: &self.notebook_id,
            notebook_name: &self.notebook_name,
            updated_at: self.updated_at,
        };
        tagged_lines(&page, &self.canvas)
    }
}

/// What a Find Tags entry is: a to-do, a paragraph tag, or a custom tag (its look is in
/// `TaggedLine::custom`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagMark {
    Todo,
    Line(LineTag),
    Custom,
}

impl TagMark {
    pub fn rank(self) -> usize {
        match self {
            TagMark::Todo => 0,
            TagMark::Line(tag) => 1 + tag.rank(),
            TagMark::Custom => 1000,
        }
    }

    pub fn label(self) -> Cow<'static, str> {
        match self {
            TagMark::Todo => t!("tag-todo"),
            TagMark::Line(tag) => tag.label(),
            TagMark::Custom => t!("tag-custom"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            TagMark::Todo => crate::icons::CHECK_SQUARE,
            TagMark::Line(tag) => tag.icon(),
            TagMark::Custom => crate::icons::TAG,
        }
    }

    /// `None` for To Do, which uses the theme accent.
    pub fn color(self) -> Option<Rgb> {
        match self {
            TagMark::Todo | TagMark::Custom => None,
            TagMark::Line(tag) => Some(tag.color()),
        }
    }
}

/// A tagged paragraph, as listed by Find Tags.
#[derive(Debug, Clone, PartialEq)]
pub struct TaggedLine {
    pub page_id: String,
    pub page_title: String,
    pub section_id: String,
    pub section_name: String,
    pub section_color: String,
    pub notebook_id: String,
    pub notebook_name: String,
    /// The note holding the paragraph.
    pub item: u64,
    pub block: u64,
    pub mark: TagMark,
    pub text: String,
    pub checked: bool,
    /// Reading-order position on its page, for a stable sort.
    pub order: usize,
    /// For `TagMark::Custom`: the tag as the paragraph keeps it.
    pub custom: Option<CustomTag>,
    /// When the line's page was last changed (Group by Date).
    pub updated_at: i64,
}

impl TaggedLine {
    /// The tag's name: a custom tag's own, else its mark's.
    pub fn tag_label(&self) -> Cow<'_, str> {
        match &self.custom {
            Some(tag) => Cow::Borrowed(tag.name.as_str()),
            None => self.mark.label(),
        }
    }

    pub fn tag_icon(&self) -> &'static str {
        match &self.custom {
            Some(tag) => tag.symbol.icon().unwrap_or(crate::icons::TAG),
            None => self.mark.icon(),
        }
    }

    /// `None` means the theme accent.
    pub fn tag_color(&self) -> Option<Rgb> {
        match &self.custom {
            Some(tag) => tag.symbol.color(),
            None => self.mark.color(),
        }
    }
}

/// The page a set of tagged lines comes from.
pub struct TaggedPage<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub section_id: &'a str,
    pub section_name: &'a str,
    pub section_color: &'a str,
    pub notebook_id: &'a str,
    pub notebook_name: &'a str,
    pub updated_at: i64,
}

/// Every tagged paragraph and to-do on a page, in reading order. Empty lines are skipped.
pub fn tagged_lines(page: &TaggedPage<'_>, canvas: &CanvasDoc) -> Vec<TaggedLine> {
    let mut items: Vec<&CanvasItem> = canvas
        .items
        .iter()
        .filter(|item| item.doc().is_some())
        .collect();
    items.sort_by_key(|item| (item.y as i32, item.x as i32));
    let docs: Vec<(u64, &Document)> = items
        .into_iter()
        .filter_map(|item| item.doc().map(|doc| (item.id, doc)))
        .collect();
    let title = display_title(page.title);
    let mut out = Vec::new();
    for (item, doc) in docs {
        for block in &doc.blocks {
            let text = block
                .text
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() {
                continue;
            }
            let text = if text.chars().count() > 160 {
                format!("{}…", text.chars().take(159).collect::<String>())
            } else {
                text
            };
            let todo = matches!(block.kind, BlockKind::Todo { .. })
                .then(|| (TagMark::Todo, block.checked(), None));
            let lines = block.tags.iter().map(|tag| {
                let checked = tag.checkable() && block.checked_tags.contains(tag);
                (TagMark::Line(*tag), checked, None)
            });
            let custom = block.custom_tags.iter().map(|tag| {
                let checked = tag.checked && tag.symbol.checkable();
                (TagMark::Custom, checked, Some(tag.clone()))
            });
            for (mark, checked, custom) in todo.into_iter().chain(lines).chain(custom) {
                out.push(TaggedLine {
                    page_id: page.id.to_string(),
                    page_title: title.to_string(),
                    section_id: page.section_id.to_string(),
                    section_name: page.section_name.to_string(),
                    section_color: page.section_color.to_string(),
                    notebook_id: page.notebook_id.to_string(),
                    notebook_name: page.notebook_name.to_string(),
                    item,
                    block: block.id,
                    mark,
                    text: text.clone(),
                    checked,
                    order: out.len(),
                    custom,
                    updated_at: page.updated_at,
                });
            }
        }
    }
    out
}

/// A copy of a page kept before later edits replaced it (the desktop notebook's page versions).
#[derive(Debug, Clone)]
pub struct PageVersion {
    pub id: i64,
    pub title: String,
    pub plain_text: String,
    pub word_count: i64,
    /// When the page last looked like this.
    pub saved_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page_id: String,
    pub title: String,
    pub excerpt: String,
    pub section_id: String,
    pub section_name: String,
    pub section_color: String,
    pub notebook_id: String,
    pub notebook_name: String,
    pub updated_at: i64,
}

/// the desktop notebook's View ▸ Rule Lines: ruled lines in four spacings, or a grid in four sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleLines {
    #[default]
    None,
    Lines(RuleSize),
    Grid(GridSize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSize {
    Narrow,
    College,
    Standard,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridSize {
    Small,
    Medium,
    Large,
    VeryLarge,
}

impl RuleLines {
    /// The ruled styles, narrowest first, as the Rule Lines menu shows them.
    pub const LINES: [RuleLines; 4] = [
        RuleLines::Lines(RuleSize::Narrow),
        RuleLines::Lines(RuleSize::College),
        RuleLines::Lines(RuleSize::Standard),
        RuleLines::Lines(RuleSize::Wide),
    ];
    pub const GRIDS: [RuleLines; 4] = [
        RuleLines::Grid(GridSize::Small),
        RuleLines::Grid(GridSize::Medium),
        RuleLines::Grid(GridSize::Large),
        RuleLines::Grid(GridSize::VeryLarge),
    ];

    pub fn code(self) -> &'static str {
        match self {
            RuleLines::None => "none",
            RuleLines::Lines(RuleSize::Narrow) => "lines-narrow",
            RuleLines::Lines(RuleSize::College) => "lines-college",
            RuleLines::Lines(RuleSize::Standard) => "lines-standard",
            RuleLines::Lines(RuleSize::Wide) => "lines-wide",
            RuleLines::Grid(GridSize::Small) => "grid-small",
            RuleLines::Grid(GridSize::Medium) => "grid-medium",
            RuleLines::Grid(GridSize::Large) => "grid-large",
            RuleLines::Grid(GridSize::VeryLarge) => "grid-very-large",
        }
    }

    /// Reads a stored code. Earlier paper styles map onto rule lines: ruled paper is
    /// college ruled, grid paper a medium grid, and blank or dotted paper has none.
    pub fn from_code(code: &str) -> Self {
        match code {
            "lines-narrow" => RuleLines::Lines(RuleSize::Narrow),
            "lines-college" | "ruled" => RuleLines::Lines(RuleSize::College),
            "lines-standard" => RuleLines::Lines(RuleSize::Standard),
            "lines-wide" => RuleLines::Lines(RuleSize::Wide),
            "grid-small" => RuleLines::Grid(GridSize::Small),
            "grid-medium" | "grid" => RuleLines::Grid(GridSize::Medium),
            "grid-large" => RuleLines::Grid(GridSize::Large),
            "grid-very-large" => RuleLines::Grid(GridSize::VeryLarge),
            _ => RuleLines::None,
        }
    }

    /// The distance between lines in page units at 100 % (1/32 in steps, as the desktop notebook's
    /// spacings are), `None` without lines.
    pub fn spacing(self) -> Option<f32> {
        match self {
            RuleLines::None => None,
            RuleLines::Lines(RuleSize::Narrow) => Some(21.0),
            RuleLines::Lines(RuleSize::College) => Some(27.0),
            RuleLines::Lines(RuleSize::Standard) => Some(33.0),
            RuleLines::Lines(RuleSize::Wide) => Some(40.0),
            RuleLines::Grid(GridSize::Small) => Some(12.0),
            RuleLines::Grid(GridSize::Medium) => Some(24.0),
            RuleLines::Grid(GridSize::Large) => Some(36.0),
            RuleLines::Grid(GridSize::VeryLarge) => Some(48.0),
        }
    }

    pub fn label(self) -> Cow<'static, str> {
        match self {
            RuleLines::None => t!("rule-none"),
            RuleLines::Lines(RuleSize::Narrow) => t!("rule-lines-narrow"),
            RuleLines::Lines(RuleSize::College) => t!("rule-lines-college"),
            RuleLines::Lines(RuleSize::Standard) => t!("rule-lines-standard"),
            RuleLines::Lines(RuleSize::Wide) => t!("rule-lines-wide"),
            RuleLines::Grid(GridSize::Small) => t!("rule-grid-small"),
            RuleLines::Grid(GridSize::Medium) => t!("rule-grid-medium"),
            RuleLines::Grid(GridSize::Large) => t!("rule-grid-large"),
            RuleLines::Grid(GridSize::VeryLarge) => t!("rule-grid-very-large"),
        }
    }
}

/// the desktop notebook's sixteen page colours (View ▸ Page Color): pale enough that text and ink stay
/// easy to read. `None` on a page is the desktop notebook's plain white.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageColor {
    Blue,
    Teal,
    Green,
    Red,
    Periwinkle,
    Cyan,
    Apple,
    Rose,
    Lavender,
    Silver,
    LemonLime,
    Tan,
    PurpleMist,
    Magenta,
    Yellow,
    Orange,
    /// Page Color › More Colors…: any colour, the same in a light theme.
    Custom(Rgb),
}

impl PageColor {
    /// In the 4 × 4 order the menu shows, row by row.
    pub const ALL: [PageColor; 16] = [
        PageColor::Blue,
        PageColor::Teal,
        PageColor::Green,
        PageColor::Red,
        PageColor::Periwinkle,
        PageColor::Cyan,
        PageColor::Apple,
        PageColor::Rose,
        PageColor::Lavender,
        PageColor::Silver,
        PageColor::LemonLime,
        PageColor::Tan,
        PageColor::PurpleMist,
        PageColor::Magenta,
        PageColor::Yellow,
        PageColor::Orange,
    ];

    pub fn code(self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            PageColor::Custom(rgb) => return Cow::Owned(rgb.hex()),
            PageColor::Blue => "blue",
            PageColor::Teal => "teal",
            PageColor::Green => "green",
            PageColor::Red => "red",
            PageColor::Periwinkle => "periwinkle",
            PageColor::Cyan => "cyan",
            PageColor::Apple => "apple",
            PageColor::Rose => "rose",
            PageColor::Lavender => "lavender",
            PageColor::Silver => "silver",
            PageColor::LemonLime => "lemon_lime",
            PageColor::Tan => "tan",
            PageColor::PurpleMist => "purple_mist",
            PageColor::Magenta => "magenta",
            PageColor::Yellow => "yellow",
            PageColor::Orange => "orange",
        })
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|color| color.code() == code)
            .or_else(|| Rgb::from_hex(code).map(PageColor::Custom))
    }

    /// The page fill in a light theme (measured from the desktop notebook's Page Color menu).
    pub fn light(self) -> Rgb {
        match self {
            PageColor::Blue => Rgb(0xe8, 0xef, 0xfa),
            PageColor::Teal => Rgb(0xe9, 0xf5, 0xf2),
            PageColor::Green => Rgb(0xea, 0xf5, 0xe9),
            PageColor::Red => Rgb(0xfa, 0xe9, 0xe9),
            PageColor::Periwinkle => Rgb(0xea, 0xed, 0xf8),
            PageColor::Cyan => Rgb(0xed, 0xf6, 0xf8),
            PageColor::Apple => Rgb(0xee, 0xf7, 0xea),
            PageColor::Rose => Rgb(0xfc, 0xf1, 0xf3),
            PageColor::Lavender => Rgb(0xef, 0xed, 0xf8),
            PageColor::Silver => Rgb(0xf4, 0xf4, 0xf4),
            PageColor::LemonLime => Rgb(0xf2, 0xf6, 0xe7),
            PageColor::Tan => Rgb(0xf5, 0xee, 0xe7),
            PageColor::PurpleMist => Rgb(0xf5, 0xee, 0xf7),
            PageColor::Magenta => Rgb(0xfb, 0xee, 0xf7),
            PageColor::Yellow => Rgb(0xfe, 0xf7, 0xe3),
            PageColor::Orange => Rgb(0xfc, 0xf3, 0xe8),
            PageColor::Custom(rgb) => rgb,
        }
    }

    /// The page fill in a dark theme: the same hue, deep enough for light text.
    pub fn dark(self) -> Rgb {
        match self {
            PageColor::Blue => Rgb(0x1e, 0x27, 0x34),
            PageColor::Teal => Rgb(0x1c, 0x2b, 0x28),
            PageColor::Green => Rgb(0x1e, 0x2a, 0x1e),
            PageColor::Red => Rgb(0x32, 0x1f, 0x20),
            PageColor::Periwinkle => Rgb(0x22, 0x24, 0x3a),
            PageColor::Cyan => Rgb(0x1b, 0x2b, 0x30),
            PageColor::Apple => Rgb(0x22, 0x2d, 0x1e),
            PageColor::Rose => Rgb(0x33, 0x23, 0x2a),
            PageColor::Lavender => Rgb(0x26, 0x23, 0x38),
            PageColor::Silver => Rgb(0x2a, 0x2a, 0x2a),
            PageColor::LemonLime => Rgb(0x28, 0x2d, 0x1c),
            PageColor::Tan => Rgb(0x2f, 0x28, 0x20),
            PageColor::PurpleMist => Rgb(0x2d, 0x23, 0x33),
            PageColor::Magenta => Rgb(0x33, 0x21, 0x2e),
            PageColor::Yellow => Rgb(0x32, 0x2d, 0x1a),
            PageColor::Orange => Rgb(0x33, 0x28, 0x1c),
            // As deep as the sixteen: their hue, a fifth of the brightness.
            PageColor::Custom(rgb) => {
                use egui::ecolor::HsvaGamma;
                let hsva = HsvaGamma::from(egui::Rgba::from(rgb.to_color32()));
                let deep = HsvaGamma {
                    h: hsva.h,
                    s: (hsva.s * 5.0).min(0.5),
                    v: 0.2,
                    a: 1.0,
                };
                let [r, g, b, _] = egui::Color32::from(deep).to_array();
                Rgb(r, g, b)
            }
        }
    }

    pub fn label(self) -> Cow<'static, str> {
        match self {
            PageColor::Blue => t!("page-color-blue"),
            PageColor::Teal => t!("page-color-teal"),
            PageColor::Green => t!("page-color-green"),
            PageColor::Red => t!("page-color-red"),
            PageColor::Periwinkle => t!("page-color-periwinkle"),
            PageColor::Cyan => t!("page-color-cyan"),
            PageColor::Apple => t!("page-color-apple"),
            PageColor::Rose => t!("page-color-rose"),
            PageColor::Lavender => t!("page-color-lavender"),
            PageColor::Silver => t!("page-color-silver"),
            PageColor::LemonLime => t!("page-color-lemon-lime"),
            PageColor::Tan => t!("page-color-tan"),
            PageColor::PurpleMist => t!("page-color-purple-mist"),
            PageColor::Magenta => t!("page-color-magenta"),
            PageColor::Yellow => t!("page-color-yellow"),
            PageColor::Orange => t!("page-color-orange"),
            PageColor::Custom(rgb) => Cow::Owned(rgb.hex()),
        }
    }
}

/// How a page looks behind its notes. It is stored as one code in the page row's `paper`
/// column (`lines-college;color=blue;notitle`), so versions, copies and restores carry it
/// whole. An old paper code (`ruled`, `dots`, …) reads as its rule lines alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageStyle {
    pub rule: RuleLines,
    pub color: Option<PageColor>,
    pub rule_color: Option<Rgb>,
    pub title_hidden: bool,
}

impl PageStyle {
    pub fn of(page: &Page) -> Self {
        Self {
            rule: page.rule,
            color: page.color,
            rule_color: page.rule_color,
            title_hidden: page.title_hidden,
        }
    }

    pub fn code(self) -> String {
        let mut code = self.rule.code().to_string();
        if let Some(color) = self.color {
            code.push_str(";color=");
            code.push_str(&color.code());
        }
        if let Some(rule_color) = self.rule_color {
            code.push_str(";rules=");
            code.push_str(&rule_color.hex());
        }
        if self.title_hidden {
            code.push_str(";notitle");
        }
        code
    }

    pub fn from_code(code: &str) -> Self {
        let mut parts = code.split(';');
        let mut style = Self {
            rule: RuleLines::from_code(parts.next().unwrap_or_default()),
            ..Self::default()
        };
        for part in parts {
            if part == "notitle" {
                style.title_hidden = true;
            } else if let Some(color) = part.strip_prefix("color=") {
                style.color = PageColor::from_code(color);
            } else if let Some(color) = part.strip_prefix("rules=") {
                style.rule_color = Rgb::from_hex(color);
            }
        }
        style
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const HIGHLIGHTERS: [Rgb; 6] = [
        Rgb(0xff, 0xe8, 0x7a),
        Rgb(0xff, 0xc9, 0xd7),
        Rgb(0xbf, 0xe9, 0xd2),
        Rgb(0xc9, 0xdd, 0xff),
        Rgb(0xe3, 0xd2, 0xff),
        Rgb(0xff, 0xdc, 0xb8),
    ];

    pub const INKS: [Rgb; 7] = [
        Rgb(0x10, 0x10, 0x14),
        Rgb(0x4c, 0x4a, 0x5a),
        Rgb(0x5b, 0x4b, 0xdb),
        Rgb(0xb8, 0x3a, 0x24),
        Rgb(0x0a, 0x75, 0x63),
        Rgb(0x8a, 0x58, 0x00),
        Rgb(0x1a, 0x5b, 0xc4),
    ];

    /// Text Highlight Color, as Office lists it: five to a row.
    pub const TEXT_HIGHLIGHTS: [(Rgb, &'static str); 15] = [
        (Rgb(0xff, 0xff, 0x00), "color-yellow"),
        (Rgb(0x00, 0xff, 0x00), "color-bright-green"),
        (Rgb(0x00, 0xff, 0xff), "color-turquoise"),
        (Rgb(0xff, 0x00, 0xff), "color-pink"),
        (Rgb(0x00, 0x00, 0xff), "color-blue"),
        (Rgb(0xff, 0x00, 0x00), "color-red"),
        (Rgb(0x00, 0x00, 0x80), "color-dark-blue"),
        (Rgb(0x00, 0x80, 0x80), "color-teal"),
        (Rgb(0x00, 0x80, 0x00), "color-green"),
        (Rgb(0x80, 0x00, 0x80), "color-violet"),
        (Rgb(0x80, 0x00, 0x00), "color-dark-red"),
        (Rgb(0x80, 0x80, 0x00), "color-dark-yellow"),
        (Rgb(0x80, 0x80, 0x80), "color-gray-50"),
        (Rgb(0xc0, 0xc0, 0xc0), "color-gray-25"),
        (Rgb(0x00, 0x00, 0x00), "color-black"),
    ];

    /// Font Color › Theme Colors: Office's theme, a column per colour, its base on top and
    /// four lighter or darker shades under it.
    pub const FONT_THEME: [[Rgb; 10]; 6] = [
        hex_row([
            0xFFFFFF, 0x000000, 0xE7E6E6, 0x44546A, 0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000,
            0x5B9BD5, 0x70AD47,
        ]),
        hex_row([
            0xF2F2F2, 0x808080, 0xD0CECE, 0xD6DCE4, 0xDAE3F3, 0xFBE5D6, 0xEDEDED, 0xFFF2CC,
            0xDEEBF7, 0xE2F0D9,
        ]),
        hex_row([
            0xD9D9D9, 0x595959, 0xAEAAAA, 0xADB9CA, 0xB4C7E7, 0xF8CBAD, 0xDBDBDB, 0xFFE699,
            0xBDD7EE, 0xC5E0B4,
        ]),
        hex_row([
            0xBFBFBF, 0x404040, 0x757171, 0x8497B0, 0x8FAADC, 0xF4B183, 0xC9C9C9, 0xFFD966,
            0x9DC3E6, 0xA9D18E,
        ]),
        hex_row([
            0xA6A6A6, 0x262626, 0x3A3838, 0x333F50, 0x2F5597, 0xC55A11, 0x7B7B7B, 0xBF9000,
            0x2E75B6, 0x548235,
        ]),
        hex_row([
            0x808080, 0x0D0D0D, 0x171616, 0x222A35, 0x203864, 0x843C0C, 0x525252, 0x7F6000,
            0x1F4E79, 0x385723,
        ]),
    ];

    /// Font Color › Standard Colors.
    pub const FONT_STANDARD: [Rgb; 10] = hex_row([
        0xC00000, 0xFF0000, 0xFFC000, 0xFFFF00, 0x92D050, 0x00B050, 0x00B0F0, 0x0070C0, 0x002060,
        0x7030A0,
    ]);

    pub fn to_color32(self) -> egui::Color32 {
        egui::Color32::from_rgb(self.0, self.1, self.2)
    }

    /// Whether text on this colour reads in white rather than in dark ink, as Office's
    /// Automatic font colour turns white on a dark highlight.
    pub fn wants_light_text(self) -> bool {
        let [r, g, b] = [self.0, self.1, self.2].map(f32::from);
        0.2126 * r + 0.7152 * g + 0.0722 * b <= 140.0
    }

    /// "#rrggbb".
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Reads "#rrggbb" back.
    pub fn from_hex(code: &str) -> Option<Rgb> {
        let digits = code.strip_prefix('#')?;
        if digits.len() != 6 || !digits.is_ascii() {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        Some(Rgb(channel(0)?, channel(2)?, channel(4)?))
    }
}

/// Ten 0xRRGGBB values as colours (Office's colour tables).
const fn hex_row(values: [u32; 10]) -> [Rgb; 10] {
    let mut row = [Rgb(0, 0, 0); 10];
    let mut at = 0;
    while at < 10 {
        let value = values[at];
        row[at] = Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8);
        at += 1;
    }
    row
}

/// A typeface for note text. Only bundled faces, so a page looks the same on every computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontFace {
    /// Carlito, with Calibri's metrics: the desktop notebook's note text, and the default.
    Body,
    Serif,
    /// The interface face (Selawik, Segoe UI's open stand-in). The old "display" face,
    /// retired in parity round 2, reads back as this face.
    #[serde(alias = "display")]
    Sans,
    Mono,
    Readable,
    Hand,
}

impl FontFace {
    pub const ALL: [FontFace; 6] = [
        FontFace::Body,
        FontFace::Sans,
        FontFace::Serif,
        FontFace::Readable,
        FontFace::Hand,
        FontFace::Mono,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FontFace::Body => "Carlito",
            FontFace::Serif => "Source Serif",
            FontFace::Sans => "Selawik",
            FontFace::Mono => "JetBrains Mono",
            FontFace::Readable => "Atkinson Hyperlegible",
            FontFace::Hand => "Patrick Hand",
        }
    }

    /// A CSS font stack for exports, falling back to common system fonts.
    pub fn css(self) -> &'static str {
        match self {
            FontFace::Body => "Carlito, Calibri, 'Segoe UI', Arial, sans-serif",
            FontFace::Serif => "'Source Serif 4', Georgia, serif",
            FontFace::Sans => "Selawik, 'Segoe UI', Arial, sans-serif",
            FontFace::Mono => "'JetBrains Mono', Consolas, monospace",
            FontFace::Readable => "'Atkinson Hyperlegible', Verdana, sans-serif",
            FontFace::Hand => "'Patrick Hand', 'Comic Sans MS', cursive",
        }
    }
}

/// The point sizes offered in the ribbon, as in the desktop notebook.
pub const FONT_SIZES: [u8; 16] = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72];

/// Screen points for a size in points, at 96 dots per inch as in the desktop notebook: its default
/// 11 pt is 14.67 px at 100 %.
pub fn pt_to_px(pt: u8) -> f32 {
    points_to_px(f32::from(pt))
}

/// [`pt_to_px`] for any number of points (Paragraph Spacing Options' values).
pub fn points_to_px(pt: f32) -> f32 {
    pt * 4.0 / 3.0
}

/// The point size of `px` screen points (the inverse of [`pt_to_px`]).
pub fn px_to_pt(px: f32) -> f32 {
    px * 0.75
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineStyle {
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<FontFace>,
    /// Size in points; `None` keeps the block's own size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u8>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub superscript: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub subscript: bool,
    /// Where the text links to: a web address, `mailto:`, `file:`, another page
    /// (`folio://page/<id>`) or a section (`folio://section/<id>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Arc<str>>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl InlineStyle {
    pub fn with_bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub fn is_plain(&self) -> bool {
        *self == InlineStyle::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleRun {
    pub start: usize,
    pub end: usize,
    pub style: InlineStyle,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyledText {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<StyleRun>,
}

impl StyledText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            runs: Vec::new(),
        }
    }

    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    pub fn style_at(&self, byte: usize) -> InlineStyle {
        self.runs
            .iter()
            .find(|run| run.start <= byte && byte < run.end)
            .map_or_else(InlineStyle::default, |run| run.style.clone())
    }

    /// The style new text typed at `caret` (a char index) should get: the style of the
    /// character before the caret, or of the first character at the very start.
    pub fn style_for_caret(&self, caret: usize) -> InlineStyle {
        let before = caret
            .saturating_sub(1)
            .min(self.char_count().saturating_sub(1));
        if self.text.is_empty() {
            return InlineStyle::default();
        }
        let mut style = self.style_at(self.char_to_byte(before));
        // Typing right after a link is plain text; only text typed inside a link joins it.
        if style.link.is_some() {
            let inside = caret > 0
                && caret < self.char_count()
                && self.style_at(self.char_to_byte(caret)).link == style.link;
            if !inside {
                style.link = None;
            }
        }
        style
    }

    pub fn char_styles(&self) -> Vec<InlineStyle> {
        let mut result = Vec::with_capacity(self.char_count());
        let mut byte = 0;
        for ch in self.text.chars() {
            result.push(self.style_at(byte));
            byte += ch.len_utf8();
        }
        result
    }

    pub fn char_to_byte(&self, char_index: usize) -> usize {
        if char_index == 0 {
            return 0;
        }
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    pub fn rebuild(&mut self, styles: &[InlineStyle]) {
        self.runs.clear();
        let mut byte = 0;
        let mut active: Option<(usize, InlineStyle)> = None;
        for (i, ch) in self.text.chars().enumerate() {
            let style = styles.get(i).cloned().unwrap_or_default();
            if active
                .as_ref()
                .is_some_and(|(_, current)| *current == style)
            {
                byte += ch.len_utf8();
                continue;
            }
            if let Some((start, current)) = active.take() {
                if !current.is_plain() && start < byte {
                    self.runs.push(StyleRun {
                        start,
                        end: byte,
                        style: current,
                    });
                }
            }
            active = (!style.is_plain()).then_some((byte, style));
            byte += ch.len_utf8();
        }
        if let Some((start, style)) = active {
            if !style.is_plain() && start < byte {
                self.runs.push(StyleRun {
                    start,
                    end: byte,
                    style,
                });
            }
        }
        self.normalize();
    }

    pub fn apply_style_chars(&mut self, range: Range<usize>, mut f: impl FnMut(&mut InlineStyle)) {
        let mut styles = self.char_styles();
        let end = range.end.min(styles.len());
        let start = range.start.min(end);
        for style in &mut styles[start..end] {
            f(style);
        }
        self.rebuild(&styles);
    }

    // The editor formats through `apply_style_chars`; these per-attribute setters stay for
    // tests and later features.
    #[allow(dead_code)]
    pub fn set_bold(&mut self, range: Range<usize>, value: bool) {
        self.apply_style_chars(range, |style| style.bold = value);
    }

    #[allow(dead_code)]
    pub fn set_italic(&mut self, range: Range<usize>, value: bool) {
        self.apply_style_chars(range, |style| style.italic = value);
    }

    #[allow(dead_code)]
    pub fn set_underline(&mut self, range: Range<usize>, value: bool) {
        self.apply_style_chars(range, |style| style.underline = value);
    }

    #[allow(dead_code)]
    pub fn set_strike(&mut self, range: Range<usize>, value: bool) {
        self.apply_style_chars(range, |style| style.strike = value);
    }

    #[allow(dead_code)]
    pub fn set_highlight(&mut self, range: Range<usize>, value: Option<Rgb>) {
        self.apply_style_chars(range, |style| style.highlight = value);
    }

    #[allow(dead_code)]
    pub fn set_color(&mut self, range: Range<usize>, value: Option<Rgb>) {
        self.apply_style_chars(range, |style| style.color = value);
    }

    pub fn set_link(&mut self, range: Range<usize>, link: Option<Arc<str>>) {
        self.apply_style_chars(range, |style| style.link = link.clone());
    }

    /// The link on char `index`, with the run of chars around it that share it.
    ///
    /// The editor asks this every frame for the char under the pointer, so it walks the
    /// runs rather than building every char's style.
    pub fn link_at(&self, index: usize) -> Option<(Range<usize>, Arc<str>)> {
        if self.runs.iter().all(|run| run.style.link.is_none()) {
            return None;
        }
        let byte = self.text.char_indices().nth(index)?.0;
        let run = self
            .runs
            .iter()
            .find(|run| run.start <= byte && byte < run.end)?;
        let url = run.style.link.clone()?;
        // Neighbouring runs (bold in part of a link, say) may carry the same link.
        let linked = |run: &&StyleRun| run.style.link.as_ref() == Some(&url);
        let (mut start, mut end) = (run.start, run.end);
        let runs = self.runs.iter().filter(linked);
        while let Some(before) = runs
            .clone()
            .find(|run| run.end == start && run.start < start)
        {
            start = before.start;
        }
        while let Some(after) = runs.clone().find(|run| run.start == end && run.end > end) {
            end = after.end;
        }
        let chars_before = |byte: usize| {
            self.text
                .char_indices()
                .take_while(|(at, _)| *at < byte)
                .count()
        };
        Some((chars_before(start)..chars_before(end), url))
    }

    /// Turns a web address that ends at char `end` into a link, as the desktop notebook does when a
    /// space or Enter follows it. Returns whether a link was added.
    /// Links every web address in the text (for text that arrives whole, such as a paste
    /// onto the paper). Returns whether a link was added.
    pub fn autolink_all(&mut self) -> bool {
        let count = self.char_count();
        let ends: Vec<usize> = self
            .text
            .chars()
            .enumerate()
            .filter(|(_, ch)| ch.is_whitespace())
            .map(|(at, _)| at)
            .chain(std::iter::once(count))
            .collect();
        let mut linked = false;
        for end in ends {
            linked |= self.autolink_word_before(end);
        }
        linked
    }

    pub fn autolink_word_before(&mut self, end: usize) -> bool {
        let chars: Vec<char> = self.text.chars().collect();
        let mut end = end.min(chars.len());
        let mut start = end;
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        // Punctuation around an address is not part of it: "(see www.x.org)."
        while start < end && matches!(chars[start], '(' | '[' | '<' | '"' | '\'') {
            start += 1;
        }
        while end > start
            && matches!(
                chars[end - 1],
                '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '>' | '"' | '\''
            )
        {
            end -= 1;
        }
        let word: String = chars[start..end].iter().collect();
        let Some(url) = web_address(&word) else {
            return false;
        };
        if self.char_styles()[start..end]
            .iter()
            .any(|style| style.link.is_some())
        {
            return false;
        }
        self.set_link(start..end, Some(Arc::from(url)));
        true
    }

    pub fn all_in_range(
        &self,
        range: Range<usize>,
        predicate: impl Fn(&InlineStyle) -> bool,
    ) -> bool {
        let styles = self.char_styles();
        let start = range.start.min(styles.len());
        let end = range.end.min(styles.len());
        start < end && styles[start..end].iter().all(predicate)
    }

    #[cfg(test)]
    pub fn sync_after_text_change(&mut self, new_text: &str, insertion_style: InlineStyle) {
        self.sync_after_edit(new_text, insertion_style, None);
    }

    /// Carries style runs across an edit. `caret` is the cursor (in chars) after the edit;
    /// it disambiguates edits next to identical characters, e.g. typing "m" before "me".
    pub fn sync_after_edit(
        &mut self,
        new_text: &str,
        insertion_style: InlineStyle,
        caret: Option<usize>,
    ) {
        if self.text == new_text {
            return;
        }
        let old_chars: Vec<char> = self.text.chars().collect();
        let new_chars: Vec<char> = new_text.chars().collect();
        let old_styles = self.char_styles();
        let inserted = new_chars.len().saturating_sub(old_chars.len());
        let prefix_limit = caret.map_or(usize::MAX, |caret| caret.saturating_sub(inserted));
        let mut prefix = 0;
        while prefix < old_chars.len()
            && prefix < new_chars.len()
            && prefix < prefix_limit
            && old_chars[prefix] == new_chars[prefix]
        {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < old_chars.len().saturating_sub(prefix)
            && suffix < new_chars.len().saturating_sub(prefix)
            && old_chars[old_chars.len() - 1 - suffix] == new_chars[new_chars.len() - 1 - suffix]
        {
            suffix += 1;
        }
        let old_end = old_chars.len() - suffix;
        let new_end = new_chars.len() - suffix;
        let mut styles = Vec::with_capacity(new_chars.len());
        styles.extend_from_slice(&old_styles[..prefix.min(old_styles.len())]);
        styles.resize(prefix + (new_end.saturating_sub(prefix)), insertion_style);
        if old_end < old_styles.len() || suffix > 0 {
            styles.extend_from_slice(&old_styles[old_end.min(old_styles.len())..]);
        }
        styles.truncate(new_chars.len());
        self.text = new_text.to_string();
        self.rebuild(&styles);
    }

    pub fn split_off_chars(&mut self, char_index: usize) -> StyledText {
        let char_index = char_index.min(self.char_count());
        let byte = self.char_to_byte(char_index);
        let styles = self.char_styles();
        let mut rest = StyledText {
            text: self.text[byte..].to_string(),
            runs: Vec::new(),
        };
        rest.rebuild(&styles[char_index..]);
        self.text.truncate(byte);
        self.rebuild(&styles[..char_index]);
        rest
    }

    pub fn append(&mut self, other: &StyledText) {
        let start = self.text.len();
        self.text.push_str(&other.text);
        for run in &other.runs {
            self.runs.push(StyleRun {
                start: start + run.start,
                end: start + run.end,
                style: run.style.clone(),
            });
        }
        self.normalize();
    }

    /// The text and styles of `range` (chars), as a text of its own.
    pub fn slice_chars(&self, range: Range<usize>) -> StyledText {
        let mut head = self.clone();
        head.split_off_chars(range.end);
        head.split_off_chars(range.start.min(range.end))
    }

    /// Puts `other`, with its styles, in at `at` (chars).
    pub fn insert_styled(&mut self, at: usize, other: &StyledText) {
        let tail = self.split_off_chars(at);
        self.append(other);
        self.append(&tail);
    }

    pub fn delete_chars(&mut self, range: Range<usize>) {
        let styles = self.char_styles();
        let start = range.start.min(styles.len());
        let end = range.end.min(styles.len());
        if start >= end {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let remaining: String = chars[..start].iter().chain(chars[end..].iter()).collect();
        let mut new_styles = styles[..start].to_vec();
        new_styles.extend_from_slice(&styles[end..]);
        self.text = remaining;
        self.rebuild(&new_styles);
    }

    pub fn normalize(&mut self) {
        self.runs.retain(|run| {
            run.start < run.end && run.start < self.text.len() && !run.style.is_plain()
        });
        self.runs.sort_by_key(|run| run.start);
        let mut merged: Vec<StyleRun> = Vec::new();
        for mut run in self.runs.drain(..) {
            run.start = floor_char_boundary(&self.text, run.start.min(self.text.len()));
            run.end = ceil_char_boundary(&self.text, run.end.min(self.text.len()));
            if run.start >= run.end {
                continue;
            }
            if let Some(last) = merged.last_mut() {
                if last.end == run.start && last.style == run.style {
                    last.end = run.end;
                    continue;
                }
            }
            merged.push(run);
        }
        self.runs = merged;
    }

    pub fn to_html(&self) -> String {
        let mut out = String::new();
        let mut byte = 0;
        let mut open = InlineStyle::default();
        for ch in self.text.chars() {
            let style = self.style_at(byte);
            if style != open {
                close_style_tags(&mut out, &open);
                open_style_tags(&mut out, &style);
                open = style;
            }
            match ch {
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '&' => out.push_str("&amp;"),
                '\n' => out.push_str("<br>"),
                _ => out.push(ch),
            }
            byte += ch.len_utf8();
        }
        close_style_tags(&mut out, &open);
        out
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn parse_each<T: serde::de::DeserializeOwned>(value: Option<serde_json::Value>) -> Vec<T> {
    match value {
        Some(serde_json::Value::Array(entries)) => entries
            .into_iter()
            .filter_map(|entry| serde_json::from_value(entry).ok())
            .collect(),
        _ => Vec::new(),
    }
}

fn floor_char_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(text: &str, mut i: usize) -> usize {
    i = i.min(text.len());
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn open_style_tags(out: &mut String, style: &InlineStyle) {
    if let Some(url) = &style.link {
        if is_openable_link(url) {
            out.push_str(&format!("<a href=\"{}\">", crate::util::html_escape(url)));
        } else if is_internal_link(url) {
            out.push_str("<span class=\"page-link\">");
        } else {
            out.push_str("<span>");
        }
    }
    if style.bold {
        out.push_str("<strong>");
    }
    if style.italic {
        out.push_str("<em>");
    }
    if style.underline {
        out.push_str("<u>");
    }
    if style.strike {
        out.push_str("<s>");
    }
    if style.code {
        out.push_str("<code>");
    }
    if let Some(color) = style.color {
        out.push_str(&format!(
            "<span style=\"color:#{:02x}{:02x}{:02x}\">",
            color.0, color.1, color.2
        ));
    }
    if let Some(color) = style.highlight {
        // Text with no colour of its own turns white on a dark highlight, as on the page.
        let class = if style.color.is_none() && color.wants_light_text() {
            " class=\"on-dark\""
        } else {
            ""
        };
        out.push_str(&format!(
            "<mark{class} style=\"background:#{:02x}{:02x}{:02x}\">",
            color.0, color.1, color.2
        ));
    }
    if style.font.is_some() || style.size.is_some() {
        let mut css = String::new();
        if let Some(face) = style.font {
            css.push_str(&format!("font-family:{};", face.css()));
        }
        if let Some(pt) = style.size {
            // CSS points are 96-DPI points too, so this is `pt_to_px(pt)` screen pixels.
            // Its line is 1.22 times its size, as in the editor, so large text grows its
            // line; the paragraph's own line (18 px, or At Least) still holds small text.
            css.push_str(&format!("font-size:{pt}pt;line-height:1.22;"));
        }
        out.push_str(&format!("<span style=\"{css}\">"));
    }
    if style.superscript {
        out.push_str("<sup>");
    }
    if style.subscript {
        out.push_str("<sub>");
    }
}

fn close_style_tags(out: &mut String, style: &InlineStyle) {
    if style.subscript {
        out.push_str("</sub>");
    }
    if style.superscript {
        out.push_str("</sup>");
    }
    if style.font.is_some() || style.size.is_some() {
        out.push_str("</span>");
    }
    if style.highlight.is_some() {
        out.push_str("</mark>");
    }
    if style.color.is_some() {
        out.push_str("</span>");
    }
    if style.code {
        out.push_str("</code>");
    }
    if style.strike {
        out.push_str("</s>");
    }
    if style.underline {
        out.push_str("</u>");
    }
    if style.italic {
        out.push_str("</em>");
    }
    if style.bold {
        out.push_str("</strong>");
    }
    if let Some(url) = &style.link {
        out.push_str(if is_openable_link(url) {
            "</a>"
        } else {
            "</span>"
        });
    }
}

/// Inline CSS for a block's alignment, list marker, indent and spacing (empty when it has
/// none).
fn block_attrs(block: &Block) -> String {
    let mut css = String::new();
    if !block.align.is_left() {
        css.push_str(&format!("text-align:{};", block.align.css()));
        // The bullet or number moves with centred and right-aligned text, as in the app.
        if block.kind.is_list() {
            css.push_str("list-style-position:inside;");
        }
    }
    if let Some(marker) = block.marker.filter(|marker| {
        matches!(block.kind, BlockKind::Bullet) && !marker.numbered()
            || matches!(block.kind, BlockKind::Numbered) && marker.numbered()
    }) {
        css.push_str(&format!("list-style-type:{};", marker.css()));
    }
    if block.indent > 0 {
        css.push_str(&format!("margin-left:{}em;", f32::from(block.indent) * 1.5));
    }
    // Paragraph Spacing Options, in points as the dialog gives them. "At least" is a
    // minimum: a heading keeps its taller lines, as on the page. The natural heights are
    // the export's own (`print_css`): 18 px lines, headings 1.25 times their size.
    let spacing = block.spacing;
    if spacing.before > 0.0 {
        css.push_str(&format!("margin-top:{}pt;", spacing.before));
    }
    if spacing.after > 0.0 {
        css.push_str(&format!("margin-bottom:{}pt;", spacing.after));
    }
    if spacing.line_at_least > 0.0 {
        let natural = if matches!(block.kind, BlockKind::Heading { .. }) {
            "1.25em"
        } else {
            "18px"
        };
        css.push_str(&format!(
            "line-height:max({natural},{}pt);",
            spacing.line_at_least
        ));
    }
    if css.is_empty() {
        String::new()
    } else {
        format!(" style=\"{css}\"")
    }
}

/// A paragraph's text as HTML, with its tags: a symbol for icon tags, a highlight for the rest.
/// Icon tags and a To Do box hang in the margin left of the text, as on the page.
fn tagged_html(block: &Block) -> String {
    let mut html = block.text.to_html();
    if let Some(color) = block.custom_tags.iter().find_map(|tag| tag.font_color) {
        html = format!("<span style=\"color:{}\">{html}</span>", color.hex());
    }
    let background = block
        .tags
        .iter()
        .find_map(|tag| tag.background())
        .or_else(|| block.custom_tags.iter().find_map(|tag| tag.highlight));
    if let Some(bg) = background {
        let font_color = block.custom_tags.iter().any(|tag| tag.font_color.is_some());
        let class = if !font_color && bg.wants_light_text() {
            " class=\"on-dark\""
        } else {
            ""
        };
        html = format!(
            "<mark{class} style=\"background:{}\">{html}</mark>",
            bg.hex()
        );
    }
    let tick = |checked: bool| if checked { "☑" } else { "☐" };
    let mut prefix = String::new();
    let mut slots = 0;
    for tag in block.tags.iter().filter(|tag| tag.background().is_none()) {
        let symbol = if tag.checkable() {
            tick(block.checked_tags.contains(tag))
        } else {
            tag.symbol()
        };
        prefix.push_str(&tag_span(&tag.label(), tag.color(), symbol));
        slots += 1;
    }
    for tag in &block.custom_tags {
        let symbol = match tag.symbol {
            TagSymbol::None => continue,
            TagSymbol::Checkbox => tick(tag.checked),
            TagSymbol::Tag(line) if line.checkable() => tick(tag.checked),
            TagSymbol::Tag(line) => line.symbol(),
        };
        let color = tag.symbol.color().unwrap_or(Rgb(0x24, 0x24, 0x24));
        prefix.push_str(&tag_span(&tag.name, color, symbol));
        slots += 1;
    }
    if let BlockKind::Todo { checked } = block.kind {
        prefix.push_str(if checked {
            "<input type=\"checkbox\" disabled checked>"
        } else {
            "<input type=\"checkbox\" disabled>"
        });
    }
    if !prefix.is_empty() {
        let slots = slots + usize::from(matches!(block.kind, BlockKind::Todo { .. }));
        let width = slots as f32 * 18.0;
        // On the page a list item's tags come before its bullet or number: they hang left
        // of the marker too, past the list's 1.6em indent (the export's `ul` and `ol`),
        // and the text still starts after the marker.
        let hang = match block.kind {
            BlockKind::Bullet | BlockKind::Numbered => {
                format!("calc(-{width:.0}px - 1.6em);margin-right:1.6em")
            }
            _ => format!("-{width:.0}px"),
        };
        prefix = format!(
            "<span class=\"tags\" style=\"margin-left:{hang};width:{width:.0}px\">{prefix}</span>"
        );
    }
    prefix + &html
}

/// One tag's symbol in an exported paragraph's margin, named by its hover title.
fn tag_span(title: &str, c: Rgb, symbol: &str) -> String {
    format!(
        "<span class=\"tag\" title=\"{}\" style=\"color:{};font-weight:700\">{symbol}</span> ",
        crate::util::html_escape(title),
        c.hex()
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    Paragraph,
    Heading {
        #[serde(default = "default_heading_level")]
        level: u8,
    },
    Bullet,
    Numbered,
    Todo {
        #[serde(default)]
        checked: bool,
    },
    Quote,
    /// the desktop notebook's Citation style: a small grey line naming a source.
    Citation,
    Code,
    Divider,
    Image {
        attachment: String,
        #[serde(default = "one")]
        width: f32,
        #[serde(default, skip_serializing_if = "PictureMeta::is_plain")]
        meta: PictureMeta,
    },
    Drawing {
        attachment: String,
        #[serde(default = "default_drawing_height")]
        height: f32,
    },
    /// A file attached inside a note, as File Attachment puts it at the caret.
    File {
        attachment: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        size: u64,
    },
    /// A typed equation inside a note (linear format or LaTeX, see math.rs).
    Equation {
        #[serde(default)]
        source: String,
        /// The formula's font size in page units, as `CanvasKind::Equation`'s `size`.
        #[serde(default = "default_equation_size")]
        scale: f32,
    },
    /// A recording inside a note; `duration_ms` is 0 while it records.
    Audio {
        attachment: String,
        #[serde(default)]
        duration_ms: u64,
    },
    /// A table: rows of cells you type in.
    Table(TableBlock),
}

fn default_heading_level() -> u8 {
    2
}

fn one() -> f32 {
    1.0
}

fn default_drawing_height() -> f32 {
    280.0
}

impl BlockKind {
    /// the desktop notebook's Page Title paragraph style (not the page's own title).
    pub const PAGE_TITLE: BlockKind = BlockKind::Heading { level: 1 };

    /// the desktop notebook's Heading `n` (1–6). It is stored one level down: level 1 is Page Title.
    pub fn heading(n: u8) -> Self {
        BlockKind::Heading {
            level: n.clamp(1, 6) + 1,
        }
    }

    /// The Styles gallery, in the desktop notebook's order.
    pub fn styles() -> [BlockKind; 11] {
        [
            Self::heading(1),
            Self::heading(2),
            Self::heading(3),
            Self::heading(4),
            Self::heading(5),
            Self::heading(6),
            Self::PAGE_TITLE,
            BlockKind::Citation,
            BlockKind::Quote,
            BlockKind::Code,
            BlockKind::Paragraph,
        ]
    }

    pub fn label(&self) -> Cow<'static, str> {
        match self {
            BlockKind::Paragraph => t!("block-text"),
            BlockKind::Heading { level: 0 | 1 } => t!("block-title"),
            BlockKind::Heading { level: 2 } => t!("block-heading-1"),
            BlockKind::Heading { level: 3 } => t!("block-heading-2"),
            BlockKind::Heading { level: 4 } => t!("block-heading-3"),
            BlockKind::Heading { level: 5 } => t!("block-heading-4"),
            BlockKind::Heading { level: 6 } => t!("block-heading-5"),
            BlockKind::Heading { .. } => t!("block-heading-6"),
            BlockKind::Citation => t!("block-citation"),
            BlockKind::Bullet => t!("block-bulleted-list"),
            BlockKind::Numbered => t!("block-numbered-list"),
            BlockKind::Todo { .. } => t!("block-todo"),
            BlockKind::Quote => t!("block-quote"),
            BlockKind::Code => t!("block-code"),
            BlockKind::Divider => t!("block-divider"),
            BlockKind::Image { .. } => t!("block-image"),
            BlockKind::Drawing { .. } => t!("block-drawing"),
            BlockKind::File { .. } => t!("item-file"),
            BlockKind::Equation { .. } => t!("item-equation"),
            BlockKind::Audio { .. } => t!("item-recording"),
            BlockKind::Table(_) => t!("block-table"),
        }
    }

    pub fn is_list(&self) -> bool {
        matches!(
            self,
            BlockKind::Bullet | BlockKind::Numbered | BlockKind::Todo { .. }
        )
    }

    pub fn is_textual(&self) -> bool {
        !matches!(
            self,
            BlockKind::Divider
                | BlockKind::Image { .. }
                | BlockKind::Drawing { .. }
                | BlockKind::File { .. }
                | BlockKind::Equation { .. }
                | BlockKind::Audio { .. }
                | BlockKind::Table(_)
        )
    }

    /// This kind, cheap to copy every frame: a table comes without its cells (its flags
    /// stay). For code that only asks which kind a block is.
    pub fn without_cells(&self) -> BlockKind {
        match self {
            BlockKind::Table(table) => BlockKind::Table(TableBlock {
                rows: Vec::new(),
                widths: Vec::new(),
                header: table.header,
                hide_borders: table.hide_borders,
            }),
            other => other.clone(),
        }
    }

    /// The attachment the block shows, for kinds that hold a file.
    pub fn attachment(&self) -> Option<&str> {
        match self {
            BlockKind::Image { attachment, .. }
            | BlockKind::Drawing { attachment, .. }
            | BlockKind::File { attachment, .. }
            | BlockKind::Audio { attachment, .. } => Some(attachment),
            _ => None,
        }
    }

    pub fn attachment_mut(&mut self) -> Option<&mut String> {
        match self {
            BlockKind::Image { attachment, .. }
            | BlockKind::Drawing { attachment, .. }
            | BlockKind::File { attachment, .. }
            | BlockKind::Audio { attachment, .. } => Some(attachment),
            _ => None,
        }
    }
}

/// the desktop notebook's paragraph tags. To Do is not here: it is a block kind of its own. The names
/// are stored, so they never change; new tags are only ever added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineTag {
    Important,
    Question,
    RememberForLater,
    Definition,
    Highlight,
    Contact,
    Address,
    Phone,
    WebSite,
    Idea,
    Password,
    Critical,
    ProjectA,
    ProjectB,
    Movie,
    Book,
    Music,
    Source,
    Blog,
    DiscussA,
    DiscussB,
    DiscussManager,
    SendEmail,
    ScheduleMeeting,
    CallBack,
    TodoPriority1,
    TodoPriority2,
    ClientRequest,
}

impl LineTag {
    /// the desktop notebook's gallery order after To Do, which is also the order tags are kept in on a
    /// paragraph.
    pub const ALL: [LineTag; 28] = [
        LineTag::Important,
        LineTag::Question,
        LineTag::RememberForLater,
        LineTag::Definition,
        LineTag::Highlight,
        LineTag::Contact,
        LineTag::Address,
        LineTag::Phone,
        LineTag::WebSite,
        LineTag::Idea,
        LineTag::Password,
        LineTag::Critical,
        LineTag::ProjectA,
        LineTag::ProjectB,
        LineTag::Movie,
        LineTag::Book,
        LineTag::Music,
        LineTag::Source,
        LineTag::Blog,
        LineTag::DiscussA,
        LineTag::DiscussB,
        LineTag::DiscussManager,
        LineTag::SendEmail,
        LineTag::ScheduleMeeting,
        LineTag::CallBack,
        LineTag::TodoPriority1,
        LineTag::TodoPriority2,
        LineTag::ClientRequest,
    ];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            LineTag::Important => t!("tag-important"),
            LineTag::Question => t!("tag-question"),
            LineTag::RememberForLater => t!("tag-remember-for-later"),
            LineTag::Definition => t!("tag-definition"),
            LineTag::Highlight => t!("tag-highlight"),
            LineTag::Contact => t!("tag-contact"),
            LineTag::Address => t!("tag-address"),
            LineTag::Phone => t!("tag-phone-number"),
            LineTag::WebSite => t!("tag-web-site"),
            LineTag::Idea => t!("tag-idea"),
            LineTag::Password => t!("tag-password"),
            LineTag::Critical => t!("tag-critical"),
            LineTag::ProjectA => t!("tag-project-a"),
            LineTag::ProjectB => t!("tag-project-b"),
            LineTag::Movie => t!("tag-movie"),
            LineTag::Book => t!("tag-book-to-read"),
            LineTag::Music => t!("tag-music"),
            LineTag::Source => t!("tag-source"),
            LineTag::Blog => t!("tag-blog"),
            LineTag::DiscussA => t!("tag-discuss-a"),
            LineTag::DiscussB => t!("tag-discuss-b"),
            LineTag::DiscussManager => t!("tag-discuss-manager"),
            LineTag::SendEmail => t!("tag-send-email"),
            LineTag::ScheduleMeeting => t!("tag-schedule-meeting"),
            LineTag::CallBack => t!("tag-call-back"),
            LineTag::TodoPriority1 => t!("tag-todo-priority-1"),
            LineTag::TodoPriority2 => t!("tag-todo-priority-2"),
            LineTag::ClientRequest => t!("tag-client-request"),
        }
    }

    /// The tags the desktop notebook draws as a box that can be ticked, besides To Do. Send in email is
    /// an envelope, and the desktop notebook never ticks it (Graph has no "send-in-email:completed").
    pub fn checkable(self) -> bool {
        matches!(
            self,
            LineTag::DiscussA
                | LineTag::DiscussB
                | LineTag::DiscussManager
                | LineTag::ScheduleMeeting
                | LineTag::CallBack
                | LineTag::TodoPriority1
                | LineTag::TodoPriority2
                | LineTag::ClientRequest
        )
    }

    pub fn icon(self) -> &'static str {
        use crate::icons;
        match self {
            LineTag::Important => icons::STAR,
            LineTag::Question => icons::QUESTION,
            LineTag::RememberForLater => icons::BOOKMARK_SIMPLE,
            LineTag::Definition => icons::BOOK_OPEN,
            LineTag::Highlight => icons::HIGHLIGHTER,
            LineTag::Contact => icons::USER,
            LineTag::Address => icons::HOUSE,
            LineTag::Phone => icons::PHONE,
            LineTag::WebSite => icons::GLOBE,
            LineTag::Idea => icons::LIGHTBULB,
            LineTag::Password => icons::KEY,
            LineTag::Critical => icons::WARNING,
            LineTag::ProjectA | LineTag::ProjectB => icons::SQUARE,
            LineTag::Movie => icons::MOVIES_AND_TV,
            LineTag::Book => icons::BOOK,
            LineTag::Music => icons::MUSIC_NOTE,
            LineTag::Source => icons::NEWS,
            LineTag::Blog => icons::CHAT,
            LineTag::DiscussA | LineTag::DiscussB => icons::PERSON_CHAT,
            LineTag::DiscussManager => icons::PEOPLE_CHAT,
            LineTag::SendEmail => icons::MAIL,
            LineTag::ScheduleMeeting => icons::CALENDAR_BLANK,
            LineTag::CallBack => icons::CALL_FORWARD,
            LineTag::TodoPriority1 => icons::CHECKBOX_1,
            LineTag::TodoPriority2 => icons::CHECKBOX_2,
            LineTag::ClientRequest => icons::CLIPBOARD_ERROR,
        }
    }

    pub fn color(self) -> Rgb {
        match self {
            LineTag::Important | LineTag::Idea | LineTag::TodoPriority2 => Rgb(0xe0, 0x9b, 0x00),
            LineTag::Question => Rgb(0x7b, 0x4d, 0xd6),
            LineTag::RememberForLater => Rgb(0xc9, 0x9a, 0x00),
            LineTag::Highlight => Rgb(0xc2, 0x18, 0x5b),
            LineTag::Definition => Rgb(0x2e, 0x8b, 0x57),
            LineTag::Contact
            | LineTag::Address
            | LineTag::Phone
            | LineTag::WebSite
            | LineTag::Music
            | LineTag::Source
            | LineTag::Blog
            | LineTag::DiscussA
            | LineTag::ScheduleMeeting
            | LineTag::CallBack
            | LineTag::TodoPriority1 => Rgb(0x1a, 0x5b, 0xc4),
            LineTag::Password => Rgb(0x8a, 0x58, 0x00),
            LineTag::Critical => Rgb(0xc8, 0x3a, 0x2a),
            LineTag::ProjectA => Rgb(0xd8, 0x3b, 0x01),
            LineTag::ProjectB => Rgb(0xe3, 0xb5, 0x6a),
            LineTag::Movie => Rgb(0x4c, 0x4a, 0x5a),
            LineTag::Book | LineTag::DiscussB => Rgb(0x0a, 0x75, 0x63),
            LineTag::DiscussManager => Rgb(0x1b, 0x3d, 0x8c),
            LineTag::SendEmail | LineTag::ClientRequest => Rgb(0xc9, 0x9a, 0x00),
        }
    }

    /// Tags shown as a highlight behind the text rather than as an icon.
    pub fn background(self) -> Option<Rgb> {
        match self {
            LineTag::RememberForLater => Some(Rgb::HIGHLIGHTERS[0]),
            LineTag::Definition => Some(Rgb::HIGHLIGHTERS[2]),
            _ => None,
        }
    }

    /// A symbol for exported and printed pages. A checkable tag shows its box instead
    /// (`tagged_html`).
    pub fn symbol(self) -> &'static str {
        match self {
            LineTag::Important => "★",
            LineTag::Question => "?",
            LineTag::Highlight => "✎",
            LineTag::Contact => "☺",
            LineTag::Address => "⌂",
            LineTag::Phone => "☎",
            LineTag::WebSite => "🌐",
            LineTag::Idea => "✦",
            LineTag::Password => "⚿",
            LineTag::Critical => "!",
            LineTag::ProjectA | LineTag::ProjectB => "■",
            LineTag::Movie => "🎞",
            LineTag::Book => "❧",
            LineTag::Music => "♪",
            LineTag::Source => "📰",
            LineTag::Blog => "💬",
            LineTag::SendEmail => "✉",
            LineTag::DiscussA
            | LineTag::DiscussB
            | LineTag::DiscussManager
            | LineTag::ScheduleMeeting
            | LineTag::CallBack
            | LineTag::TodoPriority1
            | LineTag::TodoPriority2
            | LineTag::ClientRequest => "☐",
            LineTag::RememberForLater | LineTag::Definition => "",
        }
    }

    pub fn rank(self) -> usize {
        Self::ALL
            .iter()
            .position(|tag| *tag == self)
            .unwrap_or(usize::MAX)
    }
}

/// Reads paragraph tags, skipping any this version does not know (a newer Folio may
/// have added more), so the paragraph itself is never dropped.
fn known_tags<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<LineTag>, D::Error> {
    let values = Vec::<serde_json::Value>::deserialize(deserializer)?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

/// Reads a custom tag's symbol. One this version does not know (a newer Folio's built-in
/// tag) shows none, so the paragraph and its tag are kept, as `known_tags` keeps them.
fn known_symbol<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<TagSymbol, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// What a custom tag shows beside its paragraph (Customize Tags › Symbol). Stored by name,
/// so a later change to the built-in list never changes what a tagged paragraph shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagSymbol {
    #[default]
    None,
    /// To Do's box: the tag can be ticked.
    Checkbox,
    /// A built-in tag's icon and colour.
    Tag(LineTag),
}

impl TagSymbol {
    pub fn icon(self) -> Option<&'static str> {
        match self {
            TagSymbol::None => None,
            TagSymbol::Checkbox => Some(crate::icons::CHECK_SQUARE),
            TagSymbol::Tag(tag) => Some(tag.icon()),
        }
    }

    /// `None` for the To Do box, which uses the theme accent.
    pub fn color(self) -> Option<Rgb> {
        match self {
            TagSymbol::Tag(tag) => Some(tag.color()),
            _ => None,
        }
    }

    pub fn checkable(self) -> bool {
        match self {
            TagSymbol::Checkbox => true,
            TagSymbol::Tag(tag) => tag.checkable(),
            TagSymbol::None => false,
        }
    }

    /// Every symbol Customize Tags offers, in its list's order: none, the box, then the
    /// built-in tags that have an icon.
    pub fn choices() -> Vec<TagSymbol> {
        [TagSymbol::None, TagSymbol::Checkbox]
            .into_iter()
            .chain(
                LineTag::ALL
                    .into_iter()
                    .filter(|tag| tag.background().is_none())
                    .map(TagSymbol::Tag),
            )
            .collect()
    }
}

/// A tag made with Customize Tags, as a tagged paragraph keeps it. the desktop notebook keeps a tag's
/// look with each paragraph: changing or deleting the tag later leaves tagged notes alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTag {
    pub name: String,
    #[serde(default, deserialize_with = "known_symbol")]
    pub symbol: TagSymbol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Rgb>,
    /// Ticked, when its symbol is a box.
    #[serde(default, skip_serializing_if = "is_false")]
    pub checked: bool,
}

/// The deepest outline level: the desktop notebook shows levels 1 to 9, 0 to 8 here.
pub const MAX_LEVEL: u8 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    #[serde(default = "next_block_id")]
    pub id: u64,
    pub kind: BlockKind,
    #[serde(default)]
    pub text: StyledText,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub indent: u8,
    #[serde(default, skip_serializing_if = "ParaAlign::is_left")]
    pub align: ParaAlign,
    /// The bullet or number style picked for a list item; `None` is its level's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<ListMarker>,
    /// Where a numbered item's count starts; `None` carries on from the item before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<ListStart>,
    /// Paragraph Spacing Options: the space before and after it and its least line height.
    #[serde(default, skip_serializing_if = "ParaSpacing::is_none")]
    pub spacing: ParaSpacing,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "known_tags"
    )]
    pub tags: Vec<LineTag>,
    /// Its checkable tags that are ticked (each also in `tags`).
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "known_tags"
    )]
    pub checked_tags: Vec<LineTag>,
    /// Tags made with Customize Tags, each with its own look (see `CustomTag`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_tags: Vec<CustomTag>,
    /// Its subparagraphs (the blocks right after it at a deeper level) are folded away, as
    /// when the desktop notebook's paragraph handle is double-clicked.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

/// Horizontal alignment of a paragraph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParaAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl ParaAlign {
    pub fn is_left(&self) -> bool {
        *self == ParaAlign::Left
    }

    pub fn css(self) -> &'static str {
        match self {
            ParaAlign::Left => "left",
            ParaAlign::Center => "center",
            ParaAlign::Right => "right",
        }
    }
}

/// The most Paragraph Spacing Options takes for each of its values, in points.
pub const MAX_PARA_SPACING: f32 = 200.0;

/// Paragraph Spacing Options: points before and after the paragraph, and the least line
/// height, as the desktop notebook's dialog sets them. Zero means none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaSpacing {
    #[serde(default)]
    pub before: f32,
    #[serde(default)]
    pub after: f32,
    #[serde(default)]
    pub line_at_least: f32,
}

impl ParaSpacing {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }

    /// Each value a finite number of points from 0 to [`MAX_PARA_SPACING`].
    pub fn repair(&mut self) {
        for value in [&mut self.before, &mut self.after, &mut self.line_at_least] {
            *value = finite_or(*value, 0.0).clamp(0.0, MAX_PARA_SPACING);
        }
    }
}

/// The narrowest a table column gets, and the widest one that fits its text grows.
pub const TABLE_MIN_COLUMN: f32 = 40.0;
pub const TABLE_AUTO_MAX: f32 = 320.0;

/// A table in a note, as in the desktop notebook: a grid of cells you type in. Tables are blocks of a
/// note, not page items (old page tables are converted when they load).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableBlock {
    #[serde(default)]
    pub rows: Vec<Vec<TableCell>>,
    /// Column widths in page units. 0 fits the column to its text, as the desktop notebook widens a
    /// column while you type until you drag its border.
    #[serde(default)]
    pub widths: Vec<f32>,
    /// The first row is a header: shaded, and kept on top when sorting.
    #[serde(default, skip_serializing_if = "is_false")]
    pub header: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hide_borders: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    #[serde(default)]
    pub text: StyledText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shading: Option<Rgb>,
    #[serde(default, skip_serializing_if = "ParaAlign::is_left")]
    pub align: ParaAlign,
}

impl TableBlock {
    /// An empty table, its columns fitting their text.
    pub fn new(rows: usize, cols: usize) -> Self {
        let cols = cols.max(1);
        Self {
            rows: vec![vec![TableCell::default(); cols]; rows.max(1)],
            widths: vec![0.0; cols],
            header: false,
            hide_borders: false,
        }
    }

    /// A table of plain cell texts (old page tables), every column `width` wide.
    pub fn from_strings(rows: &[Vec<String>], header: bool, width: f32) -> Self {
        let mut table = Self {
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|text| TableCell {
                            text: StyledText::new(text.as_str()),
                            ..TableCell::default()
                        })
                        .collect()
                })
                .collect(),
            widths: Vec::new(),
            header,
            hide_borders: false,
        };
        let cols = table.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
        table.widths = vec![width; cols];
        table.repair();
        table
    }

    pub fn cols(&self) -> usize {
        self.widths.len().max(1)
    }

    /// Every row as wide as the widest, at least one cell, widths in range.
    pub fn repair(&mut self) {
        if self.rows.is_empty() {
            self.rows.push(Vec::new());
        }
        let cols = self
            .rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .max(self.widths.len())
            .max(1);
        for row in &mut self.rows {
            row.resize(cols, TableCell::default());
            for cell in row.iter_mut() {
                cell.text.normalize();
            }
        }
        self.widths.resize(cols, 0.0);
        for width in &mut self.widths {
            *width = if width.is_finite() && *width > 0.0 {
                width.clamp(TABLE_MIN_COLUMN, 2400.0)
            } else {
                0.0
            };
        }
    }

    /// The cells' text: a line per row, a tab between cells (search, the reader, Keep
    /// Text Only).
    pub fn plain_text(&self) -> String {
        self.rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.text.text.replace('\n', " "))
                    .collect::<Vec<_>>()
                    .join("\t")
                    .trim_end()
                    .to_string()
            })
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn to_html(&self) -> String {
        let mut out = String::from(if self.hide_borders {
            "<table class=\"folio-table no-borders\"><colgroup>"
        } else {
            "<table class=\"folio-table\"><colgroup>"
        });
        for width in &self.widths {
            if *width > 0.0 {
                out.push_str(&format!("<col style=\"width:{width:.0}px\">"));
            } else {
                out.push_str("<col>");
            }
        }
        out.push_str("</colgroup>");
        for (index, row) in self.rows.iter().enumerate() {
            let tag = if self.header && index == 0 {
                "th"
            } else {
                "td"
            };
            out.push_str("<tr>");
            for cell in row {
                let mut style = String::new();
                if let Some(Rgb(r, g, b)) = cell.shading {
                    style.push_str(&format!("background:#{r:02x}{g:02x}{b:02x};"));
                }
                if !cell.align.is_left() {
                    style.push_str(&format!("text-align:{};", cell.align.css()));
                }
                if style.is_empty() {
                    out.push_str(&format!("<{tag}>{}</{tag}>", cell.text.to_html()));
                } else {
                    out.push_str(&format!(
                        "<{tag} style=\"{style}\">{}</{tag}>",
                        cell.text.to_html()
                    ));
                }
            }
            out.push_str("</tr>");
        }
        out.push_str("</table>");
        out
    }
}

/// What the user set on a picture: its alt text, how it is turned and cropped, and
/// (pictures on the page) whether it is the page's background.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureMeta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
    /// Quarter turns clockwise (0–3), applied after `flip`.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub turns: u8,
    /// Mirrored left to right, in the picture's own space, before turning.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// Locked behind everything on the page, to write or draw on (Set Picture as
    /// Background).
    #[serde(default, skip_serializing_if = "is_false")]
    pub background: bool,
    /// The part of the picture shown, as fractions of its own (unturned, unflipped) width
    /// and height: left, top, right, bottom. `None` shows it whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f32; 4]>,
}

impl PictureMeta {
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    pub fn rotate(&mut self, clockwise: bool) {
        self.turns = (self.turns % 4 + if clockwise { 1 } else { 3 }) % 4;
    }

    /// Mirrors the picture as it is shown: left to right, or top to bottom.
    pub fn flip_shown(&mut self, vertical: bool) {
        let turns = self.turns % 4;
        self.turns = if vertical {
            (6 - turns) % 4
        } else {
            (4 - turns) % 4
        };
        self.flip = !self.flip;
    }

    /// Turned a quarter or three quarters: the picture is as wide as it was tall.
    pub fn quarter_turned(&self) -> bool {
        self.turns % 2 == 1
    }

    /// The crop, clamped to a sane box (at least 2% each way), or the whole picture.
    pub fn crop_box(&self) -> [f32; 4] {
        let [l, t, r, b] = self.crop.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let l = l.clamp(0.0, 0.98);
        let t = t.clamp(0.0, 0.98);
        [l, t, r.clamp(l + 0.02, 1.0), b.clamp(t + 0.02, 1.0)]
    }

    /// The picture's size once cropped, from its full (unturned) size.
    pub fn cropped_size(&self, full: [f32; 2]) -> [f32; 2] {
        let [l, t, r, b] = self.crop_box();
        [full[0] * (r - l), full[1] * (b - t)]
    }

    /// The texture corner (u, v) shown at each corner of the picture: top-left,
    /// top-right, bottom-right, bottom-left. Only the cropped part is shown.
    pub fn uv_corners(&self) -> [[f32; 2]; 4] {
        let [l, t, r, b] = self.crop_box();
        self.whole_uv_corners()
            .map(|[u, v]| [l + u * (r - l), t + v * (b - t)])
    }

    /// `uv_corners` of the whole picture, ignoring the crop.
    fn whole_uv_corners(&self) -> [[f32; 2]; 4] {
        let base = if self.flip {
            [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]]
        } else {
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        };
        let turns = usize::from(self.turns % 4);
        std::array::from_fn(|corner| base[(corner + 4 - turns) % 4])
    }

    /// The crop box, in the picture's own space, that shows `shown`: a rectangle given as
    /// fractions of the whole picture as it is shown (turned and flipped), left, top, right,
    /// bottom. The crop UI works in the shown space.
    pub fn source_rect(&self, shown: [f32; 4]) -> [f32; 4] {
        let [tl, tr, br, bl] = self.whole_uv_corners();
        // The texture point shown at (x, y) of the whole picture.
        let at = |x: f32, y: f32| {
            let top = [tl[0] + (tr[0] - tl[0]) * x, tl[1] + (tr[1] - tl[1]) * x];
            let bottom = [bl[0] + (br[0] - bl[0]) * x, bl[1] + (br[1] - bl[1]) * x];
            [
                top[0] + (bottom[0] - top[0]) * y,
                top[1] + (bottom[1] - top[1]) * y,
            ]
        };
        let [x0, y0, x1, y1] = shown;
        let corners = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
        let us = corners.map(|[u, _]| u);
        let vs = corners.map(|[_, v]| v);
        let min = |values: [f32; 4]| values.into_iter().fold(f32::INFINITY, f32::min);
        let max = |values: [f32; 4]| values.into_iter().fold(f32::NEG_INFINITY, f32::max);
        [min(us), min(vs), max(us), max(vs)]
    }

    /// The crop box as the shown picture sees it, `source_rect`'s inverse: fractions of the
    /// whole picture as it is shown (turned and flipped).
    pub fn shown_crop(&self) -> [f32; 4] {
        // A turn or a flip maps each shown axis onto one texture axis, one way or the other.
        let [tl, tr, _, bl] = self.whole_uv_corners();
        let across = [tr[0] - tl[0], tr[1] - tl[1]];
        let down = [bl[0] - tl[0], bl[1] - tl[1]];
        let shown = |[u, v]: [f32; 2]| {
            let d = [u - tl[0], v - tl[1]];
            [
                d[0] * across[0] + d[1] * across[1],
                d[0] * down[0] + d[1] * down[1],
            ]
        };
        let [l, t, r, b] = self.crop_box();
        let [x0, y0] = shown([l, t]);
        let [x1, y1] = shown([r, b]);
        [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
    }
}

/// The CSS turn of a turned or mirrored picture (`transform:…;`), or nothing.
pub fn picture_css(meta: &PictureMeta) -> String {
    if meta.turns.is_multiple_of(4) && !meta.flip {
        return String::new();
    }
    let flip = if meta.flip { " scaleX(-1)" } else { "" };
    format!(
        "transform:rotate({}deg){flip};",
        u32::from(meta.turns % 4) * 90
    )
}

/// `picture_css` as a ` style="…"` attribute, or nothing.
pub fn picture_transform(meta: &PictureMeta) -> String {
    let css = picture_css(meta);
    if css.is_empty() {
        css
    } else {
        format!(" style=\"{css}\"")
    }
}

/// A list's bullet or numbering style (Home › Bullets ▾ and Numbering ▾). A block
/// without one shows the desktop notebook's default for its level: • ◦ ▪ and 1. a. i., repeating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListMarker {
    Disc,
    Circle,
    Square,
    Dash,
    Arrow,
    Check,
    Diamond,
    Star,
    Decimal,
    DecimalParen,
    ParenDecimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

impl ListMarker {
    pub const BULLETS: [ListMarker; 8] = [
        ListMarker::Disc,
        ListMarker::Circle,
        ListMarker::Square,
        ListMarker::Dash,
        ListMarker::Arrow,
        ListMarker::Check,
        ListMarker::Diamond,
        ListMarker::Star,
    ];
    pub const NUMBERS: [ListMarker; 7] = [
        ListMarker::Decimal,
        ListMarker::DecimalParen,
        ListMarker::ParenDecimal,
        ListMarker::LowerAlpha,
        ListMarker::UpperAlpha,
        ListMarker::LowerRoman,
        ListMarker::UpperRoman,
    ];

    pub fn numbered(self) -> bool {
        Self::NUMBERS.contains(&self)
    }

    /// the desktop notebook's default for a level: • ◦ ▪ and 1. a. i., then round again.
    pub fn for_level(numbered: bool, level: u8) -> Self {
        match (numbered, level % 3) {
            (false, 0) => ListMarker::Disc,
            (false, 1) => ListMarker::Circle,
            (false, _) => ListMarker::Square,
            (true, 0) => ListMarker::Decimal,
            (true, 1) => ListMarker::LowerAlpha,
            (true, _) => ListMarker::LowerRoman,
        }
    }

    /// The marker in front of item `number` (from 1) of its list.
    pub fn text(self, number: usize) -> String {
        match self {
            ListMarker::Disc => "•".into(),
            ListMarker::Circle => "◦".into(),
            ListMarker::Square => "▪".into(),
            ListMarker::Dash => "–".into(),
            ListMarker::Arrow => "➢".into(),
            ListMarker::Check => "✓".into(),
            ListMarker::Diamond => "◆".into(),
            // "★" is in none of the note fonts; the desktop notebook's star bullet becomes a sun.
            ListMarker::Star => "☼".into(),
            ListMarker::Decimal => format!("{number}."),
            ListMarker::DecimalParen => format!("{number})"),
            ListMarker::ParenDecimal => format!("({number})"),
            ListMarker::LowerAlpha => format!("{}.", alpha(number)),
            ListMarker::UpperAlpha => format!("{}.", alpha(number).to_uppercase()),
            ListMarker::LowerRoman => format!("{}.", roman(number)),
            ListMarker::UpperRoman => format!("{}.", roman(number).to_uppercase()),
        }
    }

    /// The CSS `list-style-type` exports use.
    pub fn css(self) -> String {
        match self {
            ListMarker::Disc => "disc".into(),
            ListMarker::Circle => "circle".into(),
            ListMarker::Square => "square".into(),
            ListMarker::Decimal | ListMarker::DecimalParen | ListMarker::ParenDecimal => {
                "decimal".into()
            }
            ListMarker::LowerAlpha => "lower-alpha".into(),
            ListMarker::UpperAlpha => "upper-alpha".into(),
            ListMarker::LowerRoman => "lower-roman".into(),
            ListMarker::UpperRoman => "upper-roman".into(),
            other => format!("'{} '", other.text(1)),
        }
    }
}

/// Where a numbered paragraph's count starts (Restart Numbering, Customize Numbering's
/// Start at, Continue Numbering). `None` on the block: one more than the item before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListStart {
    /// Starts again at this number.
    At(u32),
    /// Carries on from the last number at its level, across what interrupted the list.
    Continue,
}

/// The number shown beside each numbered item (0 for other blocks). Each level counts on
/// its own and starts again under every paragraph above it; a paragraph that is not a
/// numbered item ends the count at its own level only, so a note under item 1 leaves
/// item 2 its number. A change of numbering style at a level starts a new count, as in
/// the desktop notebook ("A." typed right after an "a." list is A). A block's `start` restarts the
/// count or carries it on.
pub fn list_numbers(blocks: &[Block]) -> Vec<usize> {
    let mut counters = [0usize; MAX_LEVEL as usize + 1];
    // The numbering style of the count at each level.
    let mut styles = [None; MAX_LEVEL as usize + 1];
    // The last number given at each level, kept across interruptions (Continue).
    let mut last = [0usize; MAX_LEVEL as usize + 1];
    blocks
        .iter()
        .map(|block| {
            let depth = usize::from(block.indent.min(MAX_LEVEL));
            for deeper in &mut counters[depth + 1..] {
                *deeper = 0;
            }
            if matches!(block.kind, BlockKind::Numbered) {
                let style = block.list_marker();
                if styles[depth] != style {
                    counters[depth] = 0;
                    styles[depth] = style;
                }
                counters[depth] = match block.start {
                    Some(ListStart::At(n)) => n.max(1) as usize,
                    Some(ListStart::Continue) => last[depth] + 1,
                    None => counters[depth] + 1,
                };
                last[depth] = counters[depth];
                counters[depth]
            } else {
                counters[depth] = 0;
                0
            }
        })
        .collect()
}

/// 1 → a, 26 → z, 27 → aa, as list letters count.
fn alpha(number: usize) -> String {
    let mut n = number.max(1);
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    letters.iter().rev().collect()
}

/// Lower-case Roman numerals; past 3999 the number itself.
fn roman(number: usize) -> String {
    if number == 0 || number > 3999 {
        return number.to_string();
    }
    const PARTS: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = number;
    let mut out = String::new();
    for (value, part) in PARTS {
        while n >= value {
            out.push_str(part);
            n -= value;
        }
    }
    out
}

impl Block {
    pub fn new(kind: BlockKind) -> Self {
        Self {
            id: next_block_id(),
            kind,
            text: StyledText::default(),
            indent: 0,
            align: ParaAlign::Left,
            marker: None,
            start: None,
            spacing: ParaSpacing::default(),
            tags: Vec::new(),
            checked_tags: Vec::new(),
            custom_tags: Vec::new(),
            collapsed: false,
        }
    }

    /// A list item's marker: the one picked for it when it suits the list (a number style
    /// on a numbered item, a bullet style on a bulleted one), else the desktop notebook's default for
    /// its level, • ◦ ▪ and 1. a. i. `None` for anything but a list item.
    pub fn list_marker(&self) -> Option<ListMarker> {
        let numbered = match self.kind {
            BlockKind::Numbered => true,
            BlockKind::Bullet => false,
            _ => return None,
        };
        Some(
            self.marker
                .filter(|marker| marker.numbered() == numbered)
                .unwrap_or_else(|| ListMarker::for_level(numbered, self.indent)),
        )
    }

    pub fn paragraph(text: impl Into<String>) -> Self {
        Self {
            text: StyledText::new(text),
            ..Self::new(BlockKind::Paragraph)
        }
    }

    #[cfg(test)]
    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        Self {
            text: StyledText::new(text),
            ..Self::new(BlockKind::Heading { level })
        }
    }

    pub fn bullet(text: impl Into<String>) -> Self {
        Self {
            text: StyledText::new(text),
            ..Self::new(BlockKind::Bullet)
        }
    }

    pub fn todo(text: impl Into<String>, checked: bool) -> Self {
        Self {
            text: StyledText::new(text),
            ..Self::new(BlockKind::Todo { checked })
        }
    }

    pub fn checked(&self) -> bool {
        matches!(self.kind, BlockKind::Todo { checked: true })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    #[serde(default)]
    pub blocks: Vec<Block>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            blocks: vec![Block::paragraph("")],
        }
    }
}

impl Document {
    /// Parses a stored document, skipping blocks that cannot be read instead of
    /// discarding the whole page. The second value is `true` when anything was skipped.
    pub fn from_json_lossy(json: &str) -> (Self, bool) {
        if json.trim().is_empty() {
            return (Self::default(), false);
        }
        let (mut doc, lossy) = match serde_json::from_str::<Document>(json) {
            Ok(doc) => (doc, false),
            Err(_) => match serde_json::from_str(json) {
                Ok(serde_json::Value::Object(mut value)) => (
                    Document {
                        blocks: parse_each(value.remove("blocks")),
                    },
                    true,
                ),
                _ => (Self::default(), true),
            },
        };
        doc.repair();
        (doc, lossy)
    }

    #[cfg(test)]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Gives the recordings of `attachment` in this note their length. Returns whether the
    /// note holds one.
    pub fn set_clip_length(&mut self, attachment: &str, length_ms: u64) -> bool {
        let mut found = false;
        for block in &mut self.blocks {
            if let BlockKind::Audio {
                attachment: clip,
                duration_ms,
            } = &mut block.kind
            {
                if clip == attachment {
                    *duration_ms = length_ms;
                    found = true;
                }
            }
        }
        found
    }

    pub fn repair(&mut self) {
        // An equation left blank is not kept (as a blank equation item is not).
        self.blocks.retain(|block| {
            !matches!(&block.kind, BlockKind::Equation { source, .. } if source.trim().is_empty())
        });
        if self.blocks.is_empty() {
            self.blocks.push(Block::paragraph(""));
        }
        let max = self.blocks.iter().map(|b| b.id).max().unwrap_or(0);
        seed_block_ids(max.saturating_add(1));
        let mut seen = HashSet::new();
        for block in &mut self.blocks {
            if block.id == 0 || !seen.insert(block.id) {
                block.id = next_block_id();
                seen.insert(block.id);
            }
            block.indent = block.indent.min(MAX_LEVEL);
            if !matches!(block.kind, BlockKind::Numbered) {
                block.start = None;
            }
            block.spacing.repair();
            if let BlockKind::Equation { scale, .. } = &mut block.kind {
                *scale = finite_or(*scale, default_equation_size()).clamp(8.0, 160.0);
            }
            block.text.normalize();
            if let BlockKind::Table(table) = &mut block.kind {
                table.repair();
            }
            if block.kind.is_textual() {
                block.tags.sort_by_key(|tag| tag.rank());
                block.tags.dedup();
                let tags = &block.tags;
                block
                    .checked_tags
                    .retain(|tag| tag.checkable() && tags.contains(tag));
                block.checked_tags.sort_by_key(|tag| tag.rank());
                block.checked_tags.dedup();
                block.custom_tags.retain(|tag| !tag.name.trim().is_empty());
            } else {
                block.tags.clear();
                block.checked_tags.clear();
                block.custom_tags.clear();
            }
        }
    }

    pub fn plain_text(&self) -> String {
        let mut parts = Vec::new();
        for block in &self.blocks {
            match &block.kind {
                BlockKind::Divider
                | BlockKind::Image { .. }
                | BlockKind::Drawing { .. }
                | BlockKind::Audio { .. } => {}
                BlockKind::File { name, .. } => {
                    if !name.trim().is_empty() {
                        parts.push(name.trim().to_string());
                    }
                }
                BlockKind::Equation { source, .. } => {
                    if !source.trim().is_empty() {
                        parts.push(source.trim().to_string());
                    }
                }
                BlockKind::Table(table) => {
                    let text = table.plain_text();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
                _ => {
                    if !block.text.text.trim().is_empty() {
                        parts.push(block.text.text.trim_end().to_string());
                    }
                }
            }
        }
        parts.join("\n")
    }

    #[cfg(test)]
    pub fn word_count(&self) -> i64 {
        self.plain_text().split_whitespace().count() as i64
    }

    pub fn block_index(&self, id: u64) -> Option<usize> {
        self.blocks.iter().position(|block| block.id == id)
    }

    pub fn to_html(&self) -> String {
        let mut out = String::new();
        let mut list_open = false;
        let mut numbered = false;
        // Each item says its number, so restarts and style changes export as shown.
        let numbers = list_numbers(&self.blocks);
        for (block, number) in self.blocks.iter().zip(numbers) {
            match &block.kind {
                BlockKind::Bullet | BlockKind::Todo { .. } => {
                    if list_open && numbered {
                        out.push_str("</ol>");
                        list_open = false;
                    }
                    if !list_open {
                        out.push_str("<ul>");
                        list_open = true;
                        numbered = false;
                    }
                }
                BlockKind::Numbered => {
                    if list_open && !numbered {
                        out.push_str("</ul>");
                        list_open = false;
                    }
                    if !list_open {
                        out.push_str("<ol>");
                        list_open = true;
                        numbered = true;
                    }
                }
                _ => {
                    if list_open {
                        out.push_str(if numbered { "</ol>" } else { "</ul>" });
                        list_open = false;
                    }
                }
            }
            let attrs = block_attrs(block);
            match &block.kind {
                BlockKind::Paragraph => {
                    out.push_str(&format!("<p{attrs}>{}</p>", tagged_html(block)))
                }
                BlockKind::Heading { level } => {
                    let level = (*level).clamp(1, 6);
                    out.push_str(&format!(
                        "<h{level}{attrs}>{}</h{level}>",
                        tagged_html(block)
                    ));
                }
                BlockKind::Bullet => {
                    out.push_str(&format!("<li{attrs}>{}</li>", tagged_html(block)))
                }
                BlockKind::Numbered => out.push_str(&format!(
                    "<li value=\"{number}\"{attrs}>{}</li>",
                    tagged_html(block)
                )),
                // Its box hangs left of the text with its tags (`tagged_html`).
                BlockKind::Todo { .. } => {
                    out.push_str(&format!("<li{attrs}>{}</li>", tagged_html(block)))
                }
                BlockKind::Quote => out.push_str(&format!(
                    "<blockquote{attrs}>{}</blockquote>",
                    tagged_html(block)
                )),
                BlockKind::Citation => out.push_str(&format!(
                    "<p{attrs}><small>{}</small></p>",
                    tagged_html(block)
                )),
                BlockKind::Code => {
                    out.push_str(&format!("<pre><code>{}</code></pre>", tagged_html(block)))
                }
                BlockKind::Divider => out.push_str("<hr>"),
                BlockKind::File {
                    attachment,
                    name,
                    size,
                } => {
                    // export.rs turns `data-file` into a download link when the file
                    // travels with the page.
                    let name = crate::util::html_escape(name);
                    out.push_str(&format!(
                        "<p{attrs}><a class=\"attachment\" data-file=\"{}\" download=\"{name}\">📎 {name} <span class=\"file-size\">{}</span></a></p>",
                        crate::util::html_escape(attachment),
                        crate::util::file_size(*size)
                    ));
                }
                BlockKind::Equation { source, scale } => out.push_str(&format!(
                    "<div class=\"equation\" style=\"font-size:{scale:.1}px;\">{}</div>",
                    crate::math::mathml_of(source)
                )),
                BlockKind::Audio {
                    attachment,
                    duration_ms,
                } => {
                    // export.rs gives `data-audio` a player when the clip travels with the page.
                    let label = t!(
                        "data-export-audio",
                        time = crate::audio::format_time(*duration_ms)
                    );
                    out.push_str(&format!(
                        "<p{attrs} class=\"audio\" data-audio=\"{}\">🔊 {}</p>",
                        crate::util::html_escape(attachment),
                        crate::util::html_escape(&label)
                    ));
                }
                BlockKind::Table(table) => out.push_str(&table.to_html()),
                BlockKind::Image {
                    attachment, meta, ..
                } => {
                    out.push_str(&format!(
                        "<figure><img src=\"{attachment}\" alt=\"{}\"{}><figcaption>{}</figcaption></figure>",
                        crate::util::html_escape(&meta.alt),
                        picture_transform(meta),
                        block.text.to_html()
                    ));
                }
                BlockKind::Drawing { attachment, .. } => {
                    out.push_str(&format!("<figure><img src=\"{attachment}\" alt=\"\"><figcaption>{}</figcaption></figure>", block.text.to_html()));
                }
            }
        }
        if list_open {
            out.push_str(if numbered { "</ol>" } else { "</ul>" });
        }
        out
    }
}

/// Walks a document's blocks in order and says which ones a collapsed paragraph above
/// hides. It allocates nothing, because notes are drawn every frame.
#[derive(Debug, Default)]
pub struct Folds {
    below: Option<u8>,
}

impl Folds {
    pub fn hides(&mut self, doc: &Document, index: usize) -> bool {
        let block = &doc.blocks[index];
        if let Some(level) = self.below {
            if block.indent > level {
                return true;
            }
            self.below = None;
        }
        if block.collapsed && doc.has_children(index) {
            self.below = Some(block.indent);
        }
        false
    }
}

impl Document {
    /// The paragraph at `index` with its subparagraphs: the blocks right after it at a
    /// deeper level. Moving, folding and selecting by its handle act on all of them.
    pub fn family(&self, index: usize) -> Range<usize> {
        let level = self.blocks[index].indent;
        let end = self.blocks[index + 1..]
            .iter()
            .position(|block| block.indent <= level)
            .map_or(self.blocks.len(), |offset| index + 1 + offset);
        index..end
    }

    pub fn has_children(&self, index: usize) -> bool {
        self.blocks
            .get(index + 1)
            .is_some_and(|next| next.indent > self.blocks[index].indent)
    }

    /// Which blocks a collapsed paragraph above hides (see [`Folds`] for the hot path).
    pub fn hidden_blocks(&self) -> Vec<bool> {
        let mut folds = Folds::default();
        (0..self.blocks.len())
            .map(|index| folds.hides(self, index))
            .collect()
    }

    /// Unfolds every paragraph above `index` that hides it. Returns whether any unfolded.
    pub fn expand_to(&mut self, index: usize) -> bool {
        let mut level = self.blocks[index].indent;
        let mut changed = false;
        for above in (0..index).rev() {
            if level == 0 {
                break;
            }
            if self.blocks[above].indent < level {
                level = self.blocks[above].indent;
                changed |= std::mem::take(&mut self.blocks[above].collapsed);
            }
        }
        changed
    }

    /// the desktop notebook's "show levels 1 to n" (Alt+Shift+1…9): what sits deeper than `levels`
    /// folds away under its parents. 0 unfolds everything (Alt+Shift+0).
    pub fn show_levels(&mut self, levels: u8) -> bool {
        let mut changed = false;
        for index in 0..self.blocks.len() {
            let fold =
                levels > 0 && self.blocks[index].indent + 1 >= levels && self.has_children(index);
            if self.blocks[index].collapsed != fold {
                self.blocks[index].collapsed = fold;
                changed = true;
            }
        }
        changed
    }

    /// Tab / Shift+Tab: every block of `span` one level deeper or shallower, so
    /// subparagraphs keep their place under their paragraph. Nothing happens when a block
    /// would leave 0..=MAX_LEVEL.
    pub fn shift_blocks(&mut self, span: Range<usize>, deeper: bool) -> bool {
        let blocks = &mut self.blocks[span];
        if blocks.is_empty() {
            return false;
        }
        if deeper {
            if blocks.iter().any(|block| block.indent >= MAX_LEVEL) {
                return false;
            }
            blocks.iter_mut().for_each(|block| block.indent += 1);
        } else {
            if blocks.iter().any(|block| block.indent == 0) {
                return false;
            }
            blocks.iter_mut().for_each(|block| block.indent -= 1);
        }
        true
    }

    /// Alt+Shift+↑/↓: moves the blocks of `span` (a family, or several) above the previous
    /// paragraph at their level (or above their parent), or below the next one with its
    /// subparagraphs. Returns where they now start.
    pub fn move_blocks(&mut self, span: Range<usize>, up: bool) -> Option<usize> {
        let level = self.blocks.get(span.start)?.indent;
        let target = if up {
            let mut above = span.start.checked_sub(1)?;
            while above > 0 && self.blocks[above].indent > level {
                above -= 1;
            }
            above
        } else {
            let next = span.end;
            if next >= self.blocks.len() {
                return None;
            }
            if self.blocks[next].indent == level {
                self.family(next).end
            } else {
                next + 1
            }
        };
        Some(self.move_range_to(span, target))
    }

    /// Moves the paragraph at `index` with its subparagraphs to just before block `before`
    /// (the end when `None`), as a dragged paragraph handle does. Levels stay as they are.
    pub fn move_family_before(&mut self, index: usize, before: Option<usize>) -> Option<usize> {
        let family = self.family(index);
        let target = before.unwrap_or(self.blocks.len());
        if (family.start..=family.end).contains(&target) {
            return None;
        }
        Some(self.move_range_to(family, target))
    }

    /// Moves `range` so it starts where index `target` (counted before the move) was.
    fn move_range_to(&mut self, range: Range<usize>, target: usize) -> usize {
        let moved: Vec<Block> = self.blocks.drain(range.clone()).collect();
        let at = if target > range.start {
            target - moved.len()
        } else {
            target
        };
        self.blocks.splice(at..at, moved);
        at
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn display_title(title: &str) -> Cow<'_, str> {
    if title.trim().is_empty() {
        t!("common-untitled")
    } else {
        Cow::Borrowed(title)
    }
}

/// Links to other pages look like `folio://page/<page id>`.
pub const PAGE_LINK_PREFIX: &str = "folio://page/";

pub fn page_link(page_id: &str) -> String {
    format!("{PAGE_LINK_PREFIX}{page_id}")
}

/// A link to one paragraph of a page (Copy Link to Paragraph).
pub fn paragraph_link(page_id: &str, block: u64) -> String {
    format!("{PAGE_LINK_PREFIX}{page_id}#p{block}")
}

/// A page link's page id and, for a paragraph link, the paragraph's block id.
pub fn parse_page_link(url: &str) -> Option<(&str, Option<u64>)> {
    let rest = url.trim().strip_prefix(PAGE_LINK_PREFIX)?;
    match rest.split_once("#p") {
        Some((page, block)) => Some((page, block.parse().ok())),
        None => Some((rest, None)),
    }
}

/// Links to sections (a section's Copy Link to Section) look like
/// `folio://section/<section id>`.
pub const SECTION_LINK_PREFIX: &str = "folio://section/";

pub fn section_link(section_id: &str) -> String {
    format!("{SECTION_LINK_PREFIX}{section_id}")
}

/// Turns what someone typed in the link dialog into a URL: bare addresses become
/// `https://`, bare email addresses `mailto:`. Empty means "no link".
pub fn normalize_link(address: &str) -> Option<String> {
    let address = address.trim();
    if address.is_empty() {
        return None;
    }
    let lower = address.to_lowercase();
    if [
        "http://",
        "https://",
        "mailto:",
        "file:",
        PAGE_LINK_PREFIX,
        SECTION_LINK_PREFIX,
    ]
    .iter()
    .any(|scheme| lower.starts_with(scheme))
    {
        return Some(address.to_string());
    }
    if let Some(url) = file_url(address) {
        return Some(url);
    }
    if address.contains('@') && !address.contains('/') && !address.contains(' ') {
        return Some(format!("mailto:{address}"));
    }
    Some(format!("https://{address}"))
}

/// A local path typed as a link address (`/home/me/a.pdf`, `~/a.pdf`, `C:\docs\a.txt`)
/// as a `file:` URL, or `None` if `address` is not an absolute path.
fn file_url(address: &str) -> Option<String> {
    let bytes = address.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let path = if let Some(rest) = address
        .strip_prefix("~/")
        .or_else(|| address.strip_prefix("~\\"))
    {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        format!("{}/{rest}", home.trim_end_matches(['/', '\\']))
    } else if (address.starts_with('/') && !address.starts_with("//")) || drive {
        address.to_string()
    } else {
        return None;
    };
    let path = path.replace('\\', "/");
    let mut url = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(url)
}

/// Links to a page or a section of the library, which Folio follows itself.
fn is_internal_link(url: &str) -> bool {
    url.starts_with(PAGE_LINK_PREFIX) || url.starts_with(SECTION_LINK_PREFIX)
}

/// Links Folio hands to the system: web pages, email, and local files.
pub fn is_openable_link(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    ["http://", "https://", "mailto:", "file:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// The URL for a typed web address, or `None` if `word` is not one.
fn web_address(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    for scheme in ["http://", "https://", PAGE_LINK_PREFIX, SECTION_LINK_PREFIX] {
        if lower.starts_with(scheme) && lower.len() > scheme.len() {
            return Some(word.to_string());
        }
    }
    if lower.starts_with("www.") && lower[4..].contains('.') && !lower.ends_with('.') {
        return Some(format!("https://{word}"));
    }
    None
}

#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub database_path: String,
    pub database_bytes: u64,
    pub attachment_bytes: u64,
    pub page_count: i64,
    pub notebook_count: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printout_page_keeps_its_source_and_text() {
        let old: CanvasKind = serde_json::from_str(r#"{"type":"image","attachment":"a"}"#).unwrap();
        assert_eq!(old, CanvasKind::image("a"));
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"type":"image","attachment":"a"}"#
        );
        let mut page = CanvasKind::image("p3");
        if let CanvasKind::Image { printout, .. } = &mut page {
            *printout = Some(Box::new(PrintoutPage {
                source: "pdf".into(),
                page: 3,
                text: "Fractions".into(),
                size: None,
            }));
        }
        let json = serde_json::to_string(&page).unwrap();
        assert!(
            json.contains(r#""printout":{"source":"pdf","page":3,"text":"Fractions"}"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<CanvasKind>(&json).unwrap(), page);
        assert_eq!(
            page.plain_text(),
            "Fractions",
            "search finds a printout's words"
        );
        assert_eq!(page.printout().map(|p| p.page), Some(3));
        assert_eq!(page.attachment(), Some("p3"));
    }

    #[test]
    fn styles_use_notebooks_names_and_keep_stored_headings() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let names: Vec<String> = BlockKind::styles()
            .iter()
            .map(|kind| kind.label().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "Heading 1",
                "Heading 2",
                "Heading 3",
                "Heading 4",
                "Heading 5",
                "Heading 6",
                "Page Title",
                "Citation",
                "Quote",
                "Code",
                "Normal"
            ]
        );
        // Saved headings read as before: level 2 is still the first heading.
        assert_eq!(BlockKind::heading(1), BlockKind::Heading { level: 2 });
        let json = r#"{"blocks":[{"id":1,"kind":{"type":"citation"}}]}"#;
        assert_eq!(
            Document::from_json_lossy(json).0.blocks[0].kind,
            BlockKind::Citation
        );
    }

    #[test]
    fn page_styles_round_trip_and_read_the_old_paper_codes() {
        let every_rule = std::iter::once(RuleLines::None)
            .chain(RuleLines::LINES)
            .chain(RuleLines::GRIDS);
        for rule in every_rule {
            assert_eq!(RuleLines::from_code(rule.code()), rule);
            assert_eq!(rule.spacing().is_none(), rule == RuleLines::None);
        }
        for color in PageColor::ALL {
            assert_eq!(PageColor::from_code(&color.code()), Some(color));
        }
        // The paper styles of earlier versions.
        assert_eq!(
            RuleLines::from_code("ruled"),
            RuleLines::Lines(RuleSize::College)
        );
        assert_eq!(
            RuleLines::from_code("grid"),
            RuleLines::Grid(GridSize::Medium)
        );
        assert_eq!(RuleLines::from_code("dots"), RuleLines::None);
        assert_eq!(RuleLines::from_code("plain"), RuleLines::None);
        let style = PageStyle {
            rule: RuleLines::Grid(GridSize::Large),
            color: Some(PageColor::LemonLime),
            rule_color: None,
            title_hidden: true,
        };
        assert_eq!(style.code(), "grid-large;color=lemon_lime;notitle");
        assert_eq!(PageStyle::from_code(&style.code()), style);
        // More Colors… and Rule Line Color keep any colour.
        let custom = PageStyle {
            rule: RuleLines::Lines(RuleSize::College),
            color: Some(PageColor::Custom(Rgb(0xfa, 0xf0, 0xe6))),
            rule_color: Some(Rgb(0x10, 0x7c, 0x10)),
            title_hidden: false,
        };
        assert_eq!(custom.code(), "lines-college;color=#faf0e6;rules=#107c10");
        assert_eq!(PageStyle::from_code(&custom.code()), custom);
        assert_eq!(Rgb::from_hex("#10"), None);
        assert_eq!(Rgb::from_hex("#gg0000"), None);
        // A custom page colour in a dark theme keeps its hue, deep enough for light text.
        let deep = PageColor::Custom(Rgb(0xff, 0x00, 0x00)).dark();
        assert!(deep.0 > deep.1 && deep.0 < 0x40, "{deep:?}");
        let deep = PageColor::Custom(PageColor::Blue.light()).dark();
        assert!(deep.2 > deep.0 && deep.2 < 0x40, "{deep:?}");
        assert_eq!(
            PageStyle::from_code("ruled"),
            PageStyle {
                rule: RuleLines::Lines(RuleSize::College),
                ..PageStyle::default()
            }
        );
        // Unknown parts are ignored rather than failing the page.
        assert_eq!(
            PageStyle::from_code("none;color=plaid;sparkles"),
            PageStyle::default()
        );
    }

    #[test]
    fn style_survives_typing() {
        let mut text = StyledText::new("Hello");
        text.set_bold(0..5, true);
        text.sync_after_text_change("Hello world", InlineStyle::default().with_bold());
        assert!(text.style_at(0).bold);
        assert!(text.style_at(8).bold);
        assert_eq!(text.char_count(), 11);
    }

    #[test]
    fn a_retired_archivo_mark_reads_back_as_the_interface_face() {
        let face: FontFace = serde_json::from_str("\"display\"").unwrap();
        assert_eq!(face, FontFace::Sans);
        assert_eq!(serde_json::to_string(&FontFace::Body).unwrap(), "\"body\"");
    }

    #[test]
    fn deleting_text_shifts_runs() {
        let mut text = StyledText {
            text: "Hello world".into(),
            runs: vec![StyleRun {
                start: 6,
                end: 11,
                style: InlineStyle {
                    italic: true,
                    ..Default::default()
                },
            }],
        };
        text.sync_after_text_change("Hello", InlineStyle::default());
        assert_eq!(text.runs.len(), 0);
        assert_eq!(text.text, "Hello");
    }

    #[test]
    fn typing_continues_the_style_before_the_caret() {
        let mut text = StyledText::new("one");
        text.set_bold(0..3, true);
        assert!(
            text.style_for_caret(3).bold,
            "end of a bold word stays bold"
        );
        assert!(
            text.style_for_caret(0).bold,
            "start takes the first char's style"
        );
        let mut plain_then_bold = StyledText::new("see me");
        plain_then_bold.set_bold(4..6, true);
        assert!(!plain_then_bold.style_for_caret(4).bold);
        assert!(StyledText::default().style_for_caret(0).is_plain());
    }

    #[test]
    fn caret_disambiguates_repeated_characters() {
        // Typing a plain "m" in front of a bold "me" must not split the bold word.
        let mut text = StyledText::new("see me");
        text.set_bold(4..6, true);
        text.sync_after_edit("see mme", InlineStyle::default(), Some(5));
        let bold: Vec<bool> = text.char_styles().iter().map(|s| s.bold).collect();
        assert_eq!(bold, [false, false, false, false, false, true, true]);

        // Deleting the first of two identical chars removes that char's style.
        let mut text = StyledText::new("aab");
        text.set_bold(0..1, true);
        text.sync_after_edit("ab", InlineStyle::default(), Some(0));
        assert!(text.runs.is_empty());
    }

    #[test]
    fn unreadable_canvas_items_are_skipped_not_wiped() {
        let json = r#"{"items":[
            {"id":1,"x":10,"y":20,"width":200,"height":40,"kind":{"type":"text","doc":{"blocks":[{"id":5,"kind":{"type":"paragraph"},"text":{"text":"keep me"}}]}}},
            {"id":2,"x":0,"y":0,"width":10,"height":10,"kind":{"type":"hologram"}}
        ],"ink":[{"points":[[1,2]],"color":[0,0,0],"width":2}]}"#;
        let (canvas, lossy) = CanvasDoc::from_json_lossy(json);
        assert!(lossy);
        assert_eq!(canvas.items.len(), 1);
        assert_eq!(canvas.plain_text(), "keep me");
        assert_eq!(canvas.ink.len(), 1);

        let (doc, lossy) = Document::from_json_lossy(
            r#"{"blocks":[{"kind":{"type":"quote"},"text":{"text":"ok"}},{"kind":{"type":"marquee"}}]}"#,
        );
        assert!(lossy);
        assert_eq!(doc.plain_text(), "ok");
    }

    #[test]
    fn repair_keeps_lines_thin() {
        let mut canvas = CanvasDoc::default();
        canvas.add(CanvasItem::new(
            CanvasKind::Shape {
                shape: ShapeKind::Line,
                stroke: Rgb::INKS[0],
                fill: None,
                label: String::new(),
                flip_x: false,
                flip_y: false,
                thickness: None,
            },
            0.0,
            0.0,
            200.0,
            0.0,
        ));
        canvas.repair();
        assert_eq!(canvas.items[0].height, 0.0);
    }

    #[test]
    fn repair_brings_strays_back_onto_the_page() {
        let mut canvas = CanvasDoc::default();
        let mut note = CanvasItem::new(
            CanvasKind::Text {
                doc: Document::default(),
            },
            30.0,
            100.0,
            200.0,
            40.0,
        );
        canvas.add(note.clone());
        // An item that strayed above the page (an older build, a nudge) comes back below
        // the room its grab bar needs; one added there is placed there too.
        note.id = next_block_id();
        note.y = -40.0;
        note.x = -8.0;
        canvas.items.push(note.clone());
        note.id = next_block_id();
        let added = canvas.add(note);
        // A stroke left of and above the page comes back whole, keeping its shape.
        canvas.ink.push(InkStroke {
            points: vec![[-10.0, 5.0], [20.0, -4.0], [30.0, 30.0]],
            color: Rgb::INKS[0],
            width: 2.0,
            highlighter: false,
        });
        canvas.ink.push(InkStroke {
            points: vec![[3.0, 4.0], [9.0, 12.0]],
            color: Rgb::INKS[0],
            width: 2.0,
            highlighter: false,
        });
        canvas.repair();
        assert_eq!(canvas.items[0].rect().min, egui::pos2(30.0, 100.0));
        assert_eq!(canvas.items[1].rect().min, egui::pos2(0.0, ITEM_TOP));
        assert_eq!(
            canvas.item(added).unwrap().rect().min,
            egui::pos2(0.0, ITEM_TOP)
        );
        assert_eq!(
            canvas.ink[0].points,
            vec![[0.0, 9.0], [30.0, 0.0], [40.0, 34.0]]
        );
        assert_eq!(canvas.ink[1].points, vec![[3.0, 4.0], [9.0, 12.0]]);
    }

    #[test]
    fn shape_thickness_is_optional_and_repaired() {
        let shape = |thickness| {
            CanvasItem::new(
                CanvasKind::Shape {
                    shape: ShapeKind::Ellipse,
                    stroke: Rgb::INKS[0],
                    fill: None,
                    label: String::new(),
                    flip_x: false,
                    flip_y: false,
                    thickness,
                },
                0.0,
                0.0,
                100.0,
                60.0,
            )
        };
        let mut canvas = CanvasDoc::default();
        canvas.add(shape(None));
        canvas.add(shape(Some(8.0)));
        let json = canvas.to_json();
        assert_eq!(json.matches("thickness").count(), 1, "{json}");
        let (back, lossy) = CanvasDoc::from_json_lossy(&json);
        assert!(!lossy);
        assert_eq!(back, canvas);
        // Pages saved before shapes had a thickness still load, with the thin default.
        let old = json.replacen(",\"thickness\":8.0", "", 1);
        assert_ne!(old, json);
        let (back, lossy) = CanvasDoc::from_json_lossy(&old);
        assert!(!lossy);
        assert!(back.items.iter().all(|item| matches!(
            item.kind,
            CanvasKind::Shape {
                thickness: None,
                ..
            }
        )));
        let mut wild = CanvasDoc::default();
        wild.add(shape(Some(f32::NAN)));
        wild.add(shape(Some(500.0)));
        wild.repair();
        let widths: Vec<_> = wild
            .items
            .iter()
            .map(|item| match item.kind {
                CanvasKind::Shape { thickness, .. } => thickness,
                _ => None,
            })
            .collect();
        assert_eq!(widths, vec![Some(SHAPE_LINE), Some(48.0)]);
    }

    #[test]
    fn apply_style_to_chars() {
        let mut text = StyledText::new("one two");
        text.set_bold(0..3, true);
        assert!(text.style_at(0).bold);
        assert!(!text.style_at(4).bold);
        assert_eq!(text.runs.len(), 1);
    }

    #[test]
    fn document_plain_text_skips_media() {
        let doc = Document {
            blocks: vec![
                Block::heading(1, "Title"),
                Block::paragraph("A sentence."),
                Block::new(BlockKind::Divider),
                Block::new(BlockKind::Image {
                    attachment: "x.png".into(),
                    width: 1.0,
                    meta: PictureMeta::default(),
                }),
            ],
        };
        assert_eq!(doc.plain_text(), "Title\nA sentence.");
        assert_eq!(doc.word_count(), 3);
    }

    #[test]
    fn new_formats_round_trip_and_stay_compact() {
        let style = InlineStyle {
            font: Some(FontFace::Readable),
            size: Some(14),
            superscript: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&style).unwrap();
        assert_eq!(json, r#"{"font":"readable","size":14,"superscript":true}"#);
        assert_eq!(serde_json::from_str::<InlineStyle>(&json).unwrap(), style);
        assert_eq!(
            serde_json::to_string(&InlineStyle::default()).unwrap(),
            "{}"
        );
        assert!(!serde_json::to_string(&Block::paragraph("Hi"))
            .unwrap()
            .contains("align"));
        let old: Block =
            serde_json::from_str(r#"{"id":5,"kind":{"type":"paragraph"},"text":{"text":"x"}}"#)
                .unwrap();
        assert_eq!(old.align, ParaAlign::Left);
    }

    #[test]
    fn html_carries_fonts_scripts_alignment_and_indent() {
        let mut text = StyledText::new("E=mc2");
        text.apply_style_chars(4..5, |style| style.superscript = true);
        text.apply_style_chars(0..1, |style| {
            style.font = Some(FontFace::Hand);
            style.size = Some(20);
        });
        let html = text.to_html();
        assert!(html.contains("<sup>2</sup>"), "{html}");
        assert!(
            html.contains("font-family:'Patrick Hand'") && html.contains("font-size:20pt"),
            "{html}"
        );
        let mut block = Block::paragraph("Centered");
        block.align = ParaAlign::Center;
        block.indent = 2;
        let html = Document {
            blocks: vec![block],
        }
        .to_html();
        assert!(
            html.contains(r#"<p style="text-align:center;margin-left:3em;">"#),
            "{html}"
        );
    }

    #[test]
    fn html_list_markers_follow_aligned_text() {
        let item = |kind: BlockKind, align: ParaAlign| {
            let mut block = Block::paragraph("Milk");
            block.kind = kind;
            block.align = align;
            block
        };
        let html = Document {
            blocks: vec![
                item(BlockKind::Bullet, ParaAlign::Center),
                item(BlockKind::Bullet, ParaAlign::Left),
                item(BlockKind::Numbered, ParaAlign::Right),
                item(BlockKind::Todo { checked: false }, ParaAlign::Center),
                item(BlockKind::Paragraph, ParaAlign::Center),
            ],
        }
        .to_html();
        let inside = r#"<li style="text-align:center;list-style-position:inside;">Milk</li>"#;
        assert!(html.contains(inside), "{html}");
        assert!(
            html.contains("<li>Milk</li>"),
            "left-aligned items keep it outside: {html}"
        );
        assert!(
            html.contains(
                r#"<li value="1" style="text-align:right;list-style-position:inside;">Milk"#
            ),
            "{html}"
        );
        assert!(
            html.contains(
                r#"<li style="text-align:center;list-style-position:inside;"><span class="tags" style="margin-left:-18px;width:18px"><input type="checkbox""#
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"<p style="text-align:center;">Milk</p>"#),
            "{html}"
        );
    }

    #[test]
    fn spacing_round_trips_and_exports() {
        let spacing = ParaSpacing {
            before: 6.0,
            after: 12.5,
            line_at_least: 18.0,
        };
        let block = Block {
            spacing,
            ..Block::paragraph("Spaced")
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(
            json.contains(r#""spacing":{"before":6.0,"after":12.5,"lineAtLeast":18.0}"#),
            "{json}"
        );
        assert_eq!(
            serde_json::from_str::<Block>(&json).unwrap().spacing,
            spacing
        );
        // A block without spacing saves as before, and old blocks read as none.
        let plain = serde_json::to_string(&Block::paragraph("Plain")).unwrap();
        assert!(!plain.contains("spacing"), "{plain}");
        let old: Block = serde_json::from_str(r#"{"id":7,"kind":{"type":"paragraph"}}"#).unwrap();
        assert!(old.spacing.is_none());
        let partial: Block =
            serde_json::from_str(r#"{"id":8,"kind":{"type":"paragraph"},"spacing":{"after":3.0}}"#)
                .unwrap();
        assert_eq!(partial.spacing.after, 3.0);
        assert_eq!(partial.spacing.before, 0.0);

        // Repair keeps each value a finite number of points from 0 to 200.
        let mut doc = Document {
            blocks: vec![Block {
                spacing: ParaSpacing {
                    before: f32::NAN,
                    after: 900.0,
                    line_at_least: -4.0,
                },
                ..Block::paragraph("Wild")
            }],
        };
        doc.repair();
        assert_eq!(
            doc.blocks[0].spacing,
            ParaSpacing {
                before: 0.0,
                after: MAX_PARA_SPACING,
                line_at_least: 0.0,
            }
        );

        let html = Document {
            blocks: vec![block, Block::paragraph("Plain")],
        }
        .to_html();
        assert!(
            html.contains(
                r#"<p style="margin-top:6pt;margin-bottom:12.5pt;line-height:max(18px,18pt);">Spaced</p>"#
            ),
            "{html}"
        );
        assert!(html.contains("<p>Plain</p>"), "{html}");
    }

    #[test]
    fn restart_and_continue_number_lists() {
        let numbered = |start: Option<ListStart>| Block {
            start,
            ..Block::new(BlockKind::Numbered)
        };
        let blocks = vec![
            numbered(None),
            numbered(None),
            Block::paragraph("break"),
            numbered(Some(ListStart::Continue)),
            numbered(None),
            numbered(Some(ListStart::At(10))),
            numbered(None),
        ];
        assert_eq!(list_numbers(&blocks), [1, 2, 0, 3, 4, 10, 11]);
        for start in [ListStart::At(3), ListStart::Continue] {
            let json = serde_json::to_string(&start).unwrap();
            assert_eq!(serde_json::from_str::<ListStart>(&json).unwrap(), start);
        }
        // Old blocks have no start, and a block without one saves as before.
        let plain = serde_json::to_string(&Block::new(BlockKind::Numbered)).unwrap();
        assert!(!plain.contains("start"), "{plain}");
        let html = Document { blocks }.to_html();
        assert!(html.contains("<li value=\"10\">"), "{html}");
        assert!(html.contains("<li value=\"3\">"), "{html}");

        // Repair keeps a start on numbered items only.
        let mut doc = Document {
            blocks: vec![Block {
                start: Some(ListStart::At(4)),
                ..Block::paragraph("plain")
            }],
        };
        doc.repair();
        assert_eq!(doc.blocks[0].start, None);
    }

    #[test]
    fn a_new_numbering_style_starts_a_new_count() {
        let item = |marker: ListMarker| Block {
            marker: Some(marker),
            ..Block::new(BlockKind::Numbered)
        };
        // "a. apple", "banana", then "A. Alpha" and "i. one" typed right after them.
        let blocks = vec![
            item(ListMarker::LowerAlpha),
            item(ListMarker::LowerAlpha),
            item(ListMarker::UpperAlpha),
            item(ListMarker::LowerRoman),
            item(ListMarker::LowerRoman),
        ];
        assert_eq!(list_numbers(&blocks), [1, 2, 1, 1, 2]);
        // A list of one style keeps its count, its default marker and a picked one alike.
        let blocks = vec![
            Block::new(BlockKind::Numbered),
            item(ListMarker::Decimal),
            Block::new(BlockKind::Numbered),
        ];
        assert_eq!(list_numbers(&blocks), [1, 2, 3]);
        // Continue carries the last number on, whatever the style.
        let blocks = vec![
            item(ListMarker::Decimal),
            item(ListMarker::Decimal),
            Block::paragraph("break"),
            Block {
                start: Some(ListStart::Continue),
                ..item(ListMarker::UpperRoman)
            },
        ];
        assert_eq!(list_numbers(&blocks), [1, 2, 0, 3]);
    }

    #[test]
    fn list_markers_count_in_letters_and_roman_numerals() {
        assert_eq!(alpha(1), "a");
        assert_eq!(alpha(26), "z");
        assert_eq!(alpha(27), "aa");
        assert_eq!(roman(4), "iv");
        assert_eq!(roman(1990), "mcmxc");
        assert_eq!(ListMarker::UpperRoman.text(9), "IX.");
        assert_eq!(ListMarker::ParenDecimal.text(3), "(3)");
        assert_eq!(ListMarker::for_level(true, 4), ListMarker::LowerAlpha);
    }

    #[test]
    fn list_markers_survive_serde_and_export() {
        let mut numbered = Block::new(BlockKind::Numbered);
        numbered.text = StyledText::new("Mitosis");
        numbered.marker = Some(ListMarker::LowerRoman);
        let json = serde_json::to_string(&numbered).unwrap();
        assert!(json.contains(r#""marker":"lower_roman""#), "{json}");
        let back: Block = serde_json::from_str(&json).unwrap();
        assert_eq!(back, numbered);
        let html = Document {
            blocks: vec![numbered],
        }
        .to_html();
        assert!(html.contains("list-style-type:lower-roman"), "{html}");
        // A block with no marker writes none.
        let plain = serde_json::to_string(&Block::bullet("Meiosis")).unwrap();
        assert!(!plain.contains("marker"), "{plain}");
        // A number style on a bulleted item is not its marker, so it is not exported.
        let mut bullet = Block::bullet("Meiosis");
        bullet.marker = Some(ListMarker::Decimal);
        let html = Document {
            blocks: vec![bullet],
        }
        .to_html();
        assert!(!html.contains("list-style-type"), "{html}");
    }

    #[test]
    fn typing_after_a_link_is_plain_but_inside_it_stays_linked() {
        let mut text = StyledText::new("see docs");
        text.set_link(4..8, Some(Arc::from("https://docs.rs")));
        let end = text.style_for_caret(8);
        assert!(
            end.link.is_none(),
            "text typed after a link is not part of it"
        );
        let inside = text.style_for_caret(6);
        assert_eq!(inside.link.as_deref(), Some("https://docs.rs"));
        text.sync_after_edit("see docs!", end, Some(9));
        assert!(text.style_at(8).link.is_none());
        assert_eq!(text.link_at(5).map(|(range, _)| range), Some(4..8));
    }

    #[test]
    fn a_link_spans_its_differently_formatted_parts() {
        let mut text = StyledText::new("é docs here, après");
        text.set_link(2..11, Some(Arc::from("https://docs.rs")));
        text.set_bold(4..6, true);
        text.set_link(13..18, Some(Arc::from("https://x.org")));
        assert!(text.runs.len() >= 4, "{text:#?}");
        for at in [2, 5, 10] {
            let (range, url) = text.link_at(at).expect("linked");
            assert_eq!((range, &*url), (2..11, "https://docs.rs"), "at {at}");
        }
        assert_eq!(text.link_at(1), None);
        assert_eq!(text.link_at(11), None);
        assert_eq!(text.link_at(17).map(|(range, _)| range), Some(13..18));
        assert_eq!(text.link_at(18), None);
        assert_eq!(StyledText::new("plain").link_at(2), None);
    }

    #[test]
    fn typed_addresses_become_links_without_their_punctuation() {
        let mut text = StyledText::new("visit (www.example.com). now");
        assert!(text.autolink_word_before(24));
        let (range, url) = text.link_at(8).expect("linked");
        assert_eq!(range, 7..22);
        assert_eq!(&*url, "https://www.example.com");
        let mut plain = StyledText::new("version 1.2 ");
        assert!(!plain.autolink_word_before(11));
        assert!(plain.runs.is_empty());
    }

    #[test]
    fn whole_text_links_every_address_once() {
        let mut text = StyledText::new("see www.a.org, then https://b.org/x and end");
        assert!(text.autolink_all());
        assert_eq!(text.link_at(4).map(|(range, _)| range), Some(4..13));
        assert_eq!(text.link_at(25).map(|(range, _)| range), Some(20..35));
        assert_eq!(text.link_at(15), None);
        assert!(!text.autolink_all(), "already linked text is left alone");
        let mut tail = StyledText::new("www.c.org");
        assert!(tail.autolink_all());
        assert_eq!(tail.link_at(8).map(|(range, _)| range), Some(0..9));
        assert!(!StyledText::new("plain words").autolink_all());
    }

    #[test]
    fn link_addresses_get_a_scheme_and_unknown_schemes_never_become_anchors() {
        assert_eq!(
            normalize_link("example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(normalize_link("a@b.org").as_deref(), Some("mailto:a@b.org"));
        assert_eq!(
            normalize_link(" folio://page/x ").as_deref(),
            Some("folio://page/x")
        );
        assert_eq!(normalize_link("  "), None);
        assert_eq!(
            normalize_link("/home/me/my report.pdf").as_deref(),
            Some("file:///home/me/my%20report.pdf")
        );
        assert_eq!(
            normalize_link(r"C:\docs\a.txt").as_deref(),
            Some("file:///C:/docs/a.txt")
        );
        assert_eq!(
            normalize_link("file:///tmp/x").as_deref(),
            Some("file:///tmp/x")
        );
        let mut text = StyledText::new("web page bad");
        text.set_link(0..3, Some(Arc::from("https://x.org/?a=\"b\"")));
        text.set_link(4..8, Some(Arc::from(page_link("p1").as_str())));
        text.set_link(9..12, Some(Arc::from("javascript:alert(1)")));
        let html = text.to_html();
        assert!(
            html.contains("<a href=\"https://x.org/?a=&quot;b&quot;\">web</a>"),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"page-link\">page</span>"),
            "{html}"
        );
        assert!(!html.contains("javascript"), "{html}");
    }

    #[test]
    fn paragraph_links_name_their_page_and_paragraph() {
        let link = paragraph_link("p1", 42);
        assert_eq!(link, "folio://page/p1#p42");
        assert_eq!(parse_page_link(&link), Some(("p1", Some(42))));
        assert_eq!(parse_page_link(" folio://page/p1 "), Some(("p1", None)));
        assert_eq!(parse_page_link("folio://page/p1#pxyz"), Some(("p1", None)));
        assert_eq!(parse_page_link("https://x.org/#p1"), None);
        assert_eq!(parse_page_link(&section_link("s1")), None);
    }

    #[test]
    fn section_links_are_kept_and_never_become_anchors() {
        assert_eq!(section_link("x"), "folio://section/x");
        assert_eq!(
            normalize_link(" folio://section/x ").as_deref(),
            Some("folio://section/x")
        );
        // Typed into a note, it links itself, as a page link does.
        let mut text = StyledText::new("see folio://section/x here");
        assert!(text.autolink_all());
        assert_eq!(
            text.link_at(6).map(|(range, url)| (range, url.to_string())),
            Some((4..21, "folio://section/x".to_string()))
        );
        // Exported, it is an internal link like a page's: never an anchor.
        let html = text.to_html();
        assert!(
            html.contains("<span class=\"page-link\">folio://section/x</span>"),
            "{html}"
        );
        assert!(!is_openable_link(&section_link("x")));
    }

    #[test]
    fn links_round_trip_through_json_and_old_json_still_loads() {
        let mut text = StyledText::new("docs");
        text.set_link(0..4, Some(Arc::from("https://docs.rs")));
        let json = serde_json::to_string(&text).unwrap();
        assert!(json.contains("\"link\":\"https://docs.rs\""), "{json}");
        let back: StyledText = serde_json::from_str(&json).unwrap();
        assert_eq!(back, text);
        let old: StyledText = serde_json::from_str(
            r#"{"text":"hi","runs":[{"start":0,"end":2,"style":{"bold":true}}]}"#,
        )
        .unwrap();
        assert!(old.style_at(0).bold && old.style_at(0).link.is_none());
    }

    #[test]
    fn line_tags_round_trip_and_unknown_tags_are_skipped() {
        let mut block = Block::paragraph("x");
        block.tags = vec![LineTag::RememberForLater];
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"tags\":[\"remember_for_later\"]"), "{json}");
        assert_eq!(
            serde_json::from_str::<Block>(&json).unwrap().tags,
            block.tags
        );
        let newer: Block = serde_json::from_str(
            r#"{"id":5,"kind":{"type":"paragraph"},"text":{"text":"y"},"tags":["important","from_the_future"]}"#,
        ).unwrap();
        assert_eq!(
            newer.tags,
            [LineTag::Important],
            "unknown tags are dropped, the paragraph is kept"
        );
        assert!(!serde_json::to_string(&Block::paragraph("z"))
            .unwrap()
            .contains("tags"));
    }

    #[test]
    fn new_tag_fields_round_trip_and_old_pages_load() {
        let mut block = Block::paragraph("Ring the dentist");
        block.tags = vec![LineTag::CallBack, LineTag::Important];
        block.checked_tags = vec![LineTag::CallBack];
        block.custom_tags = vec![CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: Some(Rgb::HIGHLIGHTERS[1]),
            checked: true,
        }];
        let doc = Document {
            blocks: vec![block.clone()],
        };
        let (back, lossy) = Document::from_json_lossy(&doc.to_json());
        assert!(!lossy);
        assert_eq!(back.blocks[0].checked_tags, block.checked_tags);
        assert_eq!(back.blocks[0].custom_tags, block.custom_tags);

        let old: Block = serde_json::from_str(
            r#"{"id":1,"kind":{"type":"paragraph"},"text":{"text":"x"},"tags":["important"]}"#,
        )
        .unwrap();
        assert_eq!(old.tags, [LineTag::Important]);
        assert!(old.checked_tags.is_empty() && old.custom_tags.is_empty());

        let json = serde_json::to_string(&old).unwrap();
        assert!(
            !json.contains("checkedTags") && !json.contains("customTags"),
            "{json}"
        );
    }

    #[test]
    fn unknown_checked_tags_are_skipped() {
        let newer: Block = serde_json::from_str(
            r#"{"id":5,"kind":{"type":"paragraph"},"text":{"text":"y"},"tags":["call_back"],"checkedTags":["call_back","from_the_future"]}"#,
        )
        .unwrap();
        assert_eq!(
            newer.checked_tags,
            [LineTag::CallBack],
            "unknown ticks are dropped, the paragraph is kept"
        );
        // A custom tag whose symbol is a newer Folio's built-in keeps its paragraph, its
        // name and its look; it shows no symbol here.
        let doc = Document::from_json_lossy(
            r#"{"blocks":[{"id":6,"kind":{"type":"paragraph"},"text":{"text":"z"},"customTags":[{"name":"Later","symbol":{"tag":"from_the_future"},"highlight":[255,240,0]},{"name":"Homework","symbol":"checkbox"}]}]}"#,
        );
        assert!(!doc.1, "nothing is lost");
        let kept = &doc.0.blocks[0].custom_tags;
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert_eq!(
            (kept[0].name.as_str(), kept[0].symbol),
            ("Later", TagSymbol::None)
        );
        assert!(kept[0].highlight.is_some());
        assert_eq!(kept[1].symbol, TagSymbol::Checkbox);
    }

    #[test]
    fn repair_keeps_checked_tags_inside_tags() {
        let mut block = Block::paragraph("x");
        block.checked_tags = vec![LineTag::CallBack, LineTag::Important];
        let mut doc = Document {
            blocks: vec![block],
        };
        doc.repair();
        assert!(doc.blocks[0].checked_tags.is_empty());

        doc.blocks[0].tags = vec![LineTag::CallBack];
        doc.blocks[0].checked_tags = vec![LineTag::CallBack, LineTag::Important];
        doc.blocks[0].custom_tags = vec![CustomTag {
            name: "  ".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: false,
        }];
        doc.repair();
        assert_eq!(
            doc.blocks[0].checked_tags,
            [LineTag::CallBack],
            "Important cannot be ticked"
        );
        assert!(
            doc.blocks[0].custom_tags.is_empty(),
            "a custom tag with no name is dropped"
        );
    }

    #[test]
    fn tagged_paragraphs_export_with_symbols_and_highlights() {
        let mut doc = Document {
            blocks: vec![Block::paragraph("Key idea"), Block::paragraph("Osmosis")],
        };
        doc.blocks[0].tags = vec![LineTag::Important];
        doc.blocks[1].tags = vec![LineTag::Definition];
        let html = doc.to_html();
        assert!(
            html.contains("★") && html.contains("title=\"Important\""),
            "{html}"
        );
        assert!(
            html.contains("<mark style=\"background:#bfe9d2\">Osmosis</mark>"),
            "{html}"
        );
    }

    /// Exported and printed, text on a dark highlight turns white as on the page, unless it
    /// has a colour of its own; on a light one it keeps the page's ink.
    #[test]
    fn text_on_a_dark_highlight_exports_white() {
        let mut doc = Document {
            blocks: vec![Block::paragraph("Navy red yellow")],
        };
        let text = &mut doc.blocks[0].text;
        text.set_highlight(0..4, Some(Rgb(0x00, 0x00, 0x80)));
        text.set_highlight(5..8, Some(Rgb(0x00, 0x00, 0x80)));
        text.apply_style_chars(5..8, |style| style.color = Some(Rgb(0xff, 0x00, 0x00)));
        text.set_highlight(9..15, Some(Rgb(0xff, 0xff, 0x00)));
        let html = doc.to_html();
        assert!(
            html.contains("<mark class=\"on-dark\" style=\"background:#000080\">Navy</mark>"),
            "{html}"
        );
        assert_eq!(html.matches("on-dark").count(), 1, "{html}");
        assert!(
            html.contains("<mark style=\"background:#ffff00\">yellow</mark>"),
            "{html}"
        );
        assert!(Rgb(0, 0, 0).wants_light_text() && !Rgb(0xc0, 0xc0, 0xc0).wants_light_text());
    }

    /// Exported tags hang left of the text as on the page; a list item's hang left of its
    /// bullet or number too, which they would otherwise cover.
    #[test]
    fn exported_tags_hang_left_of_text_and_list_markers() {
        let mut doc = Document {
            blocks: vec![Block::paragraph("Key idea"), Block::new(BlockKind::Bullet)],
        };
        doc.blocks[0].tags = vec![LineTag::Important];
        doc.blocks[1].tags = vec![LineTag::Important, LineTag::Question];
        let html = doc.to_html();
        assert!(
            html.contains("<p><span class=\"tags\" style=\"margin-left:-18px;width:18px\">"),
            "{html}"
        );
        assert!(
            html.contains(
                "<li><span class=\"tags\" style=\"margin-left:calc(-36px - 1.6em);margin-right:1.6em;width:36px\">"
            ),
            "{html}"
        );
    }

    #[test]
    fn tag_titles_export_in_french() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        let mut doc = Document {
            blocks: vec![Block::paragraph("Osmose")],
        };
        doc.blocks[0].tags = vec![LineTag::Question];
        let html = doc.to_html();
        assert!(html.contains("title=\"Question\""), "{html}");
        doc.blocks[0].tags = vec![LineTag::Idea];
        assert!(
            doc.to_html().contains("title=\"Idée\""),
            "{}",
            doc.to_html()
        );
    }

    #[test]
    fn custom_and_ticked_tags_are_listed_and_exported() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let mut block = Block::paragraph("Ring the dentist");
        block.tags = vec![LineTag::CallBack];
        block.checked_tags = vec![LineTag::CallBack];
        block.custom_tags = vec![CustomTag {
            name: "Homework".into(),
            symbol: TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: false,
        }];
        let doc = Document {
            blocks: vec![block],
        };
        let mut canvas = CanvasDoc::default();
        canvas.add(CanvasItem::new(
            CanvasKind::Text { doc: doc.clone() },
            40.0,
            40.0,
            300.0,
            80.0,
        ));
        let page = TaggedPage {
            id: "p1",
            title: "Week",
            section_id: "s1",
            section_name: "Home",
            section_color: "#745cf5",
            notebook_id: "n1",
            notebook_name: "Mine",
            updated_at: 1_700_000_000_000,
        };
        let lines = tagged_lines(&page, &canvas);
        assert_eq!(lines.len(), 2, "{lines:#?}");
        let call = lines
            .iter()
            .find(|line| line.mark == TagMark::Line(LineTag::CallBack))
            .unwrap();
        assert!(call.checked);
        let custom = lines
            .iter()
            .find(|line| line.mark == TagMark::Custom)
            .unwrap();
        assert_eq!(
            custom.custom.as_ref().map(|tag| tag.name.as_str()),
            Some("Homework")
        );
        assert_eq!(custom.tag_label(), "Homework");
        assert!(!custom.checked);
        assert!(lines.iter().all(|line| line.updated_at == page.updated_at));

        let html = doc.to_html();
        assert!(
            html.contains('☑'),
            "a ticked Call back shows its tick: {html}"
        );
        assert!(html.contains("title=\"Call back\""), "{html}");
        assert!(html.contains("title=\"Homework\""), "{html}");
    }

    #[test]
    fn file_items_round_trip_and_are_searchable() {
        let json = r#"{"items":[{"id":7,"x":1,"y":2,"width":260,"height":56,"kind":{"type":"file","attachment":"a1","name":"Essay.docx","size":2048}}]}"#;
        let (doc, damaged) = CanvasDoc::from_json_lossy(json);
        assert!(!damaged);
        assert_eq!(doc.items[0].kind.attachment(), Some("a1"));
        assert_eq!(doc.plain_text(), "Essay.docx");
        assert_eq!(CanvasDoc::from_json_lossy(&doc.to_json()).0, doc);
    }

    #[test]
    fn stickies_load_as_note_containers() {
        let json = r#"{"items":[{"id":7,"x":10,"y":20,"width":240,"height":150,"z":1,"kind":{"type":"sticky","doc":{"blocks":[{"id":1,"kind":{"type":"paragraph"},"text":{"text":"Bring the folder"}}]},"color":[255,232,122]}}]}"#;
        let (canvas, lossy) = CanvasDoc::from_json_lossy(json);
        assert!(!lossy, "nothing is skipped");
        assert_eq!(canvas.items.len(), 1);
        let item = &canvas.items[0];
        assert!(
            matches!(item.kind, CanvasKind::Text { .. }),
            "{:?}",
            item.kind
        );
        assert_eq!((item.x, item.y, item.width), (10.0, 20.0, 240.0));
        assert_eq!(item.kind.plain_text(), "Bring the folder");
        // Saved again, it is an ordinary note.
        assert!(canvas.to_json().contains("\"type\":\"text\""));
    }

    #[test]
    fn equations_default_their_size() {
        let (canvas, lossy) = CanvasDoc::from_json_lossy(
            r#"{"items":[{"id":5,"x":1,"y":2,"width":90,"height":40,"kind":{"type":"equation","source":"x^2"}}]}"#,
        );
        assert!(!lossy);
        match &canvas.items[0].kind {
            CanvasKind::Equation { source, size } => {
                assert_eq!(source, "x^2");
                assert_eq!(*size, 22.0);
            }
            other => panic!("expected an equation, got {other:?}"),
        }
        assert_eq!(canvas.plain_text(), "x^2");
        assert!(canvas.to_json().contains("\"type\":\"equation\""));
    }

    #[test]
    fn outlines_fold_move_and_shift_by_family() {
        let mut doc = Document {
            blocks: ["A", "a1", "a2", "B", "b1"].map(Block::paragraph).to_vec(),
        };
        for (block, level) in doc.blocks.iter_mut().zip([0, 1, 2, 0, 1]) {
            block.indent = level;
        }
        let texts = |doc: &Document| -> Vec<String> {
            doc.blocks
                .iter()
                .map(|block| block.text.text.clone())
                .collect()
        };
        let levels = |doc: &Document| -> Vec<u8> { doc.blocks.iter().map(|b| b.indent).collect() };
        assert_eq!(
            (doc.family(0), doc.family(1), doc.family(3)),
            (0..3, 1..3, 3..5)
        );

        // Folding A hides a1 and a2, not B; reaching a2 unfolds A again.
        doc.blocks[0].collapsed = true;
        assert_eq!(doc.hidden_blocks(), [false, true, true, false, false]);
        assert!(doc.expand_to(2));
        assert!(!doc.blocks[0].collapsed);

        // Show level 1: every parent folds; 0 shows everything again.
        assert!(doc.show_levels(1));
        assert_eq!(doc.hidden_blocks(), [false, true, true, false, true]);
        assert!(doc.show_levels(0));
        assert!(doc.hidden_blocks().iter().all(|hidden| !hidden));

        // B goes above A with its subparagraph, and back down.
        assert_eq!(doc.move_blocks(doc.family(3), true), Some(0));
        assert_eq!(texts(&doc), ["B", "b1", "A", "a1", "a2"]);
        assert_eq!(doc.move_blocks(doc.family(0), false), Some(3));
        assert_eq!(texts(&doc), ["A", "a1", "a2", "B", "b1"]);
        assert_eq!(doc.move_blocks(doc.family(0), true), None, "nothing above");

        // a1 and a2 go shallower together; A can't go below level 0.
        assert!(doc.shift_blocks(doc.family(1), false));
        assert_eq!(levels(&doc), [0, 0, 1, 0, 1]);
        assert!(!doc.shift_blocks(doc.family(0), false));

        // Dragged onto itself it stays; dragged to the end it moves there.
        assert_eq!(doc.move_family_before(0, Some(1)), None);
        assert_eq!(doc.move_family_before(0, None), Some(4));
        assert_eq!(texts(&doc), ["a1", "a2", "B", "b1", "A"]);

        // A folded paragraph saves its flag; an open one writes nothing new.
        doc.blocks[0].collapsed = true;
        let json = doc.to_json();
        assert!(json.contains("\"collapsed\":true"));
        assert_eq!(json.matches("collapsed").count(), 1);
        assert_eq!(Document::from_json_lossy(&json).0, doc);
    }

    #[test]
    fn new_block_kinds_round_trip_and_read_as_text() {
        let doc = Document {
            blocks: vec![
                Block::paragraph("Read before the lab"),
                Block::new(BlockKind::File {
                    attachment: "f1".into(),
                    name: "Lab sheet.pdf".into(),
                    size: 2048,
                }),
                Block::new(BlockKind::Equation {
                    source: "x^2".into(),
                    scale: 22.0,
                }),
                Block::new(BlockKind::Audio {
                    attachment: "a1".into(),
                    duration_ms: 1500,
                }),
            ],
        };
        let json = doc.to_json();
        assert!(json.contains("\"type\":\"file\""), "{json}");
        assert!(json.contains("\"type\":\"equation\""), "{json}");
        assert!(json.contains("\"type\":\"audio\""), "{json}");
        let (back, lossy) = Document::from_json_lossy(&json);
        assert!(!lossy);
        assert_eq!(back, doc);

        let text = doc.plain_text();
        assert!(text.contains("Lab sheet.pdf"), "{text}");
        assert!(text.contains("x^2"), "{text}");
        assert_eq!(text.lines().count(), 3, "a recording adds no text: {text}");

        let kinds: Vec<&BlockKind> = doc.blocks[1..].iter().map(|block| &block.kind).collect();
        assert!(kinds.iter().all(|kind| !kind.is_textual()));
        assert_eq!(kinds[0].attachment(), Some("f1"));
        assert_eq!(kinds[1].attachment(), None);
        assert_eq!(kinds[2].attachment(), Some("a1"));

        let mut moved = doc.clone();
        *moved.blocks[1].kind.attachment_mut().unwrap() = "f2".into();
        assert_eq!(moved.blocks[1].kind.attachment(), Some("f2"));
        assert!(moved.blocks[2].kind.attachment_mut().is_none());

        // An equation without a size takes the default one, as equation items do.
        let (old, _) = Document::from_json_lossy(
            r#"{"blocks":[{"id":3,"kind":{"type":"equation","source":"a+b"}}]}"#,
        );
        assert_eq!(
            old.blocks[0].kind,
            BlockKind::Equation {
                source: "a+b".into(),
                scale: default_equation_size(),
            }
        );
    }

    #[test]
    fn blank_equations_do_not_survive_a_load() {
        let doc = Document {
            blocks: vec![Block::new(BlockKind::Equation {
                source: "  ".into(),
                scale: 22.0,
            })],
        };
        let (back, _) = Document::from_json_lossy(&doc.to_json());
        assert!(!back.blocks.is_empty());
        assert!(back
            .blocks
            .iter()
            .all(|block| !matches!(block.kind, BlockKind::Equation { .. })));
    }

    #[test]
    fn tables_store_inline_and_repair_to_a_grid() {
        let mut table = TableBlock::new(2, 2);
        table.rows[1].pop();
        table.widths = vec![0.0, 5.0, f32::NAN];
        table.repair();
        assert!(table.rows.iter().all(|row| row.len() == 3));
        assert_eq!(table.widths, vec![0.0, TABLE_MIN_COLUMN, 0.0]);
        let mut block = Block::new(BlockKind::Table(table.clone()));
        if let BlockKind::Table(table) = &mut block.kind {
            table.rows[0][0].text = StyledText::new("Day");
        }
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"table\""), "{json}");
        let back: Block = serde_json::from_str(&json).unwrap();
        assert_eq!(back, block);
    }

    #[test]
    fn table_text_reaches_search_and_html() {
        let mut table = TableBlock::from_strings(
            &[
                vec!["Day".into(), "Task".into()],
                vec!["Mon".into(), "Maths".into()],
            ],
            true,
            120.0,
        );
        table.rows[1][1].shading = Some(Rgb(0xff, 0xf2, 0xcc));
        let doc = Document {
            blocks: vec![
                Block::paragraph("Week"),
                Block::new(BlockKind::Table(table)),
            ],
        };
        assert_eq!(doc.plain_text(), "Week\nDay\tTask\nMon\tMaths");
        let html = doc.to_html();
        assert!(
            html.contains("<th>Day</th>") && html.contains("background:#fff2cc"),
            "{html}"
        );
    }

    const OLD_TABLE: &str = r#"{"items":[{"id":7,"x":60.0,"y":430.0,"width":452.0,"height":170.0,"z":1,"kind":{"type":"table","rows":[["Part","Job"],["Mitochondria","Energy"]],"header":true}}],"ink":[]}"#;

    #[test]
    fn an_old_page_table_loads_as_a_table_in_a_note() {
        // Loading is parsing, then `repair` (as `Library::get_page` does).
        let (mut doc, lossy) = CanvasDoc::from_json_lossy(OLD_TABLE);
        assert!(!lossy);
        doc.repair();
        let item = &doc.items[0];
        assert_eq!(
            (item.id, item.x, item.y, item.width),
            (7, 60.0, 430.0, 452.0)
        );
        assert!(!item.auto_width, "the note keeps the old table's width");
        let CanvasKind::Text { doc: note } = &item.kind else {
            panic!("expected a note, got {:?}", item.kind);
        };
        let BlockKind::Table(table) = &note.blocks[0].kind else {
            panic!("expected a table block");
        };
        assert!(table.header);
        assert_eq!(table.widths, vec![220.0, 220.0]);
        assert_eq!(table.rows[1][0].text.text, "Mitochondria");
        assert!(doc.plain_text().contains("Mitochondria\tEnergy"));
    }

    #[test]
    fn styled_slices_keep_their_marks() {
        let mut text = StyledText::new("hello world");
        text.apply_style_chars(6..11, |style| style.bold = true);
        let slice = text.slice_chars(4..9);
        assert_eq!(slice.text, "o wor");
        assert!(slice.style_at(slice.char_to_byte(3)).bold);
        let mut target = StyledText::new("ab");
        target.insert_styled(1, &slice);
        assert_eq!(target.text, "ao worb");
        assert!(target.style_at(target.char_to_byte(4)).bold);
        assert!(!target.style_at(target.char_to_byte(6)).bold);
    }

    #[test]
    fn picture_turns_and_flips_compose_as_shown() {
        let mut meta = PictureMeta::default();
        for _ in 0..4 {
            meta.rotate(true);
        }
        assert!(meta.is_plain());
        meta.flip_shown(false);
        meta.flip_shown(false);
        assert!(meta.is_plain(), "two horizontal flips cancel");
        meta.flip_shown(false);
        meta.flip_shown(true);
        assert_eq!(
            (meta.turns, meta.flip),
            (2, false),
            "both flips are a half turn"
        );
        let quarter = PictureMeta {
            turns: 1,
            ..PictureMeta::default()
        };
        assert_eq!(
            quarter.uv_corners()[0],
            [0.0, 1.0],
            "a right turn puts the bottom-left at the top-left"
        );
    }

    #[test]
    fn crop_round_trips_through_turns_and_flips() {
        let shown = [0.1, 0.2, 0.6, 0.9];
        for turns in 0..4 {
            for flip in [false, true] {
                let mut meta = PictureMeta {
                    turns,
                    flip,
                    ..PictureMeta::default()
                };
                let whole = meta.uv_corners();
                meta.crop = Some(meta.source_rect(shown));
                let back = meta.shown_crop();
                for (got, want) in back.iter().zip(shown) {
                    assert!((got - want).abs() < 1e-4, "{turns} {flip}: {back:?}");
                }
                // Drawn, the cropped picture shows that part of the whole one: each corner
                // of the shown rectangle maps to the same texture point.
                let at = |x: f32, y: f32| {
                    let [tl, tr, br, bl] = whole;
                    let top = [tl[0] + (tr[0] - tl[0]) * x, tl[1] + (tr[1] - tl[1]) * x];
                    let bottom = [bl[0] + (br[0] - bl[0]) * x, bl[1] + (br[1] - bl[1]) * x];
                    [
                        top[0] + (bottom[0] - top[0]) * y,
                        top[1] + (bottom[1] - top[1]) * y,
                    ]
                };
                let [x0, y0, x1, y1] = shown;
                let want = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
                for (got, want) in meta.uv_corners().iter().zip(want) {
                    assert!(
                        (got[0] - want[0]).abs() < 1e-4 && (got[1] - want[1]).abs() < 1e-4,
                        "{turns} {flip}: {got:?} is not {want:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn uv_corners_follow_the_crop() {
        let meta = PictureMeta {
            crop: Some([0.25, 0.0, 1.0, 0.5]),
            ..PictureMeta::default()
        };
        let corners = meta.uv_corners();
        assert_eq!(corners[0], [0.25, 0.0]);
        assert_eq!(corners[2], [1.0, 0.5]);
        assert_eq!(meta.cropped_size([200.0, 100.0]), [150.0, 50.0]);
    }

    #[test]
    fn a_plain_picture_has_no_crop_key() {
        let json = serde_json::to_string(&PictureMeta::default()).unwrap();
        assert!(!json.contains("crop"), "{json}");
        let cropped = PictureMeta {
            crop: Some([0.5, 0.0, 1.0, 1.0]),
            ..PictureMeta::default()
        };
        assert!(!cropped.is_plain());
        let json = serde_json::to_string(&cropped).unwrap();
        assert_eq!(json, r#"{"crop":[0.5,0.0,1.0,1.0]}"#);
        assert_eq!(serde_json::from_str::<PictureMeta>(&json).unwrap(), cropped);
    }

    #[test]
    fn office_colour_tables_are_complete() {
        let highlights = Rgb::TEXT_HIGHLIGHTS.map(|(color, _)| color);
        for (at, color) in highlights.iter().enumerate() {
            assert!(
                !highlights[..at].contains(color),
                "{color:?} is listed twice"
            );
        }
        for (_, name) in Rgb::TEXT_HIGHLIGHTS {
            for lang in crate::i18n::Lang::ALL {
                assert_ne!(crate::i18n::tr_in(lang, name), name, "{name} is named");
            }
        }
        assert_eq!(Rgb::FONT_THEME[0][4], Rgb(0x44, 0x72, 0xC4));
        assert_eq!(Rgb::FONT_THEME[5][5], Rgb(0x84, 0x3C, 0x0C));
        assert_eq!(Rgb::FONT_STANDARD[9], Rgb(0x70, 0x30, 0xA0));
    }
}
