//! Importing the desktop notebooks: a `.onepkg` package, a notebook folder (its `.onetoc2`, `.one`
//! sections and section-group folders) or one `.one` section, read with `onenote_parser`.
//!
//! The import has two halves, so a failure never leaves a half-imported notebook. An
//! [`ImportJob`] reads the files on a worker thread into an [`ImportPlan`]: every page already
//! as a Folio canvas, every picture and file copied once into a staging folder next to the
//! library. `Library::import_plan` then writes the plan in one transaction.
//!
//! the desktop notebook lays pages out in half-inches and draws ink in HIMETRIC; Folio draws a point as
//! `pt_to_px(1)` units. The worker never uses `t!`: the report is worded on the UI thread.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use onenote_parser::contents::{
    Content, EmbeddedFile, EmbeddedObject, FileDataStatus, Image, Ink, List, NoteTag, Outline,
    OutlineElement, OutlineItem, RichText, Table,
};
use onenote_parser::page::PageContent;
use onenote_parser::property::common::ColorRef;
use onenote_parser::property::note_tag::NoteTagShape;
use onenote_parser::property::rich_text::{ParagraphAlignment, ParagraphStyling};
use onenote_parser::section::{Section, SectionEntry};
use onenote_parser::Parser;
use typed_path::TypedPath;

use crate::model::{
    page_link, Block, BlockKind, CanvasDoc, CanvasItem, CanvasKind, CustomTag, Document, FontFace,
    InkStroke, InlineStyle, LineTag, ListMarker, ParaAlign, PictureMeta, Rgb, StyleRun, StyledText,
    TableBlock, TableCell, TagSymbol, MAX_LEVEL,
};

/// Staging folders in the data folder start with this; `Library::open_at` removes leftovers.
pub const STAGING_PREFIX: &str = "import-";

/// What the user picked.
#[derive(Debug, Clone)]
pub enum Source {
    /// A `.onepkg` package (the desktop notebook's File › Export › Notebook).
    Package(PathBuf),
    /// A notebook folder. A picked `.onetoc2` stands for its folder.
    Folder(PathBuf),
    /// One `.one` section.
    Section(PathBuf),
}

impl Source {
    /// What `path` is, by its kind and file name; `None` when it is no the desktop notebook file.
    pub fn from_path(path: &Path) -> Option<Source> {
        if path.is_dir() {
            return Some(Source::Folder(path.to_path_buf()));
        }
        // ".onetoc2" alone (a hidden file) has no `Path::extension`, so test the name.
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if name.ends_with(".onepkg") {
            Some(Source::Package(path.to_path_buf()))
        } else if name.ends_with(".onetoc2") {
            path.parent().map(|dir| Source::Folder(dir.to_path_buf()))
        } else if name.ends_with(".one") {
            Some(Source::Section(path.to_path_buf()))
        } else {
            None
        }
    }

    /// The new notebook's name: the package's or section's file name, or the folder's.
    pub fn name(&self) -> String {
        let name = match self {
            Source::Folder(path) => path.file_name(),
            Source::Package(path) | Source::Section(path) => path.file_stem(),
        };
        name.map(|name| name.to_string_lossy().trim().to_string())
            .unwrap_or_default()
    }
}

/// A notebook ready to be written: built on the worker thread, written by
/// `Library::import_plan`.
#[derive(Debug)]
pub struct ImportPlan {
    /// The notebook's name; empty takes the default name.
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub entries: Vec<PlanEntry>,
    /// The pictures and files the pages refer to, each staged once.
    pub files: Vec<StagedFile>,
    /// Where `files` are, until `import_plan` moves them next to the attachments.
    pub staging: PathBuf,
    pub report: ImportReport,
}

#[derive(Debug, Clone)]
pub enum PlanEntry {
    Section(PlanSection),
    Group {
        name: String,
        entries: Vec<PlanEntry>,
    },
}

#[derive(Debug, Clone)]
pub struct PlanSection {
    pub name: String,
    /// `#rrggbb`, or empty to use the notebook's colour.
    pub color: String,
    pub pages: Vec<PlanPage>,
}

#[derive(Debug, Clone)]
pub struct PlanPage {
    /// The new page's id, chosen now so links between imported pages can point at it.
    pub id: String,
    pub title: String,
    /// 0 = page, 1 = subpage, 2 = sub-subpage, already valid for the page list.
    pub level: u8,
    pub created_at: i64,
    pub updated_at: i64,
    pub canvas: CanvasDoc,
}

/// One stored payload: a picture or a file, kept once however many pages show it.
#[derive(Debug, Clone)]
pub struct StagedFile {
    /// The attachment id the pages refer to.
    pub id: String,
    /// The file's name in the staging folder, and later among the attachments.
    pub stored: String,
    /// Its original name, shown on file cards.
    pub name: String,
    pub mime: String,
    pub size: u64,
    /// The first page that uses it: the attachment row belongs to that page.
    pub owner: String,
}

/// What an import brought in, and what it left out and why.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportReport {
    pub sections: usize,
    pub groups: usize,
    pub pages: usize,
    pub pictures: usize,
    pub files: usize,
    pub tables: usize,
    pub ink_strokes: usize,
    /// Sections that would not open because they have a password (or are damaged the same way).
    pub password_sections: Vec<String>,
    /// Other sections that would not open: (name, the reader's error).
    pub unreadable_sections: Vec<(String, String)>,
    /// Pictures and files whose contents the desktop notebook did not keep.
    pub missing_files: usize,
    /// Files over the attachment size limit.
    pub too_big: Vec<String>,
    /// Pictures in a format Folio cannot draw, kept as attached files.
    pub unsupported_pictures: usize,
    /// Printout pages of Office documents: onenote_parser 2.0 has no picture of them.
    pub office_printout_pages: usize,
    /// Handwriting drawn inside a paragraph, which Folio has no place for.
    pub inline_ink: usize,
    /// Equations Folio's equations can't read (built-up fractions, roots, matrices…), kept as
    /// their plain text. The others come in as equations.
    pub math: usize,
    /// Names of tags Folio does not have.
    pub unknown_tags: BTreeSet<String>,
    /// Links to the desktop notebook pages outside the import, and links Folio does not open.
    pub outside_links: usize,
}

/// What `Library::import_plan` hands back to the dialog.
#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub notebook_id: String,
    pub name: String,
    pub first_page: Option<String>,
    pub report: ImportReport,
}

/// Why nothing was imported.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    /// A lone section that would not open: the desktop notebook's password-protected sections read like
    /// a file that ends too early.
    PasswordOrDamaged {
        name: String,
    },
    /// No the desktop notebook section at that place.
    Nothing {
        name: String,
    },
    Cancelled,
    /// A package that would unpack to more than [`PACKAGE_LIMIT`].
    TooLarge {
        name: String,
    },
    /// The reader or the file system refused; `error` is their own (English) text.
    Failed {
        name: String,
        error: String,
    },
    /// The reader stopped on the file (it panicked), or the worker thread did.
    Crashed {
        name: String,
    },
}

/// The most a `.onepkg` may unpack to, so a hostile file can't fill the disk.
pub const PACKAGE_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

/// `catch`'s error when the reader panicked: `failed` turns it into `ImportError::Crashed`,
/// which the dialog words in the interface language.
const READER_STOPPED: &str = "the desktop notebook reader stopped on this file";

/// Why `name` could not be read, from a reader or file-system error.
fn failed(name: String, error: String) -> ImportError {
    if error == READER_STOPPED {
        ImportError::Crashed { name }
    } else {
        ImportError::Failed { name, error }
    }
}

/// Folio units per point: notes are drawn at `pt_to_px`, 4/3 units a point.
fn unit() -> f32 {
    crate::model::pt_to_px(1)
}

/// the desktop notebook positions and widths are in half-inches (36 pt).
fn half_inch() -> f32 {
    36.0 * unit()
}

/// the desktop notebook ink is in HIMETRIC: 1/2540 inch, that is 72/2540 pt.
fn himetric() -> f32 {
    72.0 / 2540.0 * unit()
}

/// The colour of Folio's palette (`theme::SECTION_COLORS`) nearest to a the desktop notebook section or
/// notebook colour, so the Section Color menu shows it checked and names it. the 2016 desktop notebook
/// stored paler colours than today's (#8AA8E4, #FFD869…), so the hue counts most, then the
/// colourfulness and the lightness; a grey goes to the palette's grey.
fn palette_color(color: Rgb) -> &'static str {
    /// Hue in degrees, chroma and lightness, each 0–1 but the hue.
    fn hcl(Rgb(r, g, b): Rgb) -> (f32, f32, f32) {
        let (r, g, b) = (
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
        );
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let chroma = max - min;
        let hue = if chroma == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / chroma + 2.0)
        } else {
            60.0 * ((r - g) / chroma + 4.0)
        };
        (hue, chroma, (max + min) / 2.0)
    }
    fn parse(hex: &str) -> Rgb {
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
        Rgb(channel(1), channel(3), channel(5))
    }
    const GREY: f32 = 0.12;
    let (hue, chroma, light) = hcl(color);
    let cost = |hex: &&str| {
        let (h, c, l) = hcl(parse(hex));
        let turn = (hue - h).abs();
        let turn = turn.min(360.0 - turn) / 180.0;
        16.0 * turn * turn + 0.5 * (chroma - c).powi(2) + 0.5 * (light - l).powi(2)
    };
    crate::theme::SECTION_COLORS
        .iter()
        .filter(|hex| (hcl(parse(hex)).1 < GREY) == (chroma < GREY))
        .min_by(|a, b| cost(a).total_cmp(&cost(b)))
        .copied()
        .unwrap_or(crate::theme::SECTION_COLORS[0])
}

/// Copies picture and file payloads into the staging folder, each once: an Office printout
/// points every page picture at the same document, and a deduplicated section of 11 MB
/// otherwise grew to 1.1 GB.
struct Stager {
    dir: PathBuf,
    files: Vec<StagedFile>,
    /// (hash, size) to the files staged with them.
    by_content: HashMap<(u64, u64), Vec<usize>>,
}

/// How staging a payload went.
enum Staged {
    File(usize),
    TooBig,
    Failed,
}

impl Stager {
    fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            files: Vec::new(),
            by_content: HashMap::new(),
        })
    }

    /// Streams a payload into the staging folder, hashing it on the way, and returns its index
    /// in `files`. A payload already staged is not kept twice; `page` owns a new one.
    fn stage(
        &mut self,
        mut reader: impl Read,
        name: &str,
        extension: &str,
        mime: &str,
        page: &str,
    ) -> Staged {
        let part = self.dir.join(format!("part-{}", self.files.len()));
        let key = match copy_hashing(&mut reader, &part) {
            Ok(Some(key)) => key,
            Ok(None) => {
                let _ = fs::remove_file(&part);
                return Staged::TooBig;
            }
            Err(_) => {
                let _ = fs::remove_file(&part);
                return Staged::Failed;
            }
        };
        let same = self.by_content.get(&key).and_then(|candidates| {
            candidates.iter().copied().find(|&index| {
                crate::db::same_contents(&part, &self.dir.join(&self.files[index].stored))
                    .unwrap_or(false)
            })
        });
        if let Some(index) = same {
            let _ = fs::remove_file(&part);
            return Staged::File(index);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let extension = crate::db::safe_extension(extension);
        let stored = if extension.is_empty() {
            id.clone()
        } else {
            format!("{id}.{extension}")
        };
        if fs::rename(&part, self.dir.join(&stored)).is_err() {
            let _ = fs::remove_file(&part);
            return Staged::Failed;
        }
        let index = self.files.len();
        self.files.push(StagedFile {
            id,
            stored,
            name: name.to_string(),
            mime: mime.to_string(),
            size: key.1,
            owner: page.to_string(),
        });
        self.by_content.entry(key).or_default().push(index);
        Staged::File(index)
    }
}

