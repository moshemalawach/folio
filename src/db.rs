use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use rusqlite::functions::FunctionFlags;
use rusqlite::types::ValueRef;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use uuid::Uuid;

use crate::i18n::t;
use crate::import_notebook::{ImportPlan, ImportSummary, PlanEntry};
use crate::model::{
    now_ms, Attachment, CanvasDoc, CanvasItem, CanvasKind, Document, Notebook, Page, PageStyle,
    PageSummary, PageVersion, RuleLines, SearchHit, Section, SectionGroup, Tag, TrashedSection,
    Workspace, PAGE_CONTENT_TOP, PAGE_LEFT,
};
use crate::outline;
use crate::seed;
use crate::util;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("{}", sql_error_text(.0))]
    Sql(#[from] rusqlite::Error),
    #[error("{}", file_error_text(.0))]
    Io(#[from] std::io::Error),
    #[error("{}", data_error_text(.0))]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Message(String),
}

// The wording around a library or OS error, in the UI language (the error itself is the
// system's).
fn sql_error_text(error: &rusqlite::Error) -> String {
    t!("data-error-database", error = error.to_string()).into_owned()
}
fn file_error_text(error: &std::io::Error) -> String {
    t!("data-error-file", error = error.to_string()).into_owned()
}
fn data_error_text(error: &serde_json::Error) -> String {
    t!("data-error-data", error = error.to_string()).into_owned()
}

pub type DbResult<T> = Result<T, DbError>;

/// Files bigger than this are refused, so one stray video can't swell the notebook.
pub const MAX_ATTACHMENT_BYTES: u64 = 200 * 1024 * 1024;

/// The media type recorded for an attachment, from its file extension.
pub fn mime_for_extension(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn check_attachment_size(name: &str, bytes: u64) -> DbResult<()> {
    if bytes > MAX_ATTACHMENT_BYTES {
        return Err(DbError::Message(
            t!(
                "data-error-attachment-too-big",
                name = name,
                size = util::file_size(bytes),
                max = util::file_size(MAX_ATTACHMENT_BYTES)
            )
            .into_owned(),
        ));
    }
    Ok(())
}

/// Stored files are named by id; the extension is kept only when it is plain.
pub(crate) fn safe_extension(extension: &str) -> String {
    let extension = extension.trim_start_matches('.');
    if extension.len() <= 12 && extension.chars().all(|c| c.is_ascii_alphanumeric()) {
        extension.to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// A file name that is safe to create on any system, keeping what can be kept.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.');
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned.to_string()
    }
}

/// The name a copy of an attachment gets outside the notebook (Open, Save as…): its
/// original name made safe, plus the stored file's extension when the name has none
/// (a pasted screenshot is "Pasted image"), so other apps know what it is.
pub fn attachment_file_name(meta: &Attachment) -> String {
    let name = safe_file_name(&meta.file_name);
    let stored = Path::new(&meta.stored_path)
        .extension()
        .and_then(|e| e.to_str());
    match stored {
        Some(extension) if Path::new(&name).extension().is_none() => {
            format!("{name}.{extension}")
        }
        _ => name,
    }
}

/// Whether two files hold the same bytes.
pub(crate) fn same_contents(a: &Path, b: &Path) -> io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut a, mut b) = (
        io::BufReader::new(fs::File::open(a)?),
        io::BufReader::new(fs::File::open(b)?),
    );
    let (mut left, mut right) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let read = a.read(&mut left)?;
        if read == 0 {
            return Ok(true);
        }
        b.read_exact(&mut right[..read])?;
        if left[..read] != right[..read] {
            return Ok(false);
        }
    }
}

/// What `Library::attachment_open_copy` handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenCopy {
    pub path: PathBuf,
    /// The copy was changed in another app after Folio made it. It is kept as it is
    /// (never replaced with the page's file), but those changes are not in the page.
    pub edited: bool,
}