/// Copies `reader` into a new file at `path` and returns (hash, size), or `None` once it
/// passes the attachment size limit.
fn copy_hashing(reader: &mut impl Read, path: &Path) -> io::Result<Option<(u64, u64)>> {
    use std::hash::Hasher;
    let mut out = io::BufWriter::new(fs::File::create(path)?);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > crate::db::MAX_ATTACHMENT_BYTES {
            return Ok(None);
        }
        hasher.write(&buffer[..read]);
        io::Write::write_all(&mut out, &buffer[..read])?;
    }
    io::Write::flush(&mut out)?;
    Ok(Some((hasher.finish(), size)))
}

/// The first bytes of a payload, and a reader that still yields all of it.
fn peek(mut reader: Box<dyn Read>) -> io::Result<(Vec<u8>, impl Read)> {
    let mut head = vec![0u8; 16];
    let mut filled = 0;
    while filled < head.len() {
        let read = reader.read(&mut head[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    head.truncate(filled);
    Ok((head.clone(), io::Cursor::new(head).chain(reader)))
}

/// A picture's extension and media type from its first bytes (the desktop notebook seldom records an
/// extension); `None` for formats Folio cannot draw.
fn picture_format(head: &[u8]) -> Option<(&'static str, &'static str)> {
    if head.starts_with(b"\x89PNG") {
        Some(("png", "image/png"))
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("jpg", "image/jpeg"))
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some(("webp", "image/webp"))
    } else {
        None
    }
}

/// The extension of a picture Folio cannot draw, from its first bytes, for a picture
/// the desktop notebook kept without a file name or extension.
fn other_picture_extension(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"GIF8") {
        Some("gif")
    } else if head.starts_with(b"BM") {
        Some("bmp")
    } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        Some("tif")
    } else if head.starts_with(&[0xd7, 0xcd, 0xc6, 0x9a]) {
        Some("wmf")
    } else if head.len() >= 8
        && head[0..4] == [1, 0, 0, 0]
        && u32::from_le_bytes([head[4], head[5], head[6], head[7]]) >= 88
    {
        // An EMF file starts with its header record: type 1, at least 88 bytes long.
        Some("emf")
    } else {
        None
    }
}

/// A ZIP container: the XPS document behind an Office printout page.
fn is_zip(head: &[u8]) -> bool {
    head.starts_with(b"PK\x03\x04")
}

/// A WAV recording, which Folio can play.
fn is_wav(head: &[u8]) -> bool {
    head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE"
}

/// What a source holds once read: its sections and section groups in the desktop notebook's order.
struct Parsed {
    entries: Vec<Entry>,
    color: Option<Rgb>,
}

enum Entry {
    Section(Section),
    Group(String, Vec<Entry>),
}

fn entries_of(list: &[SectionEntry]) -> Vec<Entry> {
    list.iter()
        .map(|entry| match entry {
            SectionEntry::Section(section) => Entry::Section(section.clone()),
            SectionEntry::SectionGroup(group) => Entry::Group(
                group.display_name().to_string(),
                entries_of(group.entries()),
            ),
        })
        .collect()
}

fn count_pages(entries: &[Entry]) -> usize {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Section(section) => section
                .page_series()
                .iter()
                .map(|series| series.pages().len())
                .sum(),
            Entry::Group(_, entries) => count_pages(entries),
        })
        .sum()
}

fn rgb(color: onenote_parser::property::common::Color) -> Rgb {
    Rgb(color.r(), color.g(), color.b())
}

/// A file's name without its extension, or a folder's name.
fn stem(path: &Path) -> String {
    let name = if path.is_dir() {
        path.file_name()
    } else {
        path.file_stem()
    };
    name.map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The reader's text for a file that ends too early, which is how a password-protected
/// section reads.
fn end_of_file_text() -> String {
    onenote_parser::errors::ErrorKind::UnexpectedEof.to_string()
}

fn record_unreadable(report: &mut ImportReport, name: String, error: &str) {
    if error.contains(&end_of_file_text()) {
        report.password_sections.push(name);
    } else {
        report.unreadable_sections.push((name, error.to_string()));
    }
}

/// A notebook warning such as "failed to import section X.one: Unexpected end of file".
fn notebook_warning(report: &mut ImportReport, message: &str) {
    let Some(rest) = message.strip_prefix("failed to import section ") else {
        return;
    };
    let (path, error) = rest.split_once(": ").unwrap_or((rest, ""));
    let name = stem(Path::new(&path.replace('\\', "/")));
    record_unreadable(report, name, error);
}

/// Runs a step of the reader, turning a panic on a hostile file into [`READER_STOPPED`].
fn catch<T>(step: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step))
        .unwrap_or_else(|_| Err(READER_STOPPED.to_string()))
}

/// One section, read from memory: onenote_parser 2.0 rejects Windows drive-letter paths.
fn read_section_file(path: &Path) -> Result<Section, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Parser::new()
        .parse_section_buffer(&bytes, TypedPath::derive(&name))
        .map_err(|error| error.to_string())
}

fn parse(
    source: &Source,
    staging: &Path,
    report: &mut ImportReport,
) -> Result<Parsed, ImportError> {
    match source {
        Source::Section(path) => {
            let name = stem(path);
            match catch(|| read_section_file(path)) {
                Ok(section) => Ok(Parsed {
                    entries: vec![Entry::Section(section)],
                    color: None,
                }),
                Err(error) if error.contains(&end_of_file_text()) => {
                    Err(ImportError::PasswordOrDamaged { name })
                }
                Err(error) => Err(failed(name, error)),
            }
        }
        Source::Folder(dir) => Ok(read_folder(dir, report, 0)),
        Source::Package(path) => {
            let opened = path.to_str().map(|text| {
                catch(|| {
                    Parser::new()
                        .parse_package(TypedPath::derive(text))
                        .map_err(|error| error.to_string())
                })
            });
            match opened {
                Some(Ok(notebook)) => {
                    for warning in notebook.report().warnings() {
                        notebook_warning(report, warning.message());
                    }
                    Ok(Parsed {
                        entries: entries_of(notebook.entries()),
                        color: notebook.color().map(rgb),
                    })
                }
                // A package the reader refused (no table of contents, a path it cannot
                // open): unpack it ourselves and read it as a folder.
                _ => {
                    let dir = staging.join("package");
                    unpack(path, &dir)?;
                    Ok(read_folder(&dir, report, 0))
                }
            }
        }
    }
}

/// A notebook folder: the sections and groups its `.onetoc2` lists, in its order, then the
/// `.one` files and section-group folders it does not list (the desktop notebook shows those too). When
/// the table of contents cannot be read, every section in the folder is read by name. An
/// empty result is `read`'s to judge (nothing there, or only locked sections).
fn read_folder(dir: &Path, report: &mut ImportReport, depth: usize) -> Parsed {
    let mut entries = Vec::new();
    let mut color = None;
    let toc = fs::read_dir(dir).ok().and_then(|read| {
        let mut tocs: Vec<PathBuf> = read
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file() && path.to_string_lossy().to_lowercase().ends_with(".onetoc2")
            })
            .collect();
        tocs.sort();
        tocs.into_iter().next()
    });
    let opened = toc.as_deref().and_then(Path::to_str).map(|text| {
        catch(|| {
            Parser::new()
                .parse_notebook(TypedPath::derive(text))
                .map_err(|error| error.to_string())
        })
    });
    if let Some(Ok(notebook)) = opened {
        for warning in notebook.report().warnings() {
            notebook_warning(report, warning.message());
        }
        color = notebook.color().map(rgb);
        entries = entries_of(notebook.entries());
    }
    scan_folder(dir, &mut entries, report, depth);
    Parsed { entries, color }
}

/// Adds the folder's `.one` files and section-group folders that `entries` (or the report)
/// doesn't already name, in name order. The recycle bin and hidden folders are skipped.
fn scan_folder(dir: &Path, entries: &mut Vec<Entry>, report: &mut ImportReport, depth: usize) {
    // Nested groups deeper than this are a loop (a link to a parent folder), not a notebook.
    const MAX_DEPTH: usize = 8;
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut known: Vec<String> = entries
        .iter()
        .map(|entry| match entry {
            Entry::Section(section) => section
                .display_name()
                .trim_end_matches(".one")
                .to_lowercase(),
            Entry::Group(name, _) => name.to_lowercase(),
        })
        .chain(
            report
                .password_sections
                .iter()
                .map(|name| name.to_lowercase()),
        )
        .chain(
            report
                .unreadable_sections
                .iter()
                .map(|(name, _)| name.to_lowercase()),
        )
        .collect();
    let mut paths: Vec<PathBuf> = read.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let name = stem(&path);
        if name.starts_with('.') || known.contains(&name.to_lowercase()) {
            continue;
        }
        if path.is_dir() {
            if depth >= MAX_DEPTH || name.eq_ignore_ascii_case("OneNote_RecycleBin") {
                continue;
            }
            let group = read_folder(&path, report, depth + 1);
            if !group.entries.is_empty() {
                known.push(name.to_lowercase());
                entries.push(Entry::Group(name, group.entries));
            }
        } else if path.to_string_lossy().to_lowercase().ends_with(".one") {
            known.push(name.to_lowercase());
            match catch(|| read_section_file(&path)) {
                Ok(section) => entries.push(Entry::Section(section)),
                Err(error) => record_unreadable(report, name, &error),
            }
        }
    }
}

/// Unpacks a `.onepkg` (a cabinet file) into `dir`. Names that would leave `dir` are skipped,
/// and a package that unpacks to more than [`PACKAGE_LIMIT`] is refused, so a hostile file
/// can't fill the disk.
fn unpack(path: &Path, dir: &Path) -> Result<(), ImportError> {
    let io_error = |error: io::Error| failed(stem(path), error.to_string());
    let file = fs::File::open(path).map_err(io_error)?;
    let mut cabinet = cab::Cabinet::new(file).map_err(io_error)?;
    let files: Vec<(String, u64)> = cabinet
        .folder_entries()
        .flat_map(|folder| folder.file_entries())
        .map(|file| (file.name().to_string(), u64::from(file.uncompressed_size())))
        .collect();
    if files.iter().map(|(_, size)| size).sum::<u64>() > PACKAGE_LIMIT {
        return Err(ImportError::TooLarge { name: stem(path) });
    }
    for (name, _) in files {
        let Some(relative) = safe_relative(&name) else {
            continue;
        };
        let target = dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut reader = cabinet.read_file(&name).map_err(io_error)?;
        let mut out = fs::File::create(&target).map_err(io_error)?;
        io::copy(&mut reader, &mut out).map_err(io_error)?;
    }
    Ok(())
}

/// A cabinet entry name (`Group\Section.one`) as a relative path; `None` unless every part
/// is a plain name.
fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split(['\\', '/']) {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            return None;
        }
        path.push(part);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// the desktop notebook's default note text: 11 pt. A run at that size keeps Folio's own default.
const BODY_PT: u8 = 11;

/// The block kind showing the desktop notebook's built-in style `hN` (S5: P4 stores Heading n one level
/// down, below Page Title, and clamps n to 1–6).
fn heading_kind(notebook_level: u8) -> BlockKind {
    BlockKind::heading(notebook_level)
}