/// Where Find Tags looks.
#[derive(Debug, Clone, Copy)]
pub enum TagScope<'a> {
    Section(&'a str),
    Notebook(&'a str),
    All,
}

#[derive(Debug, Clone, Copy)]
pub enum PageFilter<'a> {
    Section(&'a str),
    All,
    Trash,
}

/// What a search looks in.
#[derive(Debug, Clone, Copy)]
pub enum SearchFilter<'a> {
    All,
    Notebook(&'a str),
    Sections(&'a [String]),
}

/// A page must be left alone this long before the next change keeps a copy of it.
const VERSION_INTERVAL_MS: i64 = 10 * 60 * 1000;
/// The settings that mark `open_at`'s one-shot upgrades as done, each with the value its
/// step sets. A new library gets them all at once (see `open_at`).
const SEEDED_STEPS: &[(&str, &str)] = &[
    ("one_page_type_version", "1"),
    ("page_order_version", "1"),
    ("table_block_version", "1"),
];
/// Versions kept per page; the oldest are dropped first.
const MAX_VERSIONS: i64 = 100;

pub struct Library {
    conn: Connection,
    data_dir: PathBuf,
    database_path: PathBuf,
    attachments_dir: PathBuf,
    version_interval_ms: i64,
}

impl Library {
    pub fn open_default() -> DbResult<Self> {
        let data_dir = default_data_dir();
        Self::open_at(data_dir)
    }

    pub fn open_at(data_dir: PathBuf) -> DbResult<Self> {
        fs::create_dir_all(&data_dir)?;
        let attachments_dir = data_dir.join("attachments");
        fs::create_dir_all(&attachments_dir)?;
        let database_path = data_dir.join("folio.db");
        let conn = Connection::open(&database_path)?;
        let mut lib = Self {
            conn,
            data_dir,
            database_path,
            attachments_dir,
            version_interval_ms: VERSION_INTERVAL_MS,
        };
        lib.configure()?;
        lib.migrate()?;
        let has_been_seeded = lib.setting("seed_version")?.is_some();
        if !has_been_seeded && lib.notebook_count()? == 0 {
            let files = seed::install(&mut lib.conn, crate::i18n::lang())?;
            for file in files {
                fs::write(lib.attachments_dir.join(&file.stored_path), &file.bytes)?;
            }
            // The sample library is written in today's shape: the one-shot upgrades below
            // are for libraries made by older versions (page order, for one, would
            // flatten the sample's subpages).
            for (key, value) in SEEDED_STEPS {
                lib.set_setting(key, value)?;
            }
        }
        lib.set_setting("seed_version", "1")?;
        if lib.setting("one_page_type_version")?.is_none() {
            lib.one_page_type()?;
            lib.set_setting("one_page_type_version", "1")?;
        }
        if lib.setting("page_order_version")?.is_none() {
            lib.assign_page_order()?;
            lib.set_setting("page_order_version", "1")?;
        }
        if lib.setting("table_block_version")?.is_none() {
            lib.convert_canvas_tables()?;
            lib.set_setting("table_block_version", "1")?;
        }
        lib.prune_open_copies();
        lib.prune_import_staging();
        Ok(lib)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> DbResult<Self> {
        let conn = Connection::open_in_memory()?;
        let dir = std::env::temp_dir().join(format!("folio-test-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("attachments"))?;
        let mut lib = Self {
            conn,
            data_dir: dir.clone(),
            database_path: dir.join("folio.db"),
            attachments_dir: dir.join("attachments"),
            version_interval_ms: VERSION_INTERVAL_MS,
        };
        lib.configure()?;
        lib.migrate()?;
        Ok(lib)
    }

    /// Whether this is `open_in_memory`'s folder, which goes away with the library.
    #[cfg(test)]
    fn is_test_dir(&self) -> bool {
        self.data_dir.parent() == Some(std::env::temp_dir().as_path())
            && self
                .data_dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("folio-test-"))
    }

    #[cfg(test)]
    pub fn set_version_interval_ms(&mut self, interval: i64) {
        self.version_interval_ms = interval;
    }

    fn configure(&mut self) -> DbResult<()> {
        self.conn.busy_timeout(std::time::Duration::from_secs(5))?;
        self.conn.pragma_update(None, "journal_mode", "WAL")?;
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.conn.pragma_update(None, "synchronous", "NORMAL")?;
        // SQLite's LIKE only folds ASCII case; this lets "école" find "École".
        self.conn.create_scalar_function(
            "folio_fold",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                Ok(match ctx.get_raw(0) {
                    ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).to_lowercase(),
                    _ => String::new(),
                })
            },
        )?;
        Ok(())
    }

    fn migrate(&mut self) -> DbResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS notebooks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                color TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sections (
                id TEXT PRIMARY KEY,
                notebook_id TEXT NOT NULL REFERENCES notebooks(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                color TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS sections_notebook ON sections(notebook_id, position);
            CREATE TABLE IF NOT EXISTS pages (
                id TEXT PRIMARY KEY,
                section_id TEXT NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
                title TEXT NOT NULL DEFAULT '',
                content_json TEXT NOT NULL DEFAULT '',
                canvas_json TEXT NOT NULL DEFAULT '',
                layout TEXT NOT NULL DEFAULT 'flow',
                plain_text TEXT NOT NULL DEFAULT '',
                paper TEXT NOT NULL DEFAULT 'plain',
                pinned INTEGER NOT NULL DEFAULT 0,
                trashed INTEGER NOT NULL DEFAULT 0,
                position INTEGER NOT NULL DEFAULT 0,
                word_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS pages_section ON pages(section_id, trashed, pinned, updated_at);
            CREATE INDEX IF NOT EXISTS pages_updated ON pages(trashed, updated_at);
            CREATE TABLE IF NOT EXISTS tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                color TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS page_tags (
                page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
                tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (page_id, tag_id)
            );
            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
                file_name TEXT NOT NULL,
                stored_path TEXT NOT NULL,
                mime TEXT NOT NULL DEFAULT '',
                byte_size INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS attachments_page ON attachments(page_id);
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS bookmarks (
                id TEXT PRIMARY KEY,
                page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
                block_id INTEGER NOT NULL,
                snippet TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS bookmarks_page ON bookmarks(page_id);
            CREATE TABLE IF NOT EXISTS page_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
                title TEXT NOT NULL DEFAULT '',
                content_json TEXT NOT NULL DEFAULT '',
                canvas_json TEXT NOT NULL DEFAULT '',
                layout TEXT NOT NULL DEFAULT 'canvas',
                paper TEXT NOT NULL DEFAULT 'plain',
                plain_text TEXT NOT NULL DEFAULT '',
                word_count INTEGER NOT NULL DEFAULT 0,
                saved_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS page_versions_page ON page_versions(page_id, created_at);
            CREATE TABLE IF NOT EXISTS section_groups (
                id TEXT PRIMARY KEY,
                notebook_id TEXT NOT NULL REFERENCES notebooks(id) ON DELETE CASCADE,
                parent_id TEXT REFERENCES section_groups(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                color TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS section_groups_notebook ON section_groups(notebook_id, parent_id, position);
            CREATE TABLE IF NOT EXISTS trashed_sections (
                section_id TEXT PRIMARY KEY REFERENCES sections(id) ON DELETE CASCADE,
                group_id TEXT,
                position INTEGER NOT NULL DEFAULT 0,
                deleted_at INTEGER NOT NULL
            );
            "#,
        )?;
        ensure_column(
            &self.conn,
            "pages",
            "canvas_json",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            &self.conn,
            "pages",
            "layout",
            "TEXT NOT NULL DEFAULT 'flow'",
        )?;
        ensure_column(&self.conn, "pages", "level", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(
            &self.conn,
            "pages",
            "collapsed",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        // SQLite allows adding a REFERENCES column when its default is NULL; deleting a
        // group then cascades to its sections (and their pages).
        ensure_column(
            &self.conn,
            "sections",
            "group_id",
            "TEXT REFERENCES section_groups(id) ON DELETE CASCADE",
        )?;
        // The section whose deletion sent the page to the Recycle Bin; NULL for a page
        // deleted on its own (and every page that is not in the bin).
        ensure_column(&self.conn, "pages", "trashed_with", "TEXT")?;
        Ok(())
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn notebook_count(&self) -> DbResult<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM notebooks", [], |row| row.get(0))?)
    }

    pub fn workspace(&self) -> DbResult<Workspace> {
        let mut notebooks = Vec::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, name, color, position, created_at, updated_at FROM notebooks ORDER BY position, created_at",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Notebook {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    position: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    sections: Vec::new(),
                    groups: Vec::new(),
                })
            })?;
            for row in rows {
                notebooks.push(row?);
            }
        }
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, notebook_id, name, color, position, created_at, updated_at, group_id FROM sections
                 WHERE id NOT IN (SELECT section_id FROM trashed_sections)
                 ORDER BY position, created_at",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Section {
                    id: row.get(0)?,
                    notebook_id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    position: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    group_id: row.get(7)?,
                })
            })?;
            let mut by_notebook: HashMap<String, Vec<Section>> = HashMap::new();
            for row in rows {
                let section = row?;
                by_notebook
                    .entry(section.notebook_id.clone())
                    .or_default()
                    .push(section);
            }
            for notebook in &mut notebooks {
                notebook.sections = by_notebook.remove(&notebook.id).unwrap_or_default();
            }
        }
        {
            let mut stmt = self.conn.prepare(
                "SELECT id, notebook_id, parent_id, name, color, position, created_at, updated_at FROM section_groups ORDER BY position, created_at",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(SectionGroup {
                    id: row.get(0)?,
                    notebook_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    name: row.get(3)?,
                    color: row.get(4)?,
                    position: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?;
            let mut by_notebook: HashMap<String, Vec<SectionGroup>> = HashMap::new();
            for row in rows {
                let group = row?;
                by_notebook
                    .entry(group.notebook_id.clone())
                    .or_default()
                    .push(group);
            }
            for notebook in &mut notebooks {
                notebook.groups = by_notebook.remove(&notebook.id).unwrap_or_default();
            }
        }
        let tags = self.tags()?;
        let page_count =
            self.conn
                .query_row("SELECT COUNT(*) FROM pages WHERE trashed = 0", [], |row| {
                    row.get(0)
                })?;
        // What the Recycle Bin holds: its pages, and its sections (which may have none).
        let trash_count = self.conn.query_row(
            "SELECT (SELECT COUNT(*) FROM pages WHERE trashed = 1)
                  + (SELECT COUNT(*) FROM trashed_sections)",
            [],
            |row| row.get(0),
        )?;
        let quick_section_id = self.setting("quick_section_id")?;
        Ok(Workspace {
            notebooks,
            tags,
            quick_section_id,
            page_count,
            trash_count,
        })
    }

    pub fn tags(&self) -> DbResult<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.color,
                (SELECT COUNT(*) FROM page_tags pt JOIN pages p ON p.id = pt.page_id
                 WHERE pt.tag_id = t.id AND p.trashed = 0)
             FROM tags t ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                count: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn page_list(
        &self,
        filter: PageFilter<'_>,
        search: &str,
        limit: i64,
    ) -> DbResult<Vec<PageSummary>> {
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        match filter {
            PageFilter::Section(id) => {
                clauses.push("p.section_id = ?".into());
                args.push(id.to_string().into());
                clauses.push("p.trashed = 0".into());
            }
            PageFilter::All => clauses.push("p.trashed = 0".into()),
            // A deleted section's pages wait in the bin under it, not loose.
            PageFilter::Trash => clauses.push("p.trashed = 1 AND p.trashed_with IS NULL".into()),
        }
        let search = search.trim();
        if !search.is_empty() {
            let pattern = like_pattern(search);
            clauses.push(
                "(folio_fold(p.title) LIKE ? ESCAPE '\\' OR folio_fold(p.plain_text) LIKE ? ESCAPE '\\')"
                    .into(),
            );
            args.push(pattern.clone().into());
            args.push(pattern.into());
        }
        let order = match filter {
            PageFilter::Section(_) => "p.position, p.created_at",
            PageFilter::Trash => "p.updated_at DESC",
            PageFilter::All => "p.pinned DESC, p.updated_at DESC",
        };
        let sql = format!(
            "SELECT p.id, p.section_id, s.name, s.color, n.id, n.name, n.color,
                    p.title, p.plain_text, p.paper, p.pinned, p.trashed, p.word_count,
                    p.created_at, p.updated_at, p.level, p.collapsed, p.position
             FROM pages p
             JOIN sections s ON s.id = p.section_id
             JOIN notebooks n ON n.id = s.notebook_id
             WHERE {}
             ORDER BY {}
             LIMIT ?",
            clauses.join(" AND "),
            order
        );
        args.push(limit.into());
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), page_summary_from_row)?;
        let mut pages = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        self.attach_tags(&mut pages)?;
        Ok(pages)
    }

    /// The canvas page `id` was saved with, as written: before `get_page` repairs it.
    #[cfg(test)]
    pub fn stored_canvas_json(&self, id: &str) -> DbResult<String> {
        Ok(self
            .conn
            .query_row("SELECT canvas_json FROM pages WHERE id = ?", [id], |row| {
                row.get(0)
            })?)
    }

    pub fn get_page(&self, id: &str) -> DbResult<Option<Page>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.section_id, s.name, s.color, n.id, n.name, n.color,
                    p.title, p.paper, p.pinned, p.trashed, p.word_count,
                    p.created_at, p.updated_at, p.content_json, p.canvas_json, p.layout
             FROM pages p
             JOIN sections s ON s.id = p.section_id
             JOIN notebooks n ON n.id = s.notebook_id
             WHERE p.id = ?",
        )?;
        let page = stmt
            .query_row([id], |row| {
                let content_json: String = row.get(14)?;
                let canvas_json: String = row.get(15)?;
                let layout: String = row.get(16)?;
                let (mut canvas, lossy) = stored_canvas(&layout, &content_json, &canvas_json);
                canvas.repair();
                let style = PageStyle::from_code(&row.get::<_, String>(8)?);
                let mut damaged = Vec::new();
                if lossy {
                    damaged.push(if layout == "canvas" {
                        ("canvas", canvas_json)
                    } else {
                        ("content", content_json)
                    });
                }
                Ok((
                    damaged,
                    Page {
                        id: row.get(0)?,
                        section_id: row.get(1)?,
                        section_name: row.get(2)?,
                        section_color: row.get(3)?,
                        notebook_id: row.get(4)?,
                        notebook_name: row.get(5)?,
                        notebook_color: row.get(6)?,
                        title: row.get(7)?,
                        rule: style.rule,
                        color: style.color,
                        rule_color: style.rule_color,
                        title_hidden: style.title_hidden,
                        pinned: row.get(9)?,
                        trashed: row.get(10)?,
                        word_count: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                        canvas,
                        tags: Vec::new(),
                        notice: None,
                    },
                ))
            })
            .optional()?;
        let Some((damaged, mut page)) = page else {
            return Ok(None);
        };
        if !damaged.is_empty() {
            page.notice = Some(match self.keep_damaged_json(&page.id, &damaged) {
                Ok(dir) => {
                    t!("data-notice-damaged-saved", dir = dir.display().to_string()).into_owned()
                }
                Err(error) => t!("data-notice-damaged", error = error.to_string()).into_owned(),
            });
        }
        page.tags = self.page_tags(&page.id)?;
        Ok(Some(page))
    }

    /// Copies stored JSON that failed to parse next to the database, so the next autosave
    /// (which writes only what could be read) cannot destroy it.
    fn keep_damaged_json(&self, page_id: &str, damaged: &[(&str, String)]) -> DbResult<PathBuf> {
        use std::hash::{Hash, Hasher};
        let dir = self.data_dir.join("recovered");
        fs::create_dir_all(&dir)?;
        for (part, json) in damaged {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            json.hash(&mut hasher);
            let path = dir.join(format!("{page_id}-{part}-{:016x}.json", hasher.finish()));
            if !path.exists() {
                fs::write(&path, json)?;
            }
        }
        Ok(dir)
    }

    pub fn page_tags(&self, page_id: &str) -> DbResult<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.color,
                (SELECT COUNT(*) FROM page_tags pt JOIN pages p ON p.id = pt.page_id
                 WHERE pt.tag_id = t.id AND p.trashed = 0)
             FROM tags t JOIN page_tags pt ON pt.tag_id = t.id
             WHERE pt.page_id = ? ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([page_id], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                count: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn attach_tags(&self, pages: &mut [PageSummary]) -> DbResult<()> {
        if pages.is_empty() {
            return Ok(());
        }
        let mut stmt = self.conn.prepare(
            "SELECT pt.page_id, t.id, t.name, t.color,
                (SELECT COUNT(*) FROM page_tags pt2 JOIN pages p2 ON p2.id = pt2.page_id
                 WHERE pt2.tag_id = t.id AND p2.trashed = 0)
             FROM page_tags pt JOIN tags t ON t.id = pt.tag_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Tag {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    count: row.get(4)?,
                },
            ))
        })?;
        let mut by_page: HashMap<String, Vec<Tag>> = HashMap::new();
        for row in rows {
            let (page_id, tag) = row?;
            by_page.entry(page_id).or_default().push(tag);
        }
        for page in pages {
            page.tags = by_page.remove(&page.id).unwrap_or_default();
        }
        Ok(())
    }

    pub fn create_notebook(&mut self, name: &str, color: &str) -> DbResult<Notebook> {
        let now = now_ms();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM notebooks",
            [],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        let fallback = t!("data-default-notebook");
        let name = if name.trim().is_empty() {
            fallback.as_ref()
        } else {
            name.trim()
        };
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO notebooks (id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![id.clone(), name, color, position, now, now],
        )?;
        let section_id = Uuid::new_v4().to_string();
        let first_section = t!("data-default-first-section");
        tx.execute(
            "INSERT INTO sections (id, notebook_id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, 0, ?, ?)",
            params![section_id, id.clone(), first_section.as_ref(), color, now, now],
        )?;
        tx.commit()?;
        Ok(Notebook {
            id: id.clone(),
            name: name.to_string(),
            color: color.to_string(),
            position,
            created_at: now,
            updated_at: now,
            sections: vec![Section {
                id: section_id,
                notebook_id: id.clone(),
                name: first_section.to_string(),
                color: color.to_string(),
                position: 0,
                created_at: now,
                updated_at: now,
                group_id: None,
            }],
            groups: Vec::new(),
        })
    }

    pub fn update_notebook(&mut self, id: &str, name: &str, color: &str) -> DbResult<()> {
        let fallback = t!("data-fallback-notebook");
        let name = if name.trim().is_empty() {
            fallback.as_ref()
        } else {
            name.trim()
        };
        self.conn.execute(
            "UPDATE notebooks SET name = ?, color = ?, updated_at = ? WHERE id = ?",
            params![name, color, now_ms(), id],
        )?;
        Ok(())
    }

    pub fn delete_notebook(&mut self, id: &str) -> DbResult<()> {
        let quick = self.setting("quick_section_id")?;
        let quick_belongs: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sections WHERE notebook_id = ? AND id = ?)",
            params![id, quick.clone().unwrap_or_default()],
            |row| row.get(0),
        )?;
        let tx = self.conn.transaction()?;
        let doomed = page_ids(
            &tx,
            "SELECT p.id FROM pages p JOIN sections s ON s.id = p.section_id WHERE s.notebook_id = ?",
            [id],
        )?;
        let files = release_attachments(&tx, &doomed)?;
        tx.execute("DELETE FROM notebooks WHERE id = ?", [id])?;
        if quick_belongs {
            tx.execute("DELETE FROM settings WHERE key = 'quick_section_id'", [])?;
        }
        tx.commit()?;
        self.remove_files(files);
        Ok(())
    }

    /// Swaps a notebook with its neighbour above (`up`) or below; false at the end.
    pub fn move_notebook(&mut self, id: &str, up: bool) -> DbResult<bool> {
        let tx = self.conn.transaction()?;
        let mut order = page_ids(
            &tx,
            "SELECT id FROM notebooks ORDER BY position, created_at",
            [],
        )?;
        let Some(index) = order.iter().position(|notebook| notebook == id) else {
            return Ok(false);
        };
        let neighbour = if up {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|&below| below < order.len())
        };
        let Some(neighbour) = neighbour else {
            return Ok(false);
        };
        order.swap(index, neighbour);
        for (position, notebook) in order.iter().enumerate() {
            tx.execute(
                "UPDATE notebooks SET position = ? WHERE id = ?",
                params![position as i64, notebook],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    #[cfg(test)]
    pub fn create_section(
        &mut self,
        notebook_id: &str,
        name: &str,
        color: &str,
    ) -> DbResult<Section> {
        self.create_section_in(notebook_id, None, name, color)
    }

    /// A new section at the end of `group_id` (or of the notebook's top level).
    pub fn create_section_in(
        &mut self,
        notebook_id: &str,
        group_id: Option<&str>,
        name: &str,
        color: &str,
    ) -> DbResult<Section> {
        if let Some(group) = group_id {
            self.check_group(notebook_id, group)?;
        }
        let now = now_ms();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM sections WHERE notebook_id = ? AND group_id IS ?",
            params![notebook_id, group_id],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        let fallback = t!("data-default-section");
        let name = if name.trim().is_empty() {
            fallback.as_ref()
        } else {
            name.trim()
        };
        self.conn.execute(
            "INSERT INTO sections (id, notebook_id, name, color, position, created_at, updated_at, group_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![id, notebook_id, name, color, position, now, now, group_id],
        )?;
        Ok(Section {
            id,
            notebook_id: notebook_id.to_string(),
            name: name.to_string(),
            color: color.to_string(),
            position,
            created_at: now,
            updated_at: now,
            group_id: group_id.map(str::to_string),
        })
    }

    /// Errors unless `group_id` is a group of `notebook_id`.
    fn check_group(&self, notebook_id: &str, group_id: &str) -> DbResult<()> {
        let owner: Option<String> = self
            .conn
            .query_row(
                "SELECT notebook_id FROM section_groups WHERE id = ?",
                [group_id],
                |row| row.get(0),
            )
            .optional()?;
        if owner.as_deref() == Some(notebook_id) {
            Ok(())
        } else {
            Err(DbError::Message(
                t!("data-error-group-elsewhere").into_owned(),
            ))
        }
    }

    pub fn create_section_group(
        &mut self,
        notebook_id: &str,
        parent_id: Option<&str>,
        name: &str,
        color: &str,
    ) -> DbResult<SectionGroup> {
        if let Some(parent) = parent_id {
            self.check_group(notebook_id, parent)?;
        }
        let now = now_ms();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM section_groups WHERE notebook_id = ? AND parent_id IS ?",
            params![notebook_id, parent_id],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        let fallback = t!("data-default-group");
        let name = if name.trim().is_empty() {
            fallback.as_ref()
        } else {
            name.trim()
        };
        self.conn.execute(
            "INSERT INTO section_groups (id, notebook_id, parent_id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![id, notebook_id, parent_id, name, color, position, now, now],
        )?;
        Ok(SectionGroup {
            id,
            notebook_id: notebook_id.to_string(),
            parent_id: parent_id.map(str::to_string),
            name: name.to_string(),
            color: color.to_string(),
            position,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_section_group(&mut self, id: &str, name: &str, color: &str) -> DbResult<()> {
        let fallback = t!("data-fallback-group");
        let name = if name.trim().is_empty() {
            fallback.as_ref()
        } else {
            name.trim()
        };
        self.conn.execute(
            "UPDATE section_groups SET name = ?, color = ?, updated_at = ? WHERE id = ?",
            params![name, color, now_ms(), id],
        )?;
        Ok(())
    }

    /// Deletes a group and the groups inside it, as the desktop notebook does: their sections go to the
    /// Notebook Recycle Bin with their pages, and the groups are not kept. Only the
    /// sections' pages can come back; nothing else held anything.
    pub fn delete_section_group(&mut self, id: &str) -> DbResult<()> {
        const TREE: &str = "WITH RECURSIVE tree(id) AS (
                SELECT ?1 UNION ALL SELECT g.id FROM section_groups g JOIN tree ON g.parent_id = tree.id
            )";
        let quick = self.setting("quick_section_id")?;
        let tx = self.conn.transaction()?;
        // Sections already in the bin too: they keep their own deletion.
        let sections = page_ids(
            &tx,
            &format!("{TREE} SELECT id FROM sections WHERE group_id IN (SELECT id FROM tree)"),
            [id],
        )?;
        let now = now_ms();
        for section in &sections {
            trash_section_in(&tx, section, now)?;
            // Out of the group, or deleting it would delete the section too (its group_id
            // cascades). The bin keeps the group it was in.
            tx.execute(
                "UPDATE sections SET group_id = NULL WHERE id = ?",
                [section],
            )?;
        }
        tx.execute("DELETE FROM section_groups WHERE id = ?", [id])?;
        if quick.is_some_and(|quick| sections.contains(&quick)) {
            tx.execute("DELETE FROM settings WHERE key = 'quick_section_id'", [])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Puts a section into `group_id` (or the notebook's top level), before the section
    /// `before` there (or at the end). Sections never leave their notebook.
    pub fn move_section(
        &mut self,
        id: &str,
        group_id: Option<&str>,
        before: Option<&str>,
    ) -> DbResult<()> {
        let notebook: String = self.conn.query_row(
            "SELECT notebook_id FROM sections WHERE id = ?",
            [id],
            |row| row.get(0),
        )?;
        if let Some(group) = group_id {
            self.check_group(&notebook, group)?;
        }
        let tx = self.conn.transaction()?;
        let mut order = page_ids(
            &tx,
            "SELECT id FROM sections WHERE notebook_id = ? AND group_id IS ? AND id != ? ORDER BY position, created_at",
            params![notebook, group_id, id],
        )?;
        let at = before
            .and_then(|before| order.iter().position(|section| section == before))
            .unwrap_or(order.len());
        order.insert(at, id.to_string());
        tx.execute(
            "UPDATE sections SET group_id = ?, updated_at = ? WHERE id = ?",
            params![group_id, now_ms(), id],
        )?;
        for (position, section) in order.iter().enumerate() {
            tx.execute(
                "UPDATE sections SET position = ? WHERE id = ?",
                params![position as i64, section],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves a section to the end of another notebook's top level or of a group.
    pub fn move_section_to(
        &mut self,
        id: &str,
        notebook_id: &str,
        group_id: Option<&str>,
    ) -> DbResult<()> {
        if let Some(group) = group_id {
            self.check_group(notebook_id, group)?;
        }
        let (old_notebook, old_group): (String, Option<String>) = self.conn.query_row(
            "SELECT notebook_id, group_id FROM sections WHERE id = ?",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let tx = self.conn.transaction()?;
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM sections
             WHERE notebook_id = ? AND group_id IS ? AND id != ?",
            params![notebook_id, group_id, id],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE sections SET notebook_id = ?, group_id = ?, position = ?, updated_at = ? WHERE id = ?",
            params![notebook_id, group_id, position, now_ms(), id],
        )?;
        // Close the gap it leaves behind.
        let left = page_ids(
            &tx,
            "SELECT id FROM sections WHERE notebook_id = ? AND group_id IS ? AND id != ? ORDER BY position, created_at",
            params![old_notebook, old_group, id],
        )?;
        for (position, section) in left.iter().enumerate() {
            tx.execute(
                "UPDATE sections SET position = ? WHERE id = ?",
                params![position as i64, section],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A copy of a section and its pages (not the trashed ones), at the end of the target,
    /// named as the original.
    pub fn copy_section(
        &mut self,
        id: &str,
        notebook_id: &str,
        group_id: Option<&str>,
    ) -> DbResult<Section> {
        let (name, color): (String, String) = self.conn.query_row(
            "SELECT name, color FROM sections WHERE id = ?",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let pages = section_outline(&self.conn, id)?;
        let section = self.create_section_in(notebook_id, group_id, &name, &color)?;
        if let Err(error) = self.copy_outline_into(&pages, &section.id) {
            // Nothing is left half-made: the new section goes with the pages copied so far.
            let _ = self.delete_section(&section.id);
            return Err(error);
        }
        Ok(section)
    }

    /// Copies the pages of `pages` (a section's outline), in order, to the end of
    /// `section_id`, each at its level there.
    fn copy_outline_into(&mut self, pages: &[outline::Entry], section_id: &str) -> DbResult<()> {
        for entry in pages {
            if let Some(copy) = self.copy_page_to(&entry.id, section_id)? {
                self.place_page(&copy.id, section_id, None, Some(entry.level))?;
            }
        }
        Ok(())
    }

    /// Moves every page of `from` (trashed ones too) to the end of `into`, keeping their
    /// order and levels, then deletes `from`. Returns how many pages moved.
    pub fn merge_section(&mut self, from: &str, into: &str) -> DbResult<usize> {
        if from == into {
            return Err(DbError::Message(t!("data-error-merge-self").into_owned()));
        }
        // Both must exist: pages never go into a missing section, nor a section away
        // without its pages going somewhere.
        for section in [from, into] {
            self.conn
                .query_row("SELECT id FROM sections WHERE id = ?", [section], |row| {
                    row.get::<_, String>(0)
                })?;
        }
        let entries = section_outline(&self.conn, from)?;
        let levels: Vec<u8> = entries.iter().map(|entry| entry.level).collect();
        // Each family (a page and its subpages) moves with its first page.
        let mut index = 0;
        while index < entries.len() {
            self.place_page(&entries[index].id, into, None, Some(0))?;
            index = outline::family(&levels, index).end;
        }
        // What is left is the trash; it keeps its place there, now in `into`.
        let trashed = self.conn.execute(
            "UPDATE pages SET section_id = ? WHERE section_id = ?",
            params![into, from],
        )?;
        // Quick notes go where the quick notes now are.
        if self.setting("quick_section_id")?.as_deref() == Some(from) {
            self.set_setting("quick_section_id", into)?;
        }
        self.delete_section(from)?;
        Ok(entries.len() + trashed)
    }

    pub fn update_section(&mut self, id: &str, name: &str, color: &str) -> DbResult<()> {
        let fallback = t!("data-fallback-section");
        let name = if name.trim().is_empty() {
            fallback.as_ref()
        } else {
            name.trim()
        };
        self.conn.execute(
            "UPDATE sections SET name = ?, color = ?, updated_at = ? WHERE id = ?",
            params![name, color, now_ms(), id],
        )?;
        Ok(())
    }

    /// Sends a section to the Notebook Recycle Bin, as the desktop notebook does: it leaves its
    /// notebook, and its pages wait in the bin with it until it is restored or the bin is
    /// emptied. Pages that were already in the bin stay there on their own.
    pub fn trash_section(&mut self, id: &str) -> DbResult<()> {
        let quick = self.setting("quick_section_id")?;
        let tx = self.conn.transaction()?;
        trash_section_in(&tx, id, now_ms())?;
        if quick.as_deref() == Some(id) {
            tx.execute("DELETE FROM settings WHERE key = 'quick_section_id'", [])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Brings a section back from the Recycle Bin with the pages deleted with it: into the
    /// section group it was in if that is still in its notebook, else at the notebook's top
    /// level, after the sections there.
    pub fn restore_section(&mut self, id: &str) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        let Some(group) = tx
            .query_row(
                "SELECT group_id FROM trashed_sections WHERE section_id = ?",
                [id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
        else {
            return Ok(());
        };
        let group = match group {
            Some(group)
                if tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM section_groups g JOIN sections s
                     ON s.notebook_id = g.notebook_id WHERE g.id = ?1 AND s.id = ?2)",
                    params![group, id],
                    |row| row.get::<_, bool>(0),
                )? =>
            {
                Some(group)
            }
            _ => None,
        };
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM sections
             WHERE notebook_id = (SELECT notebook_id FROM sections WHERE id = ?1)
             AND group_id IS ?2 AND id != ?1",
            params![id, group],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE sections SET group_id = ?, position = ?, updated_at = ? WHERE id = ?",
            params![group, position, now_ms(), id],
        )?;
        tx.execute(
            "UPDATE pages SET trashed = 0, trashed_with = NULL WHERE trashed_with = ?",
            [id],
        )?;
        tx.execute("DELETE FROM trashed_sections WHERE section_id = ?", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Errors when section `id` is in the Recycle Bin: its pages wait there with it, and it
    /// takes no new ones.
    fn refuse_section_in_bin(&self, id: &str) -> DbResult<()> {
        let in_bin: bool = self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM trashed_sections WHERE section_id = ?)",
            [id],
            |row| row.get(0),
        )?;
        if in_bin {
            Err(DbError::Message(
                t!("data-error-section-in-bin").into_owned(),
            ))
        } else {
            Ok(())
        }
    }

    /// Delete Forever for a section in the Recycle Bin: it goes for good with its pages.
    pub fn delete_trashed_section(&mut self, id: &str) -> DbResult<()> {
        self.delete_section(id)
    }

    /// The sections in the Recycle Bin, the last deleted first.
    pub fn trashed_sections(&self) -> DbResult<Vec<TrashedSection>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.notebook_id, n.name, s.name, s.color,
                    (SELECT COUNT(*) FROM pages p WHERE p.trashed_with = s.id), t.deleted_at
             FROM trashed_sections t
             JOIN sections s ON s.id = t.section_id
             JOIN notebooks n ON n.id = s.notebook_id
             ORDER BY t.deleted_at DESC, s.name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TrashedSection {
                id: row.get(0)?,
                notebook_id: row.get(1)?,
                notebook_name: row.get(2)?,
                name: row.get(3)?,
                color: row.get(4)?,
                page_count: row.get(5)?,
                deleted_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deletes a section and its pages for good, with no stop in the Recycle Bin.
    pub fn delete_section(&mut self, id: &str) -> DbResult<()> {
        let quick = self.setting("quick_section_id")?;
        let tx = self.conn.transaction()?;
        let doomed = page_ids(&tx, "SELECT id FROM pages WHERE section_id = ?", [id])?;
        let files = release_attachments(&tx, &doomed)?;
        tx.execute("DELETE FROM sections WHERE id = ?", [id])?;
        if quick.as_deref() == Some(id) {
            tx.execute("DELETE FROM settings WHERE key = 'quick_section_id'", [])?;
        }
        tx.commit()?;
        self.remove_files(files);
        Ok(())
    }

    /// Section views used to list pinned pages first, then the most recently edited. Keep
    /// that as the starting manual order.
    fn assign_page_order(&mut self) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        let sections = page_ids(&tx, "SELECT id FROM sections", [])?;
        for section in sections {
            let pages = page_ids(
                &tx,
                "SELECT id FROM pages WHERE section_id = ? AND trashed = 0 ORDER BY pinned DESC, updated_at DESC",
                [&section],
            )?;
            for (position, id) in pages.iter().enumerate() {
                tx.execute(
                    "UPDATE pages SET position = ?, level = 0 WHERE id = ?",
                    params![position as i64, id],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every page becomes a the desktop notebook page, once: flow rows (and their kept versions) turn into
    /// one note under the title, content an older layout put where the title and date now sit
    /// moves down below them, and the old document column is emptied. Rows whose JSON cannot
    /// be read whole are left alone: `get_page` sets their unreadable part aside instead.
    fn one_page_type(&mut self) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        for table in ["pages", "page_versions"] {
            let rows = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT rowid, layout, content_json, canvas_json FROM {table}"
                ))?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (rowid, layout, content, canvas_json) in rows {
                let (mut canvas, lossy) = stored_canvas(&layout, &content, &canvas_json);
                if lossy {
                    continue;
                }
                let moved = settle_below_header(&mut canvas);
                if layout == "canvas" && !moved && content.is_empty() {
                    continue;
                }
                let plain = canvas.plain_text();
                let words = plain.split_whitespace().count() as i64;
                tx.execute(
                    &format!(
                        "UPDATE {table} SET layout = 'canvas', content_json = '', canvas_json = ?,
                         plain_text = ?, word_count = ? WHERE rowid = ?"
                    ),
                    params![canvas.to_json(), plain, words, rowid],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Tables used to be page items; they are blocks in a note now. Loading converts
    /// them (`CanvasDoc::repair`); this writes the converted pages back once, so stored
    /// pages and their search text match what is shown.
    fn convert_canvas_tables(&mut self) -> DbResult<()> {
        let ids: Vec<String> = {
            let mut statement = self
                .conn
                .prepare("SELECT id FROM pages WHERE canvas_json LIKE '%\"type\":\"table\"%'")?;
            let rows = statement.query_map([], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in ids {
            let Some(page) = self.get_page(&id)? else {
                continue;
            };
            // As in `one_page_type`: a page whose JSON could not be read whole is left
            // alone, so writing back what could be read never loses the rest.
            if page.notice.is_some() {
                continue;
            }
            self.conn.execute(
                "UPDATE pages SET canvas_json = ?1, plain_text = ?2, word_count = ?3 WHERE id = ?4",
                params![
                    page.canvas.to_json(),
                    page.plain_text(),
                    page.word_count(),
                    id
                ],
            )?;
        }
        Ok(())
    }

    pub fn create_canvas_page(&mut self, section_id: &str, title: &str) -> DbResult<Page> {
        // It would be out of sight, and go when the bin is emptied.
        self.refuse_section_in_bin(section_id)?;
        let now = now_ms();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM pages WHERE section_id = ?",
            [section_id],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        let canvas = CanvasDoc::default();
        let paper = self.new_page_rule()?.unwrap_or_default().code();
        self.conn.execute(
            "INSERT INTO pages (id, section_id, title, content_json, canvas_json, layout, plain_text, paper, pinned, trashed, position, word_count, created_at, updated_at)
             VALUES (?, ?, ?, '', ?, 'canvas', '', ?, 0, 0, ?, 0, ?, ?)",
            params![id, section_id, title.trim(), canvas.to_json(), paper, position, now, now],
        )?;
        self.conn.execute(
            "UPDATE sections SET updated_at = ? WHERE id = ?",
            params![now, section_id],
        )?;
        self.get_page(&id)?
            .ok_or_else(|| DbError::Message(t!("data-error-page-not-created").into_owned()))
    }

    pub fn save_page(&mut self, page: &Page) -> DbResult<()> {
        let plain = page.plain_text();
        let words = page.word_count();
        let canvas = page.canvas.to_json();
        let now = now_ms();
        let interval = self.version_interval_ms;
        let tx = self.conn.transaction()?;
        // A version is a safety net: never let it stop the page itself from saving.
        if let Err(error) = keep_version(&tx, page, &canvas, now, interval) {
            eprintln!("folio: could not keep a version of the page: {error}");
        }
        let updated = tx.execute(
            "UPDATE pages SET title = ?, content_json = '', canvas_json = ?, layout = 'canvas', plain_text = ?, paper = ?, word_count = ?, created_at = ?, updated_at = ? WHERE id = ?",
            params![
                page.title,
                canvas,
                plain,
                PageStyle::of(page).code(),
                words,
                page.created_at,
                now,
                page.id
            ],
        )?;
        if updated == 0 {
            return Err(DbError::Message(
                t!("data-error-page-gone-in-notebook").into_owned(),
            ));
        }
        tx.commit()?;
        Ok(())
    }

    /// The page's kept versions, newest first.
    pub fn page_versions(&self, page_id: &str) -> DbResult<Vec<PageVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, plain_text, word_count, saved_at FROM page_versions
             WHERE page_id = ? ORDER BY saved_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([page_id], |row| {
            Ok(PageVersion {
                id: row.get(0)?,
                title: row.get(1)?,
                plain_text: row.get(2)?,
                word_count: row.get(3)?,
                saved_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Puts a kept version back as the page's content. The page as it is now is kept as a
    /// version first, so the restore can be undone from the same list.
    pub fn restore_version(&mut self, page_id: &str, version_id: i64) -> DbResult<()> {
        let now = now_ms();
        let tx = self.conn.transaction()?;
        // Read the version before keeping the current page: pruning may drop the oldest one.
        let version = tx
            .query_row(
                "SELECT title, content_json, canvas_json, layout, paper, plain_text, word_count
                 FROM page_versions WHERE id = ? AND page_id = ?",
                params![version_id, page_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, content, canvas, layout, paper, plain, words)) = version else {
            return Err(DbError::Message(t!("data-error-version-gone").into_owned()));
        };
        insert_version(&tx, page_id, now)?;
        let updated = tx.execute(
            "UPDATE pages SET title = ?, content_json = ?, canvas_json = ?, layout = ?, paper = ?, plain_text = ?, word_count = ?, updated_at = ? WHERE id = ?",
            params![title, content, canvas, layout, paper, plain, words, now, page_id],
        )?;
        if updated == 0 {
            return Err(DbError::Message(
                t!("data-error-page-gone-in-notebook").into_owned(),
            ));
        }
        tx.commit()?;
        Ok(())
    }

    /// Forgets every kept version of a page. Returns how many were deleted.
    pub fn delete_page_versions(&mut self, page_id: &str) -> DbResult<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM page_versions WHERE page_id = ?", [page_id])?)
    }

    /// Forgets one kept version of a page (Delete Version). Returns whether it existed.
    pub fn delete_version(&mut self, page_id: &str, version_id: i64) -> DbResult<bool> {
        Ok(self.conn.execute(
            "DELETE FROM page_versions WHERE id = ? AND page_id = ?",
            params![version_id, page_id],
        )? > 0)
    }

    /// Forgets every kept version of the pages in a section (History › Delete All Versions
    /// in Section). Returns how many were deleted.
    pub fn delete_versions_in_section(&mut self, section_id: &str) -> DbResult<usize> {
        Ok(self.conn.execute(
            "DELETE FROM page_versions
             WHERE page_id IN (SELECT id FROM pages WHERE section_id = ?)",
            [section_id],
        )?)
    }

    /// Forgets every kept version of the pages in a notebook, in all its sections and
    /// section groups. Returns how many were deleted.
    pub fn delete_versions_in_notebook(&mut self, notebook_id: &str) -> DbResult<usize> {
        Ok(self.conn.execute(
            "DELETE FROM page_versions
             WHERE page_id IN (
                 SELECT p.id FROM pages p
                 JOIN sections s ON s.id = p.section_id
                 WHERE s.notebook_id = ?
             )",
            [notebook_id],
        )?)
    }

    /// Moves a page to the trash. A folded page takes its subpages with it, as in the desktop notebook;
    /// otherwise its subpages move up a level. Returns every trashed page id.
    pub fn trash_page(&mut self, id: &str) -> DbResult<Vec<String>> {
        let section = self.page_section_id(id)?;
        let collapsed: bool = self
            .conn
            .query_row("SELECT collapsed FROM pages WHERE id = ?", [id], |row| {
                row.get(0)
            })
            .optional()?
            .unwrap_or(false);
        let tx = self.conn.transaction()?;
        let mut entries = match &section {
            Some(section) => section_outline(&tx, section)?,
            None => Vec::new(),
        };
        let doomed: Vec<String> = match entries.iter().position(|entry| entry.id == id) {
            Some(index) => {
                let levels: Vec<u8> = entries.iter().map(|entry| entry.level).collect();
                let range = outline::family(&levels, index);
                if collapsed {
                    entries.drain(range).map(|entry| entry.id).collect()
                } else {
                    for entry in &mut entries[range.start + 1..range.end] {
                        entry.level = entry.level.saturating_sub(1);
                    }
                    vec![entries.remove(index).id]
                }
            }
            None => vec![id.to_string()],
        };
        outline::normalize(&mut entries);
        let now = now_ms();
        for page in &doomed {
            tx.execute(
                "UPDATE pages SET trashed = 1, pinned = 0, updated_at = ? WHERE id = ?",
                params![now, page],
            )?;
        }
        write_outline(&tx, &entries)?;
        tx.commit()?;
        Ok(doomed)
    }

    /// Brings a page back from the trash, at the end of its section as a top-level page.
    /// A page whose section is in the bin too waits for its section.
    pub fn restore_page(&mut self, id: &str) -> DbResult<()> {
        if let Some(section) = self.page_section_id(id)? {
            self.refuse_section_in_bin(&section)?;
        }
        let tx = self.conn.transaction()?;
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM pages
             WHERE trashed = 0 AND section_id = (SELECT section_id FROM pages WHERE id = ?)",
            [id],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE pages SET trashed = 0, level = 0, collapsed = 0, position = ?, updated_at = ? WHERE id = ?",
            params![position, now_ms(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_page_permanently(&mut self, id: &str) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        let files = release_attachments(&tx, &[id.to_string()])?;
        tx.execute("DELETE FROM pages WHERE id = ?", [id])?;
        tx.commit()?;
        self.remove_files(files);
        Ok(())
    }

    /// Empty Recycle Bin: its sections, with their pages, and its pages go for good.
    pub fn empty_trash(&mut self) -> DbResult<()> {
        let tx = self.conn.transaction()?;
        let doomed = page_ids(
            &tx,
            "SELECT id FROM pages WHERE trashed = 1
             OR section_id IN (SELECT section_id FROM trashed_sections)",
            [],
        )?;
        let files = release_attachments(&tx, &doomed)?;
        tx.execute(
            "DELETE FROM sections WHERE id IN (SELECT section_id FROM trashed_sections)",
            [],
        )?;
        tx.execute("DELETE FROM pages WHERE trashed = 1", [])?;
        tx.commit()?;
        self.remove_files(files);
        Ok(())
    }

    fn remove_files(&self, stored_paths: Vec<String>) {
        for stored in stored_paths {
            let path = self.attachment_path(&stored);
            if let Err(error) = fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("folio: could not remove {}: {error}", path.display());
                }
            }
            // Stored files are named by attachment id; its opened copies go with it.
            if let Some(id) = Path::new(&stored).file_stem().and_then(|id| id.to_str()) {
                let _ = fs::remove_dir_all(self.open_dir().join(id));
            }
        }
    }

    /// Whether page `page_id` is `container_id` or lies inside it: its section, a
    /// section group holding that section (at any depth), or its notebook.
    pub fn page_is_within(&self, page_id: &str, container_id: &str) -> DbResult<bool> {
        if page_id == container_id {
            return Ok(true);
        }
        Ok(self.conn.query_row(
            "WITH RECURSIVE up(id) AS (
                 SELECT s.group_id FROM pages p JOIN sections s ON s.id = p.section_id
                 WHERE p.id = ?1 AND s.group_id IS NOT NULL
                 UNION ALL
                 SELECT g.parent_id FROM section_groups g JOIN up ON g.id = up.id
                 WHERE g.parent_id IS NOT NULL
             )
             SELECT EXISTS (
                 SELECT 1 FROM pages p JOIN sections s ON s.id = p.section_id
                 WHERE p.id = ?1 AND ?2 IN (s.id, s.notebook_id)
             ) OR EXISTS (SELECT 1 FROM up WHERE id = ?2)",
            params![page_id, container_id],
            |row| row.get(0),
        )?)
    }

    /// A copy of a page, title kept, at the end of `section_id` (top level), with its own copies
    /// of its attachments (Move or Copy › Copy, and Paste).
    pub fn copy_page_to(&mut self, id: &str, section_id: &str) -> DbResult<Option<Page>> {
        let Some(page) = self.get_page(id)? else {
            return Ok(None);
        };
        let title = page.title.clone();
        // Only the page itself is copied, never its subpages (as the desktop notebook does); the new row
        // is already the last, top-level page of `section_id`.
        let new_id = self.insert_copy(page, section_id, &title)?;
        self.get_page(&new_id)
    }

    /// Inserts a copy of `page` titled `title` as the last, top-level page of `section_id`,
    /// then fills it in (`fill_duplicate`). Returns the copy's id; a copy that fails
    /// half-way is removed again.
    fn insert_copy(&mut self, page: Page, section_id: &str, title: &str) -> DbResult<String> {
        let new_id = Uuid::new_v4().to_string();
        let now = now_ms();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM pages WHERE section_id = ?",
            [section_id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO pages (id, section_id, title, content_json, canvas_json, layout, plain_text, paper, pinned, trashed, position, word_count, created_at, updated_at)
             VALUES (?, ?, ?, '', '', 'canvas', ?, ?, 0, 0, ?, ?, ?, ?)",
            params![
                new_id,
                section_id,
                title,
                page.plain_text(),
                PageStyle::of(&page).code(),
                position,
                page.word_count,
                now,
                now
            ],
        )?;
        match self.fill_duplicate(page, &new_id) {
            Ok(()) => Ok(new_id),
            Err(error) => {
                // Don't leave a half-made copy behind (its attachment rows go with it).
                let _ = self.delete_page_permanently(&new_id);
                Err(error)
            }
        }
    }

    /// Copies a page's pictures, content and tags into the freshly inserted row `new_id`.
    fn fill_duplicate(&mut self, page: Page, new_id: &str) -> DbResult<()> {
        // Give the copy its own attachment rows and files, so deleting either page
        // never takes the other's pictures with it.
        let mut copy = page;
        let mut remap = HashMap::new();
        for attachment_id in page_attachment_ids(&copy) {
            if remap.contains_key(&attachment_id) {
                continue;
            }
            let Some(duplicate) = self.copy_attachment(new_id, &attachment_id)? else {
                continue;
            };
            remap.insert(attachment_id, duplicate.id);
        }
        remap_page_attachments(&mut copy, &remap);
        self.conn.execute(
            "UPDATE pages SET content_json = '', canvas_json = ? WHERE id = ?",
            params![copy.canvas.to_json(), new_id],
        )?;
        for tag in copy.tags {
            self.conn.execute(
                "INSERT OR IGNORE INTO page_tags (page_id, tag_id) VALUES (?, ?)",
                params![new_id, tag.id],
            )?;
        }
        Ok(())
    }

    pub fn page_section_id(&self, id: &str) -> DbResult<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT section_id FROM pages WHERE id = ?", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// The title of a page that is not in the trash.
    pub fn page_title(&self, id: &str) -> DbResult<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT title FROM pages WHERE id = ? AND trashed = 0",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Runs `change` on a section's page order and saves the result if it changed. The
    /// page `reveal` (the one moved or created) stays in view: the pages it now sits
    /// under are unfolded.
    fn edit_outline(
        &mut self,
        section_id: &str,
        reveal: &str,
        change: impl FnOnce(&mut Vec<outline::Entry>) -> bool,
    ) -> DbResult<bool> {
        let tx = self.conn.transaction()?;
        let mut entries = section_outline(&tx, section_id)?;
        let changed = change(&mut entries);
        if changed {
            write_outline(&tx, &entries)?;
            unfold_ancestors(&tx, &entries, reveal)?;
            tx.commit()?;
        }
        Ok(changed)
    }

    /// Ctrl+Alt+] (`delta = 1`) / Ctrl+Alt+[ (`-1`) on a page and its subpages.
    pub fn shift_page_level(&mut self, id: &str, delta: i8) -> DbResult<bool> {
        let Some(section) = self.page_section_id(id)? else {
            return Ok(false);
        };
        self.edit_outline(&section, id, |entries| outline::shift(entries, id, delta))
    }

    /// Alt+Shift+↑ (`up`) / ↓ on a page and its subpages (`outline::step`).
    pub fn step_page(&mut self, id: &str, up: bool) -> DbResult<bool> {
        let Some(section) = self.page_section_id(id)? else {
            return Ok(false);
        };
        self.edit_outline(&section, id, |entries| outline::step(entries, id, up))
    }

    pub fn set_page_collapsed(&mut self, id: &str, collapsed: bool) -> DbResult<()> {
        self.conn.execute(
            "UPDATE pages SET collapsed = ? WHERE id = ?",
            params![collapsed, id],
        )?;
        Ok(())
    }

    /// Moves a page and its subpages before `before` in `section_id` (or to its end). With
    /// `level` the page takes that level. Moving into another section without a level makes it
    /// a top-level page. Trashed pages move alone.
    pub fn place_page(
        &mut self,
        id: &str,
        section_id: &str,
        before: Option<&str>,
        level: Option<u8>,
    ) -> DbResult<bool> {
        let Some(from) = self.page_section_id(id)? else {
            return Ok(false);
        };
        if from == section_id {
            return self.edit_outline(section_id, id, |entries| {
                outline::place(entries, id, before, level)
            });
        }
        let tx = self.conn.transaction()?;
        let mut source = section_outline(&tx, &from)?;
        let moving = outline::take_family(&mut source, id);
        let ids: Vec<String> = if moving.is_empty() {
            vec![id.to_string()]
        } else {
            moving.iter().map(|entry| entry.id.clone()).collect()
        };
        let mut target = section_outline(&tx, section_id)?;
        outline::insert_family(&mut target, moving, before, level.or(Some(0)));
        let now = now_ms();
        for page in &ids {
            tx.execute(
                "UPDATE pages SET section_id = ?, updated_at = ? WHERE id = ?",
                params![section_id, now, page],
            )?;
        }
        write_outline(&tx, &source)?;
        write_outline(&tx, &target)?;
        unfold_ancestors(&tx, &target, id)?;
        tx.commit()?;
        Ok(true)
    }

    /// Moves a page (with its subpages) to the end of another section. Returns whether it
    /// moved: a page already in `section_id` stays where it is, untouched.
    pub fn move_page(&mut self, id: &str, section_id: &str) -> DbResult<bool> {
        if self.page_section_id(id)?.as_deref() == Some(section_id) {
            return Ok(false);
        }
        self.place_page(id, section_id, None, Some(0))
    }

    /// A new page right below `after_id`: after its subpages at its level, or as its first
    /// subpage (Ctrl+Alt+N / Ctrl+Alt+Shift+N).
    pub fn insert_page_after(&mut self, after_id: &str, subpage: bool) -> DbResult<Page> {
        let Some(section) = self.page_section_id(after_id)? else {
            return Err(DbError::Message(t!("error-page-gone").into_owned()));
        };
        let page = self.create_canvas_page(&section, "")?;
        self.edit_outline(&section, &page.id, |entries| {
            outline::put_after(entries, &page.id, after_id, subpage)
        })?;
        self.get_page(&page.id)?
            .ok_or_else(|| DbError::Message(t!("data-error-page-not-created").into_owned()))
    }

    pub fn search(&self, query: &str, limit: i64) -> DbResult<Vec<SearchHit>> {
        self.search_in(query, SearchFilter::All, limit)
    }

    /// `search` limited to a notebook or some sections (the desktop notebook's search scopes).
    pub fn search_in(
        &self,
        query: &str,
        filter: SearchFilter<'_>,
        limit: i64,
    ) -> DbResult<Vec<SearchHit>> {
        let terms = search_terms(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        // Every word (or quoted phrase) somewhere on the page: its title, its text or a tag.
        let mut matches = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        for term in &terms {
            let pattern = like_pattern(term);
            matches.push(
                "(folio_fold(p.title) LIKE ? ESCAPE '\\'
                  OR folio_fold(p.plain_text) LIKE ? ESCAPE '\\'
                  OR EXISTS (
                      SELECT 1 FROM page_tags pt JOIN tags t ON t.id = pt.tag_id
                      WHERE pt.page_id = p.id AND folio_fold(t.name) LIKE ? ESCAPE '\\'
                  ))",
            );
            args.extend([
                pattern.clone().into(),
                pattern.clone().into(),
                pattern.into(),
            ]);
        }
        let scope = match filter {
            SearchFilter::All => String::new(),
            SearchFilter::Notebook(id) => {
                args.push(id.to_string().into());
                " AND n.id = ?".to_string()
            }
            SearchFilter::Sections([]) => return Ok(Vec::new()),
            SearchFilter::Sections(ids) => {
                args.extend(ids.iter().map(|id| id.clone().into()));
                format!(" AND s.id IN ({})", vec!["?"; ids.len()].join(", "))
            }
        };
        args.push(limit.into());
        let sql = format!(
            "SELECT p.id, p.title, p.plain_text, s.id, s.name, s.color, n.id, n.name, p.updated_at
             FROM pages p
             JOIN sections s ON s.id = p.section_id
             JOIN notebooks n ON n.id = s.notebook_id
             WHERE p.trashed = 0 AND {}{scope}
             ORDER BY p.updated_at DESC
             LIMIT ?",
            matches.join(" AND ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), |row| {
            search_hit_from_row(row, &terms[0])
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Pages edited since `since` (ms), newest first, for History › Recent Edits; every page
    /// when `since` is `None`. Trashed pages are left out.
    pub fn recent_pages(&self, since: Option<i64>, limit: i64) -> DbResult<Vec<SearchHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.title, p.plain_text, s.id, s.name, s.color, n.id, n.name, p.updated_at
             FROM pages p
             JOIN sections s ON s.id = p.section_id
             JOIN notebooks n ON n.id = s.notebook_id
             WHERE p.trashed = 0 AND (?1 IS NULL OR p.updated_at >= ?1)
             ORDER BY p.updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since, limit], |row| search_hit_from_row(row, ""))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Tagged paragraphs and to-dos on pages outside the trash, for Find Tags.
    pub fn tagged_lines(&self, scope: TagScope<'_>) -> DbResult<Vec<crate::model::TaggedLine>> {
        let (section, notebook) = match scope {
            TagScope::Section(id) => (id, ""),
            TagScope::Notebook(id) => ("", id),
            TagScope::All => ("", ""),
        };
        // The LIKE filter only skips pages that cannot have tags; parsing decides.
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.title, p.layout, p.content_json, p.canvas_json,
                    s.id, s.name, s.color, n.id, n.name, p.updated_at
             FROM pages p
             JOIN sections s ON s.id = p.section_id
             JOIN notebooks n ON n.id = s.notebook_id
             WHERE p.trashed = 0 AND (?1 = '' OR s.id = ?1) AND (?2 = '' OR n.id = ?2)
               AND (p.content_json LIKE '%\"tags\":[%' OR p.canvas_json LIKE '%\"tags\":[%'
                    OR p.content_json LIKE '%\"customTags\":[%' OR p.canvas_json LIKE '%\"customTags\":[%'
                    OR p.content_json LIKE '%\"type\":\"todo\"%' OR p.canvas_json LIKE '%\"type\":\"todo\"%')
             ORDER BY n.position, s.position, p.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![section, notebook], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, i64>(10)?,
            ))
        })?;
        let mut lines = Vec::new();
        for row in rows {
            let (
                id,
                title,
                layout,
                content,
                canvas,
                section_id,
                section_name,
                section_color,
                notebook_id,
                notebook_name,
                updated_at,
            ) = row?;
            let (mut canvas, _) = stored_canvas(&layout, &content, &canvas);
            canvas.repair(); // same ids as get_page produces
            let page = crate::model::TaggedPage {
                id: &id,
                title: &title,
                section_id: &section_id,
                section_name: &section_name,
                section_color: &section_color,
                notebook_id: &notebook_id,
                notebook_name: &notebook_name,
                updated_at,
            };
            lines.extend(crate::model::tagged_lines(&page, &canvas));
        }
        Ok(lines)
    }

    /// The rule lines every new page starts with (View ▸ Rule Lines ▸ Always Create Pages
    /// with Rule Lines), or `None` for plain pages.
    pub fn new_page_rule(&self) -> DbResult<Option<RuleLines>> {
        Ok(self
            .setting("new_page_rule")?
            .map(|code| RuleLines::from_code(&code))
            .filter(|rule| *rule != RuleLines::None))
    }

    pub fn set_new_page_rule(&mut self, rule: Option<RuleLines>) -> DbResult<()> {
        // An empty value deletes the setting.
        self.set_setting("new_page_rule", rule.map_or("", RuleLines::code))
    }

    pub fn setting(&self, key: &str) -> DbResult<Option<String>> {
        let value = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value.filter(|value: &String| !value.is_empty()))
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> DbResult<()> {
        if value.is_empty() {
            self.conn
                .execute("DELETE FROM settings WHERE key = ?", [key])?;
        } else {
            self.conn.execute(
                "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        Ok(())
    }

    pub fn import_image(
        &mut self,
        page_id: &str,
        source: &Path,
    ) -> DbResult<crate::model::Attachment> {
        let bytes = fs::read(source)?;
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let mime = match extension.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => "image/png",
        };
        self.save_image_bytes(
            page_id,
            &extension,
            mime,
            &bytes,
            source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image"),
        )
    }

    /// Stores `bytes` as an attachment of `page_id`. `name` is the original file name;
    /// the stored file is named by id, keeping a plain extension (none when empty).
    pub fn save_attachment_bytes(
        &mut self,
        page_id: &str,
        extension: &str,
        mime: &str,
        bytes: &[u8],
        name: &str,
    ) -> DbResult<Attachment> {
        check_attachment_size(name, bytes.len() as u64)?;
        let id = Uuid::new_v4().to_string();
        let extension = safe_extension(extension);
        let file_name = if extension.is_empty() {
            id.clone()
        } else {
            format!("{id}.{extension}")
        };
        let path = self.attachments_dir.join(&file_name);
        fs::write(&path, bytes)?;
        let attachment = Attachment {
            id: id.clone(),
            page_id: page_id.to_string(),
            file_name: name.to_string(),
            stored_path: file_name,
            mime: mime.to_string(),
            byte_size: bytes.len() as u64,
            created_at: now_ms(),
        };
        self.insert_attachment(&attachment)?;
        Ok(attachment)
    }

    /// A copy of attachment `id`, with its own row and file, for `page_id` (a duplicated
    /// page, a picture or file pasted onto another page). The file is copied on disk,
    /// not read into memory, and the size limit for new files doesn't apply: it is
    /// already in the notebook (a long recording can pass it). `None` when the
    /// attachment or its file is gone.
    pub fn copy_attachment(&mut self, page_id: &str, id: &str) -> DbResult<Option<Attachment>> {
        let Some(meta) = self.attachment(id)? else {
            return Ok(None);
        };
        let source = self.attachment_path(&meta.stored_path);
        if !source.is_file() {
            return Ok(None);
        }
        let new_id = Uuid::new_v4().to_string();
        let extension = safe_extension(
            Path::new(&meta.stored_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(""),
        );
        let file_name = if extension.is_empty() {
            new_id.clone()
        } else {
            format!("{new_id}.{extension}")
        };
        let byte_size = fs::copy(&source, self.attachments_dir.join(&file_name))?;
        let copy = Attachment {
            id: new_id,
            page_id: page_id.to_string(),
            file_name: meta.file_name,
            stored_path: file_name,
            mime: meta.mime,
            byte_size,
            created_at: now_ms(),
        };
        self.insert_attachment(&copy)?;
        Ok(Some(copy))
    }

    fn insert_attachment(&self, attachment: &Attachment) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO attachments (id, page_id, file_name, stored_path, mime, byte_size, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                attachment.id,
                attachment.page_id,
                attachment.file_name,
                attachment.stored_path,
                attachment.mime,
                attachment.byte_size as i64,
                attachment.created_at
            ],
        )?;
        Ok(())
    }

    pub fn save_image_bytes(
        &mut self,
        page_id: &str,
        extension: &str,
        mime: &str,
        bytes: &[u8],
        name: &str,
    ) -> DbResult<Attachment> {
        let extension = if extension.trim_start_matches('.').is_empty() {
            "png"
        } else {
            extension
        };
        self.save_attachment_bytes(page_id, extension, mime, bytes, name)
    }

    /// Attaches any file from disk (the desktop notebook's Insert → File Attachment).
    pub fn import_file(&mut self, page_id: &str, source: &Path) -> DbResult<Attachment> {
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        // Checked before reading, so a huge file is never loaded.
        check_attachment_size(&name, fs::metadata(source)?.len())?;
        let bytes = fs::read(source)?;
        let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("");
        self.save_attachment_bytes(
            page_id,
            extension,
            mime_for_extension(extension),
            &bytes,
            &name,
        )
    }

    /// A copy of an attachment under its original name, in `<data>/open/<id>/`, for the
    /// system's own app (which needs the real name and extension). Changes made there
    /// are not saved back into the page, and a copy made earlier is never overwritten:
    /// it may still be open in that app, or hold changes the user saved there.
    pub fn attachment_open_copy(&self, id: &str) -> DbResult<OpenCopy> {
        let meta = self
            .attachment(id)?
            .ok_or_else(|| DbError::Message(t!("data-error-file-missing").into_owned()))?;
        let source = self.attachment_path(&meta.stored_path);
        let dir = self.open_dir().join(&meta.id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(attachment_file_name(&meta));
        if path.exists() {
            let edited = !same_contents(&source, &path)?;
            return Ok(OpenCopy { path, edited });
        }
        fs::copy(&source, &path)?;
        Ok(OpenCopy {
            path,
            edited: false,
        })
    }

    /// Where attachments are copied to be opened with other apps.
    fn open_dir(&self) -> PathBuf {
        self.data_dir.join("open")
    }

    /// Removes the opened copies of attachments that no longer exist (their page was
    /// deleted while Folio was closed, or before copies were cleaned up with it).
    fn prune_open_copies(&self) {
        let Ok(entries) = fs::read_dir(self.open_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(id) = name.to_str() else {
                continue;
            };
            if matches!(self.attachment(id), Ok(None)) && entry.path().is_dir() {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Registers an empty attachment for a file still being written (an audio
    /// recording); `set_attachment_size` records its final size.
    pub fn reserve_attachment(
        &mut self,
        page_id: &str,
        extension: &str,
        mime: &str,
        name: &str,
    ) -> DbResult<Attachment> {
        self.save_attachment_bytes(page_id, extension, mime, &[], name)
    }

    pub fn set_attachment_size(&mut self, id: &str, bytes: u64) -> DbResult<()> {
        self.conn.execute(
            "UPDATE attachments SET byte_size = ? WHERE id = ?",
            params![bytes as i64, id],
        )?;
        Ok(())
    }

    /// Removes an attachment that never made it onto a page (a recording that failed
    /// to start).
    pub fn discard_attachment(&mut self, id: &str) -> DbResult<()> {
        if let Some(meta) = self.attachment(id)? {
            self.conn
                .execute("DELETE FROM attachments WHERE id = ?", [id])?;
            let _ = fs::remove_file(self.attachment_path(&meta.stored_path));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn attachment_count(&self) -> DbResult<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM attachments", [], |row| row.get(0))?)
    }

    pub fn attachment(&self, id: &str) -> DbResult<Option<crate::model::Attachment>> {
        let attachment = self
            .conn
            .query_row(
                "SELECT id, page_id, file_name, stored_path, mime, byte_size, created_at FROM attachments WHERE id = ?",
                [id],
                |row| {
                    Ok(crate::model::Attachment {
                        id: row.get(0)?,
                        page_id: row.get(1)?,
                        file_name: row.get(2)?,
                        stored_path: row.get(3)?,
                        mime: row.get(4)?,
                        byte_size: row.get::<_, i64>(5)? as u64,
                        created_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(attachment)
    }

    pub fn attachment_path(&self, stored_path: &str) -> PathBuf {
        self.attachments_dir.join(stored_path)
    }

    /// Writes an imported the desktop notebook. Its staged files join the attachments, and
    /// everything else goes in one transaction: on any error nothing is added and the moved
    /// files are removed. The staging folder is removed either way.
    pub fn import_plan(&mut self, plan: ImportPlan) -> DbResult<ImportSummary> {
        let mut moved = Vec::new();
        let written = self.write_plan(&plan, &mut moved);
        if written.is_err() {
            for path in moved {
                let _ = fs::remove_file(path);
            }
        }
        let _ = fs::remove_dir_all(&plan.staging);
        let (notebook_id, name, first_page) = written?;
        Ok(ImportSummary {
            notebook_id,
            name,
            first_page,
            report: plan.report,
        })
    }

    fn write_plan(
        &mut self,
        plan: &ImportPlan,
        moved: &mut Vec<PathBuf>,
    ) -> DbResult<(String, String, Option<String>)> {
        for file in &plan.files {
            let to = self.attachments_dir.join(&file.stored);
            let from = plan.staging.join(&file.stored);
            if fs::rename(&from, &to).is_err() {
                // Another disk: copy, then let the staging folder go.
                fs::copy(&from, &to)?;
            }
            moved.push(to);
        }
        let now = now_ms();
        let notebook_id = Uuid::new_v4().to_string();
        let fallback = t!("data-default-notebook");
        let name = if plan.name.trim().is_empty() {
            fallback.as_ref()
        } else {
            plan.name.trim()
        }
        .to_string();
        let tx = self.conn.transaction()?;
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM notebooks",
            [],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO notebooks (id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![notebook_id, name, plan.color, position, now, now],
        )?;
        let mut first_page = None;
        insert_entries(
            &tx,
            &notebook_id,
            None,
            &plan.color,
            &plan.entries,
            now,
            &mut first_page,
        )?;
        for file in &plan.files {
            tx.execute(
                "INSERT INTO attachments (id, page_id, file_name, stored_path, mime, byte_size, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![file.id, file.owner, file.name, file.stored, file.mime, file.size as i64, now],
            )?;
        }
        tx.commit()?;
        Ok((notebook_id, name, first_page))
    }

    /// Removes staging folders an import left behind (the app closed while it was reading).
    /// Only Folio's own are touched: `import-` and a UUID, as `ImportJob::start` names them,
    /// since the data folder may be one the user already kept other folders in.
    fn prune_import_staging(&self) {
        let Ok(entries) = fs::read_dir(&self.data_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let leftover = name
                .to_str()
                .and_then(|name| name.strip_prefix(crate::import_notebook::STAGING_PREFIX))
                .is_some_and(|id| Uuid::parse_str(id).is_ok());
            if leftover && entry.path().is_dir() {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    pub fn storage_info(&self) -> DbResult<crate::model::StorageInfo> {
        let db_bytes = fs::metadata(&self.database_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let mut attachment_bytes = 0u64;
        if let Ok(entries) = fs::read_dir(&self.attachments_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    attachment_bytes += meta.len();
                }
            }
        }
        let page_count =
            self.conn
                .query_row("SELECT COUNT(*) FROM pages WHERE trashed = 0", [], |row| {
                    row.get(0)
                })?;
        let notebook_count = self
            .conn
            .query_row("SELECT COUNT(*) FROM notebooks", [], |row| row.get(0))?;
        Ok(crate::model::StorageInfo {
            database_path: self.database_path.display().to_string(),
            database_bytes: db_bytes,
            attachment_bytes,
            page_count,
            notebook_count,
        })
    }

    /// Writes a compacted copy of the database to `path` and copies the pictures into a
    /// sibling `<name>-attachments` folder. Returns that folder.
    pub fn backup_to(&self, path: &Path) -> DbResult<PathBuf> {
        let live = fs::canonicalize(&self.database_path).unwrap_or(self.database_path.clone());
        let same_file = fs::canonicalize(path).is_ok_and(|target| target == live);
        if same_file || path.starts_with(&self.data_dir) {
            return Err(DbError::Message(
                t!("data-error-backup-folder").into_owned(),
            ));
        }
        // VACUUM INTO refuses to overwrite, so write next to the target and then move it
        // into place: an existing backup is only replaced once the new one is complete.
        let partial = path.with_extension("folio-partial");
        if partial.exists() {
            fs::remove_file(&partial)?;
        }
        if let Err(error) = self
            .conn
            .execute("VACUUM INTO ?", [partial.to_string_lossy().to_string()])
        {
            let _ = fs::remove_file(&partial);
            return Err(error.into());
        }
        fs::rename(&partial, path)?;
        let stem = path.file_stem().map_or_else(
            || "folio-backup".into(),
            |s| s.to_string_lossy().to_string(),
        );
        let media_dir = path.with_file_name(format!("{stem}-attachments"));
        fs::create_dir_all(&media_dir)?;
        for entry in fs::read_dir(&self.attachments_dir)?.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                fs::copy(entry.path(), media_dir.join(entry.file_name()))?;
            }
        }
        Ok(media_dir)
    }
}

/// A test library's folder in /tmp is removed with it, or every run leaves one behind.
#[cfg(test)]
impl Drop for Library {
    fn drop(&mut self) {
        if self.is_test_dir() {
            let _ = fs::remove_dir_all(&self.data_dir);
        }
    }
}

/// Inserts imported section groups, sections and pages under `group_id` (or the notebook's
/// top level), in the plan's order.
fn insert_entries(
    tx: &Transaction<'_>,
    notebook_id: &str,
    group_id: Option<&str>,
    color: &str,
    entries: &[PlanEntry],
    now: i64,
    first_page: &mut Option<String>,
) -> DbResult<()> {
    let (mut groups, mut sections) = (0i64, 0i64);
    for entry in entries {
        match entry {
            PlanEntry::Group { name, entries } => {
                let id = Uuid::new_v4().to_string();
                let fallback = t!("data-fallback-group");
                let name = if name.trim().is_empty() {
                    fallback.as_ref()
                } else {
                    name.trim()
                };
                tx.execute(
                    "INSERT INTO section_groups (id, notebook_id, parent_id, name, color, position, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    params![id, notebook_id, group_id, name, color, groups, now, now],
                )?;
                groups += 1;
                insert_entries(tx, notebook_id, Some(&id), color, entries, now, first_page)?;
            }
            PlanEntry::Section(section) => {
                let id = Uuid::new_v4().to_string();
                let fallback = t!("data-fallback-section");
                let name = if section.name.trim().is_empty() {
                    fallback.as_ref()
                } else {
                    section.name.trim()
                };
                let section_color = if section.color.is_empty() {
                    color
                } else {
                    section.color.as_str()
                };
                tx.execute(
                    "INSERT INTO sections (id, notebook_id, name, color, position, created_at, updated_at, group_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    params![id, notebook_id, name, section_color, sections, now, now, group_id],
                )?;
                sections += 1;
                for (position, page) in section.pages.iter().enumerate() {
                    let plain = page.canvas.plain_text();
                    let words = plain.split_whitespace().count() as i64;
                    // S8: the columns of `create_canvas_page`'s insert, plus the outline
                    // level; `paper` keeps its default (no rule lines, no colour).
                    tx.execute(
                        "INSERT INTO pages (id, section_id, title, content_json, canvas_json, layout, plain_text, position, level, word_count, created_at, updated_at)
                         VALUES (?, ?, ?, '', ?, 'canvas', ?, ?, ?, ?, ?, ?)",
                        params![
                            page.id,
                            id,
                            page.title,
                            page.canvas.to_json(),
                            plain,
                            position as i64,
                            page.level,
                            words,
                            page.created_at,
                            page.updated_at
                        ],
                    )?;
                    first_page.get_or_insert_with(|| page.id.clone());
                }
            }
        }
    }
    Ok(())
}

/// `Library::trash_section`'s steps inside `tx`: the section's group and place are kept
/// for its restore, and its pages not already in the bin go there with it. A section
/// already in the bin keeps its first deletion.
fn trash_section_in(tx: &Transaction<'_>, id: &str, now: i64) -> DbResult<()> {
    tx.execute(
        "INSERT OR IGNORE INTO trashed_sections (section_id, group_id, position, deleted_at)
         SELECT id, group_id, position, ?2 FROM sections WHERE id = ?1",
        params![id, now],
    )?;
    tx.execute(
        "UPDATE pages SET trashed = 1, trashed_with = ?1, updated_at = ?2
         WHERE section_id = ?1 AND trashed = 0",
        params![id, now],
    )?;
    Ok(())
}

fn page_ids(tx: &Transaction<'_>, sql: &str, args: impl rusqlite::Params) -> DbResult<Vec<String>> {
    let mut stmt = tx.prepare(sql)?;
    let rows = stmt.query_map(args, |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// A section's pages in manual order. Trashed pages are not part of it.
fn section_outline(conn: &Connection, section_id: &str) -> DbResult<Vec<outline::Entry>> {
    let mut stmt = conn.prepare(
        "SELECT id, level FROM pages WHERE section_id = ? AND trashed = 0 ORDER BY position, created_at",
    )?;
    let rows = stmt.query_map([section_id], |row| {
        Ok(outline::Entry {
            id: row.get(0)?,
            level: row.get::<_, i64>(1)?.clamp(0, outline::MAX_LEVEL as i64) as u8,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Saves a section's page order. A page left without subpages is no longer folded, so a
/// page that later becomes its subpage is not hidden (or trashed with it) unseen.
fn write_outline(conn: &Connection, entries: &[outline::Entry]) -> DbResult<()> {
    let levels: Vec<u8> = entries.iter().map(|entry| entry.level).collect();
    let mut stmt = conn.prepare(
        "UPDATE pages SET position = ?, level = ?, collapsed = (collapsed AND ?) WHERE id = ?",
    )?;
    for (position, entry) in entries.iter().enumerate() {
        stmt.execute(params![
            position as i64,
            entry.level as i64,
            outline::has_children(&levels, position),
            entry.id
        ])?;
    }
    Ok(())
}

/// Unfolds the pages that `id` now sits under, so it shows in the page list (the desktop notebook
/// expands a page when it gets a new subpage).
fn unfold_ancestors(conn: &Connection, entries: &[outline::Entry], id: &str) -> DbResult<()> {
    let mut stmt = conn.prepare("UPDATE pages SET collapsed = 0 WHERE id = ?")?;
    for ancestor in outline::ancestors(entries, id) {
        stmt.execute([ancestor])?;
    }
    Ok(())
}

/// Before `page` overwrites its stored row, keeps that row as a version when the save
/// really changes the page and its newest version is older than `interval`.
fn keep_version(
    tx: &Transaction<'_>,
    page: &Page,
    canvas: &str,
    now: i64,
    interval: i64,
) -> DbResult<()> {
    let newest: Option<i64> = tx.query_row(
        "SELECT MAX(created_at) FROM page_versions WHERE page_id = ?",
        [&page.id],
        |row| row.get(0),
    )?;
    if newest.is_some_and(|at| now - at < interval) {
        return Ok(());
    }
    let stored = tx
        .query_row(
            "SELECT title, canvas_json, paper, plain_text FROM pages WHERE id = ?",
            [&page.id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((title, stored_canvas, paper, plain)) = stored else {
        return Ok(());
    };
    // A legacy flow row keeps its text in `content_json` and an empty `canvas_json`, so it
    // still counts as changed by any save that writes a canvas.
    let unchanged =
        title == page.title && stored_canvas == canvas && paper == PageStyle::of(page).code();
    if unchanged || is_blank_page(&title, &plain, &stored_canvas) {
        return Ok(());
    }
    insert_version(tx, &page.id, now)
}

/// Copies the page's stored row into `page_versions` and drops the oldest copies past
/// `MAX_VERSIONS`.
fn insert_version(tx: &Transaction<'_>, page_id: &str, now: i64) -> DbResult<()> {
    tx.execute(
        "INSERT INTO page_versions (page_id, title, content_json, canvas_json, layout, paper, plain_text, word_count, saved_at, created_at)
         SELECT id, title, content_json, canvas_json, layout, paper, plain_text, word_count, updated_at, ?
         FROM pages WHERE id = ?",
        params![now, page_id],
    )?;
    tx.execute(
        "DELETE FROM page_versions WHERE page_id = ?1 AND id NOT IN (
             SELECT id FROM page_versions WHERE page_id = ?1
             ORDER BY created_at DESC, id DESC LIMIT ?2)",
        params![page_id, MAX_VERSIONS],
    )?;
    Ok(())
}

/// Width of the note a flow page's text moves into: about as wide as the old flow column.
const FLOW_NOTE_WIDTH: f32 = 700.0;

/// A flow page (the text-only pages of earlier versions) as the desktop notebook pages always are: its
/// text in one note under the title, starting under the title's first letter. A blank
/// document gives an empty page.
fn canvas_from_flow(document: Document) -> CanvasDoc {
    let mut canvas = CanvasDoc::default();
    let blank = document
        .blocks
        .iter()
        .all(|block| block.kind.is_textual() && block.text.text.trim().is_empty());
    if !blank {
        // The note's id must not repeat one of its blocks' ids.
        let highest = document
            .blocks
            .iter()
            .map(|block| block.id)
            .max()
            .unwrap_or(0);
        crate::model::seed_block_ids(highest.saturating_add(1));
        canvas.add(CanvasItem::new(
            CanvasKind::Text { doc: document },
            PAGE_LEFT - crate::canvas::NOTE_PAD_X,
            PAGE_CONTENT_TOP,
            FLOW_NOTE_WIDTH,
            160.0,
        ));
    }
    canvas
}

/// A stored page body as the canvas it is shown as: a canvas row as it is, any other
/// layout (a flow row left by an older Folio, or by a test library that skipped
/// `open_at`'s steps) through [`canvas_from_flow`]. The flag says part of it could not be
/// read.
fn stored_canvas(layout: &str, content_json: &str, canvas_json: &str) -> (CanvasDoc, bool) {
    if layout == "canvas" {
        CanvasDoc::from_json_lossy(canvas_json)
    } else {
        let (document, lossy) = Document::from_json_lossy(content_json);
        (canvas_from_flow(document), lossy)
    }
}

/// Moves a page's content so nothing sits where the title and date now go, or off the
/// page's top-left corner: everything shifts together, keeping its layout. Returns
/// whether anything moved.
fn settle_below_header(canvas: &mut CanvasDoc) -> bool {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    for item in &canvas.items {
        min_x = min_x.min(item.x);
        min_y = min_y.min(item.y);
    }
    for stroke in &canvas.ink {
        for point in &stroke.points {
            min_x = min_x.min(point[0]);
            min_y = min_y.min(point[1]);
        }
    }
    if !min_x.is_finite() || !min_y.is_finite() {
        return false;
    }
    let dx = (-min_x).max(0.0);
    let dy = (PAGE_CONTENT_TOP - min_y).max(0.0);
    if dx == 0.0 && dy == 0.0 {
        return false;
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
    true
}

/// A page with nothing on it yet is not worth keeping as a version.
fn is_blank_page(title: &str, plain: &str, canvas: &str) -> bool {
    if !title.trim().is_empty() || !plain.trim().is_empty() {
        return false;
    }
    let (canvas, _) = CanvasDoc::from_json_lossy(canvas);
    canvas.items.is_empty() && canvas.ink.is_empty()
}

/// Before pages are deleted: keeps attachments that another surviving page still shows
/// (re-homing the row), and returns the stored files that nothing will reference any more.
fn release_attachments(tx: &Transaction<'_>, doomed: &[String]) -> DbResult<Vec<String>> {
    let mut files = Vec::new();
    for page_id in doomed {
        let attachments: Vec<(String, String)> = {
            let mut stmt =
                tx.prepare("SELECT id, stored_path FROM attachments WHERE page_id = ?")?;
            let rows = stmt.query_map([page_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (attachment_id, stored_path) in attachments {
            let pattern = like_pattern(&attachment_id);
            // Kept versions count as users too, so restoring one never finds its picture gone.
            let mut stmt = tx.prepare(
                "SELECT id FROM pages
                 WHERE (content_json LIKE ?1 ESCAPE '\\' OR canvas_json LIKE ?1 ESCAPE '\\')
                 UNION ALL
                 SELECT page_id FROM page_versions
                 WHERE (content_json LIKE ?1 ESCAPE '\\' OR canvas_json LIKE ?1 ESCAPE '\\')",
            )?;
            let users = stmt
                .query_map([&pattern], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if let Some(survivor) = users.iter().find(|id| !doomed.contains(id)) {
                tx.execute(
                    "UPDATE attachments SET page_id = ? WHERE id = ?",
                    params![survivor, attachment_id],
                )?;
            } else {
                files.push(stored_path);
            }
        }
    }
    Ok(files)
}

fn page_attachment_ids(page: &Page) -> Vec<String> {
    fn from_doc(doc: &Document, out: &mut Vec<String>) {
        for block in &doc.blocks {
            if let Some(attachment) = block.kind.attachment() {
                out.push(attachment.to_string());
            }
        }
    }
    let mut ids = Vec::new();
    for item in &page.canvas.items {
        if let Some(attachment) = item.kind.attachment() {
            ids.push(attachment.to_string());
        } else if let Some(doc) = item.doc() {
            from_doc(doc, &mut ids);
        }
        // A printout page's PDF, even with its icon gone, so the copy opens its own.
        if let Some(printout) = item.kind.printout() {
            ids.push(printout.source.clone());
        }
    }
    ids
}

fn remap_page_attachments(page: &mut Page, remap: &HashMap<String, String>) {
    fn in_doc(doc: &mut Document, remap: &HashMap<String, String>) {
        for block in &mut doc.blocks {
            if let Some(attachment) = block.kind.attachment_mut() {
                if let Some(new_id) = remap.get(attachment.as_str()) {
                    *attachment = new_id.clone();
                }
            }
        }
    }
    for item in &mut page.canvas.items {
        if let Some(attachment) = item.kind.attachment_mut() {
            if let Some(new_id) = remap.get(attachment.as_str()) {
                *attachment = new_id.clone();
            }
        } else if let Some(doc) = item.doc_mut() {
            in_doc(doc, remap);
        }
        // The copy's printout pages point at the copy's PDF, as its icon does, so the icon
        // offers no second Insert Printout over them.
        if let Some(printout) = item.kind.printout_mut() {
            if let Some(new_id) = remap.get(printout.source.as_str()) {
                printout.source = new_id.clone();
            }
        }
    }
}

fn ensure_column(conn: &Connection, table: &str, column: &str, declaration: &str) -> DbResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let exists = columns.into_iter().flatten().any(|name| name == column);
    if !exists {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {declaration}"
        ))?;
    }
    Ok(())
}

/// A row of `search_in` or `recent_pages`; the excerpt is centred on `query` (its start
/// when `query` is empty).
fn search_hit_from_row(row: &Row<'_>, query: &str) -> rusqlite::Result<SearchHit> {
    let title: String = row.get(1)?;
    let plain: String = row.get(2)?;
    Ok(SearchHit {
        page_id: row.get(0)?,
        title: if title.trim().is_empty() {
            t!("common-untitled").into_owned()
        } else {
            title
        },
        excerpt: snippet(&plain, query, 130),
        section_id: row.get(3)?,
        section_name: row.get(4)?,
        section_color: row.get(5)?,
        notebook_id: row.get(6)?,
        notebook_name: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn page_summary_from_row(row: &Row<'_>) -> rusqlite::Result<PageSummary> {
    let title: String = row.get(7)?;
    let plain: String = row.get(8)?;
    Ok(PageSummary {
        id: row.get(0)?,
        section_id: row.get(1)?,
        section_name: row.get(2)?,
        section_color: row.get(3)?,
        notebook_id: row.get(4)?,
        notebook_name: row.get(5)?,
        notebook_color: row.get(6)?,
        title: if title.trim().is_empty() {
            t!("common-untitled").into_owned()
        } else {
            title
        },
        excerpt: snippet(&plain, "", 150),
        pinned: row.get(10)?,
        trashed: row.get(11)?,
        word_count: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        tags: Vec::new(),
        level: row
            .get::<_, i64>(15)?
            .clamp(0, crate::outline::MAX_LEVEL as i64) as u8,
        collapsed: row.get(16)?,
        position: row.get(17)?,
    })
}

/// A search's words, as the desktop notebook reads them: each word must be on the page, in any order,
/// and words in quotes ("…", “…” or « … ») must be there as typed. An unclosed quote runs
/// to the end.
pub fn search_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut word = String::new();
    let mut phrase: Option<String> = None;
    for c in query.chars() {
        match phrase.take() {
            Some(text) if matches!(c, '"' | '”' | '»') => push_term(&mut terms, text),
            Some(mut text) => {
                text.push(c);
                phrase = Some(text);
            }
            None if matches!(c, '"' | '“' | '«') => {
                push_term(&mut terms, std::mem::take(&mut word));
                phrase = Some(String::new());
            }
            None if c.is_whitespace() => push_term(&mut terms, std::mem::take(&mut word)),
            None => word.push(c),
        }
    }
    push_term(&mut terms, word);
    if let Some(text) = phrase {
        push_term(&mut terms, text);
    }
    terms
}

fn push_term(terms: &mut Vec<String>, term: String) {
    let term = term.trim();
    if !term.is_empty() {
        terms.push(term.to_string());
    }
}

fn like_pattern(query: &str) -> String {
    format!(
        "%{}%",
        query
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

fn snippet(text: &str, query: &str, max_chars: usize) -> String {
    // A preview shows an equation as text, never its LaTeX.
    let cleaned: String = crate::math::readable_text(text)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return String::new();
    }
    if query.is_empty() {
        return truncate_chars(&cleaned, max_chars);
    }
    let Some(char_pos) = find_case_insensitive(&cleaned, query) else {
        return truncate_chars(&cleaned, max_chars);
    };
    let context = max_chars / 2;
    let start = char_pos.saturating_sub(context);
    let total = cleaned.chars().count();
    let end = (start + max_chars).min(total);
    let piece: String = cleaned.chars().skip(start).take(end - start).collect();
    if start > 0 {
        format!("…{piece}")
    } else if end < total {
        format!("{piece}…")
    } else {
        piece
    }
}

/// Char index in `haystack` where `needle` starts, ignoring case.
fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    crate::util::find_all_folded(haystack, needle)
        .first()
        .map(|range| range.start)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

/// An environment variable that is set, non-empty, and (for XDG paths) absolute.
fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = env_dir("FOLIO_DATA_DIR") {
        return dir;
    }
    #[cfg(target_os = "linux")]
    {
        let base = env_dir("XDG_DATA_HOME")
            .filter(|dir| dir.is_absolute())
            .or_else(|| env_dir("HOME").map(|home| home.join(".local/share")));
        return base.unwrap_or_else(|| PathBuf::from(".")).join("folio");
    }
    #[cfg(target_os = "macos")]
    {
        return env_dir("HOME")
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Folio");
    }
    #[cfg(target_os = "windows")]
    {
        return env_dir("APPDATA")
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Folio");
    }
    #[allow(unreachable_code)]
    PathBuf::from(".folio")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row the sample library writes, without timestamps, file sizes and the
    /// process-wide block and item ids (they come from a global counter). The columns come
    /// from the schema, so a new column is covered without editing this.
    fn seed_dump(lib: &Library) -> String {
        fn without_ids(text: &str) -> String {
            fn strip(value: &mut serde_json::Value) {
                match value {
                    serde_json::Value::Object(map) => {
                        map.remove("id");
                        map.values_mut().for_each(strip);
                    }
                    serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
                    _ => {}
                }
            }
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(mut value) => {
                    strip(&mut value);
                    value.to_string()
                }
                Err(_) => text.to_string(),
            }
        }
        let mut out = String::new();
        for table in [
            "notebooks",
            "section_groups",
            "sections",
            "pages",
            "attachments",
            "tags",
            "page_tags",
            "settings",
        ] {
            let mut info = lib
                .conn
                .prepare(&format!(
                    "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
                ))
                .unwrap();
            let columns: Vec<String> = info
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap)
                .filter(|name| {
                    !matches!(
                        name.as_str(),
                        "created_at" | "updated_at" | "saved_at" | "byte_size"
                    )
                })
                .collect();
            out.push_str(&format!("# {table}\n"));
            let mut stmt = lib
                .conn
                .prepare(&format!(
                    "SELECT {} FROM {table} ORDER BY 1, 2",
                    columns.join(", ")
                ))
                .unwrap();
            let mut rows = stmt.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                // `pages.trashed_with` came after the hash was taken: left out while NULL
                // (no page of the sample is in the Recycle Bin), so a page marked as deleted
                // with a section still changes the dump.
                let cells: Vec<String> = (0..columns.len())
                    .filter_map(|i| match row.get::<_, rusqlite::types::Value>(i).unwrap() {
                        rusqlite::types::Value::Null if columns[i] == "trashed_with" => None,
                        rusqlite::types::Value::Text(text) => Some(without_ids(&text)),
                        other => Some(format!("{other:?}")),
                    })
                    .collect();
                out.push_str(&cells.join(" | "));
                out.push('\n');
            }
        }
        out
    }

    /// FNV-1a: stable across Rust versions, unlike `DefaultHasher`.
    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// The English sample library must not change by accident: the README pictures and UI
    /// tests are built on it. It includes the worksheet printout's text and file sizes, so
    /// a new hayro can change it too. If a change is intended, read the dump, check it,
    /// and paste the new hash.
    const ENGLISH_SEED_HASH: u64 = 0xc146_41c1_e828_a457;

    #[test]
    fn the_english_sample_library_is_unchanged() {
        let mut lib = Library::open_in_memory().unwrap();
        crate::seed::install(&mut lib.conn, crate::i18n::Lang::En).unwrap();
        let dump = seed_dump(&lib);
        let hash = fnv1a(&dump);
        assert_eq!(
            hash, ENGLISH_SEED_HASH,
            "the English sample library changed (hash {hash:#018x}):\n{dump}"
        );
        // A sample page marked as deleted with its section would change it too.
        lib.conn
            .execute(
                "UPDATE pages SET trashed_with = section_id WHERE id = (SELECT MIN(id) FROM pages)",
                [],
            )
            .unwrap();
        assert_ne!(fnv1a(&seed_dump(&lib)), hash);
    }

    #[test]
    fn the_french_sample_library_has_the_english_shape() {
        use crate::i18n::Lang;
        let seeded = |lang: Lang| {
            let mut lib = Library::open_in_memory().unwrap();
            crate::seed::install(&mut lib.conn, lang).unwrap();
            lib
        };
        let (en, fr) = (seeded(Lang::En), seeded(Lang::Fr));
        let skeleton = |lib: &Library| {
            let mut out = Vec::new();
            for sql in [
                "SELECT id || ' ' || color || ' ' || position FROM notebooks ORDER BY id",
                "SELECT id || ' ' || notebook_id || ' ' || color || ' ' || position FROM section_groups ORDER BY id",
                "SELECT id || ' ' || notebook_id || ' ' || COALESCE(group_id, '-') || ' ' || color || ' ' || position FROM sections ORDER BY id",
                "SELECT id || ' ' || section_id || ' ' || layout || ' ' || paper || ' ' || position || ' ' || level FROM pages ORDER BY id",
                "SELECT id || ' ' || page_id || ' ' || stored_path || ' ' || mime FROM attachments ORDER BY id",
                "SELECT key || ' ' || value FROM settings ORDER BY key",
            ] {
                let mut stmt = lib.conn.prepare(sql).unwrap();
                let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
                out.extend(rows.map(Result::unwrap));
            }
            out
        };
        assert_eq!(skeleton(&en), skeleton(&fr));
        for id in crate::seed::PAGE_IDS {
            let shape = |lib: &Library| {
                let page = lib.get_page(id).unwrap().unwrap();
                let items: Vec<String> = page
                    .canvas
                    .items
                    .iter()
                    .map(|item| {
                        let outline: Vec<String> = item
                            .doc()
                            .map(|doc| {
                                doc.blocks
                                    .iter()
                                    .map(|block| {
                                        format!(
                                            "{:?} {} {:?} {}",
                                            std::mem::discriminant(&block.kind),
                                            block.indent,
                                            block.tags,
                                            block.collapsed
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        format!(
                            "{:?} {} {} {} {} {} {outline:?}",
                            std::mem::discriminant(&item.kind),
                            item.x,
                            item.y,
                            item.width,
                            item.height,
                            item.z
                        )
                    })
                    .collect();
                (items, page.canvas.ink.len())
            };
            assert_eq!(shape(&en), shape(&fr), "{id}");
        }
        // And the words are French.
        let names = |sql: &str| -> Vec<String> {
            let mut stmt = fr.conn.prepare(sql).unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
            rows.map(Result::unwrap).collect()
        };
        assert_eq!(
            names("SELECT name FROM notebooks ORDER BY position"),
            ["École", "Personnel"]
        );
        assert_eq!(
            names("SELECT title FROM pages WHERE id IN ('page-school-week', 'page-school-frise', 'page-school-diagram') ORDER BY id"),
            ["La cellule végétale", "Le Moyen Âge", "Ma semaine"]
        );
        assert!(
            names("SELECT plain_text FROM pages WHERE id = 'page-school-week'")[0]
                .contains("La rentrée")
        );
    }

    #[test]
    fn the_french_sample_library_follows_french_typography() {
        let mut lib = Library::open_in_memory().unwrap();
        crate::seed::install(&mut lib.conn, crate::i18n::Lang::Fr).unwrap();
        let mut lines: Vec<String> = Vec::new();
        for sql in [
            "SELECT name FROM notebooks",
            "SELECT name FROM section_groups",
            "SELECT name FROM sections",
            "SELECT title FROM pages",
            // The printout's text comes back from the PDF, not from our words; its words
            // are checked through the PDF's source lines in seed.rs's tests.
            "SELECT plain_text FROM pages WHERE id <> 'page-school-worksheet'",
        ] {
            let mut stmt = lib.conn.prepare(sql).unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
            for text in rows.map(Result::unwrap) {
                lines.extend(text.lines().map(str::to_string));
            }
        }
        let problems: Vec<String> = lines
            .iter()
            .flat_map(|line| crate::i18n::tests::typography_problems(line))
            .collect();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_new_library_has_the_sample_files_and_keeps_its_subpages() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open_at(dir.path().to_path_buf()).unwrap();
        let mut stmt = lib
            .conn
            .prepare("SELECT id, stored_path, mime FROM attachments ORDER BY id")
            .unwrap();
        let files: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            files.iter().any(|(id, ..)| id == "seed-castle"),
            "{files:?}"
        );
        for (id, path, mime) in &files {
            let bytes = std::fs::read(lib.attachment_path(path))
                .unwrap_or_else(|error| panic!("{id} is on disk: {error}"));
            if mime.starts_with("image/") {
                image::load_from_memory(&bytes).unwrap_or_else(|error| panic!("{id}: {error}"));
            }
        }
        // The one-shot upgrades skipped the new library: the subpage is still a subpage.
        let level: i64 = lib
            .conn
            .query_row(
                "SELECT level FROM pages WHERE id = 'page-school-castles'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(level, 1);
    }

    #[test]
    fn the_sample_library_installs_in_both_languages() {
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::Fr] {
            let mut lib = Library::open_in_memory().unwrap();
            crate::seed::install(&mut lib.conn, lang).unwrap();
            let pages: i64 = lib
                .conn
                .query_row("SELECT COUNT(*) FROM pages", [], |row| row.get(0))
                .unwrap();
            assert_eq!(pages as usize, crate::seed::PAGE_IDS.len(), "{lang:?}");
            for id in crate::seed::PAGE_IDS {
                let page = lib
                    .get_page(id)
                    .unwrap()
                    .unwrap_or_else(|| panic!("{id} in {lang:?}"));
                assert!(!page.title.is_empty(), "{id} in {lang:?}");
                assert!(!page.canvas.items.is_empty(), "{id} in {lang:?}");
            }
        }
    }

    #[test]
    fn default_names_follow_the_language() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        let mut lib = Library::open_in_memory().unwrap();
        let nb = lib.create_notebook("  ", "#745cf5").unwrap();
        assert_eq!(nb.name, "Nouveau bloc-notes");
        assert_eq!(nb.sections[0].name, "Première section");
        let section = lib.create_section_in(&nb.id, None, "", "#745cf5").unwrap();
        assert_eq!(section.name, "Nouvelle section");
        let group = lib
            .create_section_group(&nb.id, None, "", "#745cf5")
            .unwrap();
        assert_eq!(group.name, "Nouveau groupe de sections");
        lib.update_notebook(&nb.id, "", "#745cf5").unwrap();
        let workspace = lib.workspace().unwrap();
        let stored = workspace.notebooks.iter().find(|n| n.id == nb.id).unwrap();
        assert_eq!(stored.name, "Bloc-notes");
        // The first section's name was stored, not just returned.
        assert!(stored.sections.iter().any(|s| s.name == "Première section"));
        // The space before ":" is a no-break space.
        let error = DbError::from(rusqlite::Error::QueryReturnedNoRows).to_string();
        assert!(
            error.starts_with("erreur de base de données\u{a0}: "),
            "{error}"
        );
    }

    fn note(text: &str) -> CanvasItem {
        CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![crate::model::Block::paragraph(text)],
                },
            },
            40.0,
            40.0,
            300.0,
            60.0,
        )
    }

    #[test]
    fn workspace_crud_and_search() {
        let mut lib = Library::open_in_memory().unwrap();
        let nb = lib.create_notebook("Test", "#745cf5").unwrap();
        let section = lib.workspace().unwrap().notebooks[0].sections[0].clone();
        let mut page = lib.create_canvas_page(&section.id, "A page").unwrap();
        page.canvas.add(note("A searchable sentence."));
        page.rule = crate::model::RuleLines::Lines(crate::model::RuleSize::College);
        lib.save_page(&page).unwrap();
        let pages = lib
            .page_list(PageFilter::Section(&section.id), "", 50)
            .unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title, "A page");
        assert!(pages[0].excerpt.contains("searchable"));
        let hits = lib.search("searchable", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(lib
            .workspace()
            .unwrap()
            .notebooks
            .iter()
            .any(|n| n.id == nb.id));
    }

    fn first_section(lib: &Library) -> String {
        lib.workspace().unwrap().notebooks[0].sections[0].id.clone()
    }

    #[test]
    fn search_handles_case_folding_that_changes_length() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Voyage")
            .unwrap();
        page.canvas.add(note("İİİİ İstanbul çay à l'École"));
        lib.save_page(&page).unwrap();
        // Used to panic slicing at a byte offset taken from the lowercased string.
        let hits = lib.search("çay", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].excerpt.contains("çay"));
        // Non-ASCII case-insensitive match.
        assert_eq!(lib.search("école", 10).unwrap().len(), 1);
        assert_eq!(
            lib.page_list(PageFilter::All, "ÉCOLE", 10).unwrap().len(),
            1
        );
        assert_eq!(snippet("ab", "x", 10), "ab");
        assert_eq!(find_case_insensitive("İx", "x"), Some(1));
    }

    /// A page's preview in the Recycle Bin, the page list and search results shows its
    /// equation as text, never its LaTeX source; the stored text is unchanged.
    #[test]
    fn previews_show_equations_as_text() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let mut page = lib.create_canvas_page(&section, "Roots").unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: Document {
                    blocks: vec![
                        crate::model::Block::paragraph("Halves"),
                        crate::model::Block::new(crate::model::BlockKind::Equation {
                            source: r"x=\frac{1}{2}".into(),
                            scale: 20.0,
                        }),
                    ],
                },
            },
            40.0,
            40.0,
            300.0,
            60.0,
        ));
        lib.save_page(&page).unwrap();
        let excerpt =
            |lib: &Library, filter| lib.page_list(filter, "", 10).unwrap()[0].excerpt.clone();
        assert_eq!(
            excerpt(&lib, PageFilter::Section(&section)),
            "Halves x = 1/2"
        );
        assert_eq!(
            lib.search("Halves", 10).unwrap()[0].excerpt,
            "Halves x = 1/2"
        );
        lib.trash_page(&page.id).unwrap();
        assert_eq!(excerpt(&lib, PageFilter::Trash), "Halves x = 1/2");
        assert!(lib
            .get_page(&page.id)
            .unwrap()
            .unwrap()
            .plain_text()
            .contains(r"\frac"));
    }

    #[test]
    fn search_in_keeps_to_its_scope() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = lib.create_notebook("Astronomy", "#745cf5").unwrap();
        let b = lib.create_notebook("Biology", "#2e7d32").unwrap();
        let (s_a, s_b) = (a.sections[0].id.clone(), b.sections[0].id.clone());
        let comet = |lib: &mut Library, section: &str| {
            let mut page = lib.create_canvas_page(section, "comet notes").unwrap();
            page.canvas.add(note("A comet passed by."));
            lib.save_page(&page).unwrap();
            page.id
        };
        let in_a = comet(&mut lib, &s_a);
        let in_b = comet(&mut lib, &s_b);
        let ids = |hits: Vec<SearchHit>| -> Vec<String> {
            let mut ids: Vec<String> = hits.into_iter().map(|hit| hit.page_id).collect();
            ids.sort();
            ids
        };
        let mut both = vec![in_a.clone(), in_b.clone()];
        both.sort();
        assert_eq!(
            ids(lib.search_in("comet", SearchFilter::All, 10).unwrap()),
            both
        );
        assert_eq!(ids(lib.search("comet", 10).unwrap()), both);
        assert_eq!(
            ids(lib
                .search_in("comet", SearchFilter::Notebook(&a.id), 10)
                .unwrap()),
            vec![in_a.clone()]
        );
        let only_b = [s_b.clone()];
        let hits = lib
            .search_in("comet", SearchFilter::Sections(&only_b), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].page_id.as_str(), hits[0].notebook_id.as_str()),
            (in_b.as_str(), b.id.as_str())
        );
        assert!(hits[0].excerpt.contains("comet"));
        let both_sections = [s_a, s_b];
        assert_eq!(
            ids(lib
                .search_in("comet", SearchFilter::Sections(&both_sections), 10)
                .unwrap()),
            both
        );
        assert!(lib
            .search_in("comet", SearchFilter::Sections(&[]), 10)
            .unwrap()
            .is_empty());
        assert!(lib
            .search_in("  ", SearchFilter::Notebook(&a.id), 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn recent_pages_lists_the_newest_first_and_skips_the_trash() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids = add_pages(&mut lib, &section, &["Old", "Middle", "", "Binned"]);
        for (id, at) in ids.iter().zip([1_000_i64, 2_000, 3_000, 4_000]) {
            lib.conn
                .execute(
                    "UPDATE pages SET updated_at = ? WHERE id = ?",
                    params![at, id],
                )
                .unwrap();
        }
        // Trashing stamps the page as the newest edit; it is still left out.
        lib.trash_page(&ids[3]).unwrap();
        let listed = |hits: Vec<SearchHit>| -> Vec<String> {
            hits.into_iter().map(|hit| hit.page_id).collect()
        };
        assert_eq!(
            listed(lib.recent_pages(None, 10).unwrap()),
            vec![ids[2].clone(), ids[1].clone(), ids[0].clone()]
        );
        assert_eq!(
            listed(lib.recent_pages(Some(2_000), 10).unwrap()),
            vec![ids[2].clone(), ids[1].clone()]
        );
        assert!(lib.recent_pages(Some(6_000), 10).unwrap().is_empty());
        assert_eq!(
            listed(lib.recent_pages(None, 1).unwrap()),
            vec![ids[2].clone()]
        );
        let newest = &lib.recent_pages(None, 10).unwrap()[0];
        assert_eq!(newest.title, t!("common-untitled"));
        assert_eq!(
            (newest.updated_at, newest.section_id.as_str()),
            (3_000, section.as_str())
        );
    }

    #[test]
    fn a_copy_keeps_pictures_after_original_is_deleted() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Pics")
            .unwrap();
        let attachment = lib
            .save_image_bytes(&page.id, "png", "image/png", b"not-really-a-png", "a.png")
            .unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::image(attachment.id.clone()),
            0.0,
            0.0,
            100.0,
            100.0,
        ));
        lib.save_page(&page).unwrap();
        let copy = lib
            .copy_page_to(&page.id, &page.section_id)
            .unwrap()
            .unwrap();
        let copy_attachment = match &copy.canvas.items[0].kind {
            CanvasKind::Image { attachment, .. } => attachment.clone(),
            _ => unreachable!(),
        };
        assert_ne!(copy_attachment, attachment.id);

        let original_file = lib.attachment_path(&attachment.stored_path);
        lib.trash_page(&page.id).unwrap();
        lib.empty_trash().unwrap();
        assert!(!original_file.exists(), "deleted page's picture is removed");
        let kept = lib.attachment(&copy_attachment).unwrap().unwrap();
        assert!(lib.attachment_path(&kept.stored_path).exists());
    }

    #[test]
    fn shared_legacy_attachment_survives_deleting_its_owner() {
        // Pages duplicated by older builds share one attachment row.
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let mut a = lib.create_canvas_page(&section, "A").unwrap();
        let mut b = lib.create_canvas_page(&section, "B").unwrap();
        let shared = lib
            .save_image_bytes(&a.id, "png", "image/png", b"px", "a.png")
            .unwrap();
        for page in [&mut a, &mut b] {
            page.canvas.add(CanvasItem::new(
                CanvasKind::image(shared.id.clone()),
                0.0,
                0.0,
                100.0,
                100.0,
            ));
            lib.save_page(page).unwrap();
        }
        lib.delete_page_permanently(&a.id).unwrap();
        let row = lib.attachment(&shared.id).unwrap().expect("row re-homed");
        assert_eq!(row.page_id, b.id);
        assert!(lib.attachment_path(&shared.stored_path).exists());
    }

    #[test]
    fn saving_a_deleted_page_is_an_error_and_backup_overwrites() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let page = lib
            .create_canvas_page(&first_section(&lib), "Gone")
            .unwrap();
        lib.delete_page_permanently(&page.id).unwrap();
        assert!(lib.save_page(&page).is_err());

        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("backup.db");
        fs::write(&target, b"old").unwrap();
        let media = lib.backup_to(&target).unwrap();
        assert!(fs::metadata(&target).unwrap().len() > 3);
        assert!(media.ends_with("backup-attachments"));
        assert!(!target.with_extension("folio-partial").exists());
        // Backing up into the live folder (or onto the live file) is refused.
        assert!(lib.backup_to(&lib.data_dir().join("copy.db")).is_err());
    }

    #[test]
    fn tags_and_trash() {
        let mut lib = Library::open_in_memory().unwrap();
        let nb = lib.create_notebook("Test", "#745cf5").unwrap();
        let section = lib
            .workspace()
            .unwrap()
            .notebooks
            .iter()
            .find(|n| n.id == nb.id)
            .unwrap()
            .sections[0]
            .clone();
        let page = lib.create_canvas_page(&section.id, "Tagged").unwrap();
        // Page tags have no UI any more, but tagged pages from before keep them.
        lib.conn
            .execute(
                "INSERT INTO tags (id, name, color, created_at) VALUES ('t1', 'important', '#745cf5', 0)",
                [],
            )
            .unwrap();
        lib.conn
            .execute(
                "INSERT INTO page_tags (page_id, tag_id) VALUES (?, 't1')",
                [&page.id],
            )
            .unwrap();
        assert_eq!(lib.page_tags(&page.id).unwrap().len(), 1);
        lib.trash_page(&page.id).unwrap();
        assert_eq!(lib.page_list(PageFilter::Trash, "", 50).unwrap().len(), 1);
        lib.restore_page(&page.id).unwrap();
        assert_eq!(lib.page_list(PageFilter::All, "", 50).unwrap().len(), 1);
    }

    #[test]
    fn flow_pages_and_old_layouts_become_notebook_pages() {
        use crate::model::{Block, PAGE_CONTENT_TOP, PAGE_LEFT};
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let flow = Document {
            blocks: vec![Block::paragraph("Old flow text")],
        };
        // A flow page as Folio 0.1 stored it, with a kept version, and a page whose layout
        // nobody knows.
        for (id, layout) in [("flow", "flow"), ("odd", "sideways")] {
            lib.conn
                .execute(
                    "INSERT INTO pages (id, section_id, title, content_json, layout, plain_text, paper, position, created_at, updated_at)
                     VALUES (?, ?, 'Old', ?, ?, 'Old flow text', 'ruled', 0, 1, 1)",
                    params![id, section, flow.to_json(), layout],
                )
                .unwrap();
        }
        lib.conn
            .execute(
                "INSERT INTO page_versions (page_id, title, content_json, canvas_json, layout, paper, plain_text, word_count, saved_at, created_at)
                 VALUES ('flow', 'Old', ?, '', 'flow', 'plain', 'Old flow text', 3, 1, 1)",
                params![flow.to_json()],
            )
            .unwrap();
        // A canvas page whose note sits where the title now goes.
        let mut high = lib.create_canvas_page(&section, "High").unwrap();
        let note_id = high.canvas.add(note("Under the title"));
        lib.save_page(&high).unwrap();

        // Read before the pass, a flow row already is a the desktop notebook page.
        let page = lib.get_page("flow").unwrap().unwrap();
        assert_eq!(page.canvas.items.len(), 1);
        assert_eq!(
            page.canvas.items[0].rect().min,
            egui::pos2(PAGE_LEFT - crate::canvas::NOTE_PAD_X, PAGE_CONTENT_TOP)
        );
        assert_eq!(page.plain_text(), "Old flow text");

        lib.one_page_type().unwrap();
        let stored: Vec<(String, String)> = {
            let mut stmt = lib
                .conn
                .prepare(
                    "SELECT layout, content_json FROM pages
                     UNION ALL SELECT layout, content_json FROM page_versions",
                )
                .unwrap();
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        assert!(
            stored
                .iter()
                .all(|(layout, content)| layout == "canvas" && content.is_empty()),
            "{stored:?}"
        );
        assert_eq!(
            lib.get_page("odd").unwrap().unwrap().plain_text(),
            "Old flow text"
        );
        // The high note moved below the date, and only down.
        let high = lib.get_page(&high.id).unwrap().unwrap();
        let moved = high.canvas.item(note_id).unwrap().rect();
        assert_eq!((moved.left(), moved.top()), (40.0, PAGE_CONTENT_TOP));
        // Running it again changes nothing.
        lib.one_page_type().unwrap();
        assert_eq!(lib.get_page(&high.id).unwrap().unwrap().canvas, high.canvas);

        // A flow version kept by an older Folio after the pass still restores as a the desktop notebook page.
        lib.conn
            .execute(
                "INSERT INTO page_versions (page_id, title, content_json, canvas_json, layout, paper, plain_text, word_count, saved_at, created_at)
                 VALUES (?, 'Then', ?, '', 'flow', 'plain', 'Old flow text', 3, 2, 2)",
                params![high.id, flow.to_json()],
            )
            .unwrap();
        // Saving "High" kept its first, empty state as a version too: pick the flow one.
        let version = lib
            .page_versions(&high.id)
            .unwrap()
            .into_iter()
            .find(|version| version.title == "Then")
            .unwrap()
            .id;
        lib.restore_version(&high.id, version).unwrap();
        let restored = lib.get_page(&high.id).unwrap().unwrap();
        assert_eq!(restored.canvas.items.len(), 1);
        assert_eq!(restored.plain_text(), "Old flow text");
    }

    /// A page table as Folio stored it before tables moved into notes (as in model.rs).
    const OLD_TABLE: &str = r#"{"items":[{"id":7,"x":60.0,"y":430.0,"width":452.0,"height":170.0,"z":1,"kind":{"type":"table","rows":[["Part","Job"],["Mitochondria","Energy"]],"header":true}}],"ink":[]}"#;

    #[test]
    fn old_page_tables_are_rewritten_once_and_stay_searchable() {
        let dir = tempfile::tempdir().unwrap();
        {
            let lib = Library::open_at(dir.path().to_path_buf()).unwrap();
            let section: String = lib
                .conn
                .query_row("SELECT id FROM sections LIMIT 1", [], |row| row.get(0))
                .unwrap();
            lib.conn
                .execute(
                    "INSERT INTO pages (id, section_id, title, canvas_json, layout, created_at, updated_at)
                     VALUES ('page-old-table', ?1, 'Old table', ?2, 'canvas', 0, 0)",
                    params![section, OLD_TABLE],
                )
                .unwrap();
            lib.conn
                .execute("DELETE FROM settings WHERE key = 'table_block_version'", [])
                .unwrap();
        }
        let lib = Library::open_at(dir.path().to_path_buf()).unwrap();
        let stored: String = lib
            .conn
            .query_row(
                "SELECT canvas_json FROM pages WHERE id = 'page-old-table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&stored).unwrap();
        assert_eq!(json["items"][0]["kind"]["type"], "text");
        assert_eq!(
            json["items"][0]["kind"]["doc"]["blocks"][0]["kind"]["type"],
            "table"
        );
        let hits = lib.search("mitochondria", 10).unwrap();
        assert!(
            hits.iter().any(|hit| hit.page_id == "page-old-table"),
            "{hits:?}"
        );
    }

    #[test]
    fn the_page_date_saves_and_new_pages_follow_always_rule_lines() {
        use crate::model::{GridSize, RuleLines};
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let mut page = lib.create_canvas_page(&section, "Dated").unwrap();
        page.created_at = 1_000_000;
        lib.save_page(&page).unwrap();
        assert_eq!(
            lib.get_page(&page.id).unwrap().unwrap().created_at,
            1_000_000
        );

        let paper = |lib: &Library, id: &str| -> String {
            lib.conn
                .query_row("SELECT paper FROM pages WHERE id = ?", [id], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        assert_eq!(lib.new_page_rule().unwrap(), None);
        let plain = lib.create_canvas_page(&section, "Plain").unwrap();
        assert_eq!(paper(&lib, &plain.id), "none");
        lib.set_new_page_rule(Some(RuleLines::Grid(GridSize::Small)))
            .unwrap();
        let ruled = lib.create_canvas_page(&section, "Ruled").unwrap();
        assert_eq!(paper(&lib, &ruled.id), "grid-small");
        lib.set_new_page_rule(None).unwrap();
        assert_eq!(lib.new_page_rule().unwrap(), None);

        // The page style (rule lines, colour, hidden title) saves and copies whole.
        let mut styled = lib.get_page(&page.id).unwrap().unwrap();
        styled.rule = RuleLines::Grid(GridSize::Large);
        styled.color = Some(crate::model::PageColor::Lavender);
        styled.title_hidden = true;
        lib.save_page(&styled).unwrap();
        let back = lib.get_page(&page.id).unwrap().unwrap();
        assert_eq!(
            (back.rule, back.color, back.title_hidden),
            (styled.rule, styled.color, true)
        );
        let copy = lib.copy_page_to(&page.id, &section).unwrap().unwrap();
        assert_eq!(
            (copy.rule, copy.color, copy.title_hidden),
            (styled.rule, styled.color, true)
        );
    }

    #[test]
    fn versions_keep_the_page_from_before_each_editing_session() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        lib.set_version_interval_ms(60_000);

        // A blank new page is never kept.
        let mut blank = lib.create_canvas_page(&first_section(&lib), "").unwrap();
        blank.canvas.add(note("Hello"));
        lib.save_page(&blank).unwrap();
        assert!(lib.page_versions(&blank.id).unwrap().is_empty());

        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Diary")
            .unwrap();
        page.canvas.add(note("Monday"));
        lib.save_page(&page).unwrap();
        assert_eq!(lib.page_versions(&page.id).unwrap().len(), 1);
        page.canvas.add(note("Tuesday"));
        lib.save_page(&page).unwrap();
        assert_eq!(
            lib.page_versions(&page.id).unwrap().len(),
            1,
            "a second change within the interval keeps nothing new"
        );

        // Time passes: saving the same content still keeps nothing...
        lib.conn
            .execute(
                "UPDATE page_versions SET created_at = created_at - 120000",
                [],
            )
            .unwrap();
        lib.save_page(&page).unwrap();
        assert_eq!(lib.page_versions(&page.id).unwrap().len(), 1);
        // ...but the next real change keeps the page as it was before it.
        page.canvas.add(note("Wednesday"));
        lib.save_page(&page).unwrap();
        let versions = lib.page_versions(&page.id).unwrap();
        assert_eq!(versions.len(), 2);
        assert!(versions[0].plain_text.contains("Tuesday"));
        assert!(!versions[0].plain_text.contains("Wednesday"));
        assert_eq!(versions[1].title, "Diary");
    }

    #[test]
    fn versions_are_pruned_and_a_restore_keeps_the_current_page() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        lib.set_version_interval_ms(0);
        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Plan")
            .unwrap();
        for n in 0..105 {
            page.title = format!("Draft {n}");
            lib.save_page(&page).unwrap();
        }
        let versions = lib.page_versions(&page.id).unwrap();
        assert_eq!(versions.len(), 100);
        assert_eq!(versions[0].title, "Draft 103");
        let oldest = versions.last().unwrap().clone();
        assert_eq!(oldest.title, "Draft 4");

        lib.restore_version(&page.id, oldest.id).unwrap();
        let restored = lib.get_page(&page.id).unwrap().unwrap();
        assert_eq!(restored.title, "Draft 4");
        let versions = lib.page_versions(&page.id).unwrap();
        assert_eq!(versions.len(), 100);
        assert_eq!(
            versions[0].title, "Draft 104",
            "the page before the restore is kept"
        );

        // A version of another page cannot be restored here.
        let other = lib
            .create_canvas_page(&first_section(&lib), "Other")
            .unwrap();
        assert!(lib.restore_version(&other.id, versions[0].id).is_err());
        assert_eq!(lib.delete_page_versions(&page.id).unwrap(), 100);
        assert!(lib.page_versions(&page.id).unwrap().is_empty());
    }

    #[test]
    fn deleting_one_version_keeps_the_others() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        lib.set_version_interval_ms(0);
        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Plan")
            .unwrap();
        for n in 0..3 {
            page.title = format!("Draft {n}");
            lib.save_page(&page).unwrap();
        }
        let versions = lib.page_versions(&page.id).unwrap();
        assert_eq!(versions.len(), 3);
        let middle = versions[1].clone();

        // A version of another page is not this page's to delete.
        let other = lib
            .create_canvas_page(&first_section(&lib), "Other")
            .unwrap();
        assert!(!lib.delete_version(&other.id, middle.id).unwrap());

        assert!(lib.delete_version(&page.id, middle.id).unwrap());
        let left = lib.page_versions(&page.id).unwrap();
        assert_eq!(
            left.iter().map(|version| version.id).collect::<Vec<_>>(),
            [versions[0].id, versions[2].id]
        );
        assert!(
            !lib.delete_version(&page.id, middle.id).unwrap(),
            "a version is deleted once"
        );
        assert_eq!(lib.get_page(&page.id).unwrap().unwrap().title, "Draft 2");
    }

    #[test]
    fn versions_go_with_their_page_and_keep_its_pictures() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        lib.set_version_interval_ms(0);
        let owner = lib
            .create_canvas_page(&first_section(&lib), "Owner")
            .unwrap();
        let attachment = lib
            .save_image_bytes(&owner.id, "png", "image/png", b"not-really-a-png", "a.png")
            .unwrap();
        // An older page shares the picture (as pages made before copies were separate did),
        // then removes it: only its kept version still shows it.
        let mut user = lib
            .create_canvas_page(&first_section(&lib), "User")
            .unwrap();
        user.canvas.add(CanvasItem::new(
            CanvasKind::image(attachment.id.clone()),
            0.0,
            0.0,
            100.0,
            100.0,
        ));
        lib.save_page(&user).unwrap();
        user.canvas.items.clear();
        lib.save_page(&user).unwrap();
        assert_eq!(lib.page_versions(&user.id).unwrap().len(), 2);

        let copy = lib
            .copy_page_to(&user.id, &user.section_id)
            .unwrap()
            .unwrap();
        assert!(lib.page_versions(&copy.id).unwrap().is_empty());

        lib.delete_page_permanently(&owner.id).unwrap();
        let kept = lib
            .attachment(&attachment.id)
            .unwrap()
            .expect("picture kept");
        assert_eq!(kept.page_id, user.id);
        assert!(lib.attachment_path(&kept.stored_path).exists());

        lib.delete_page_permanently(&user.id).unwrap();
        let left: i64 = lib
            .conn
            .query_row(
                "SELECT COUNT(*) FROM page_versions WHERE page_id = ?",
                [&user.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn versions_are_deleted_by_section_and_notebook() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.set_version_interval_ms(0);
        let a = lib.create_notebook("Astronomy", "#745cf5").unwrap();
        let b = lib.create_notebook("Biology", "#2e7d32").unwrap();
        let first = a.sections[0].id.clone();
        let second = lib.create_section(&a.id, "Planets", "#745cf5").unwrap().id;
        let group = lib
            .create_section_group(&a.id, None, "Moons", "#745cf5")
            .unwrap();
        let grouped = lib
            .create_section_in(&a.id, Some(&group.id), "Titan", "#745cf5")
            .unwrap()
            .id;
        // Two saves that change the page keep two versions of it.
        let versioned = |lib: &mut Library, section: &str, title: &str| {
            let mut page = lib.create_canvas_page(section, title).unwrap();
            page.canvas.add(note("One"));
            lib.save_page(&page).unwrap();
            page.canvas.add(note("Two"));
            lib.save_page(&page).unwrap();
            assert_eq!(lib.page_versions(&page.id).unwrap().len(), 2);
            page.id
        };
        let in_first = versioned(&mut lib, &first, "Comets");
        let in_second = versioned(&mut lib, &second, "Mars");
        let in_group = versioned(&mut lib, &grouped, "Lakes");
        let in_b = versioned(&mut lib, &b.sections[0].id, "Cells");
        let kept = |lib: &Library, id: &str| lib.page_versions(id).unwrap().len();

        assert_eq!(lib.delete_versions_in_section(&first).unwrap(), 2);
        assert_eq!(
            [&in_first, &in_second, &in_group, &in_b].map(|id| kept(&lib, id)),
            [0, 2, 2, 2]
        );
        assert_eq!(lib.delete_versions_in_section(&first).unwrap(), 0);

        assert_eq!(lib.delete_versions_in_notebook(&b.id).unwrap(), 2);
        assert_eq!(
            [&in_second, &in_group, &in_b].map(|id| kept(&lib, id)),
            [2, 2, 0]
        );
        // A notebook takes its grouped sections with it.
        assert_eq!(lib.delete_versions_in_notebook(&a.id).unwrap(), 4);
        assert_eq!([&in_second, &in_group].map(|id| kept(&lib, id)), [0, 0]);
        // The pages themselves stay.
        assert!(lib.get_page(&in_group).unwrap().is_some());
    }

    #[test]
    fn tagged_lines_follow_the_scope_and_skip_the_trash() {
        use crate::model::{Block, LineTag, TagMark};
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("School", "#745cf5").unwrap();
        let biology = first_section(&lib);
        let notebook = lib.workspace().unwrap().notebooks[0].id.clone();
        let maths = lib
            .create_section(&notebook, "Maths", "#745cf5")
            .unwrap()
            .id;
        let tagged = |lib: &mut Library, section: &str, title: &str, tag: LineTag| {
            let mut page = lib.create_canvas_page(section, title).unwrap();
            let mut doc = Document {
                blocks: vec![
                    Block::paragraph(title),
                    Block::todo("Revise", true),
                    Block::paragraph(""),
                ],
            };
            doc.blocks[0].tags = vec![tag];
            doc.blocks[2].tags = vec![LineTag::Idea]; // empty: not listed
            page.canvas.add(CanvasItem::new(
                CanvasKind::Text { doc },
                40.0,
                40.0,
                300.0,
                80.0,
            ));
            lib.save_page(&page).unwrap();
            page
        };
        let cells = tagged(&mut lib, &biology, "Cells divide", LineTag::Important);
        tagged(&mut lib, &maths, "Pythagoras", LineTag::Definition);
        let gone = tagged(&mut lib, &maths, "Old", LineTag::Question);
        lib.trash_page(&gone.id).unwrap();
        let all = lib.tagged_lines(TagScope::All).unwrap();
        assert_eq!(all.len(), 4, "{all:#?}");
        assert!(all.iter().all(|line| line.page_id != gone.id));
        assert!(
            all.iter().all(|line| line.updated_at > 0),
            "Group by Date needs each page's date: {all:#?}"
        );
        assert!(all
            .iter()
            .any(|line| line.mark == TagMark::Todo && line.checked && line.text == "Revise"));
        let important = all
            .iter()
            .find(|line| line.mark == TagMark::Line(LineTag::Important))
            .unwrap();
        assert_eq!(
            (important.item, important.page_title.as_str()),
            (cells.canvas.items[0].id, "Cells divide")
        );
        assert_eq!(
            lib.tagged_lines(TagScope::Section(&maths)).unwrap().len(),
            2
        );
        assert_eq!(
            lib.tagged_lines(TagScope::Notebook(&notebook))
                .unwrap()
                .len(),
            4
        );

        // A page whose only tag is a custom one is listed too.
        let mut page = lib.create_canvas_page(&biology, "Homework").unwrap();
        let mut doc = Document {
            blocks: vec![Block::paragraph("Essay")],
        };
        doc.blocks[0].custom_tags = vec![crate::model::CustomTag {
            name: "Homework".into(),
            symbol: crate::model::TagSymbol::Checkbox,
            font_color: None,
            highlight: None,
            checked: false,
        }];
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text { doc },
            40.0,
            40.0,
            300.0,
            80.0,
        ));
        lib.save_page(&page).unwrap();
        let custom: Vec<_> = lib
            .tagged_lines(TagScope::Section(&biology))
            .unwrap()
            .into_iter()
            .filter(|line| line.mark == TagMark::Custom)
            .collect();
        assert_eq!(custom.len(), 1, "{custom:#?}");
        assert_eq!(custom[0].tag_label(), "Homework");
    }

    fn order(lib: &Library, section: &str) -> Vec<(String, u8)> {
        lib.page_list(PageFilter::Section(section), "", 50)
            .unwrap()
            .into_iter()
            .map(|page| (page.title, page.level))
            .collect()
    }

    fn rows(list: &[(&str, u8)]) -> Vec<(String, u8)> {
        list.iter()
            .map(|(title, level)| (title.to_string(), *level))
            .collect()
    }

    /// Canvas pages titled `titles`, in order, at the end of `section`; returns their ids.
    fn add_pages(lib: &mut Library, section: &str, titles: &[&str]) -> Vec<String> {
        titles
            .iter()
            .map(|title| lib.create_canvas_page(section, title).unwrap().id)
            .collect()
    }

    #[test]
    fn search_finds_every_word_in_any_order_and_quoted_phrases_as_typed() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Quokkas")
            .unwrap();
        page.canvas.add(note("Rottnest quokkas smile widely."));
        lib.save_page(&page).unwrap();
        let count = |query: &str| lib.search(query, 10).unwrap().len();
        assert_eq!(count("smile quokkas"), 1, "words in any order");
        assert_eq!(count("smile Rottnest WIDELY"), 1);
        assert_eq!(count("smile penguins"), 0, "every word must be there");
        assert_eq!(count("\"quokkas smile\""), 1, "a quoted phrase as typed");
        assert_eq!(count("\"smile quokkas\""), 0);
        assert_eq!(count("« quokkas smile »"), 1, "French quotes too");
        assert_eq!(count("\"\""), 0, "nothing to look for");
        assert_eq!(search_terms("a \"b c\" d"), ["a", "b c", "d"]);
        assert_eq!(search_terms("  \"open"), ["open"]);
        let hits = lib.search("widely smile", 10).unwrap();
        assert!(
            hits[0].excerpt.contains("widely"),
            "the snippet shows the first word"
        );
    }

    #[test]
    fn pages_step_up_and_down_in_their_section() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids: Vec<String> = ["A", "B", "C"]
            .iter()
            .map(|title| lib.create_canvas_page(&section, title).unwrap().id)
            .collect();
        let order = |lib: &Library| -> Vec<String> {
            lib.page_list(PageFilter::Section(&section), "", 50)
                .unwrap()
                .into_iter()
                .map(|page| page.title)
                .collect()
        };
        assert_eq!(order(&lib), ["A", "B", "C"]);
        assert!(lib.step_page(&ids[1], true).unwrap());
        assert_eq!(order(&lib), ["B", "A", "C"]);
        assert!(!lib.step_page(&ids[1], true).unwrap());
        assert!(lib.step_page(&ids[0], false).unwrap());
        assert_eq!(order(&lib), ["B", "C", "A"]);
    }

    #[test]
    fn pages_keep_manual_order_and_subpages() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids = add_pages(&mut lib, &section, &["A", "B", "C", "D"]);
        let (a, b, c, d) = (&ids[0], &ids[1], &ids[2], &ids[3]);
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 0), ("C", 0), ("D", 0)])
        );
        // Pinned pages (from before pinning left the UI) don't float to the top of a section.
        lib.conn
            .execute("UPDATE pages SET pinned = 1 WHERE id = ?", [d])
            .unwrap();
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 0), ("C", 0), ("D", 0)])
        );

        // The first page of a section is always a top-level page.
        assert!(!lib.shift_page_level(a, 1).unwrap());
        assert!(lib.shift_page_level(b, 1).unwrap());
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 1), ("C", 0), ("D", 0)])
        );
        assert!(lib.shift_page_level(c, 1).unwrap());
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 1), ("C", 1), ("D", 0)])
        );
        // At most one level deeper than the page above: B is level 1, so C can reach 2.
        assert!(lib.shift_page_level(c, 1).unwrap());
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 1), ("C", 2), ("D", 0)])
        );
        assert!(!lib.shift_page_level(c, 1).unwrap());

        // Promoting B takes its subpage C along.
        assert!(lib.shift_page_level(b, -1).unwrap());
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 0), ("C", 1), ("D", 0)])
        );
    }

    #[test]
    fn trashing_a_folded_parent_takes_its_subpages() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids = add_pages(&mut lib, &section, &["A", "B", "C", "D"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        lib.shift_page_level(&ids[2], 1).unwrap();
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 1), ("C", 1), ("D", 0)])
        );

        lib.set_page_collapsed(&ids[0], true).unwrap();
        let trashed = lib.trash_page(&ids[0]).unwrap();
        assert_eq!(trashed, ids[..3].to_vec());
        assert_eq!(order(&lib, &section), rows(&[("D", 0)]));
        assert_eq!(lib.page_list(PageFilter::Trash, "", 50).unwrap().len(), 3);

        // A restored page comes back at the end, as a top-level page.
        lib.restore_page(&ids[1]).unwrap();
        assert_eq!(order(&lib, &section), rows(&[("D", 0), ("B", 0)]));

        // An unfolded parent goes alone and its subpages move up a level.
        let more = add_pages(&mut lib, &section, &["E", "F"]);
        lib.shift_page_level(&more[1], 1).unwrap();
        assert_eq!(
            order(&lib, &section),
            rows(&[("D", 0), ("B", 0), ("E", 0), ("F", 1)])
        );
        assert_eq!(lib.trash_page(&more[0]).unwrap(), vec![more[0].clone()]);
        assert_eq!(order(&lib, &section), rows(&[("D", 0), ("B", 0), ("F", 0)]));
    }

    fn folded(lib: &Library, section: &str) -> Vec<String> {
        lib.page_list(PageFilter::Section(section), "", 50)
            .unwrap()
            .into_iter()
            .filter(|page| page.collapsed)
            .map(|page| page.title)
            .collect()
    }

    #[test]
    fn a_folded_page_unfolds_when_it_gets_a_subpage() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids = add_pages(&mut lib, &section, &["A", "B", "C"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        lib.set_page_collapsed(&ids[0], true).unwrap();
        assert_eq!(folded(&lib, &section), vec!["A"]);

        // Ctrl+Alt+Shift+N on a folded page: the new subpage shows.
        let new = lib.insert_page_after(&ids[0], true).unwrap();
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("Untitled page", 1), ("B", 1), ("C", 0)])
        );
        assert!(folded(&lib, &section).is_empty());

        // Demoting a page under a folded page unfolds it too.
        lib.set_page_collapsed(&ids[0], true).unwrap();
        assert!(lib.shift_page_level(&ids[2], 1).unwrap());
        assert_eq!(order(&lib, &section)[3], ("C".to_string(), 1));
        assert!(folded(&lib, &section).is_empty());

        // So does dragging a page under it, at a subpage level.
        lib.set_page_collapsed(&ids[0], true).unwrap();
        assert!(lib
            .place_page(&ids[2], &section, Some(&new.id), Some(1))
            .unwrap());
        assert_eq!(order(&lib, &section)[1], ("C".to_string(), 1));
        assert!(folded(&lib, &section).is_empty());

        // A page whose last subpage leaves is no longer folded, so a page that later
        // becomes its subpage neither hides under it nor goes to the trash with it.
        lib.set_page_collapsed(&ids[0], true).unwrap();
        assert!(lib.shift_page_level(&ids[2], -1).unwrap());
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("C", 0), ("Untitled page", 1), ("B", 1)])
        );
        assert!(folded(&lib, &section).is_empty());
        assert!(lib.shift_page_level(&ids[2], 1).unwrap());
        assert_eq!(lib.trash_page(&ids[0]).unwrap(), vec![ids[0].clone()]);
        assert_eq!(
            order(&lib, &section),
            rows(&[("C", 0), ("Untitled page", 1), ("B", 1)])
        );
    }

    #[test]
    fn moving_a_page_moves_its_subpages() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let first = first_section(&lib);
        let second = lib
            .create_section(&notebook.id, "Second", "#745cf5")
            .unwrap()
            .id;
        let ids = add_pages(&mut lib, &first, &["A", "B", "C"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        assert_eq!(order(&lib, &first), rows(&[("A", 0), ("B", 1), ("C", 0)]));

        assert!(lib.move_page(&ids[0], &second).unwrap());
        assert_eq!(order(&lib, &first), rows(&[("C", 0)]));
        assert_eq!(order(&lib, &second), rows(&[("A", 0), ("B", 1)]));
        assert_eq!(
            lib.page_section_id(&ids[1]).unwrap().as_deref(),
            Some(second.as_str())
        );

        // Moving it where it already is changes nothing, not even its edit time.
        let edited = lib.get_page(&ids[0]).unwrap().unwrap().updated_at;
        assert!(!lib.move_page(&ids[0], &second).unwrap());
        assert_eq!(order(&lib, &second), rows(&[("A", 0), ("B", 1)]));
        assert_eq!(lib.get_page(&ids[0]).unwrap().unwrap().updated_at, edited);

        // Placing the only page of a section at its end changes nothing.
        assert!(!lib.place_page(&ids[2], &first, None, None).unwrap());

        // Ctrl+Alt+Shift+N: a new, untitled first subpage right below A.
        let new = lib.insert_page_after(&ids[0], true).unwrap();
        assert_eq!(
            order(&lib, &second),
            rows(&[("A", 0), ("Untitled page", 1), ("B", 1)])
        );

        // Dragging B above the new page puts it there, at the new page's level.
        assert!(lib
            .place_page(&ids[1], &second, Some(&new.id), Some(1))
            .unwrap());
        assert_eq!(
            order(&lib, &second),
            rows(&[("A", 0), ("B", 1), ("Untitled page", 1)])
        );
        assert_eq!(
            lib.page_section_id(&new.id).unwrap().as_deref(),
            Some(second.as_str())
        );
    }

    #[test]
    fn a_pasted_copy_lands_after_the_original() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids = add_pages(&mut lib, &section, &["A", "B", "C"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        // Copy B, then Paste on B: the copy keeps the title and goes right after B's
        // subpages, at B's level (the page list places it before C).
        let copy = lib.copy_page_to(&ids[1], &section).unwrap().unwrap();
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 1), ("C", 0), ("B", 0)])
        );
        assert!(lib
            .place_page(&copy.id, &section, Some(&ids[2]), Some(1))
            .unwrap());
        assert_eq!(
            order(&lib, &section),
            rows(&[("A", 0), ("B", 1), ("B", 1), ("C", 0)])
        );
        let listed: Vec<String> = lib
            .page_list(PageFilter::Section(&section), "", 50)
            .unwrap()
            .into_iter()
            .map(|page| page.id)
            .collect();
        assert_eq!(listed[2], copy.id);
    }

    #[test]
    fn initial_order_follows_the_old_listing() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = first_section(&lib);
        let ids = add_pages(&mut lib, &section, &["B", "C", "D"]);
        lib.conn
            .execute("UPDATE pages SET pinned = 1 WHERE id = ?", [&ids[2]])
            .unwrap();
        for (id, updated) in ids.iter().zip([100_i64, 300, 200]) {
            lib.conn
                .execute(
                    "UPDATE pages SET updated_at = ? WHERE id = ?",
                    params![updated, id],
                )
                .unwrap();
        }
        lib.assign_page_order().unwrap();
        // Pinned first, then the most recently edited.
        assert_eq!(order(&lib, &section), rows(&[("D", 0), ("C", 0), ("B", 0)]));
    }

    #[test]
    fn section_groups_nest_and_delete_into_the_bin() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("School", "#745cf5").unwrap().id;
        let year = lib
            .create_section_group(&notebook, None, "Year 1", "#745cf5")
            .unwrap()
            .id;
        let term = lib
            .create_section_group(&notebook, Some(&year), "Term 1", "#745cf5")
            .unwrap()
            .id;
        let maths = lib
            .create_section_in(&notebook, Some(&term), "Maths", "#745cf5")
            .unwrap()
            .id;
        let mut page = lib.create_canvas_page(&maths, "Pics").unwrap();
        let attachment = lib
            .save_image_bytes(&page.id, "png", "image/png", b"not-really-a-png", "a.png")
            .unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::image(attachment.id.clone()),
            0.0,
            0.0,
            100.0,
            100.0,
        ));
        lib.save_page(&page).unwrap();
        let picture = lib.attachment_path(&attachment.stored_path);
        assert!(picture.exists());
        lib.set_setting("quick_section_id", &maths).unwrap();

        let workspace = lib.workspace().unwrap();
        let school = &workspace.notebooks[0];
        assert_eq!(school.groups.len(), 2);
        let section = school
            .sections
            .iter()
            .find(|section| section.id == maths)
            .unwrap();
        assert_eq!(section.group_id.as_deref(), Some(term.as_str()));

        // Deleting the outer group takes the inner group from the notebook; its section goes
        // to the Recycle Bin with its pages and pictures, until the bin is emptied.
        lib.delete_section_group(&year).unwrap();
        let workspace = lib.workspace().unwrap();
        let school = &workspace.notebooks[0];
        assert!(school.groups.is_empty());
        assert!(school.sections.iter().all(|section| section.id != maths));
        let binned: Vec<String> = lib
            .trashed_sections()
            .unwrap()
            .into_iter()
            .map(|section| section.id)
            .collect();
        assert_eq!(binned, std::slice::from_ref(&maths));
        assert!(lib.get_page(&page.id).unwrap().unwrap().trashed);
        assert!(picture.exists(), "kept while its section is in the bin");
        assert_eq!(lib.setting("quick_section_id").unwrap(), None);
        lib.empty_trash().unwrap();
        assert!(lib.get_page(&page.id).unwrap().is_none());
        assert!(!picture.exists(), "the group's pictures go with the bin");
        let names: Vec<&str> = school
            .sections
            .iter()
            .map(|section| section.name.as_str())
            .collect();
        assert_eq!(names, ["First section"]);
    }

    #[test]
    fn deleting_a_group_sends_its_sections_to_the_bin() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("School", "#745cf5").unwrap().id;
        let year = lib
            .create_section_group(&notebook, None, "Year 1", "#745cf5")
            .unwrap()
            .id;
        let term = lib
            .create_section_group(&notebook, Some(&year), "Term 1", "#745cf5")
            .unwrap()
            .id;
        let history = lib
            .create_section_in(&notebook, Some(&year), "History", "#745cf5")
            .unwrap()
            .id;
        let maths = lib
            .create_section_in(&notebook, Some(&term), "Maths", "#745cf5")
            .unwrap()
            .id;
        // A section of the group already in the bin keeps its own deletion.
        let art = lib
            .create_section_in(&notebook, Some(&term), "Art", "#745cf5")
            .unwrap()
            .id;
        let kings = lib.create_canvas_page(&history, "Kings").unwrap().id;
        let sums = lib.create_canvas_page(&maths, "Sums").unwrap().id;
        let paints = lib.create_canvas_page(&art, "Paints").unwrap().id;
        lib.trash_section(&art).unwrap();
        let art_deleted = lib.trashed_sections().unwrap()[0].deleted_at;

        lib.delete_section_group(&year).unwrap();
        let workspace = lib.workspace().unwrap();
        let school = workspace
            .notebooks
            .iter()
            .find(|candidate| candidate.id == notebook)
            .unwrap();
        assert!(school.groups.is_empty(), "the groups are gone");
        let names: Vec<&str> = school.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["First section"]);
        let mut binned: Vec<(String, i64)> = lib
            .trashed_sections()
            .unwrap()
            .into_iter()
            .map(|section| (section.name, section.page_count))
            .collect();
        binned.sort();
        assert_eq!(
            binned,
            [
                ("Art".to_string(), 1),
                ("History".to_string(), 1),
                ("Maths".to_string(), 1)
            ]
        );
        let art_entry = lib
            .trashed_sections()
            .unwrap()
            .into_iter()
            .find(|section| section.id == art)
            .unwrap();
        assert_eq!(art_entry.deleted_at, art_deleted);
        for page in [&kings, &sums, &paints] {
            assert!(lib.get_page(page).unwrap().unwrap().trashed, "{page}");
        }
        assert!(trash_ids(&lib).is_empty(), "no page is loose in the bin");

        // Its group is gone: it comes back at the notebook's top level, with its page.
        lib.restore_section(&maths).unwrap();
        assert_eq!(section_place(&lib, &maths), Some(None));
        assert!(!lib.get_page(&sums).unwrap().unwrap().trashed);
        assert_eq!(section_place(&lib, &history), None, "the others wait");
    }

    #[test]
    fn sections_move_into_groups_within_their_notebook() {
        let mut lib = Library::open_in_memory().unwrap();
        let first_notebook = lib.create_notebook("One", "#745cf5").unwrap();
        let other_notebook = lib.create_notebook("Two", "#745cf5").unwrap();
        let first = first_notebook.sections[0].id.clone();
        let second = lib
            .create_section(&first_notebook.id, "Second", "#745cf5")
            .unwrap()
            .id;
        let third = lib
            .create_section(&first_notebook.id, "Third", "#745cf5")
            .unwrap()
            .id;
        let elsewhere = lib
            .create_section_group(&other_notebook.id, None, "Elsewhere", "#745cf5")
            .unwrap()
            .id;
        let group = lib
            .create_section_group(&first_notebook.id, None, "Term 1", "#745cf5")
            .unwrap()
            .id;
        let inside = lib
            .create_section_in(&first_notebook.id, Some(&group), "Inside", "#745cf5")
            .unwrap()
            .id;
        // Sections never change notebook.
        assert!(lib.move_section(&first, Some(&elsewhere), None).is_err());

        let listed = |lib: &Library, group: Option<&str>| -> Vec<String> {
            lib.workspace()
                .unwrap()
                .notebooks
                .iter()
                .find(|notebook| notebook.id == first_notebook.id)
                .unwrap()
                .sections
                .iter()
                .filter(|section| section.group_id.as_deref() == group)
                .map(|section| section.id.clone())
                .collect()
        };
        assert_eq!(
            listed(&lib, None),
            [first.as_str(), second.as_str(), third.as_str()]
        );

        lib.move_section(&first, Some(&group), None).unwrap();
        assert_eq!(
            listed(&lib, Some(&group)),
            [inside.as_str(), first.as_str()]
        );
        assert_eq!(listed(&lib, None), [second.as_str(), third.as_str()]);

        lib.move_section(&first, None, Some(&third)).unwrap();
        assert_eq!(listed(&lib, Some(&group)), [inside.as_str()]);
        assert_eq!(
            listed(&lib, None),
            [second.as_str(), first.as_str(), third.as_str()]
        );
    }

    #[test]
    fn notebooks_move_up_and_down() {
        let mut lib = Library::open_in_memory().unwrap();
        let ids: Vec<String> = ["A", "B", "C"]
            .iter()
            .map(|name| lib.create_notebook(name, "#745cf5").unwrap().id)
            .collect();
        let names = |lib: &Library| -> Vec<String> {
            lib.workspace()
                .unwrap()
                .notebooks
                .into_iter()
                .map(|notebook| notebook.name)
                .collect()
        };
        assert_eq!(names(&lib), ["A", "B", "C"]);

        assert!(lib.move_notebook(&ids[1], true).unwrap());
        assert_eq!(names(&lib), ["B", "A", "C"]);
        // B is first now, and C is last: neither can go further.
        assert!(!lib.move_notebook(&ids[1], true).unwrap());
        assert!(!lib.move_notebook(&ids[2], false).unwrap());
        assert!(!lib.move_notebook("no-such-notebook", true).unwrap());
        assert_eq!(names(&lib), ["B", "A", "C"]);

        assert!(lib.move_notebook(&ids[0], false).unwrap());
        assert_eq!(names(&lib), ["B", "C", "A"]);
        let positions: Vec<i64> = lib
            .workspace()
            .unwrap()
            .notebooks
            .iter()
            .map(|notebook| notebook.position)
            .collect();
        assert_eq!(positions, [0, 1, 2]);
    }

    #[test]
    fn copy_page_to_keeps_the_title_and_copies_pictures() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let source = notebook.sections[0].id.clone();
        let target = lib
            .create_section(&notebook.id, "Target", "#745cf5")
            .unwrap()
            .id;
        add_pages(&mut lib, &target, &["First", "Second"]);
        let ids = add_pages(&mut lib, &source, &["Pics", "Child"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        let mut page = lib.get_page(&ids[0]).unwrap().unwrap();
        let attachment = lib
            .save_image_bytes(&page.id, "png", "image/png", b"not-really-a-png", "a.png")
            .unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::image(attachment.id.clone()),
            0.0,
            0.0,
            100.0,
            100.0,
        ));
        lib.save_page(&page).unwrap();

        let copy = lib.copy_page_to(&page.id, &target).unwrap().unwrap();
        assert_ne!(copy.id, page.id);
        assert_eq!(copy.title, "Pics");
        assert_eq!(copy.section_id, target);
        let copied: Vec<&str> = copy
            .canvas
            .items
            .iter()
            .filter_map(|item| item.kind.attachment())
            .collect();
        assert_eq!(copied.len(), 1);
        assert_ne!(copied[0], attachment.id, "the copy has its own picture");
        let meta = lib.attachment(copied[0]).unwrap().unwrap();
        assert_eq!(meta.page_id, copy.id);
        assert_eq!(
            fs::read(lib.attachment_path(&meta.stored_path)).unwrap(),
            fs::read(lib.attachment_path(&attachment.stored_path)).unwrap()
        );
        // The copy comes last, without the original's subpage; the original stays.
        assert_eq!(
            order(&lib, &target),
            rows(&[("First", 0), ("Second", 0), ("Pics", 0)])
        );
        assert_eq!(order(&lib, &source), rows(&[("Pics", 0), ("Child", 1)]));
        // A subpage's copy is a top-level page.
        lib.copy_page_to(&ids[1], &target).unwrap().unwrap();
        assert_eq!(
            order(&lib, &target),
            rows(&[("First", 0), ("Second", 0), ("Pics", 0), ("Child", 0)])
        );
        assert!(lib.copy_page_to("no-such-page", &target).unwrap().is_none());
    }

    /// A copied printout's pages point at the copy's own PDF, as its icon does: the icon
    /// then offers no second Insert Printout, and Open Original opens the copy's file.
    #[test]
    fn a_copied_printout_points_at_its_own_pdf() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let section = notebook.sections[0].id.clone();
        // A printout under its icon, and one whose icon was deleted.
        let ids = add_pages(&mut lib, &section, &["Worksheet", "Pages only"]);
        for (index, id) in ids.iter().enumerate() {
            let mut page = lib.get_page(id).unwrap().unwrap();
            let pdf = lib
                .save_attachment_bytes(id, "pdf", "application/pdf", b"%PDF-1.4", "sheet.pdf")
                .unwrap();
            if index == 0 {
                page.canvas.add(CanvasItem::new(
                    CanvasKind::File {
                        attachment: pdf.id.clone(),
                        name: pdf.file_name.clone(),
                        size: pdf.byte_size,
                    },
                    0.0,
                    0.0,
                    260.0,
                    56.0,
                ));
            }
            for number in 1..=2u32 {
                let picture = lib
                    .save_image_bytes(id, "webp", "image/webp", b"page", "sheet page.webp")
                    .unwrap();
                let mut kind = CanvasKind::image(picture.id);
                if let CanvasKind::Image { printout, .. } = &mut kind {
                    *printout = Some(Box::new(crate::model::PrintoutPage {
                        source: pdf.id.clone(),
                        page: number,
                        text: String::new(),
                        size: None,
                    }));
                }
                page.canvas.add(CanvasItem::new(
                    kind,
                    0.0,
                    number as f32 * 1068.0,
                    816.0,
                    1056.0,
                ));
            }
            lib.save_page(&page).unwrap();
        }
        let sources = |page: &Page| -> Vec<String> {
            page.canvas
                .items
                .iter()
                .filter_map(|item| item.kind.printout())
                .map(|printout| printout.source.clone())
                .collect()
        };

        let copy = lib.copy_page_to(&ids[0], &section).unwrap().unwrap();
        let icon = copy
            .canvas
            .items
            .iter()
            .find_map(|item| match &item.kind {
                CanvasKind::File { attachment, .. } => Some(attachment.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(sources(&copy), [icon.clone(), icon.clone()]);
        assert_eq!(lib.attachment(&icon).unwrap().unwrap().page_id, copy.id);

        let original = lib.get_page(&ids[1]).unwrap().unwrap();
        let copy = lib.copy_page_to(&ids[1], &section).unwrap().unwrap();
        let copied = sources(&copy);
        assert_eq!(copied[0], copied[1], "one PDF for the copy's pages");
        assert_ne!(copied[0], sources(&original)[0]);
        let pdf = lib.attachment(&copied[0]).unwrap().unwrap();
        assert_eq!(
            (pdf.page_id, pdf.file_name),
            (copy.id, "sheet.pdf".to_string())
        );
    }

    #[test]
    fn sections_move_and_copy_between_notebooks() {
        let mut lib = Library::open_in_memory().unwrap();
        let one = lib.create_notebook("One", "#745cf5").unwrap();
        let two = lib.create_notebook("Two", "#2e7d32").unwrap();
        let first = one.sections[0].id.clone();
        let moving = lib.create_section(&one.id, "Comets", "#d13438").unwrap().id;
        let last = lib.create_section(&one.id, "Last", "#745cf5").unwrap().id;
        let group = lib
            .create_section_group(&two.id, None, "Term 1", "#2e7d32")
            .unwrap()
            .id;
        let inside = lib
            .create_section_in(&two.id, Some(&group), "Inside", "#2e7d32")
            .unwrap()
            .id;
        let ids = add_pages(&mut lib, &moving, &["Halley", "Tail", "Binned"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        lib.trash_page(&ids[2]).unwrap();
        let section = |lib: &Library, id: &str| -> Section {
            lib.workspace()
                .unwrap()
                .notebooks
                .into_iter()
                .flat_map(|notebook| notebook.sections)
                .find(|section| section.id == id)
                .unwrap()
        };
        let place = |lib: &Library, id: &str| {
            let section = section(lib, id);
            (section.notebook_id, section.group_id, section.position)
        };

        // A group must belong to the notebook named.
        assert!(lib.move_section_to(&moving, &one.id, Some(&group)).is_err());
        lib.move_section_to(&moving, &two.id, Some(&group)).unwrap();
        assert_eq!(
            place(&lib, &moving),
            (two.id.clone(), Some(group.clone()), 1)
        );
        assert_eq!(place(&lib, &inside).2, 0);
        // The notebook it left closes up.
        assert_eq!(place(&lib, &first), (one.id.clone(), None, 0));
        assert_eq!(place(&lib, &last), (one.id.clone(), None, 1));
        // Its pages went with it.
        assert_eq!(order(&lib, &moving), rows(&[("Halley", 0), ("Tail", 1)]));
        assert_eq!(lib.page_section_id(&ids[2]).unwrap(), Some(moving.clone()));
        // Back to the end of a notebook's top level.
        lib.move_section_to(&moving, &one.id, None).unwrap();
        assert_eq!(place(&lib, &moving), (one.id.clone(), None, 2));
        assert_eq!(place(&lib, &inside).2, 0);

        let copy = lib.copy_section(&moving, &two.id, Some(&group)).unwrap();
        assert_ne!(copy.id, moving);
        assert_eq!(
            (copy.name.as_str(), copy.color.as_str()),
            ("Comets", "#d13438")
        );
        assert_eq!(
            place(&lib, &copy.id),
            (two.id.clone(), Some(group.clone()), 1)
        );
        assert_eq!(order(&lib, &copy.id), rows(&[("Halley", 0), ("Tail", 1)]));
        let trash = lib.page_list(PageFilter::Trash, "", 50).unwrap();
        assert_eq!(trash.len(), 1, "the trashed page is not copied");
        assert_eq!(trash[0].section_id, moving);
        // The original keeps its pages.
        assert_eq!(order(&lib, &moving), rows(&[("Halley", 0), ("Tail", 1)]));
        assert!(lib.copy_section("no-such-section", &two.id, None).is_err());
    }

    #[test]
    fn merging_moves_every_page_and_deletes_the_section() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let into = notebook.sections[0].id.clone();
        let from = lib
            .create_section(&notebook.id, "From", "#745cf5")
            .unwrap()
            .id;
        let ids = add_pages(&mut lib, &from, &["Parent", "Child", "Binned"]);
        lib.shift_page_level(&ids[1], 1).unwrap();
        lib.trash_page(&ids[2]).unwrap();
        lib.set_setting("quick_section_id", &from).unwrap();

        let error = lib.merge_section(&from, &from).unwrap_err();
        assert_eq!(error.to_string(), t!("data-error-merge-self"));
        assert!(lib.merge_section(&from, "no-such-section").is_err());
        assert_eq!(order(&lib, &from), rows(&[("Parent", 0), ("Child", 1)]));

        assert_eq!(lib.merge_section(&from, &into).unwrap(), 3);
        assert_eq!(order(&lib, &into), rows(&[("Parent", 0), ("Child", 1)]));
        let trash = lib.page_list(PageFilter::Trash, "", 50).unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(
            (trash[0].id.as_str(), trash[0].section_id.as_str()),
            (ids[2].as_str(), into.as_str())
        );
        let sections: Vec<String> = lib.workspace().unwrap().notebooks[0]
            .sections
            .iter()
            .map(|section| section.id.clone())
            .collect();
        assert_eq!(sections, [into.as_str()]);
        // It was the quick-notes section: new quick notes go where its pages went.
        assert_eq!(
            lib.setting("quick_section_id").unwrap().as_deref(),
            Some(into.as_str())
        );
        // Merged pages go after the ones already there.
        let other = lib
            .create_section(&notebook.id, "Other", "#745cf5")
            .unwrap()
            .id;
        add_pages(&mut lib, &other, &["Late"]);
        assert_eq!(lib.merge_section(&other, &into).unwrap(), 1);
        assert_eq!(
            order(&lib, &into),
            rows(&[("Parent", 0), ("Child", 1), ("Late", 0)])
        );
    }

    #[test]
    fn any_file_can_be_attached_opened_and_duplicated() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let mut page = lib
            .create_canvas_page(&first_section(&lib), "Files")
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("Lesson Plan.PDF");
        fs::write(&pdf, b"%PDF-1.4 fake").unwrap();
        let attachment = lib.import_file(&page.id, &pdf).unwrap();
        assert_eq!(attachment.mime, "application/pdf");
        assert_eq!(attachment.file_name, "Lesson Plan.PDF");
        assert_eq!(attachment.byte_size, 13);
        assert!(attachment.stored_path.ends_with(".pdf"));
        // Opening hands the system a copy under the original name.
        let opened = lib.attachment_open_copy(&attachment.id).unwrap();
        assert_eq!(opened.path.file_name().unwrap(), "Lesson Plan.PDF");
        assert_eq!(fs::read(&opened.path).unwrap(), b"%PDF-1.4 fake");
        assert!(!opened.edited);
        assert_eq!(lib.attachment_open_copy(&attachment.id).unwrap(), opened);
        // A copy changed and saved in the other app is never overwritten, whether its
        // length changed or not; Folio says it is not the page's own file.
        for edit in [&b"%PDF-1.4 edited in another app"[..], b"%PDF-1.4 FAKE"] {
            fs::write(&opened.path, edit).unwrap();
            let again = lib.attachment_open_copy(&attachment.id).unwrap();
            assert_eq!(again.path, opened.path);
            assert!(again.edited);
            assert_eq!(fs::read(&again.path).unwrap(), edit, "the edits are kept");
        }
        let stored = lib.attachment_path(&attachment.stored_path);
        assert_eq!(fs::read(stored).unwrap(), b"%PDF-1.4 fake");
        // A name without an extension (a pasted screenshot) gets the stored file's.
        let shot = lib
            .save_image_bytes(&page.id, "png", "image/png", b"png", "Pasted image")
            .unwrap();
        let shot_copy = lib.attachment_open_copy(&shot.id).unwrap();
        assert_eq!(shot_copy.path.file_name().unwrap(), "Pasted image.png");
        assert_eq!(attachment_file_name(&attachment), "Lesson Plan.PDF");
        lib.discard_attachment(&shot.id).unwrap();
        // A file without an extension keeps none.
        let makefile = dir.path().join("Makefile");
        fs::write(&makefile, b"all:").unwrap();
        let plain = lib.import_file(&page.id, &makefile).unwrap();
        assert!(!plain.stored_path.contains('.'));
        assert_eq!(plain.mime, "application/octet-stream");
        for attached in [&attachment, &plain] {
            page.canvas.add(CanvasItem::new(
                CanvasKind::File {
                    attachment: attached.id.clone(),
                    name: attached.file_name.clone(),
                    size: attached.byte_size,
                },
                0.0,
                0.0,
                260.0,
                56.0,
            ));
        }
        lib.save_page(&page).unwrap();
        assert_eq!(
            lib.search("lesson plan", 10).unwrap().len(),
            1,
            "file names are searchable"
        );
        let copy = lib
            .copy_page_to(&page.id, &page.section_id)
            .unwrap()
            .unwrap();
        let ids: Vec<&str> = copy
            .canvas
            .items
            .iter()
            .filter_map(|item| item.kind.attachment())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(
            !ids.contains(&attachment.id.as_str()),
            "the copy has its own files"
        );
        for id in &ids {
            let meta = lib.attachment(id).unwrap().unwrap();
            assert!(lib.attachment_path(&meta.stored_path).exists());
            assert_eq!(meta.stored_path.contains('.'), meta.file_name.contains('.'));
        }
        // Opened copies go when their attachment goes, and stray ones at start-up.
        let copy_opened = lib.attachment_open_copy(ids[0]).unwrap().path;
        let copy_dir = copy_opened.parent().unwrap().to_path_buf();
        lib.trash_page(&copy.id).unwrap();
        lib.delete_page_permanently(&copy.id).unwrap();
        assert!(!copy_dir.exists(), "{copy_dir:?}");
        let stray = lib.open_dir().join(Uuid::new_v4().to_string());
        fs::create_dir_all(&stray).unwrap();
        fs::write(stray.join("old.pdf"), b"x").unwrap();
        lib.prune_open_copies();
        assert!(!stray.exists());
        assert!(opened.path.exists(), "copies of live attachments stay");
        // Oversized files are refused before they are read.
        let error = check_attachment_size("film.mov", MAX_ATTACHMENT_BYTES + 1).unwrap_err();
        assert!(error.to_string().contains("200"), "{error}");
    }

    #[test]
    fn a_page_knows_the_sections_groups_and_notebook_it_is_in() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let other = lib.create_notebook("Other", "#745cf5").unwrap();
        let outer = lib
            .create_section_group(&notebook.id, None, "Year", "#745cf5")
            .unwrap();
        let inner = lib
            .create_section_group(&notebook.id, Some(&outer.id), "Term", "#745cf5")
            .unwrap();
        let section = lib
            .create_section_in(&notebook.id, Some(&inner.id), "Maths", "#745cf5")
            .unwrap();
        let page = lib.create_canvas_page(&section.id, "Lecture").unwrap();
        let elsewhere = lib
            .create_canvas_page(&first_section(&lib), "Elsewhere")
            .unwrap();
        for container in [&page.id, &section.id, &inner.id, &outer.id, &notebook.id] {
            assert!(
                lib.page_is_within(&page.id, container).unwrap(),
                "{container}"
            );
        }
        for container in [&elsewhere.id, &other.id] {
            assert!(
                !lib.page_is_within(&page.id, container).unwrap(),
                "{container}"
            );
        }
        assert!(!lib.page_is_within(&elsewhere.id, &outer.id).unwrap());
    }

    #[test]
    fn recordings_reserve_an_attachment_and_record_its_size() {
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let page = lib
            .create_canvas_page(&first_section(&lib), "Lecture")
            .unwrap();
        let reserved = lib
            .reserve_attachment(&page.id, "wav", "audio/wav", "Audio.wav")
            .unwrap();
        assert_eq!(reserved.byte_size, 0);
        let path = lib.attachment_path(&reserved.stored_path);
        assert!(path.exists());
        lib.set_attachment_size(&reserved.id, 1234).unwrap();
        assert_eq!(
            lib.attachment(&reserved.id).unwrap().unwrap().byte_size,
            1234
        );
        lib.discard_attachment(&reserved.id).unwrap();
        assert!(lib.attachment(&reserved.id).unwrap().is_none());
        assert!(!path.exists());
    }

    fn notebook_plan(lib: &Library, fixture: &str) -> crate::import_notebook::ImportPlan {
        use crate::import_notebook::{read, Progress, Source};
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/notebooks")
            .join(fixture);
        let staging = lib
            .data_dir()
            .join(format!("{}test", crate::import_notebook::STAGING_PREFIX));
        read(
            &Source::from_path(&path).unwrap(),
            &staging,
            &Progress::default(),
        )
        .unwrap()
    }

    #[test]
    fn a_notebook_section_becomes_a_searchable_notebook() {
        let mut lib = Library::open_in_memory().unwrap();
        let before = lib.workspace().unwrap().notebooks.len();
        let plan = notebook_plan(&lib, "mpl/handwriting_recognition.one");
        let staging = plan.staging.clone();
        let summary = lib.import_plan(plan).unwrap();
        assert!(!staging.exists(), "the staging folder is removed");
        let workspace = lib.workspace().unwrap();
        assert_eq!(workspace.notebooks.len(), before + 1);
        let notebook = workspace
            .notebooks
            .iter()
            .find(|n| n.id == summary.notebook_id)
            .unwrap();
        assert_eq!(notebook.name, "handwriting_recognition");
        assert_eq!(notebook.sections.len(), 1);
        assert_eq!(
            notebook.sections[0].color,
            crate::theme::SECTION_COLORS[8],
            "a palette colour (Blue Mist), so Section Color ticks it"
        );
        let page = lib
            .get_page(summary.first_page.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(page.title, "Test Page");
        assert_eq!(page.created_at, 1_603_795_643_000);
        assert_eq!(page.canvas.ink.len(), 62);
        assert_eq!(lib.attachment_count().unwrap(), 1, "the screenshot, once");
        assert!(lib
            .search("consetetur", 10)
            .unwrap()
            .iter()
            .any(|hit| hit.page_id == page.id));
    }

    #[test]
    fn a_failed_notebook_import_adds_nothing() {
        let mut lib = Library::open_in_memory().unwrap();
        let before = lib.workspace().unwrap().notebooks.len();
        let mut plan = notebook_plan(&lib, "mpl/handwriting_recognition.one");
        let staging = plan.staging.clone();
        // Two pages with one id: the second insert fails inside the transaction.
        if let Some(crate::import_notebook::PlanEntry::Section(section)) = plan.entries.first_mut()
        {
            let copy = section.pages[0].clone();
            section.pages.push(copy);
        }
        assert!(lib.import_plan(plan).is_err());
        assert_eq!(lib.workspace().unwrap().notebooks.len(), before);
        assert_eq!(lib.attachment_count().unwrap(), 0);
        let files = fs::read_dir(lib.data_dir().join("attachments"))
            .unwrap()
            .count();
        assert_eq!(files, 0, "moved files are removed again");
        assert!(!staging.exists());
    }

    /// Whether section `id` is in the workspace (not in the Recycle Bin), and its group.
    fn section_place(lib: &Library, id: &str) -> Option<Option<String>> {
        lib.workspace()
            .unwrap()
            .notebooks
            .iter()
            .flat_map(|notebook| notebook.sections.iter())
            .find(|section| section.id == id)
            .map(|section| section.group_id.clone())
    }

    fn trash_ids(lib: &Library) -> Vec<String> {
        lib.page_list(PageFilter::Trash, "", 50)
            .unwrap()
            .into_iter()
            .map(|page| page.id)
            .collect()
    }

    #[test]
    fn a_deleted_section_waits_in_the_bin_and_comes_back() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let group = lib
            .create_section_group(&notebook.id, None, "Travel", "#745cf5")
            .unwrap();
        let section = lib
            .create_section_in(&notebook.id, Some(&group.id), "Rivers", "#ff0000")
            .unwrap();
        let mut kept = lib.create_canvas_page(&section.id, "Nile").unwrap();
        kept.canvas.add(note("Hippopotamus"));
        lib.save_page(&kept).unwrap();
        let alone = lib.create_canvas_page(&section.id, "Thames").unwrap();
        lib.trash_page(&alone.id).unwrap();
        lib.set_setting("quick_section_id", &section.id).unwrap();
        assert_eq!(lib.search("Hippopotamus", 10).unwrap().len(), 1);

        lib.trash_section(&section.id).unwrap();
        assert_eq!(
            section_place(&lib, &section.id),
            None,
            "gone from the notebook"
        );
        assert_eq!(
            trash_ids(&lib),
            std::slice::from_ref(&alone.id),
            "only the page deleted alone"
        );
        let trashed = lib.trashed_sections().unwrap();
        assert_eq!(trashed.len(), 1);
        let entry = &trashed[0];
        assert_eq!(
            (
                &entry.id,
                entry.notebook_id.as_str(),
                entry.notebook_name.as_str()
            ),
            (&section.id, notebook.id.as_str(), "Test")
        );
        assert_eq!(
            (entry.name.as_str(), entry.color.as_str()),
            ("Rivers", "#ff0000")
        );
        assert_eq!(entry.page_count, 1, "the page deleted with it");
        assert!(lib.search("Hippopotamus", 10).unwrap().is_empty());
        assert_eq!(lib.workspace().unwrap().trash_count, 3);
        assert_eq!(lib.setting("quick_section_id").unwrap(), None);
        // Its pages wait for it, and it takes no new page, which would be out of sight
        // and go with the bin.
        assert!(lib.restore_page(&kept.id).is_err());
        assert!(lib.restore_page(&alone.id).is_err());
        assert!(lib.create_canvas_page(&section.id, "Hidden").is_err());
        assert!(lib.insert_page_after(&alone.id, false).is_err());

        lib.restore_section(&section.id).unwrap();
        assert_eq!(
            section_place(&lib, &section.id),
            Some(Some(group.id.clone())),
            "back in its group"
        );
        assert!(lib.trashed_sections().unwrap().is_empty());
        let pages: Vec<String> = lib
            .page_list(PageFilter::Section(&section.id), "", 50)
            .unwrap()
            .into_iter()
            .map(|page| page.id)
            .collect();
        assert_eq!(pages, [kept.id.clone()]);
        assert_eq!(lib.search("Hippopotamus", 10).unwrap().len(), 1);
        assert_eq!(
            trash_ids(&lib),
            std::slice::from_ref(&alone.id),
            "still in the bin"
        );
        lib.restore_page(&alone.id).unwrap();
        assert!(trash_ids(&lib).is_empty());
        assert_eq!(lib.workspace().unwrap().trash_count, 0);

        // Deleted again, then its group goes: it comes back at the notebook's top level,
        // after the sections there.
        lib.trash_section(&section.id).unwrap();
        lib.conn
            .execute(
                "UPDATE sections SET group_id = NULL WHERE id = ?",
                [&section.id],
            )
            .unwrap();
        lib.conn
            .execute("DELETE FROM section_groups WHERE id = ?", [&group.id])
            .unwrap();
        lib.restore_section(&section.id).unwrap();
        let workspace = lib.workspace().unwrap();
        let sections = &workspace
            .notebooks
            .iter()
            .find(|n| n.id == notebook.id)
            .unwrap()
            .sections;
        assert_eq!(sections.last().unwrap().id, section.id);
        assert_eq!(sections.last().unwrap().group_id, None);
        // Restoring what is not in the bin does nothing.
        lib.restore_section(&section.id).unwrap();
    }

    #[test]
    fn emptying_the_bin_deletes_trashed_sections_for_good() {
        let mut lib = Library::open_in_memory().unwrap();
        let notebook = lib.create_notebook("Test", "#745cf5").unwrap();
        let section = lib
            .create_section(&notebook.id, "Pictures", "#745cf5")
            .unwrap();
        let mut page = lib.create_canvas_page(&section.id, "Pics").unwrap();
        let attachment = lib
            .save_image_bytes(&page.id, "png", "image/png", b"not-really-a-png", "a.png")
            .unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::image(attachment.id.clone()),
            0.0,
            0.0,
            100.0,
            100.0,
        ));
        lib.save_page(&page).unwrap();
        let alone = lib.create_canvas_page(&section.id, "Alone").unwrap();
        lib.trash_page(&alone.id).unwrap();
        let file = lib.attachment_path(&attachment.stored_path);
        lib.trash_section(&section.id).unwrap();
        assert!(file.exists(), "kept while the section is in the bin");

        // Delete Forever on one section leaves the others in the bin.
        let other = lib
            .create_section(&notebook.id, "Other", "#745cf5")
            .unwrap();
        let other_page = lib.create_canvas_page(&other.id, "Other page").unwrap();
        lib.trash_section(&other.id).unwrap();
        lib.delete_trashed_section(&other.id).unwrap();
        assert!(lib.get_page(&other_page.id).unwrap().is_none());
        assert_eq!(lib.trashed_sections().unwrap().len(), 1);

        lib.empty_trash().unwrap();
        assert!(lib.trashed_sections().unwrap().is_empty());
        assert!(lib.get_page(&page.id).unwrap().is_none());
        assert!(lib.get_page(&alone.id).unwrap().is_none());
        assert!(!file.exists(), "its picture is removed");
        assert_eq!(lib.attachment_count().unwrap(), 0);
        let left: i64 = lib
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE id = ?",
                [&section.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        assert_eq!(lib.workspace().unwrap().trash_count, 0);
    }

    /// A library made before sections went to the Recycle Bin (no `trashed_sections`, no
    /// `pages.trashed_with`) opens with every section and page as it was, and its pages in
    /// the bin stay there, loose.
    #[test]
    fn a_library_from_before_the_section_bin_opens_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let (sections, pages, trashed) = {
            let mut lib = Library::open_at(dir.path().to_path_buf()).unwrap();
            let section = lib.workspace().unwrap().notebooks[0].sections[0].id.clone();
            let page = lib.create_canvas_page(&section, "Binned").unwrap();
            lib.trash_page(&page.id).unwrap();
            // The schema as it was.
            lib.conn
                .execute_batch(
                    "DROP TABLE trashed_sections; ALTER TABLE pages DROP COLUMN trashed_with;",
                )
                .unwrap();
            let count = |lib: &Library, sql: &str| -> i64 {
                lib.conn.query_row(sql, [], |row| row.get(0)).unwrap()
            };
            (
                count(&lib, "SELECT COUNT(*) FROM sections"),
                count(&lib, "SELECT COUNT(*) FROM pages WHERE trashed = 0"),
                vec![page.id],
            )
        };
        let mut lib = Library::open_at(dir.path().to_path_buf()).unwrap();
        let workspace = lib.workspace().unwrap();
        let listed: usize = workspace
            .notebooks
            .iter()
            .map(|notebook| notebook.sections.len())
            .sum();
        assert_eq!(listed as i64, sections);
        assert_eq!(workspace.page_count, pages);
        assert_eq!(trash_ids(&lib), trashed);
        assert!(lib.trashed_sections().unwrap().is_empty());
        // And the bin takes sections now.
        let section = workspace.notebooks[0].sections[0].id.clone();
        lib.trash_section(&section).unwrap();
        assert_eq!(lib.trashed_sections().unwrap().len(), 1);
        lib.restore_section(&section).unwrap();
        assert_eq!(lib.workspace().unwrap().page_count, pages);
    }

    #[test]
    fn opening_a_library_removes_staging_left_by_an_import() {
        let dir = tempfile::tempdir().unwrap();
        let staging = format!(
            "{}{}",
            crate::import_notebook::STAGING_PREFIX,
            Uuid::new_v4()
        );
        fs::create_dir_all(dir.path().join(&staging).join("part")).unwrap();
        // FOLIO_DATA_DIR may point at a folder that already held the user's own files.
        fs::create_dir_all(dir.path().join("import-2023/photos")).unwrap();
        let _lib = Library::open_at(dir.path().to_path_buf()).unwrap();
        assert!(!dir.path().join(&staging).exists());
        assert!(
            dir.path().join("import-2023/photos").is_dir(),
            "a folder Folio did not make is left alone"
        );
    }
}

#[cfg(test)]
mod real_library {
    use super::*;

    /// Opens a copy of a real library (`FOLIO_CHECK_DIR`) and loads every page with the
    /// current code, reporting pages whose stored JSON could not be fully read.
    #[test]
    #[ignore]
    fn every_page_loads() {
        let Some(dir) = std::env::var_os("FOLIO_CHECK_DIR") else {
            return;
        };
        let lib = Library::open_at(PathBuf::from(dir)).unwrap();
        let ids: Vec<String> = {
            let mut stmt = lib.conn.prepare("SELECT id FROM pages").unwrap();
            let rows = stmt.query_map([], |row| row.get(0)).unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        let mut damaged = 0;
        for id in &ids {
            let page = lib.get_page(id).unwrap().expect("page");
            if page.notice.is_some() {
                damaged += 1;
            }
            let _ = page.plain_text();
        }
        eprintln!(
            "loaded {} pages, {damaged} with unreadable parts",
            ids.len()
        );
        assert_eq!(damaged, 0);
        assert!(!lib.search("a", 5).unwrap().is_empty() || ids.is_empty());
    }
}