/// A desktop notebook paragraph as a Folio block at outline `level`. Lists are set by the caller.
fn block_of(text: &RichText, level: u8, report: &mut ImportReport) -> Block {
    let math = text
        .text_run_formatting()
        .iter()
        .any(|run| run.math_formatting());
    // An equation alone in its paragraph comes in as an equation. Folio's equations carry no
    // tags, so a tagged one stays text.
    let equation = if math && text.note_tags().is_empty() {
        equation_source(text)
    } else {
        None
    };
    let mut block = match equation {
        Some(source) => Block::new(BlockKind::Equation {
            source,
            scale: crate::model::default_equation_size(),
        }),
        None => {
            if math {
                report.math += 1;
            }
            let kind = match text.paragraph_style().style_id() {
                Some(id) if id.len() == 2 && id.starts_with('h') => {
                    heading_kind(id[1..].parse().unwrap_or(1))
                }
                Some("PageTitle") => BlockKind::PAGE_TITLE,
                Some("cite") => BlockKind::Citation,
                Some("blockquote") => BlockKind::Quote,
                Some("code") => BlockKind::Code,
                _ => BlockKind::Paragraph,
            };
            let body = matches!(kind, BlockKind::Paragraph);
            let mut block = Block::new(kind);
            block.text = styled_text(text, body);
            apply_tags(&mut block, text.note_tags(), report);
            block
        }
    };
    block.indent = level;
    block.align = match text.paragraph_alignment() {
        ParagraphAlignment::Center => ParaAlign::Center,
        ParagraphAlignment::Right => ParaAlign::Right,
        _ => ParaAlign::Left,
    };
    report.inline_ink += text
        .embedded_objects()
        .iter()
        .filter(|object| matches!(object, EmbeddedObject::Ink(_)))
        .count();
    block
}

/// One run of a paragraph: its formatting, where it starts and ends in the desktop notebook's UTF-16
/// units, and its text as stored (field marks, line breaks and all).
struct Run<'a> {
    format: &'a ParagraphStyling,
    start: u32,
    end: u32,
    text: &'a str,
}

/// The paragraph's runs in order; none when it has no run formatting.
fn runs(text: &RichText) -> Vec<Run<'_>> {
    let raw = text.text();
    let mut ends = text.text_run_indices().to_vec();
    ends.push(raw.encode_utf16().count() as u32);
    let mut start = 0u32;
    text.text_run_formatting()
        .iter()
        .zip(ends)
        .map(|(format, end)| {
            let end = end.max(start);
            let run = Run {
                format,
                start,
                end,
                text: &raw[utf16_to_byte(raw, start)..utf16_to_byte(raw, end)],
            };
            start = end;
            run
        })
        .collect()
}

/// the desktop notebook's stored text as Folio keeps it: without the U+FDDF field mark, and with the desktop notebook's
/// line break inside a paragraph (Shift+Enter, U+000B) as Folio's, a `\n`. A Windows line end
/// becomes one `\n` too.
fn clean_text(text: &str) -> String {
    text.replace('\u{fddf}', "")
        .replace("\r\n", "\n")
        .replace(['\u{b}', '\r'], "\n")
}

/// A paragraph that is an equation and nothing else, as the source of a Folio equation;
/// `None` when it also holds text, or when Folio's equations can't read it.
fn equation_source(text: &RichText) -> Option<String> {
    let mut math = String::new();
    for run in runs(text) {
        if run.format.hidden() {
            continue;
        }
        if !run.format.math_formatting() && !run.text.trim().is_empty() {
            return None;
        }
        math.push_str(run.text);
    }
    linear_equation(&math)
}

/// the desktop notebook's math text as a Folio equation in linear format, its letters plain (`𝑎=𝑏` is
/// `a=b`). `None` for what Folio's equations can't read: built-up parts (a fraction, a root,
/// a matrix: the desktop notebook marks them with U+FDD0–U+FDEF and keeps their shape elsewhere), line
/// breaks, private-use symbols, and anything `math::parse` refuses.
fn linear_equation(math: &str) -> Option<String> {
    let source = plain_math(&clean_text(math)).trim().to_string();
    let readable = |ch: char| {
        !ch.is_control() && !matches!(ch, '\u{fdd0}'..='\u{fdef}' | '\u{e000}'..='\u{f8ff}')
    };
    (!source.is_empty() && source.chars().all(readable) && crate::math::parse(&source).is_ok())
        .then_some(source)
}

/// The page's title: the visible text of the first paragraph in the desktop notebook's title box, on one
/// line; empty for an untitled page. Not `Page::title_text`: when the title box is empty,
/// onenote_parser 2.0 gives the page's first line of text instead, which would then show
/// twice, and the page would no longer be untitled as in the desktop notebook.
fn page_title(page: &onenote_parser::page::Page) -> String {
    page.title()
        .and_then(|title| title.contents().first())
        .and_then(|outline| outline.items().first())
        .and_then(OutlineItem::element)
        .and_then(|element| element.contents().first())
        .and_then(Content::rich_text)
        .map(|text| one_line(&styled_text(text, false).text))
        .unwrap_or_default()
}

/// `text` on one line, as a page title: line breaks and tabs become spaces.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .trim()
        .to_string()
}

/// The paragraph's visible text and formatting. the desktop notebook counts in UTF-16 units and keeps
/// hidden runs (the field code behind a link, `\u{fddf}HYPERLINK "…"`); Folio counts bytes
/// and keeps neither. Links stay as the desktop notebook wrote them until `rewrite_links`.
fn styled_text(text: &RichText, body: bool) -> StyledText {
    let runs = runs(text);
    if runs.is_empty() {
        return StyledText::new(clean_text(text.text()));
    }
    let paragraph = text.paragraph_style();
    let links = text.hyperlinks();
    let mut out = StyledText::default();
    for run in runs {
        let format = run.format;
        let link = links
            .iter()
            .find(|link| link.start() <= run.start && run.end <= link.end())
            .map(|link| Arc::from(link.target()));
        let piece = clean_text(run.text);
        if format.hidden() || piece.is_empty() {
            continue;
        }
        let piece = if format.math_formatting() {
            plain_math(&piece)
        } else {
            piece
        };
        let style = InlineStyle {
            bold: format.bold(),
            italic: format.italic(),
            underline: format.underline(),
            strike: format.strikethrough(),
            superscript: format.superscript(),
            subscript: format.subscript(),
            color: color_of(format.font_color()),
            highlight: color_of(format.highlight()),
            font: format.font().or(paragraph.font()).and_then(font_face),
            size: run_size(format.font_size(), paragraph.font_size(), body),
            link,
            ..InlineStyle::default()
        };
        let at = out.text.len();
        out.text.push_str(&piece);
        if !style.is_plain() {
            out.runs.push(StyleRun {
                start: at,
                end: out.text.len(),
                style,
            });
        }
    }
    out.normalize();
    out
}

/// An equation's letters and digits as plain ones. the desktop notebook writes them as Unicode's
/// mathematical alphanumerics (𝑎 for an italic a), which none of Folio's fonts draw; the
/// run keeps the desktop notebook's italic, so `𝑎=𝑏` still reads as a = b in italics.
fn plain_math(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            let plain = match code {
                // Thirteen Latin alphabets (bold, italic, script, …), A–Z then a–z.
                0x1D400..=0x1D6A3 => match (code - 0x1D400) % 52 {
                    n @ 0..=25 => 'A' as u32 + n,
                    n => 'a' as u32 + n - 26,
                },
                0x1D6A4 => 'ı' as u32,
                0x1D6A5 => 'ȷ' as u32,
                // Five Greek alphabets: Α–Ω (ϴ in Σ's gap), ∇, α–ω, ∂ and six variants.
                0x1D6A8..=0x1D7C9 => match (code - 0x1D6A8) % 58 {
                    17 => 0x3F4,
                    n @ 0..=24 => 0x391 + n,
                    25 => 0x2207,
                    n @ 26..=50 => 0x3B1 + n - 26,
                    51 => 0x2202,
                    n => [0x3F5, 0x3D1, 0x3F0, 0x3D5, 0x3F1, 0x3D6][(n - 52) as usize],
                },
                0x1D7CA => 0x3DC,
                0x1D7CB => 0x3DD,
                // Five sets of digits.
                0x1D7CE..=0x1D7FF => '0' as u32 + (code - 0x1D7CE) % 10,
                // The italic h sits outside the block, as the Planck constant.
                0x210E => 'h' as u32,
                _ => code,
            };
            char::from_u32(plain).unwrap_or(ch)
        })
        .collect()
}

/// A UTF-16 offset (the parser's unit) as a byte offset into `text` (Folio's unit).
fn utf16_to_byte(text: &str, units: u32) -> usize {
    let mut seen = 0u32;
    for (byte, ch) in text.char_indices() {
        if seen >= units {
            return byte;
        }
        seen += ch.len_utf16() as u32;
    }
    text.len()
}

/// A run's size in points when it differs from what the block shows anyway: plain text's
/// 11 pt, or a heading's own size (a heading run only keeps a size its paragraph doesn't).
fn run_size(run: Option<u16>, paragraph: Option<u16>, body: bool) -> Option<u8> {
    let half_points = if body {
        run.or(paragraph)?
    } else {
        run.filter(|size| Some(*size) != paragraph)?
    };
    let points = (f32::from(half_points) / 2.0).round().clamp(6.0, 96.0) as u8;
    (!(body && points == BODY_PT)).then_some(points)
}

fn color_of(color: Option<ColorRef>) -> Option<Rgb> {
    match color? {
        ColorRef::Manual { r, g, b } => Some(Rgb(r, g, b)),
        ColorRef::Auto => None,
    }
}

/// Folio's nearest bundled face for a the desktop notebook font; `None` keeps the default note font
/// (Calibri and its look-alike Carlito, and anything unknown). S7: return only variants that exist.
fn font_face(name: &str) -> Option<FontFace> {
    let name = name.to_lowercase();
    let any = |names: &[&str]| names.iter().any(|candidate| name.contains(candidate));
    if any(&[
        "consolas",
        "courier",
        "lucida console",
        "cascadia",
        "menlo",
        "monaco",
    ]) {
        Some(FontFace::Mono)
    } else if any(&[
        "comic sans",
        "segoe print",
        "segoe script",
        "ink free",
        "bradley hand",
    ]) {
        Some(FontFace::Hand)
    } else if name.contains("math") {
        None
    } else if any(&[
        "times",
        "cambria",
        "georgia",
        "garamond",
        "book antiqua",
        "palatino",
        "constantia",
    ]) {
        Some(FontFace::Serif)
    } else if any(&[
        "arial",
        "helvetica",
        "segoe ui",
        "verdana",
        "tahoma",
        "aptos",
        "trebuchet",
    ]) {
        Some(FontFace::Sans)
    } else {
        None
    }
}

/// What a the desktop notebook tag becomes in Folio.
#[derive(Debug, Clone, PartialEq)]
enum TagMatch {
    Todo,
    Line(LineTag),
    /// A tag Folio has no built-in for, kept with its name and look.
    Custom(CustomTag),
}

/// the desktop notebook's tags drawn as a box that can be ticked.
fn is_checkbox(shape: NoteTagShape) -> bool {
    use NoteTagShape::*;
    matches!(
        shape,
        GreenCheckBox
            | YellowCheckBox
            | BlueCheckBox
            | GreenStarCheckBox
            | YellowStarCheckBox
            | BlueStarCheckBox
            | GreenExclamationCheckBox
            | YellowExclamationCheckBox
            | BlueExclamationCheckBox
            | GreenRightArrowCheckBox
            | YellowRightArrowCheckBox
            | BlueRightArrowCheckBox
            | BlueCheckBox1
            | BlueCheckBox2
            | BlueCheckBox3
            | GreenCheckBox1
            | GreenCheckBox2
            | GreenCheckBox3
            | YellowCheckBox1
            | YellowCheckBox2
            | YellowCheckBox3
            | BluePersonCheckBox
            | YellowPersonCheckBox
            | GreenPersonCheckBox
            | BlueFlagCheckBox
            | RedFlagCheckBox
            | GreenFlagCheckBox
    )
}

/// The built-in tags the desktop notebook added after Folio's first eleven, by the name the desktop notebook gives
/// them in English or French. They are matched before the icons: most are boxes, which
/// the icons would take for To Do.
fn tag_by_name(label: &str) -> Option<TagMatch> {
    let tag = match label.trim().to_lowercase().as_str() {
        "web site to visit" | "site web à visiter" => LineTag::WebSite,
        "password" | "mot de passe" => LineTag::Password,
        "project a" | "projet a" => LineTag::ProjectA,
        "project b" | "projet b" => LineTag::ProjectB,
        "movie to see" | "film à voir" => LineTag::Movie,
        "music to listen to" | "musique à écouter" => LineTag::Music,
        "source for article" | "source pour un article" => LineTag::Source,
        "remember for blog" | "à retenir pour le blog" => LineTag::Blog,
        "discuss with person a" | "à discuter avec la personne a" => LineTag::DiscussA,
        "discuss with person b" | "à discuter avec la personne b" => LineTag::DiscussB,
        "discuss with manager" | "à discuter avec le responsable" => LineTag::DiscussManager,
        "send in email" | "à envoyer par courrier" => LineTag::SendEmail,
        "schedule meeting" | "planifier une réunion" => LineTag::ScheduleMeeting,
        "call back" | "rappeler" => LineTag::CallBack,
        "to do priority 1" | "tâche priorité 1" => LineTag::TodoPriority1,
        "to do priority 2" | "tâche priorité 2" => LineTag::TodoPriority2,
        "client request" | "demande du client" => LineTag::ClientRequest,
        _ => return None,
    };
    Some(TagMatch::Line(tag))
}

/// By the names of the desktop notebook's newer built-in tags first; then by the tag's icon; then by
/// the older tags' names, which the desktop notebook writes in the user's language (English and French
/// here); then, for icon-less tags, by highlight colour (green is Definition, other
/// colours Remember for later). A tag still unknown is kept as a custom tag with its name,
/// its box (if it has one) and its colours; `None` only for one with no name.
fn tag_of(tag: &NoteTag) -> Option<TagMatch> {
    use NoteTagShape::*;
    use TagMatch::{Line, Todo};
    let definition = tag.definition()?;
    let by_icon = || match definition.shape() {
        shape if is_checkbox(shape) => Some(Todo),
        YellowStar | BlueStar | GreenStar => Some(Line(LineTag::Important)),
        QuestionMark | QuestionBalloon => Some(Line(LineTag::Question)),
        ContactInformation | Contact | InstantMessagingContactPerson => {
            Some(Line(LineTag::Contact))
        }
        Home => Some(Line(LineTag::Address)),
        MobilePhone | TelephoneWithClock => Some(Line(LineTag::Phone)),
        LightBulb => Some(Line(LineTag::Idea)),
        HighPriority => Some(Line(LineTag::Critical)),
        OpenBook => Some(Line(LineTag::Book)),
        Pen => Some(Line(LineTag::Highlight)),
        _ => None,
    };
    tag_by_name(definition.label())
        .or_else(by_icon)
        .or_else(|| match definition.label().trim().to_lowercase().as_str() {
            "to do" | "à faire" => Some(Todo),
            "important" => Some(Line(LineTag::Important)),
            "question" => Some(Line(LineTag::Question)),
            "remember for later" => Some(Line(LineTag::RememberForLater)),
            "definition" | "définition" => Some(Line(LineTag::Definition)),
            "highlight" => Some(Line(LineTag::Highlight)),
            "contact" => Some(Line(LineTag::Contact)),
            "address" | "adresse" => Some(Line(LineTag::Address)),
            "phone number" | "numéro de téléphone" => Some(Line(LineTag::Phone)),
            "idea" | "idée" => Some(Line(LineTag::Idea)),
            "critical" | "critique" => Some(Line(LineTag::Critical)),
            "book to read" | "livre à lire" => Some(Line(LineTag::Book)),
            _ => None,
        })
        .or_else(
            || match (definition.shape(), color_of(definition.highlight_color())) {
                (NoIcon, Some(Rgb(r, g, b))) if g > r && g > b => Some(Line(LineTag::Definition)),
                (NoIcon, Some(_)) => Some(Line(LineTag::RememberForLater)),
                _ => None,
            },
        )
        .or_else(|| {
            let name = definition.label().trim();
            (!name.is_empty()).then(|| {
                TagMatch::Custom(CustomTag {
                    name: name.to_string(),
                    symbol: if is_checkbox(definition.shape()) {
                        TagSymbol::Checkbox
                    } else {
                        TagSymbol::None
                    },
                    font_color: color_of(definition.text_color()),
                    highlight: color_of(definition.highlight_color()),
                    checked: false,
                })
            })
        })
}

fn apply_tags(block: &mut Block, tags: &[NoteTag], report: &mut ImportReport) {
    for tag in tags {
        let done = tag.item_status().completed();
        match tag_of(tag) {
            Some(TagMatch::Todo) => block.kind = BlockKind::Todo { checked: done },
            Some(TagMatch::Line(line)) => {
                if !block.tags.contains(&line) {
                    block.tags.push(line);
                }
                // Call back, Discuss with…: the desktop notebook's tick comes along.
                if line.checkable() && done && !block.checked_tags.contains(&line) {
                    block.checked_tags.push(line);
                }
            }
            Some(TagMatch::Custom(mut custom)) => {
                if !block
                    .custom_tags
                    .iter()
                    .any(|kept| kept.name == custom.name)
                {
                    custom.checked = done && custom.symbol.checkable();
                    block.custom_tags.push(custom);
                }
            }
            None => {
                let name = tag
                    .definition()
                    .map(|definition| definition.label().trim().to_string());
                report.unknown_tags.insert(
                    name.filter(|name| !name.is_empty())
                        .unwrap_or_else(|| "?".into()),
                );
            }
        }
    }
    block.tags.sort_by_key(|tag| tag.rank());
    block.checked_tags.sort_by_key(|tag| tag.rank());
}

/// How far an import has got, shared with the dialog.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicUsize,
    /// 0 until the files are open and their pages counted.
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

/// Reads one notebook's pages into Folio pages; holds what they share.
struct Mapper<'a> {
    stager: Stager,
    report: ImportReport,
    /// the desktop notebook page ids (upper case, no braces) to the new pages' ids, for links.
    pages: HashMap<String, String>,
    progress: &'a Progress,
}

/// A picture that was staged: drawn, or kept as a file card when Folio can't draw it.
enum Picture {
    Shown(usize),
    Kept(Box<Block>),
}

impl Mapper<'_> {
    fn stage(
        &mut self,
        reader: impl Read,
        name: &str,
        extension: &str,
        mime: &str,
        page: &str,
    ) -> Option<usize> {
        match self.stager.stage(reader, name, extension, mime, page) {
            Staged::File(index) => Some(index),
            Staged::TooBig => {
                self.report.too_big.push(name.to_string());
                None
            }
            Staged::Failed => {
                self.report.missing_files += 1;
                None
            }
        }
    }

    /// A note container at the desktop notebook's position and width; `None` when it holds nothing.
    fn container(&mut self, outline: &Outline, page: &str) -> Option<CanvasItem> {
        let x = outline.offset_horizontal().unwrap_or(0.0) * half_inch();
        let y = outline.offset_vertical().unwrap_or(0.0) * half_inch();
        // one2html uses 13 half-inches when the desktop notebook stored no width.
        let width = outline.layout_max_width().unwrap_or(13.0) * half_inch();
        let mut blocks = Vec::new();
        for item in outline.items() {
            self.walk(item, 0, width, page, &mut blocks);
        }
        if blocks
            .iter()
            .all(|block| block.kind.is_textual() && block.text.text.trim().is_empty())
        {
            return None;
        }
        let mut doc = Document { blocks };
        doc.repair();
        // `new` leaves `auto_width` off: the note keeps the desktop notebook's width.
        Some(CanvasItem::new(CanvasKind::Text { doc }, x, y, width, 24.0))
    }

    fn walk(
        &mut self,
        item: &OutlineItem,
        level: u8,
        width: f32,
        page: &str,
        blocks: &mut Vec<Block>,
    ) {
        match item {
            OutlineItem::Group(group) => {
                for child in group.outlines() {
                    self.walk(child, level.saturating_add(1), width, page, blocks);
                }
            }
            OutlineItem::Element(element) => self.element(element, level, width, page, blocks),
        }
    }

    /// One outline paragraph with what it holds, then its children one level deeper.
    fn element(
        &mut self,
        element: &OutlineElement,
        level: u8,
        width: f32,
        page: &str,
        blocks: &mut Vec<Block>,
    ) {
        let level = level.min(MAX_LEVEL);
        // A bullet is a glyph; a number is U+FFFD and a format code.
        let list = element.list_contents().first();
        let numbered = list.is_some_and(|list| list.list_format().contains(&'\u{fffd}'));
        for content in element.contents() {
            let made: Vec<Block> = match content {
                Content::RichText(text) => {
                    let mut block = block_of(text, level, &mut self.report);
                    if let (Some(list), BlockKind::Paragraph) = (list, &block.kind) {
                        block.kind = if numbered {
                            BlockKind::Numbered
                        } else {
                            BlockKind::Bullet
                        };
                        // Like Folio's own lists, keep only a marker its level doesn't show.
                        block.marker = list_marker(list)
                            .filter(|marker| *marker != ListMarker::for_level(numbered, level));
                    }
                    vec![block]
                }
                Content::Image(image) => {
                    self.picture_block(image, width, page).into_iter().collect()
                }
                Content::Table(table) => self.table_block(table, width, page),
                Content::EmbeddedFile(file) => self.file_block(file, page).into_iter().collect(),
                Content::Ink(_) => {
                    self.report.inline_ink += 1;
                    Vec::new()
                }
                Content::Unknown => Vec::new(),
            };
            for mut block in made {
                block.indent = level;
                blocks.push(block);
            }
        }
        for child in element.children() {
            self.walk(child, level.saturating_add(1), width, page, blocks);
        }
    }

    /// Stages a picture. Folio draws PNG, JPEG and WebP; other formats become file cards. An
    /// Office printout page holds only the printed document (onenote_parser 2.0 has no picture
    /// of it): it is counted, not kept.
    fn picture(&mut self, image: &Image, page: &str) -> Option<Picture> {
        let Some(reader) = image.read() else {
            self.report.missing_files += 1;
            return None;
        };
        let Ok((head, reader)) = peek(reader) else {
            self.report.missing_files += 1;
            return None;
        };
        let name = image.image_filename().unwrap_or("").to_string();
        if let Some((extension, mime)) = picture_format(&head) {
            let index = self.stage(reader, &name, extension, mime, page)?;
            self.report.pictures += 1;
            return Some(Picture::Shown(index));
        }
        if image.displayed_page_number().is_some() && is_zip(&head) {
            self.report.office_printout_pages += 1;
            return None;
        }
        // The name's own extension, else the one the desktop notebook recorded, else the bytes': a file
        // card without one would open in nothing.
        let own = Path::new(&name).extension().and_then(|e| e.to_str());
        let extension = crate::db::safe_extension(
            own.or(image.extension())
                .filter(|e| !e.trim_start_matches('.').is_empty())
                .or(other_picture_extension(&head))
                .unwrap_or(""),
        );
        let name = match (name.is_empty(), own.is_some() || extension.is_empty()) {
            (true, true) => "picture".to_string(),
            (true, false) => format!("picture.{extension}"),
            (false, true) => name,
            (false, false) => format!("{name}.{extension}"),
        };
        let index = self.stage(
            reader,
            &name,
            &extension,
            crate::db::mime_for_extension(&extension),
            page,
        )?;
        self.report.unsupported_pictures += 1;
        Some(Picture::Kept(Box::new(self.file_card(index, false))))
    }

    /// A picture inside a note, as wide (as a share of the note) as the desktop notebook showed it.
    fn picture_block(&mut self, image: &Image, width: f32, page: &str) -> Option<Block> {
        match self.picture(image, page)? {
            Picture::Shown(index) => {
                let shown = image
                    .layout_max_width()
                    .or(image.picture_width())
                    .map(|w| w * half_inch());
                let share = shown.map_or(1.0, |w| (w / width.max(1.0)).clamp(0.1, 1.0));
                let attachment = self.stager.files[index].id.clone();
                let meta = PictureMeta {
                    alt: image.alt_text().unwrap_or("").trim().to_string(),
                    ..PictureMeta::default()
                };
                Some(Block::new(BlockKind::Image {
                    attachment,
                    width: share,
                    meta,
                }))
            }
            Picture::Kept(block) => Some(*block),
        }
    }

    /// An attached file as a file card, or as a recording when it is a WAV file (the format
    /// Folio plays). A file whose contents the desktop notebook did not keep is counted, not shown.
    fn file_block(&mut self, file: &EmbeddedFile, page: &str) -> Option<Block> {
        if !matches!(file.data_status(), FileDataStatus::Available) {
            self.report.missing_files += 1;
            return None;
        }
        let name = file.filename().to_string();
        let Ok((head, reader)) = peek(file.read()) else {
            self.report.missing_files += 1;
            return None;
        };
        let wav = is_wav(&head);
        let extension = if wav {
            "wav".to_string()
        } else {
            Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string()
        };
        let mime = if wav {
            "audio/wav"
        } else {
            crate::db::mime_for_extension(&extension)
        };
        let index = self.stage(reader, &name, &extension, mime, page)?;
        self.report.files += 1;
        Some(self.file_card(index, wav))
    }

    fn file_card(&self, index: usize, recording: bool) -> Block {
        let file = &self.stager.files[index];
        if recording {
            let duration_ms =
                crate::audio::duration_ms(&self.stager.dir.join(&file.stored)).unwrap_or(0);
            Block::new(BlockKind::Audio {
                attachment: file.id.clone(),
                duration_ms,
            })
        } else {
            Block::new(BlockKind::File {
                attachment: file.id.clone(),
                name: file.name.clone(),
                size: file.size,
            })
        }
    }

    /// A table as a table block. A desktop notebook cell is a small outline of its own: its
    /// paragraphs become the cell's lines, with their formatting. Pictures and files can't
    /// live in a cell, so they follow the table in the same note (`width` is the note's), in
    /// the order met: nothing staged is left without a page.
    fn table_block(&mut self, table: &Table, width: f32, page: &str) -> Vec<Block> {
        let widths: Vec<f32> = table.col_widths().iter().map(|w| w * half_inch()).collect();
        let mut rows = Vec::new();
        let mut after = Vec::new();
        for row in table.contents() {
            let mut cells = Vec::new();
            for cell in row.contents() {
                let mut blocks = Vec::new();
                for element in cell.contents() {
                    self.element(element, 0, width, page, &mut blocks);
                }
                // A cell holds text only: an equation in it stays as its text.
                for block in &mut blocks {
                    if let BlockKind::Equation { source, .. } = &block.kind {
                        *block = Block::paragraph(source.clone());
                        self.report.math += 1;
                    }
                }
                let (lines, other): (Vec<Block>, Vec<Block>) = blocks
                    .into_iter()
                    .partition(|block| block.kind.is_textual());
                after.extend(other);
                let mut text = StyledText::default();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        text.append(&StyledText::new("\n"));
                    }
                    text.append(&line.text);
                }
                cells.push(TableCell {
                    text,
                    shading: cell.background_color().map(rgb),
                    align: lines.first().map_or(ParaAlign::Left, |line| line.align),
                });
            }
            rows.push(cells);
        }
        if rows.is_empty() {
            return Vec::new();
        }
        self.report.tables += 1;
        let mut block = TableBlock {
            rows,
            widths,
            header: false,
            hide_borders: !table.borders_visible(),
        };
        block.repair();
        let mut out = vec![Block::new(BlockKind::Table(block))];
        out.extend(after);
        out
    }
}

/// The bullet or number a the desktop notebook list shows; `None` when Folio has no such marker, so the
/// item shows its level's default.
fn list_marker(list: &List) -> Option<ListMarker> {
    let format = list.list_format();
    if let Some(at) = format.iter().position(|c| *c == '\u{fffd}') {
        // U+FFFD stands for the number; the code after it is Word's number format (0 is 1, 1 is
        // I, 2 is i, 3 is A, 4 is a), and the other characters are written as they are.
        let before: String = format[..at].iter().collect();
        let code = u32::from(*format.get(at + 1)?);
        let after: String = format.get(at + 2..).unwrap_or_default().iter().collect();
        return match (before.as_str(), code, after.as_str()) {
            ("", 0, ".") => Some(ListMarker::Decimal),
            ("", 0, ")") => Some(ListMarker::DecimalParen),
            ("(", 0, ")") => Some(ListMarker::ParenDecimal),
            ("", 1, ".") => Some(ListMarker::UpperRoman),
            ("", 2, ".") => Some(ListMarker::LowerRoman),
            ("", 3, ".") => Some(ListMarker::UpperAlpha),
            ("", 4, ".") => Some(ListMarker::LowerAlpha),
            _ => None,
        };
    }
    let glyph = *format.first()?;
    // Symbol fonts may store their glyphs in the private use area, at U+F000 + the code.
    let glyph = match u32::from(glyph) {
        code @ 0xf020..=0xf0ff => char::from_u32(code - 0xf000)?,
        _ => glyph,
    };
    let wingdings = list
        .list_font()
        .is_some_and(|font| font.to_lowercase().contains("wingdings"));
    if wingdings {
        return match glyph {
            'l' => Some(ListMarker::Disc),
            '§' | 'n' => Some(ListMarker::Square),
            'Ø' | 'è' => Some(ListMarker::Arrow),
            'ü' | 'þ' => Some(ListMarker::Check),
            'u' | 'v' => Some(ListMarker::Diamond),
            '«' => Some(ListMarker::Star),
            _ => None,
        };
    }
    match glyph {
        '•' | '·' | '●' => Some(ListMarker::Disc),
        'o' | '○' | '◦' => Some(ListMarker::Circle),
        '▪' | '■' => Some(ListMarker::Square),
        '-' | '–' | '—' | '−' => Some(ListMarker::Dash),
        '➢' | '→' | '➔' | '►' => Some(ListMarker::Arrow),
        '✓' | '✔' => Some(ListMarker::Check),
        '◆' | '♦' | '❖' => Some(ListMarker::Diamond),
        '★' | '☆' | '☼' | '*' => Some(ListMarker::Star),
        _ => None,
    }
}

impl Mapper<'_> {
    fn entries(&mut self, entries: &[Entry]) -> Vec<PlanEntry> {
        let mut out = Vec::new();
        for entry in entries {
            if self.progress.cancel.load(Ordering::Relaxed) {
                break;
            }
            out.push(match entry {
                Entry::Section(section) => PlanEntry::Section(self.section(section)),
                Entry::Group(name, children) => {
                    self.report.groups += 1;
                    PlanEntry::Group {
                        name: name.clone(),
                        entries: self.entries(children),
                    }
                }
            });
        }
        out
    }

    fn section(&mut self, section: &Section) -> PlanSection {
        let mut pages: Vec<PlanPage> = Vec::new();
        for page in section
            .page_series()
            .iter()
            .flat_map(|series| series.pages())
        {
            if self.progress.cancel.load(Ordering::Relaxed) {
                break;
            }
            let id = uuid::Uuid::new_v4().to_string();
            self.pages
                .insert(guid_key(page.link_target_id()), id.clone());
            pages.push(self.page(page, id));
            self.report.pages += 1;
            self.progress.done.fetch_add(1, Ordering::Relaxed);
        }
        // The page list's rule: no page more than one level below the page above it.
        let mut levels: Vec<crate::outline::Entry> = pages
            .iter()
            .map(|page| crate::outline::Entry {
                id: page.id.clone(),
                level: page.level,
            })
            .collect();
        crate::outline::normalize(&mut levels);
        for (page, entry) in pages.iter_mut().zip(levels) {
            page.level = entry.level;
        }
        self.report.sections += 1;
        PlanSection {
            name: section
                .display_name()
                .trim_end_matches(".one")
                .trim()
                .to_string(),
            color: section
                .color()
                .map(|color| palette_color(rgb(color)).to_string())
                .unwrap_or_default(),
            pages,
        }
    }

    fn page(&mut self, page: &onenote_parser::page::Page, id: String) -> PlanPage {
        let mut canvas = CanvasDoc::default();
        for content in page.contents() {
            match content {
                PageContent::Outline(outline) => {
                    if let Some(item) = self.container(outline, &id) {
                        place(&mut canvas, item);
                    }
                }
                PageContent::Image(image) => {
                    if let Some(item) = self.page_picture(image, &id) {
                        place(&mut canvas, item);
                    }
                }
                PageContent::EmbeddedFile(file) => {
                    if let Some(card) = self.file_block(file, &id) {
                        let x = file.offset_horizontal().unwrap_or(1.0) * half_inch();
                        let y = file.offset_vertical().unwrap_or(0.0) * half_inch();
                        let doc = Document { blocks: vec![card] };
                        let note = CanvasItem::new(
                            CanvasKind::Text { doc },
                            x,
                            y,
                            13.0 * half_inch(),
                            24.0,
                        );
                        place(&mut canvas, note);
                    }
                }
                PageContent::Ink(ink) => self.ink(ink, (0.0, 0.0), &mut canvas.ink),
                PageContent::Unknown => {}
            }
        }
        clear_header(&mut canvas);
        canvas.repair();
        PlanPage {
            id,
            title: page_title(page),
            level: page
                .level()
                .saturating_sub(1)
                .clamp(0, i32::from(crate::outline::MAX_LEVEL)) as u8,
            created_at: page.created_time().unix_timestamp() * 1000,
            updated_at: page.updated_time().unix_timestamp() * 1000,
            canvas,
        }
    }

    /// A picture placed on the page (a printout page, a pasted screenshot) at the desktop notebook's
    /// position and size.
    fn page_picture(&mut self, image: &Image, page: &str) -> Option<CanvasItem> {
        let x = image.offset_horizontal().unwrap_or(0.0) * half_inch();
        let y = image.offset_vertical().unwrap_or(0.0) * half_inch();
        match self.picture(image, page)? {
            Picture::Shown(index) => {
                let file = &self.stager.files[index];
                let size = match (
                    image.layout_max_width().or(image.picture_width()),
                    image.layout_max_height().or(image.picture_height()),
                ) {
                    (Some(w), Some(h)) => (w * half_inch(), h * half_inch()),
                    // Pixels at 96 per inch: 0.75 pt each.
                    _ => image::image_dimensions(self.stager.dir.join(&file.stored))
                        .map(|(w, h)| (w as f32 * 0.75 * unit(), h as f32 * 0.75 * unit()))
                        .unwrap_or((320.0, 240.0)),
                };
                // The printout marker stays off: its `source` must be the PDF's attachment,
                // which onenote_parser 2.0 does not link to the page's pictures.
                let mut kind = CanvasKind::image(file.id.clone());
                if let CanvasKind::Image { meta, .. } = &mut kind {
                    meta.alt = image.alt_text().unwrap_or("").trim().to_string();
                    meta.background = image.is_background();
                }
                Some(CanvasItem::new(kind, x, y, size.0, size.1))
            }
            Picture::Kept(card) => {
                let doc = Document {
                    blocks: vec![*card],
                };
                Some(CanvasItem::new(
                    CanvasKind::Text { doc },
                    x,
                    y,
                    13.0 * half_inch(),
                    24.0,
                ))
            }
        }
    }

    /// Page ink. In onenote_parser 2.0 a stroke's path is its first point, then each point as
    /// an offset from the one before, in HIMETRIC, inside the group's box.
    fn ink(&mut self, ink: &Ink, origin: (f32, f32), out: &mut Vec<InkStroke>) {
        let origin = (
            origin.0 + ink.offset_horizontal().unwrap_or(0.0) * half_inch(),
            origin.1 + ink.offset_vertical().unwrap_or(0.0) * half_inch(),
        );
        let (bx, by) = ink.bounding_box().map_or((0.0, 0.0), |b| (b.x(), b.y()));
        for stroke in ink.ink_strokes() {
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let points: Vec<[f32; 2]> = stroke
                .path()
                .iter()
                .map(|point| {
                    x += point.x();
                    y += point.y();
                    [
                        origin.0 + (x - bx) * himetric(),
                        origin.1 + (y - by) * himetric(),
                    ]
                })
                .collect();
            if points.is_empty() {
                continue;
            }
            // A COLORREF is 0x00BBGGRR; none means automatic, which the desktop notebook draws black.
            let c = stroke.color().unwrap_or(0);
            out.push(InkStroke {
                points,
                color: Rgb(
                    (c & 0xff) as u8,
                    ((c >> 8) & 0xff) as u8,
                    ((c >> 16) & 0xff) as u8,
                ),
                width: (stroke.width() * himetric()).max(0.5),
                highlighter: stroke.transparency().is_some_and(|alpha| alpha > 0),
            });
            self.report.ink_strokes += 1;
        }
        for child in ink.child_groups() {
            self.ink(child, origin, out);
        }
    }
}

/// Adds an item where the desktop notebook put it. `CanvasDoc::add` keeps items on the page, which would
/// move an item near the top edge by itself; `clear_header` moves the whole page instead, so
/// notes, pictures and ink keep their places relative to each other.
fn place(canvas: &mut CanvasDoc, item: CanvasItem) {
    let (x, y) = (item.x, item.y);
    canvas.add(item);
    if let Some(added) = canvas.items.last_mut() {
        added.x = x;
        added.y = y;
    }
}

/// Moves a page's content below Folio's page title and date. the desktop notebook's own title sits in the
/// same place, so most pages move by a few units at most. Content left of the page edge is
/// also moved in.
fn clear_header(canvas: &mut CanvasDoc) {
    let points = || canvas.ink.iter().flat_map(|stroke| stroke.points.iter());
    let top = canvas
        .items
        .iter()
        .map(|item| item.y)
        .chain(points().map(|p| p[1]))
        .fold(f32::INFINITY, f32::min);
    let left = canvas
        .items
        .iter()
        .map(|item| item.x)
        .chain(points().map(|p| p[0]))
        .fold(f32::INFINITY, f32::min);
    let top_limit = crate::canvas::PAGE_CONTENT_TOP;
    let dy = if top.is_finite() && top < top_limit {
        top_limit - top
    } else {
        0.0
    };
    let dx = if left.is_finite() && left < 0.0 {
        -left
    } else {
        0.0
    };
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    for item in &mut canvas.items {
        item.x += dx;
        item.y += dy;
    }
    for stroke in &mut canvas.ink {
        for point in &mut stroke.points {
            point[0] += dx;
            point[1] += dy;
        }
    }
}

/// A desktop notebook page id as links name it: upper case, without braces.
fn guid_key(id: &str) -> String {
    id.trim_matches(|c| c == '{' || c == '}')
        .to_ascii_uppercase()
}

/// Points links to imported pages at the new pages. Other the desktop notebook links, and links Folio
/// would not open safely from someone else's file, keep their text without the link.
fn rewrite_links(
    entries: &mut [PlanEntry],
    pages: &HashMap<String, String>,
    report: &mut ImportReport,
) {
    for entry in entries {
        match entry {
            PlanEntry::Group { entries, .. } => rewrite_links(entries, pages, report),
            PlanEntry::Section(section) => {
                for page in &mut section.pages {
                    for item in &mut page.canvas.items {
                        if let Some(doc) = item.doc_mut() {
                            rewrite_doc(doc, pages, report);
                        }
                    }
                }
            }
        }
    }
}

fn rewrite_doc(doc: &mut Document, pages: &HashMap<String, String>, report: &mut ImportReport) {
    for block in &mut doc.blocks {
        rewrite_text(&mut block.text, pages, report);
        // A table's cells are styled text of their own.
        if let BlockKind::Table(table) = &mut block.kind {
            for cell in table.rows.iter_mut().flatten() {
                rewrite_text(&mut cell.text, pages, report);
            }
        }
    }
}

fn rewrite_text(text: &mut StyledText, pages: &HashMap<String, String>, report: &mut ImportReport) {
    for run in &mut text.runs {
        let Some(link) = run.style.link.take() else {
            continue;
        };
        run.style.link = imported_link(&link, pages);
        if run.style.link.is_none() {
            report.outside_links += 1;
        }
    }
    text.normalize();
}

fn imported_link(link: &str, pages: &HashMap<String, String>) -> Option<Arc<str>> {
    // `to_ascii_lowercase` keeps byte offsets, so `at` is valid in `link` too.
    let lower = link.to_ascii_lowercase();
    if ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        return Some(Arc::from(link));
    }
    if !lower.starts_with("onenote:") {
        return None;
    }
    let at = lower.find("page-id=")? + "page-id=".len();
    let guid: String = link[at..]
        .trim_start_matches('{')
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    pages
        .get(&guid.to_ascii_uppercase())
        .map(|id| Arc::from(page_link(id)))
}

/// An import running on its own thread.
pub struct ImportJob {
    name: String,
    progress: Arc<Progress>,
    handle: Option<JoinHandle<Result<ImportPlan, ImportError>>>,
}

impl ImportJob {
    /// Starts reading `source`. Its pictures and files are staged in a new `import-…` folder
    /// in `data_dir`, next to the attachments they will join.
    pub fn start(source: Source, data_dir: &Path) -> ImportJob {
        let name = source.name();
        let progress = Arc::new(Progress::default());
        let staging = data_dir.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
        let shared = Arc::clone(&progress);
        let handle = std::thread::Builder::new()
            .name("notebook-import".into())
            .spawn(move || {
                let result = read(&source, &staging, &shared);
                if result.is_err() {
                    let _ = fs::remove_dir_all(&staging);
                }
                result
            })
            .ok();
        ImportJob {
            name,
            progress,
            handle,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// (pages read, pages in all); the total is 0 while the files are being opened.
    pub fn progress(&self) -> (usize, usize) {
        (
            self.progress.done.load(Ordering::Relaxed),
            self.progress.total.load(Ordering::Relaxed),
        )
    }

    pub fn cancel(&self) {
        self.progress.cancel.store(true, Ordering::Relaxed);
    }

    /// The outcome once the thread has finished; `None` while it is still reading. A plan
    /// that arrives after Cancel was clicked (the reader had already passed its last check)
    /// is dropped with its staged files: the user cancelled, so nothing is written.
    pub fn poll(&mut self) -> Option<Result<ImportPlan, ImportError>> {
        if self
            .handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            return None;
        }
        Some(match self.handle.take().map(JoinHandle::join) {
            Some(Ok(Ok(plan))) if self.progress.cancel.load(Ordering::Relaxed) => {
                let _ = fs::remove_dir_all(&plan.staging);
                Err(ImportError::Cancelled)
            }
            Some(Ok(result)) => result,
            // The thread panicked or never started. Nothing reached the library, and its
            // staging folder is removed the next time the library opens.
            _ => Err(ImportError::Crashed {
                name: self.name.clone(),
            }),
        })
    }
}

/// Reads `source` into a plan, staging payloads in `staging`. Runs on the worker thread.
pub(crate) fn read(
    source: &Source,
    staging: &Path,
    progress: &Progress,
) -> Result<ImportPlan, ImportError> {
    let name = source.name();
    let stager = Stager::new(staging.to_path_buf()).map_err(|error| ImportError::Failed {
        name: name.clone(),
        error: error.to_string(),
    })?;
    let mut report = ImportReport::default();
    let parsed = parse(source, staging, &mut report)?;
    if parsed.entries.is_empty() {
        // A folder or package with nothing readable: say why rather than add an empty notebook.
        return Err(match report.password_sections.first() {
            Some(section) => ImportError::PasswordOrDamaged {
                name: section.clone(),
            },
            None => match report.unreadable_sections.first() {
                Some((section, error)) => failed(section.clone(), error.clone()),
                None => ImportError::Nothing { name },
            },
        });
    }
    progress
        .total
        .store(count_pages(&parsed.entries), Ordering::Relaxed);
    let mut mapper = Mapper {
        stager,
        report,
        pages: HashMap::new(),
        progress,
    };
    let mut entries = mapper.entries(&parsed.entries);
    if progress.cancel.load(Ordering::Relaxed) {
        return Err(ImportError::Cancelled);
    }
    let Mapper {
        stager,
        mut report,
        pages,
        ..
    } = mapper;
    rewrite_links(&mut entries, &pages, &mut report);
    // Without a colour of its own the notebook takes its first section's, else the palette's
    // first, as a new notebook does.
    let color = parsed
        .color
        .map(|color| palette_color(color).to_string())
        .or_else(|| first_section_color(&entries))
        .unwrap_or_else(|| crate::theme::SECTION_COLORS[0].to_string());
    Ok(ImportPlan {
        name,
        color,
        entries,
        files: stager.files,
        staging: staging.to_path_buf(),
        report,
    })
}

fn first_section_color(entries: &[PlanEntry]) -> Option<String> {
    entries.iter().find_map(|entry| match entry {
        PlanEntry::Section(section) => (!section.color.is_empty()).then(|| section.color.clone()),
        PlanEntry::Group { entries, .. } => first_section_color(entries),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staging() -> (tempfile::TempDir, Stager) {
        let dir = tempfile::tempdir().unwrap();
        let stager = Stager::new(dir.path().join("stage")).unwrap();
        (dir, stager)
    }

    #[test]
    fn newer_tags_are_found_by_their_english_or_french_name() {
        use crate::i18n::{pin_thread_lang, Lang};
        let line = |tag| Some(TagMatch::Line(tag));
        assert_eq!(tag_by_name("Call back"), line(LineTag::CallBack));
        assert_eq!(tag_by_name("rappeler"), line(LineTag::CallBack));
        assert_eq!(
            tag_by_name(" To Do priority 1 "),
            line(LineTag::TodoPriority1)
        );
        // An unknown name is left to `tag_of`, which keeps it as a custom tag; the older
        // tags go by their icon first.
        assert_eq!(tag_by_name("Homework"), None);
        assert_eq!(tag_by_name("Important"), None);
        // Each newer tag is found by the name Folio shows it under, in either language.
        let older = [
            LineTag::Important,
            LineTag::Question,
            LineTag::RememberForLater,
            LineTag::Definition,
            LineTag::Highlight,
            LineTag::Contact,
            LineTag::Address,
            LineTag::Phone,
            LineTag::Idea,
            LineTag::Critical,
            LineTag::Book,
        ];
        for lang in [Lang::En, Lang::Fr] {
            pin_thread_lang(lang);
            for tag in LineTag::ALL.into_iter().filter(|tag| !older.contains(tag)) {
                assert_eq!(tag_by_name(&tag.label()), line(tag), "{lang:?}");
            }
        }
        pin_thread_lang(Lang::En);
    }

    #[test]
    fn the_same_payload_is_staged_once() {
        let (_dir, mut stager) = staging();
        let first = stager.stage(&b"same bytes"[..], "a.png", "png", "image/png", "page-a");
        let again = stager.stage(&b"same bytes"[..], "b.png", "png", "image/png", "page-b");
        let other = stager.stage(&b"other bytes"[..], "c.png", "png", "image/png", "page-b");
        let (Staged::File(first), Staged::File(again), Staged::File(other)) = (first, again, other)
        else {
            panic!("staging failed");
        };
        assert_eq!(first, again);
        assert_ne!(first, other);
        assert_eq!(
            stager.files[first].owner, "page-a",
            "the first page owns a shared file"
        );
        let on_disk = fs::read_dir(&stager.dir).unwrap().count();
        assert_eq!(
            on_disk, 2,
            "one file per distinct payload, no leftover parts"
        );
    }

    #[test]
    fn equations_come_in_as_plain_letters() {
        assert_eq!(plain_math("𝑎=𝑏"), "a=b");
        assert_eq!(plain_math("𝐀𝐳 ℎ𝑥 𝟐𝟗"), "Az hx 29");
        assert_eq!(plain_math("𝛼+𝛽≤𝜋, 𝛩 𝛳 𝜕𝑓 𝛁"), "α+β≤π, Θ ϴ ∂f ∇");
        assert_eq!(plain_math("x² − √y"), "x² − √y", "the rest is left alone");
    }

    #[test]
    fn equations_folio_reads_become_equations_and_the_rest_stays_text() {
        assert_eq!(linear_equation("𝑎=𝑏").as_deref(), Some("a=b"));
        assert_eq!(linear_equation(" 𝑥^2+1 ").as_deref(), Some("x^2+1"));
        // A built-up fraction: the desktop notebook's object marks and argument separators.
        assert_eq!(linear_equation("\u{fdd0}𝑎\u{fdee}𝑏\u{fdef}"), None);
        assert_eq!(linear_equation("𝑎\u{b}𝑏"), None, "two lines");
        assert_eq!(linear_equation("\u{f020}"), None, "a symbol-font glyph");
        assert_eq!(linear_equation("  "), None);
    }

    #[test]
    fn line_breaks_inside_a_paragraph_become_folio_ones() {
        // Shift+Enter in the desktop notebook is U+000B; egui would draw it as a box.
        assert_eq!(clean_text("one\u{b}two"), "one\ntwo");
        assert_eq!(clean_text("one\r\ntwo\rthree"), "one\ntwo\nthree");
        assert_eq!(clean_text("\u{fddf}HYPERLINK"), "HYPERLINK");
        assert_eq!(clean_text("one\u{b}two").len(), "one\u{b}two".len());
        // A page title stays on one line.
        assert_eq!(one_line(" Plan\u{b}for\tMay\n"), "Plan for May");
    }

    #[test]
    fn page_titles_come_from_the_title_box_only() {
        // `Page::title_text` falls back to the first line of the page when the title box is
        // empty; `page_title` reads the title box alone. These fixtures' title boxes hold
        // text, or nothing above an empty first line.
        for (path, titles) in [
            (
                "mpl/handwriting_recognition.one",
                &["Test Page", "Test Page"][..],
            ),
            ("mit/AlternativePackaging.one", &["Test Suite Sample"][..]),
            ("mit/OneWithFileData.one", &[""][..]),
            (
                "mit/OneWithoutFileData.one",
                &["The OneNote file without file data"][..],
            ),
        ] {
            let section = read_section_file(&fixture(path)).unwrap();
            let found: Vec<String> = section
                .page_series()
                .iter()
                .flat_map(|series| series.pages())
                .map(page_title)
                .collect();
            assert_eq!(found, titles, "{path}");
        }
    }

    #[test]
    fn section_colours_take_the_nearest_palette_colour() {
        use crate::theme::SECTION_COLORS;
        for hex in SECTION_COLORS {
            let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap();
            let color = Rgb(channel(1), channel(3), channel(5));
            assert_eq!(palette_color(color), hex, "a palette colour is itself");
        }
        // the 2016 desktop notebook's paler colours, as the fixtures store them: Blue Mist, Yellow, Tan
        // and Red Chalk.
        assert_eq!(palette_color(Rgb(0x8a, 0xa8, 0xe4)), SECTION_COLORS[8]);
        assert_eq!(palette_color(Rgb(0xff, 0xd8, 0x69)), SECTION_COLORS[1]);
        assert_eq!(palette_color(Rgb(0xe8, 0xd1, 0x99)), SECTION_COLORS[10]);
        assert_eq!(palette_color(Rgb(0xba, 0x75, 0x75)), SECTION_COLORS[14]);
        // Greys go to the palette's grey, Silver, whatever their lightness.
        for grey in [Rgb(0, 0, 0), Rgb(0x95, 0x95, 0xaa), Rgb(255, 255, 255)] {
            assert_eq!(palette_color(grey), SECTION_COLORS[15]);
        }
    }

    #[test]
    fn pictures_folio_cannot_draw_keep_an_extension() {
        assert_eq!(other_picture_extension(b"GIF89a...."), Some("gif"));
        assert_eq!(other_picture_extension(b"BM6\0\0\0"), Some("bmp"));
        assert_eq!(other_picture_extension(b"II*\0\x08\0"), Some("tif"));
        assert_eq!(other_picture_extension(b"MM\0*\0\0"), Some("tif"));
        assert_eq!(
            other_picture_extension(&[0xd7, 0xcd, 0xc6, 0x9a, 0, 0]),
            Some("wmf")
        );
        assert_eq!(
            other_picture_extension(&[1, 0, 0, 0, 108, 0, 0, 0]),
            Some("emf")
        );
        assert_eq!(other_picture_extension(&[1, 0, 0, 0, 4, 0, 0, 0]), None);
        assert_eq!(other_picture_extension(b"plain text"), None);
    }

    #[test]
    fn a_huge_package_or_a_stopped_reader_is_worded_by_folio() {
        assert_eq!(
            failed("Notes".into(), READER_STOPPED.into()),
            ImportError::Crashed {
                name: "Notes".into()
            }
        );
        assert_eq!(
            failed("Notes".into(), "No such file".into()),
            ImportError::Failed {
                name: "Notes".into(),
                error: "No such file".into()
            }
        );
        // A package whose two files claim 4 GB each is refused before anything is unpacked.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.onepkg");
        let mut builder = cab::CabinetBuilder::new();
        let folder = builder.add_folder(cab::CompressionType::None);
        folder.add_file("a.one");
        folder.add_file("b.one");
        let mut writer = builder.build(fs::File::create(&path).unwrap()).unwrap();
        while let Some(mut file) = writer.next_file().unwrap() {
            io::Write::write_all(&mut file, b"tiny").unwrap();
        }
        writer.finish().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        for name in [&b"a.one\0"[..], &b"b.one\0"[..]] {
            // A file entry is its uncompressed size, then 12 more bytes, then its name.
            let at = bytes
                .windows(name.len())
                .position(|window| window == name)
                .unwrap()
                - 16;
            bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        fs::write(&path, &bytes).unwrap();
        let out = dir.path().join("out");
        assert_eq!(
            unpack(&path, &out),
            Err(ImportError::TooLarge {
                name: "huge".into()
            })
        );
        assert!(!out.exists(), "nothing was unpacked");
    }

    #[test]
    fn payloads_are_recognised_by_their_bytes() {
        assert_eq!(
            picture_format(b"\x89PNG\r\n\x1a\n"),
            Some(("png", "image/png"))
        );
        assert_eq!(
            picture_format(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(("jpg", "image/jpeg"))
        );
        assert_eq!(picture_format(b"GIF89a"), None);
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(is_wav(b"RIFF\0\0\0\0WAVEfmt "));
        let (head, mut all) = peek(Box::new(&b"0123456789abcdefXYZ"[..])).unwrap();
        assert_eq!(head, b"0123456789abcdef");
        let mut back = Vec::new();
        all.read_to_end(&mut back).unwrap();
        assert_eq!(back, b"0123456789abcdefXYZ");
    }

    pub(super) fn fixture(path: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/notebooks")
            .join(path)
    }

    #[test]
    fn package_names_cannot_leave_the_folder() {
        assert_eq!(
            safe_relative("Group\\Section.one"),
            Some(PathBuf::from("Group").join("Section.one"))
        );
        for bad in ["..\\x.one", "C:\\x.one", "/etc/passwd", "a\\\\b", ""] {
            assert_eq!(safe_relative(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn sources_are_told_apart_and_named() {
        let section = Source::from_path(&fixture("mpl/handwriting_recognition.one")).unwrap();
        assert!(matches!(section, Source::Section(_)));
        assert_eq!(section.name(), "handwriting_recognition");
        let toc = Source::from_path(&fixture("mpl/notebook/Open Notebook.onetoc2")).unwrap();
        assert!(matches!(&toc, Source::Folder(dir) if dir.ends_with("notebook")));
        assert_eq!(toc.name(), "notebook");
        assert!(matches!(
            Source::from_path(&fixture("mit/interop-desktop-stored.onepkg")),
            Some(Source::Package(_))
        ));
        assert!(Source::from_path(&fixture("README.md")).is_none());
    }

    #[test]
    fn a_folder_brings_its_unlisted_section_group() {
        let mut report = ImportReport::default();
        let dir = tempfile::tempdir().unwrap();
        let parsed = parse(
            &Source::Folder(fixture("mpl/notebook")),
            dir.path(),
            &mut report,
        )
        .unwrap();
        let names: Vec<String> = parsed
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Section(section) => section.display_name().to_string(),
                Entry::Group(name, entries) => format!("{name} ({})", entries.len()),
            })
            .collect();
        assert_eq!(names, ["New Section 1", "New Section Group (2)"]);
        assert_eq!(count_pages(&parsed.entries), 4);
    }

    #[test]
    fn a_password_protected_section_says_so() {
        let mut report = ImportReport::default();
        let dir = tempfile::tempdir().unwrap();
        let result = parse(
            &Source::Section(fixture("mit/Encryption.one")),
            dir.path(),
            &mut report,
        );
        assert!(
            matches!(result, Err(ImportError::PasswordOrDamaged { name }) if name == "Encryption")
        );
    }

    #[test]
    fn a_package_reads_its_sections_in_order() {
        let mut report = ImportReport::default();
        let dir = tempfile::tempdir().unwrap();
        let parsed = parse(
            &Source::Package(fixture("mit/interop-desktop-stored.onepkg")),
            dir.path(),
            &mut report,
        )
        .unwrap();
        let names: Vec<&str> = parsed
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Section(section) => section.display_name(),
                Entry::Group(name, _) => name.as_str(),
            })
            .collect();
        assert_eq!(names, ["OneWithoutFileData", "OneWithFileData"]);
        assert_eq!(parsed.color, Some(Rgb(186, 117, 117)));
    }

    #[test]
    fn links_point_at_imported_pages_and_drop_the_rest() {
        let mut pages = HashMap::new();
        pages.insert(
            "62800B88-EC08-4170-BDB6-885CBB47FF99".to_string(),
            "new-page".to_string(),
        );
        let inside = "onenote:https://d.docs.live.net/x/Another%20section.one#Page%202&section-id={C271F3B1-5F22-457F-9DEA-F2B938D9B3D7}&page-id={62800b88-ec08-4170-bdb6-885cbb47ff99}&end";
        assert_eq!(
            imported_link(inside, &pages).as_deref(),
            Some("folio://page/new-page")
        );
        assert_eq!(
            imported_link("https://example.com", &pages).as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            imported_link(
                "onenote:#Other&page-id={00000000-0000-0000-0000-000000000000}&end",
                &pages
            ),
            None
        );
        assert_eq!(imported_link("file:///etc/passwd", &pages), None);
        assert_eq!(imported_link("javascript:alert(1)", &pages), None);
    }

    #[test]
    fn content_moves_below_the_page_title() {
        let mut canvas = CanvasDoc::default();
        let doc = Document::default();
        place(
            &mut canvas,
            CanvasItem::new(CanvasKind::Text { doc }, -10.0, 10.0, 300.0, 24.0),
        );
        canvas.ink.push(InkStroke {
            points: vec![[0.0, 5.0]],
            color: Rgb(0, 0, 0),
            width: 1.0,
            highlighter: false,
        });
        clear_header(&mut canvas);
        let top = crate::canvas::PAGE_CONTENT_TOP;
        assert_eq!(canvas.ink[0].points[0], [10.0, top]);
        assert_eq!((canvas.items[0].x, canvas.items[0].y), (0.0, top + 5.0));
    }

    #[test]
    fn the_job_reads_on_its_own_thread_and_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let mut job = ImportJob::start(
            Source::Section(fixture("mpl/handwriting_recognition.one")),
            dir.path(),
        );
        let plan = loop {
            if let Some(outcome) = job.poll() {
                break outcome.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(job.progress(), (2, 2));
        assert!(plan.staging.starts_with(dir.path()));
        assert!(plan
            .staging
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX));

        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let staging = dir.path().join("cancelled");
        let outcome = read(
            &Source::Section(fixture("mpl/handwriting_recognition.one")),
            &staging,
            &progress,
        );
        assert_eq!(outcome.unwrap_err(), ImportError::Cancelled);
    }

    #[test]
    fn cancel_after_the_reader_finished_still_cancels() {
        let dir = tempfile::tempdir().unwrap();
        let mut job = ImportJob::start(
            Source::Section(fixture("mpl/handwriting_recognition.one")),
            dir.path(),
        );
        // The reader is done, but no frame has polled it yet when Cancel is clicked.
        while job
            .handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        job.cancel();
        assert_eq!(job.poll().unwrap().unwrap_err(), ImportError::Cancelled);
        let left: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert!(left.is_empty(), "the staged files are removed: {left:?}");
    }

    // Mapping, fixture by fixture. The expected values come from the parser itself (the
    // `p9check` spike), turned into Folio units with `half_inch()` = 36 pt at `pt_to_px`.

    fn plan_of(path: &str) -> (tempfile::TempDir, ImportPlan) {
        let dir = tempfile::tempdir().unwrap();
        let source = Source::from_path(&fixture(path)).unwrap();
        let plan = read(&source, &dir.path().join("stage"), &Progress::default()).unwrap();
        (dir, plan)
    }

    fn sections(entries: &[PlanEntry]) -> Vec<&PlanSection> {
        entries
            .iter()
            .flat_map(|entry| match entry {
                PlanEntry::Section(section) => vec![section],
                PlanEntry::Group { entries, .. } => sections(entries),
            })
            .collect()
    }

    fn notes(page: &PlanPage) -> Vec<&Document> {
        page.canvas
            .items
            .iter()
            .filter_map(|item| item.doc())
            .collect()
    }

    fn blocks(page: &PlanPage) -> Vec<&Block> {
        notes(page)
            .into_iter()
            .flat_map(|doc| doc.blocks.iter())
            .collect()
    }

    #[test]
    fn a_rich_desktop_section_keeps_its_outline_tags_links_and_ink() {
        let (_dir, plan) = plan_of("mpl/handwriting_recognition.one");
        let report = &plan.report;
        assert_eq!(
            (
                report.sections,
                report.pages,
                report.pictures,
                report.tables
            ),
            (1, 2, 1, 2)
        );
        assert_eq!(
            (
                report.ink_strokes,
                report.math,
                report.inline_ink,
                report.outside_links
            ),
            (62, 0, 0, 0),
            "the equation comes in as an equation, so it is not reported"
        );
        assert!(report.unknown_tags.is_empty());
        let section = sections(&plan.entries)[0];
        assert_eq!(
            (section.name.as_str(), section.color.as_str()),
            ("handwriting_recognition", crate::theme::SECTION_COLORS[8]),
            "the 2016 desktop notebook's #8AA8E4 is shown as Blue Mist"
        );
        let page = &section.pages[0];
        assert_eq!(
            (page.title.as_str(), page.level, page.created_at),
            ("Test Page", 0, 1_603_795_643_000)
        );
        // The second outline holds one empty paragraph: no empty container is made.
        let items: Vec<_> = page
            .canvas
            .items
            .iter()
            .filter(|item| item.doc().is_some())
            .collect();
        assert_eq!(items.len(), 1);
        let near = |a: f32, b: f32| (a - b).abs() < 0.1;
        assert!(near(items[0].x, 48.0), "{}", items[0].x);
        assert!(
            near(items[0].y, 115.2f32.max(crate::canvas::PAGE_CONTENT_TOP)),
            "{}",
            items[0].y
        );
        assert!(near(items[0].width, 576.48), "{}", items[0].width);
        let blocks = blocks(page);
        let find = |text: &str| {
            blocks
                .iter()
                .find(|block| block.text.text == text)
                .copied()
                .unwrap_or_else(|| panic!("{text:?}"))
        };
        assert!(matches!(find("ABCDEF").kind, BlockKind::Paragraph));
        assert!(
            blocks
                .iter()
                .any(|b| b.text.text == "ABCDEF" && b.kind == BlockKind::Heading { level: 2 }),
            "the desktop notebook's h1 is Folio's Heading 1, one level below Page Title"
        );
        assert!(matches!(
            find("ABCDEFG").kind,
            BlockKind::Todo { checked: true }
        ));
        let unticked = blocks
            .iter()
            .find(|b| b.text.text == "ABCDEF" && matches!(b.kind, BlockKind::Todo { .. }))
            .unwrap();
        assert_eq!(
            (unticked.kind.clone(), unticked.indent),
            (BlockKind::Todo { checked: false }, 1)
        );
        assert_eq!(find("ABCDEFGH").tags, vec![LineTag::Important]);
        let bullets: Vec<u8> = blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Bullet)
            .map(|b| b.indent)
            .collect();
        assert_eq!(bullets, [1, 2, 3]);
        assert_eq!(
            blocks
                .iter()
                .filter(|b| b.kind == BlockKind::Numbered)
                .count(),
            6
        );
        assert!(blocks.iter().any(|b| b.align == ParaAlign::Center));
        assert!(blocks.iter().any(|b| b.align == ParaAlign::Right));
        let link = blocks
            .iter()
            .flat_map(|b| b.text.runs.iter().map(move |run| (b, run)))
            .find(|(_, run)| run.style.link.as_deref() == Some("https://example.com"))
            .expect("the link survives");
        assert_eq!(&link.0.text.text[link.1.start..link.1.end], "magna");
        assert!(!blocks
            .iter()
            .any(|b| b.text.text.contains('\u{fddf}') || b.text.text.contains("HYPERLINK")));
        assert!(blocks
            .iter()
            .any(|b| b.text.runs.iter().any(|run| run.style.bold)));
        assert!(blocks
            .iter()
            .any(|b| matches!(b.kind, BlockKind::Image { .. })));
        // the desktop notebook writes the equation as 𝑎=𝑏 (mathematical italics); it comes in as an
        // equation in linear format, with plain letters, which the Equation tab edits.
        let equation = BlockKind::Equation {
            source: "a=b".into(),
            scale: crate::model::default_equation_size(),
        };
        assert_eq!(blocks.iter().filter(|b| b.kind == equation).count(), 1);
        assert!(!blocks.iter().any(|b| b.text.text.contains("a=b")));
        assert_eq!(page.canvas.ink.len(), 62);
        // Arial is not Calibri, so plain runs say so (S7: FontFace::Sans exists).
        assert!(blocks.iter().any(|b| b
            .text
            .runs
            .iter()
            .any(|run| run.style.font == Some(FontFace::Sans))));
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].mime, "image/jpeg");
        assert!(plan.staging.join(&plan.files[0].stored).is_file());
    }

    #[test]
    fn a_onedrive_notebook_folder_keeps_groups_printouts_and_files() {
        let (_dir, plan) = plan_of("mpl/notebook");
        assert_eq!(plan.name, "notebook");
        let report = &plan.report;
        assert_eq!((report.sections, report.groups, report.pages), (3, 1, 4));
        assert_eq!(
            (report.pictures, report.files, report.ink_strokes),
            (2, 1, 1)
        );
        assert!(
            matches!(&plan.entries[1], PlanEntry::Group { name, entries } if name == "New Section Group" && entries.len() == 2)
        );
        let all = sections(&plan.entries);
        let printout = all[2]
            .pages
            .iter()
            .find(|page| page.title == "Test Page 3")
            .unwrap();
        let picture = printout
            .canvas
            .items
            .iter()
            .find(|item| matches!(item.kind, CanvasKind::Image { .. }))
            .unwrap();
        assert!((picture.x - 44.0).abs() < 0.1, "{}", picture.x);
        assert!(
            (picture.y - 97.96f32.max(crate::canvas::PAGE_CONTENT_TOP)).abs() < 0.1,
            "{}",
            picture.y
        );
        assert!((picture.width - 595.0).abs() < 0.1 && (picture.height - 842.0).abs() < 0.1);
        let with_file = all[2]
            .pages
            .iter()
            .find(|page| page.title == "Test Page 4")
            .unwrap();
        let card = blocks(with_file)
            .into_iter()
            .find(|b| matches!(b.kind, BlockKind::File { .. }))
            .expect("the MP3 as a file card");
        assert!(
            matches!(&card.kind, BlockKind::File { name, size, .. } if name == "ff-16b-2c-44100hz.mp3" && *size == 77_279)
        );
    }

    #[test]
    fn a_package_keeps_section_colours_and_attached_files() {
        let (_dir, plan) = plan_of("mit/interop-desktop-stored.onepkg");
        // the 2016 desktop notebook's #BA7575, #FFD869 and #E8D199 as the palette's Red Chalk, Yellow and
        // Tan, so the Section Color menu ticks and names them.
        use crate::theme::SECTION_COLORS;
        assert_eq!(
            (plan.name.as_str(), plan.color.as_str()),
            ("interop-desktop-stored", SECTION_COLORS[14])
        );
        let all = sections(&plan.entries);
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["OneWithoutFileData", "OneWithFileData"]);
        assert_eq!(
            (all[0].color.as_str(), all[1].color.as_str()),
            (SECTION_COLORS[1], SECTION_COLORS[10])
        );
        assert_eq!(
            all[1].pages[0].title, "",
            "an untitled the desktop notebook page stays untitled"
        );
        let card = blocks(&all[1].pages[0])
            .into_iter()
            .find(|b| matches!(b.kind, BlockKind::File { .. }))
            .unwrap();
        assert!(
            matches!(&card.kind, BlockKind::File { name, size, .. } if name == "testing.docx" && *size == 14_660)
        );
        assert!(blocks(&all[0].pages[0])[0]
            .text
            .text
            .starts_with("This is a test file"));
        assert_eq!(
            plan.files[0].mime,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
    }

    #[test]
    fn a_onedrive_section_with_a_page_title_style_imports() {
        let (_dir, plan) = plan_of("mit/AlternativePackaging.one");
        let page = &sections(&plan.entries)[0].pages[0];
        assert_eq!(page.title, "Test Suite Sample");
        // The second outline is empty and is left out.
        assert_eq!(notes(page).len(), 1);
        assert_eq!(blocks(page)[0].text.text, "Test Suite Sample");
    }
}
