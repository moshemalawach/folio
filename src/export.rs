use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use crate::canvas::TITLE_RULE_MIN;
use crate::db::Library;
use crate::i18n::t;
use crate::model::{
    display_title, CanvasItem, CanvasKind, Page, Rgb, ShapeKind, PAGE_CONTENT_TOP, PAGE_LEFT,
    SHAPE_LINE, TITLE_TOP,
};
use crate::util;

/// Files and recordings up to this size travel inside an exported page.
const EMBED_LIMIT: u64 = 10 * 1024 * 1024;

pub fn print_page(page: &Page, library: &Library) -> Result<PathBuf, String> {
    let html = canvas_print_html(page, library);
    // Written inside the user's own data folder rather than the shared temp directory,
    // where a predictable name could be pre-created by someone else. One file per page,
    // replaced on each print.
    let dir = library.data_dir().join("print");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!("{}.html", page.id));
    std::fs::write(&path, html).map_err(|error| error.to_string())?;
    open::that(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

pub fn canvas_print_html(page: &Page, library: &Library) -> String {
    canvas_html(page, library, true)
}

pub fn canvas_export_html(page: &Page, library: &Library) -> String {
    canvas_html(page, library, false)
}

/// Writes each page as `NN-<slug>.html` (canvas export, pictures embedded) into `dir/<name>/`
/// and an `index.html` listing them with links, for File › Export of a section or notebook.
/// Returns the folder written.
pub fn export_pages_to_folder(
    dir: &Path,
    name: &str,
    pages: &[Page],
    library: &Library,
) -> Result<PathBuf, String> {
    let mut slug = util::file_slug(name);
    if slug.is_empty() {
        slug = util::file_slug(&display_title(""));
    }
    let folder = new_folder(dir, &slug)?;
    let written = write_pages(&folder, name, pages, library);
    if written.is_err() {
        // A failed export leaves no half-written folder behind.
        let _ = std::fs::remove_dir_all(&folder);
    }
    written.map(|()| folder)
}

/// `export_pages_to_folder`'s files: each page, then the index linking them.
fn write_pages(folder: &Path, name: &str, pages: &[Page], library: &Library) -> Result<(), String> {
    // "01" to "99", or as many digits as the count needs, so the files sort in order.
    let digits = pages.len().to_string().len().max(2);
    let mut links = String::new();
    for (index, page) in pages.iter().enumerate() {
        let title = display_title(&page.title);
        let page_slug = util::file_slug(&title);
        let file = if page_slug.is_empty() {
            format!("{:0digits$}.html", index + 1)
        } else {
            format!("{:0digits$}-{page_slug}.html", index + 1)
        };
        std::fs::write(folder.join(&file), canvas_export_html(page, library))
            .map_err(|error| error.to_string())?;
        let _ = write!(
            links,
            "<li><a href=\"{}\">{}</a></li>",
            util::html_escape(&file),
            util::html_escape(&title)
        );
    }
    let body = format!("<h1>{}</h1><ol>{links}</ol>", util::html_escape(name));
    std::fs::write(folder.join("index.html"), util::html_document(name, &body))
        .map_err(|error| error.to_string())
}

/// A new folder `slug` in `dir`, or "slug (2)", "slug (3)" and so on when that name is
/// taken: an export never writes into a folder that was already there.
fn new_folder(dir: &Path, slug: &str) -> Result<PathBuf, String> {
    for n in 1..1000 {
        let name = if n == 1 {
            slug.to_string()
        } else {
            format!("{slug} ({n})")
        };
        let folder = dir.join(name);
        match std::fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).to_string())
}

/// How wide a page's title is on its one line, as the canvas draws it (`TITLE_SIZE` in a
/// light face, and 24 px after it). With no fonts here to measure it, a character counts
/// as a generous 0.6 em and a wide one (CJK) as a whole em, so the sheet always has room
/// for the title and never cuts it off.
fn title_width(title: &str) -> f32 {
    let em = crate::theme::TITLE_SIZE;
    let text: f32 = title
        .chars()
        .map(|c| if u32::from(c) >= 0x2E80 { em } else { 0.6 * em })
        .sum();
    text + 24.0
}

fn canvas_html(page: &Page, library: &Library, print: bool) -> String {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for item in &page.canvas.items {
        min_x = min_x.min(item.x);
        min_y = min_y.min(item.y);
        max_x = max_x.max(item.x + item.width);
        max_y = max_y.max(item.y + item.height);
    }
    for stroke in &page.canvas.ink {
        for point in &stroke.points {
            min_x = min_x.min(point[0]);
            min_y = min_y.min(point[1]);
            max_x = max_x.max(point[0]);
            max_y = max_y.max(point[1]);
        }
    }
    // The title, its rule and the date sit on the page where the canvas draws them, the
    // title on one line however long it is.
    if !page.title_hidden {
        min_x = min_x.min(PAGE_LEFT);
        min_y = min_y.min(TITLE_TOP);
        let rule = title_width(&display_title(&page.title)).max(TITLE_RULE_MIN);
        max_x = max_x.max(PAGE_LEFT + rule);
        max_y = max_y.max(PAGE_CONTENT_TOP);
    }
    if !min_x.is_finite() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 1200.0;
        max_y = 700.0;
    }
    let pad = 50.0;
    min_x -= pad;
    min_y -= pad;
    max_x += pad;
    max_y += pad;
    let width = (max_x - min_x).max(400.0);
    let height = (max_y - min_y).max(260.0);
    // The page is laid out at its own size inside `.sheet-inner`, which one CSS transform
    // shrinks to fit the sheet: item boxes, ink and text all scale together, exactly once.
    // A printout is read page after page: it keeps the sheet's width and runs on over
    // as many printed sheets as it needs, instead of shrinking to fit one.
    let printout = page
        .canvas
        .items
        .iter()
        .any(|item| item.kind.printout().is_some());
    let scale = if printout {
        (1080.0 / width).clamp(0.15, 1.0)
    } else {
        (1080.0 / width).min(720.0 / height).clamp(0.15, 1.0)
    };
    let scaled_width = width * scale;
    let scaled_height = height * scale;

    let mut items = String::new();
    let mut ordered: Vec<&CanvasItem> = page.canvas.items.iter().collect();
    // Background pictures lie under everything, as the page paints them, whatever their z.
    ordered.sort_by_key(|item| (!item.is_background(), item.z));
    for item in ordered {
        let left = item.x - min_x;
        let top = item.y - min_y;
        let item_width = item.width;
        let item_height = item.height;
        let style = format!(
            "left:{left:.2}px;top:{top:.2}px;width:{item_width:.2}px;height:{item_height:.2}px;"
        );
        match &item.kind {
            CanvasKind::Text { doc } => {
                // Its tags hang in the note's left padding, as on the page.
                let lead = crate::editor::tag_lead(doc);
                let _ = write!(
                    items,
                    "<div class=\"note\" style=\"{style}padding-left:{:.0}px;\">{}</div>",
                    8.0 + lead,
                    inline_block_media(inline_attachments(doc.to_html(), library), library, print)
                );
            }
            CanvasKind::Image {
                attachment,
                meta,
                printout,
            } => {
                // A cropped picture travels cropped, so the page shows only its kept part.
                let src = meta
                    .crop
                    .and_then(|_| cropped_data_uri(library, attachment, meta.crop_box()))
                    .or_else(|| attachment_data_uri(library, attachment))
                    .unwrap_or_default();
                // A printout page reads "Printout page 2" unless the picture has its own
                // alt text, which wins.
                let class = if printout.is_some() {
                    "picture printout"
                } else {
                    "picture"
                };
                let alt = match printout {
                    Some(printout) if meta.alt.is_empty() => {
                        util::html_escape(&t!("printout-page-alt", page = printout.page))
                    }
                    _ => util::html_escape(&meta.alt),
                };
                // A quarter-turned picture's <img> keeps the picture's own proportions:
                // it is as wide as the box is tall, centred and turned into the box (past
                // the stylesheet's `max-width: 100%`, which would squeeze it to the box).
                let size = if meta.quarter_turned() {
                    format!(
                        "position:absolute;left:50%;top:50%;width:{h:.0}px;height:{w:.0}px;max-width:none;max-height:none;margin-left:-{hh:.0}px;margin-top:-{hw:.0}px;",
                        w = item_width,
                        h = item_height,
                        hh = item_height / 2.0,
                        hw = item_width / 2.0
                    )
                } else {
                    String::new()
                };
                let turn = crate::model::picture_css(meta);
                let _ = write!(
                    items,
                    "<div class=\"{class}\" style=\"{style}\"><img src=\"{src}\" alt=\"{alt}\" style=\"{size}{turn}\"></div>"
                );
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
                let thickness = thickness.unwrap_or(SHAPE_LINE);
                if matches!(shape, ShapeKind::Line | ShapeKind::Arrow) {
                    // A straight line's box can be zero pixels tall; pad it so it shows.
                    let pad = 8.0;
                    let (x1, x2) = if *flip_x {
                        (item_width, 0.0)
                    } else {
                        (0.0, item_width)
                    };
                    let (y1, y2) = if *flip_y {
                        (item_height, 0.0)
                    } else {
                        (0.0, item_height)
                    };
                    let line = hex(stroke);
                    let (defs, marker_attr) = if *shape == ShapeKind::Arrow {
                        let id = format!("arrow-{}", item.id);
                        (
                            format!("<defs><marker id=\"{id}\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"3\" orient=\"auto\"><path d=\"M0,0 L0,6 L7,3 z\" fill=\"#{line}\"/></marker></defs>"),
                            format!("marker-end=\"url(#{id})\""),
                        )
                    } else {
                        (String::new(), String::new())
                    };
                    let _ = write!(
                        items,
                        "<svg class=\"connector\" style=\"left:{:.2}px;top:{:.2}px;width:{:.2}px;height:{:.2}px;\" viewBox=\"0 0 {:.2} {:.2}\" xmlns=\"http://www.w3.org/2000/svg\">{defs}<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#{line}\" stroke-width=\"{thickness:.1}\" {marker_attr}/></svg>",
                        left - pad,
                        top - pad,
                        item_width + pad * 2.0,
                        item_height + pad * 2.0,
                        item_width + pad * 2.0,
                        item_height + pad * 2.0,
                        x1 + pad,
                        y1 + pad,
                        x2 + pad,
                        y2 + pad,
                    );
                    continue;
                }
                let line = hex(stroke);
                let fill_hex = fill
                    .as_ref()
                    .map(|color| format!("#{}", hex(color)))
                    .unwrap_or_else(|| "none".to_string());
                let shape_tag = svg_shape(
                    *shape,
                    item_width,
                    item_height,
                    *flip_y,
                    thickness,
                    &line,
                    &fill_hex,
                );
                let label = if label.is_empty() {
                    String::new()
                } else {
                    format!(
                        "<div class=\"shape-label\">{}</div>",
                        util::html_escape(label)
                    )
                };
                let _ = write!(
                    items,
                    "<div class=\"shape\" style=\"{style}\">{shape_tag}{label}</div>"
                );
            }
            CanvasKind::Timeline { color, milestones } => {
                let accent = hex(color);
                let _ = write!(
                    items,
                    "<div class=\"timeline\" style=\"{style}--accent:#{accent};\">"
                );
                let _ = write!(items, "<div class=\"timeline-line\"></div>");
                let slot = 100.0 / milestones.len().max(1) as f32;
                for (index, milestone) in milestones.iter().enumerate() {
                    let percent = (index as f32 + 0.5) * slot;
                    let _ = write!(
                        items,
                        "<div class=\"milestone\" style=\"left:{percent:.2}%;width:{slot:.2}%;\"><span class=\"milestone-head\"><span class=\"milestone-title\">{}</span><span class=\"milestone-date\">{}</span></span><span class=\"milestone-note\">{}</span></div>",
                        util::html_escape(&milestone.title),
                        util::html_escape(&milestone.date),
                        util::html_escape(&milestone.note)
                    );
                }
                items.push_str("</div>");
            }
            CanvasKind::File {
                attachment,
                name,
                size,
            } => {
                let card = format!(
                    "<span class=\"file-icon\">📎</span><span class=\"file-name\">{}</span><span class=\"file-size\">{}</span>",
                    util::html_escape(name),
                    util::file_size(*size)
                );
                // A standalone export carries small files inside the page; print shows the card.
                let href = (!print && *size <= EMBED_LIMIT)
                    .then(|| attachment_data_uri(library, attachment))
                    .flatten();
                match href {
                    Some(href) => {
                        let _ = write!(
                            items,
                            "<a class=\"attachment\" style=\"{style}\" download=\"{}\" href=\"{href}\">{card}</a>",
                            util::html_escape(name)
                        );
                    }
                    None => {
                        let _ = write!(
                            items,
                            "<div class=\"attachment\" style=\"{style}\">{card}</div>"
                        );
                    }
                }
            }
            CanvasKind::Audio {
                attachment,
                duration_ms,
            } => {
                let path = library
                    .attachment(attachment)
                    .ok()
                    .flatten()
                    .map(|meta| library.attachment_path(&meta.stored_path));
                let length = match *duration_ms {
                    0 => path
                        .as_deref()
                        .and_then(crate::audio::duration_ms)
                        .unwrap_or(0),
                    ms => ms,
                };
                let size = path
                    .as_deref()
                    .and_then(|path| std::fs::metadata(path).ok())
                    .map_or(u64::MAX, |meta| meta.len());
                let label = format!(
                    "🔊 {}",
                    t!(
                        "data-export-audio",
                        time = crate::audio::format_time(length)
                    )
                );
                let src = (!print && size <= EMBED_LIMIT)
                    .then(|| attachment_data_uri(library, attachment))
                    .flatten();
                match src {
                    Some(src) => {
                        let _ = write!(
                            items,
                            "<div class=\"audio\" style=\"{style}\"><audio controls preload=\"none\" title=\"{}\" src=\"{src}\"></audio></div>",
                            util::html_escape(&label)
                        );
                    }
                    None => {
                        let _ = write!(
                            items,
                            "<div class=\"audio\" style=\"{style}\">{}</div>",
                            util::html_escape(&label)
                        );
                    }
                }
            }
            // Converted to a note holding a table when it loads (CanvasDoc::repair); never drawn.
            CanvasKind::Table { .. } => {}
            CanvasKind::Equation { source, size } => {
                // Page units, like every other item: `.sheet-inner` does the scaling.
                let font = *size;
                let _ = write!(
                    items,
                    "<div class=\"equation\" style=\"{style}font-size:{font:.1}px;\">{}</div>",
                    crate::math::mathml_of(source)
                );
            }
        }
    }

    let mut ink = String::new();
    if !page.canvas.ink.is_empty() {
        let _ = write!(
            ink,
            "<svg class=\"ink\" width=\"{:.2}\" height=\"{:.2}\" viewBox=\"0 0 {:.2} {:.2}\" xmlns=\"http://www.w3.org/2000/svg\">",
            width, height, width, height
        );
        for stroke in &page.canvas.ink {
            let points: Vec<String> = stroke
                .points
                .iter()
                .map(|point| format!("{:.1},{:.1}", point[0] - min_x, point[1] - min_y))
                .collect();
            let opacity = if stroke.highlighter { "0.35" } else { "1" };
            let stroke_width = if stroke.highlighter {
                stroke.width * 4.0
            } else {
                stroke.width
            };
            let color = hex(&stroke.color);
            let _ = write!(
                ink,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"#{color}\" stroke-width=\"{stroke_width:.2}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" opacity=\"{opacity}\"/>",
                points.join(" ")
            );
        }
        ink.push_str("</svg>");
    }

    // The header is at least as wide as the canvas's rule, wider under a long title.
    let header = if page.title_hidden {
        String::new()
    } else {
        format!(
            "<div class=\"page-header\" style=\"left:{:.2}px;top:{:.2}px;min-width:{TITLE_RULE_MIN:.0}px;\"><h1>{}</h1><hr><p>{}</p></div>",
            PAGE_LEFT - min_x,
            TITLE_TOP - min_y,
            util::html_escape(&display_title(&page.title)),
            util::html_escape(&export_stamp(page.created_at)),
        )
    };

    let (fill, lines) = page_background_css(page, min_x, min_y);

    let script = if print {
        "<script>addEventListener('load',()=>setTimeout(()=>print(),450));</script>"
    } else {
        ""
    };
    let lang = crate::i18n::lang().code();
    format!(
        "<html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>{}</title>{}</head><body><main><div class=\"sheet\" style=\"width:{scaled_width:.2}px;height:{scaled_height:.2}px;{fill}\"><div class=\"sheet-inner\" style=\"width:{width:.2}px;height:{height:.2}px;transform:scale({scale:.4});{lines}\">{header}{items}{ink}</div></div></main>{script}</body></html>",
        util::html_escape(&display_title(&page.title)),
        print_css(),
    )
}

/// The date and time under an exported title, as the page header shows them.
fn export_stamp(created_at: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(created_at)
        .single()
        .map(|dt| format!("{}  {}", util::long_date(dt), util::clock_time(dt)))
        .unwrap_or_default()
}

/// The exported sheet's background: the page colour (for the sheet) and its rule lines or
/// grid (for `.sheet-inner`, which holds the page from `(min_x, min_y)` at its own scale), in
/// the same blue as on screen and at the same places, with the desktop notebook's red margin line on
/// ruled pages.
fn page_background_css(page: &Page, min_x: f32, min_y: f32) -> (String, String) {
    use crate::canvas::{FIRST_RULE, RULE_MARGIN};
    use crate::model::RuleLines;
    let fill = page.color.map_or_else(
        || "#ffffff".to_string(),
        |color| format!("#{}", hex(&color.light())),
    );
    let fill = format!("background:{fill};");
    let Some(step) = page.rule.spacing() else {
        return (fill, String::new());
    };
    let line = page
        .rule_color
        .map_or_else(|| "#a9c8e8".to_string(), |color| color.hex());
    let under = step - 1.0;
    // Each tile draws its line in its last pixel: shift the tiles so lines are centred where
    // the page draws them, `FIRST_RULE` plus whole spacings down and whole spacings across.
    let down = (FIRST_RULE - min_y + 0.5).rem_euclid(step);
    let across = (0.5 - min_x).rem_euclid(step);
    let rows = format!(
        "repeating-linear-gradient(to bottom,transparent 0,transparent {under}px,{line} {under}px,{line} {step}px)"
    );
    let lines = match page.rule {
        RuleLines::Grid(_) => format!(
            "background-image:{rows},repeating-linear-gradient(to right,transparent 0,transparent {under}px,{line} {under}px,{line} {step}px);background-position:0 {down:.2}px,{across:.2}px 0;"
        ),
        _ => {
            let margin = RULE_MARGIN - 0.5 - min_x;
            if margin + 1.0 > 0.0 {
                format!(
                    "background-image:linear-gradient(to right,transparent {margin:.2}px,#e8a0a0 {margin:.2}px,#e8a0a0 {:.2}px,transparent {:.2}px),{rows};background-position:0 0,0 {down:.2}px;",
                    margin + 1.0,
                    margin + 1.0
                )
            } else {
                format!("background-image:{rows};background-position:0 {down:.2}px;")
            }
        }
    };
    (fill, lines)
}

/// Puts the pictures a note holds (a converted flow page's images and sketches) inside
/// the page, as `canvas_html` does for picture items.
fn inline_attachments(mut body: String, library: &Library) -> String {
    let ids: Vec<String> = body
        .split("src=\"")
        .skip(1)
        .filter_map(|part| part.split('"').next().map(str::to_string))
        .collect();
    for id in ids {
        if let Some(src) = attachment_data_uri(library, &id) {
            body = body.replace(&format!("src=\"{id}\""), &format!("src=\"{src}\""));
        }
    }
    body
}

/// Gives a note's file and recording blocks their content: a download link or a player
/// for files small enough to carry inside the page, the plain card otherwise (and in
/// print, where nothing plays).
fn inline_block_media(mut html: String, library: &Library, print: bool) -> String {
    if print {
        return html;
    }
    for (marker, audio) in [("data-file=\"", false), ("data-audio=\"", true)] {
        let ids: Vec<String> = html
            .split(marker)
            .skip(1)
            .filter_map(|part| part.split('"').next().map(str::to_string))
            .collect();
        for id in ids {
            // The size on disk, as for recording items: a recording's stored size is
            // only set when it stops.
            let path = library
                .attachment(&id)
                .ok()
                .flatten()
                .map(|meta| library.attachment_path(&meta.stored_path));
            let size = path
                .and_then(|path| std::fs::metadata(path).ok())
                .map_or(u64::MAX, |meta| meta.len());
            let Some(uri) = (size <= EMBED_LIMIT)
                .then(|| attachment_data_uri(library, &id))
                .flatten()
            else {
                continue;
            };
            let needle = format!("{marker}{id}\"");
            let replacement = if audio {
                // The paragraph holds the player, ahead of its label.
                format!("{needle}><audio controls preload=\"none\" src=\"{uri}\"></audio> ")
            } else {
                format!("{needle} href=\"{uri}\"")
            };
            let needle = if audio { format!("{needle}>") } else { needle };
            html = html.replacen(&needle, &replacement, 1);
        }
    }
    html
}

fn attachment_data_uri(library: &Library, id: &str) -> Option<String> {
    let meta = library.attachment(id).ok().flatten()?;
    let bytes = std::fs::read(library.attachment_path(&meta.stored_path)).ok()?;
    let mime = if meta.mime.is_empty() {
        "image/png"
    } else {
        &meta.mime
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// A page picture cropped as shown, as a PNG data URI (exports and prints carry pictures
/// inside the page). `None` if it cannot be read, and the whole picture goes instead.
fn cropped_data_uri(library: &Library, id: &str, crop: [f32; 4]) -> Option<String> {
    let meta = library.attachment(id).ok().flatten()?;
    let bytes = std::fs::read(library.attachment_path(&meta.stored_path)).ok()?;
    // Upright, as the page shows it: the crop is a share of the upright picture.
    let image = crate::images::decode(&bytes, u32::MAX)?;
    let (width, height) = (image.width(), image.height());
    let [l, t, r, b] = crop;
    let x = ((l * width as f32).round() as u32).min(width.saturating_sub(1));
    let y = ((t * height as f32).round() as u32).min(height.saturating_sub(1));
    let crop_width = (((r - l) * width as f32).round() as u32).clamp(1, width - x);
    let crop_height = (((b - t) * height as f32).round() as u32).clamp(1, height - y);
    let mut png = Vec::new();
    image
        .crop_imm(x, y, crop_width, crop_height)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

fn hex(color: &Rgb) -> String {
    format!("{:02x}{:02x}{:02x}", color.0, color.1, color.2)
}

fn svg_shape(
    shape: ShapeKind,
    width: f32,
    height: f32,
    flip_y: bool,
    thickness: f32,
    stroke: &str,
    fill: &str,
) -> String {
    let w = width.max(1.0);
    let h = height.max(1.0);
    // Keep the whole outline inside the box, however thick.
    let i = (thickness / 2.0).max(1.0);
    let common = format!("fill=\"{fill}\" stroke=\"#{stroke}\" stroke-width=\"{thickness:.1}\"");
    let polygon = |corners: &[(f32, f32)]| {
        let points: Vec<String> = corners
            .iter()
            .map(|(x, y)| format!("{x:.1},{y:.1}"))
            .collect();
        format!("<polygon points=\"{}\" {common}/>", points.join(" "))
    };
    let tag = match shape {
        ShapeKind::Rect => format!(
            "<rect x=\"{i:.1}\" y=\"{i:.1}\" width=\"{:.1}\" height=\"{:.1}\" {common}/>",
            w - 2.0 * i,
            h - 2.0 * i
        ),
        ShapeKind::RoundRect => format!(
            "<rect x=\"{i:.1}\" y=\"{i:.1}\" rx=\"9\" width=\"{:.1}\" height=\"{:.1}\" {common}/>",
            w - 2.0 * i,
            h - 2.0 * i
        ),
        ShapeKind::Ellipse => format!(
            "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" {common}/>",
            w / 2.0,
            h / 2.0,
            w / 2.0 - i,
            h / 2.0 - i
        ),
        ShapeKind::Diamond => polygon(&[
            (w / 2.0, i),
            (w - i, h / 2.0),
            (w / 2.0, h - i),
            (i, h / 2.0),
        ]),
        // A flipped triangle points down.
        ShapeKind::Triangle if flip_y => polygon(&[(w / 2.0, h - i), (w - i, i), (i, i)]),
        ShapeKind::Triangle => polygon(&[(w / 2.0, i), (w - i, h - i), (i, h - i)]),
        // Lines and arrows are drawn by the caller, which pads their box.
        ShapeKind::Line | ShapeKind::Arrow => return String::new(),
    };
    format!("<svg width=\"100%\" height=\"100%\" viewBox=\"0 0 {w:.1} {h:.1}\">{tag}</svg>")
}

fn print_css() -> String {
    // The desktop notebook look: a white page, Calibri-like notes at 11 pt on 18 px lines, the
    // title in a light face with a hairline under it, grey date, blue headings.
    String::from(
        r#"<style>
@page { size: A4 landscape; margin: 10mm; }
* { box-sizing: border-box; }
body { margin: 0; color: #242424; background: #f3f3f3; font-family: Carlito, Calibri, "Segoe UI", Arial, sans-serif; }
main { max-width: 1400px; margin: 0 auto; padding: 24px 30px 40px; background: #fff; }
.page-header { position: absolute; }
.page-header h1 { margin: 0; font-family: "Calibri Light", "Segoe UI Light", Carlito, Calibri, sans-serif; font-weight: 300; font-size: 26.67px; line-height: 36px; color: #424242; white-space: nowrap; }
.page-header hr { margin: 10px 0 7px; border: 0; border-top: 1px solid #e6e6e6; }
.page-header p { margin: 0; font: 13px "Segoe UI", Selawik, Carlito, Calibri, sans-serif; color: #9e9e9e; }
.connector { position: absolute; overflow: visible; }
.sheet { position: relative; overflow: hidden; transform-origin: top left; }
.sheet-inner { position: relative; transform-origin: top left; }
.note { position: absolute; padding: 5px 8px; font-size: 14.67px; line-height: 18px; overflow: visible; }
.note p { margin: 0 0 4px; }
.note h1, .note h2, .note h3, .note h4, .note h5, .note h6 { font-weight: normal; line-height: 1.25; margin: 0 0 4px; }
.note h1 { font-family: "Calibri Light", Carlito, Calibri, sans-serif; font-size: 26.67px; color: #424242; }
.note h2 { font-size: 21.33px; color: #1e4e79; }
.note h3 { font-size: 18.67px; color: #2e75b5; }
.note h4 { font-size: 16px; color: #1f3763; }
.note h5 { font-size: 16px; color: #2f5496; }
.note h6 { font-size: 14.67px; color: #2e75b5; }
.note ul, .note ol { padding-left: 1.6em; margin: 0 0 4px; }
.note li:has(> .tags > input[type=checkbox]) { list-style: none; margin-left: -1.6em; }
.note .tags { display: inline-block; text-align: right; white-space: nowrap; padding-right: 4px; }
.note .tags input[type=checkbox] { margin: 0; vertical-align: -2px; }
.note blockquote { margin: 0 0 4px; font-style: italic; color: #616161; }
.note small { color: #616161; }
.note hr { border: 0; border-top: 1px solid #e6e6e6; }
.note img { max-width: 100%; height: auto; display: block; }
.note code, pre { font-family: Consolas, "Courier New", monospace; font-size: 13px; }
.note table.folio-table { border-collapse: collapse; margin: 3px 0; }
.note table.folio-table td, .note table.folio-table th { border: 1px solid #c8c8c8; padding: 3px 6px; vertical-align: top; text-align: left; font-weight: normal; }
.note table.folio-table th { background: #f5f5f5; }
.note table.folio-table.no-borders td, .note table.folio-table.no-borders th { border-color: transparent; }
a { color: #0563c1; }
.page-link { color: #0563c1; }
mark { color: inherit; }
mark.on-dark { color: #fff; }
.picture { position: absolute; display: flex; align-items: center; justify-content: center; }
.picture img { max-width: 100%; max-height: 100%; object-fit: contain; }
.picture.printout { break-inside: avoid; }
.shape { position: absolute; }
.shape svg { position: absolute; inset: 0; }
.shape-label { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; text-align: center; white-space: pre-wrap; font: 14px Carlito, Calibri, sans-serif; padding: 8px; }
.timeline { position: absolute; border: 1px solid #e6e6e6; background: #fff; overflow: hidden; }
.timeline-line { position: absolute; left: 4%; right: 4%; top: 52%; height: 2px; margin-top: -1px; background: var(--accent); }
.milestone { position: absolute; top: 52%; height: 0; transform: translateX(-50%); text-align: center; font-family: "Segoe UI", Selawik, Carlito, Calibri, sans-serif; }
.milestone:before { content: ""; position: absolute; left: 50%; top: -5px; margin-left: -5px; width: 10px; height: 10px; border-radius: 50%; background: var(--accent); }
.milestone-head { position: absolute; left: 3px; right: 3px; bottom: 11px; }
.milestone-title, .milestone-date { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.milestone-title { font-size: 12px; font-weight: 600; }
.milestone-date { font-size: 10px; color: var(--accent); margin-top: 2px; }
.milestone-note { position: absolute; left: 3px; right: 3px; top: 14px; font-size: 10px; color: #616161; }
.equation { position: absolute; display: flex; align-items: center; justify-content: center; color: #242424; }
.equation math { font-family: "Cambria Math", "STIX Two Math", "Latin Modern Math", serif; }
.attachment { position: absolute; display: flex; align-items: center; gap: 10px; padding: 0 14px; border: 1px solid #e6e6e6; border-radius: 4px; background: #fff; color: #242424; text-decoration: none; font: 13px "Segoe UI", Selawik, Carlito, Calibri, sans-serif; overflow: hidden; }
.attachment .file-icon { font-size: 18px; }
.attachment .file-name { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.attachment .file-size { color: #616161; font-size: 11px; }
.audio { position: absolute; display: flex; align-items: center; padding: 0 12px; border: 1px solid #e6e6e6; border-radius: 4px; background: #fff; font: 13px "Segoe UI", Selawik, Carlito, Calibri, sans-serif; color: #242424; }
.audio audio { width: 100%; }
.ink { position: absolute; left: 0; top: 0; pointer-events: none; }
@media print { body { background: white; } main { padding: 0; } }
</style>"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_page() -> (Library, Page) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open_at(dir.path().to_path_buf()).unwrap();
        let page = lib.get_page("page-school-week").unwrap().unwrap();
        (lib, page)
    }

    #[test]
    fn canvas_print_contains_content_and_is_styled() {
        let (lib, page) = sample_page();
        let html = canvas_print_html(&page, &lib);
        assert!(html.contains("What matters this week"));
        assert!(html.contains("<style>"));
        assert!(!html.contains("</style>body"));
        // Items keep their colours: the term's blue frise and the red pen star.
        assert!(html.contains("--accent:#0166ac;"), "{html}");
        assert!(html.contains("stroke=\"#d13438\""), "{html}");
        // The reminders are an ordinary note container (the old sticky note's words), with
        // no sticky paper of their own.
        assert!(html.contains("Bring the blue folder on Thursday."));
        assert!(!html.contains("note sticky"));
        assert!(html.contains("September"));
    }

    #[test]
    fn exported_rule_lines_fall_where_the_page_draws_them() {
        use crate::canvas::FIRST_RULE;
        use crate::model::{RuleLines, RuleSize};
        let (lib, mut page) = sample_page();
        page.canvas = crate::model::CanvasDoc::default();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: crate::model::Document::default(),
            },
            crate::canvas::PAGE_LEFT,
            crate::canvas::PAGE_CONTENT_TOP,
            400.0,
            40.0,
        ));
        page.rule = RuleLines::Lines(RuleSize::College);
        let html = canvas_print_html(&page, &lib);
        // The sheet starts 50 left of the note and 50 above the title; the lines are drawn
        // in page units inside `.sheet-inner`, one through `FIRST_RULE` and every 27 from it.
        let (min_x, min_y) = (crate::canvas::PAGE_LEFT - 50.0, TITLE_TOP - 50.0);
        let down = (FIRST_RULE - min_y + 0.5).rem_euclid(27.0);
        let inner = html
            .split("class=\"sheet-inner\" style=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the inner sheet");
        assert!(
            inner.contains(&format!("background-position:0 0,0 {down:.2}px;")),
            "{inner}"
        );
        assert!(
            inner.contains("transparent 26px,#a9c8e8 26px,#a9c8e8 27px"),
            "{inner}"
        );
        // The red margin line sits 28 in from the page's edge, 30 into a sheet starting at -2.
        let margin = crate::canvas::RULE_MARGIN - 0.5 - min_x;
        assert!(
            inner.contains(&format!(
                "transparent {margin:.2}px,#e8a0a0 {margin:.2}px,#e8a0a0 {:.2}px",
                margin + 1.0
            )),
            "{inner}"
        );
        // Rule Line Color: the page's own colour for its lines.
        page.rule_color = Some(Rgb(0x10, 0x7c, 0x10));
        let html = canvas_print_html(&page, &lib);
        assert!(
            html.contains("transparent 26px,#107c10 26px,#107c10 27px"),
            "{html}"
        );
    }

    /// An old page table loads as a note holding a table, and exports as a table in that
    /// note, its header row in `th` cells.
    #[test]
    fn tables_in_notes_export_with_their_cells() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open_at(dir.path().to_path_buf()).unwrap();
        let page = lib.get_page("page-school-diagram").unwrap().unwrap();
        let html = canvas_print_html(&page, &lib);
        assert!(!html.contains("table-wrap"), "{html}");
        let table = html
            .split("<div class=\"note\"")
            .find_map(|note| note.split("<table class=\"folio-table\">").nth(1))
            .and_then(|rest| rest.split("</table>").next())
            .expect("a table in a note");
        assert!(table.contains("<th>Part</th><th>Job</th>"), "{table}");
        assert!(
            table.contains("<td>Chloroplast</td><td>Makes food</td>"),
            "{table}"
        );
        assert!(html.contains(".note table.folio-table td, .note table.folio-table th {"));
    }

    #[test]
    fn frise_milestones_export_with_their_dot_on_the_line() {
        let (lib, page) = sample_page();
        let html = canvas_print_html(&page, &lib);
        // Title and date sit together above the line, the note below it, each a third of
        // the frise wide (three milestones).
        assert!(
            html.contains(concat!(
                r#"<div class="milestone" style="left:16.67%;width:33.33%;">"#,
                r#"<span class="milestone-head"><span class="milestone-title">September</span>"#,
                r#"<span class="milestone-date">Week 1</span></span>"#,
                r#"<span class="milestone-note">Back to school</span></div>"#
            )),
            "{html}"
        );
        // The milestone is anchored on the line, its dot centred there.
        for rule in [
            ".timeline-line { position: absolute; left: 4%; right: 4%; top: 52%;",
            ".milestone { position: absolute; top: 52%; height: 0;",
            ".milestone:before { content: \"\"; position: absolute; left: 50%; top: -5px;",
            ".milestone-head { position: absolute; left: 3px; right: 3px; bottom: 11px; }",
            ".milestone-note { position: absolute; left: 3px; right: 3px; top: 14px;",
        ] {
            assert!(html.contains(rule), "{rule}");
        }
    }

    #[test]
    fn shapes_export_pointing_their_way_at_their_width() {
        let (lib, mut page) = sample_page();
        page.canvas = crate::model::CanvasDoc::default();
        let shape = |shape, flip_y, thickness, y| {
            CanvasItem::new(
                CanvasKind::Shape {
                    shape,
                    stroke: Rgb::INKS[0],
                    fill: None,
                    label: String::new(),
                    flip_x: false,
                    flip_y,
                    thickness,
                },
                0.0,
                y,
                100.0,
                80.0,
            )
        };
        page.canvas
            .add(shape(ShapeKind::Triangle, false, None, 0.0));
        page.canvas
            .add(shape(ShapeKind::Triangle, true, None, 200.0));
        page.canvas
            .add(shape(ShapeKind::Ellipse, false, Some(6.0), 400.0));
        let html = canvas_export_html(&page, &lib);
        assert!(
            html.contains(r#"<polygon points="50.0,1.0 99.0,79.0 1.0,79.0" "#),
            "pointing up: {html}"
        );
        assert!(
            html.contains(r#"<polygon points="50.0,79.0 99.0,1.0 1.0,1.0" "#),
            "pointing down: {html}"
        );
        assert!(
            html.contains(r#"rx="47.0" ry="37.0" fill="none""#)
                && html.contains(r#"stroke-width="6.0""#),
            "a thick outline stays inside its box: {html}"
        );
    }

    #[test]
    fn wide_pages_export_scaled_once() {
        let (lib, mut page) = sample_page();
        page.canvas = crate::model::CanvasDoc::default();
        let note = |x, width| {
            CanvasItem::new(
                CanvasKind::Text {
                    doc: crate::model::Document::default(),
                },
                x,
                50.0,
                width,
                100.0,
            )
        };
        // With the 50 px margin the page spans -2..2158 (the title starts at PAGE_LEFT), so
        // it is shown at half size; it starts 50 above the title.
        page.canvas.add(note(PAGE_LEFT, 100.0));
        page.canvas.add(note(1078.0, 1030.0));
        page.canvas.ink.push(crate::model::InkStroke {
            points: vec![[100.0, 100.0], [2000.0, 150.0]],
            color: Rgb::INKS[0],
            width: 3.0,
            highlighter: false,
        });
        let html = canvas_export_html(&page, &lib);
        assert!(html.contains("transform:scale(0.5000)"), "{html}");
        assert!(
            html.contains("left:1080.00px;top:60.00px;width:1030.00px;height:100.00px;"),
            "items are placed in page units: {html}"
        );
        assert!(
            html.contains(r#"<svg class="ink" width="2160.00" height="260.00""#)
                && html.contains(r#"points="102.0,110.0 2002.0,160.0""#),
            "ink is drawn in page units: {html}"
        );
    }

    /// The title, its rule and the date are on the sheet, where the canvas draws them (at
    /// PAGE_LEFT and TITLE_TOP), not in a header above it; a hidden title is not exported.
    #[test]
    fn the_title_and_date_sit_on_the_page() {
        let (lib, mut page) = sample_page();
        let html = canvas_print_html(&page, &lib);
        let inner = html
            .split("class=\"sheet-inner\"")
            .nth(1)
            .expect("the sheet");
        assert!(inner.contains("class=\"page-header\""), "{inner}");
        assert!(!html.contains("<header>"));
        // The sheet starts 50 above the title (nothing on this page is higher), so the
        // header sits 50 down.
        let header = inner
            .split("class=\"page-header\" style=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the header's place");
        assert!(header.contains("top:50.00px;"), "{header}");
        assert!(
            inner.contains(&format!(
                "<h1>{}</h1><hr><p>",
                util::html_escape(&display_title(&page.title))
            )),
            "{inner}"
        );
        page.title_hidden = true;
        let html = canvas_print_html(&page, &lib);
        assert!(!html.contains("class=\"page-header\""), "{html}");
    }

    /// A long title keeps to one line, as the canvas draws it, on a sheet wide enough for
    /// it: the sheet neither cuts it off nor wraps it down over the notes under the date.
    #[test]
    fn a_long_title_keeps_its_line_on_the_sheet() {
        let (lib, mut page) = sample_page();
        page.title = "Meeting notes with the design team about the spring launch".into();
        page.canvas = crate::model::CanvasDoc::default();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: crate::model::Document::default(),
            },
            PAGE_LEFT - crate::canvas::NOTE_PAD_X,
            PAGE_CONTENT_TOP,
            200.0,
            40.0,
        ));
        let html = canvas_export_html(&page, &lib);
        let width: f32 = html
            .split("class=\"sheet-inner\" style=\"width:")
            .nth(1)
            .and_then(|rest| rest.split("px").next())
            .and_then(|width| width.parse().ok())
            .expect("the sheet's width");
        // The sheet starts 50 left of the note, so the title 60 in; 13 px a character is
        // more than the title takes.
        let chars = page.title.chars().count() as f32;
        assert!(width >= 60.0 + chars * 13.0, "{width}");
        assert!(
            print_css().contains("color: #424242; white-space: nowrap; }"),
            "the title never wraps"
        );
    }

    fn page_with(lib: &mut Library, title: &str) -> Page {
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = lib.workspace().unwrap().notebooks[0].sections[0].id.clone();
        lib.create_canvas_page(&section, title).unwrap()
    }

    #[test]
    fn exports_carry_the_language_and_french_dates() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::Fr);
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Leçon");
        page.updated_at = 1_792_065_600_000; // 15 October 2026, noon UTC: mid-month in any zone
        page.created_at = 1_792_065_600_000;
        let html = canvas_export_html(&page, &lib);
        assert!(html.contains("<html lang=\"fr\">"), "{html}");
        assert!(html.contains("octobre 2026"), "{html}");
        let print = canvas_print_html(&page, &lib);
        assert!(print.contains("<html lang=\"fr\">"), "{print}");
    }

    #[test]
    fn attached_files_are_cards_and_small_ones_travel_with_the_export() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Files");
        let attachment = lib
            .save_attachment_bytes(&page.id, "txt", "text/plain", b"hello", "notes.txt")
            .unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::File {
                attachment: attachment.id,
                name: "notes.txt".into(),
                size: 5,
            },
            0.0,
            0.0,
            260.0,
            56.0,
        ));
        let export = canvas_export_html(&page, &lib);
        assert!(export.contains("download=\"notes.txt\""));
        assert!(export.contains("data:text/plain;base64,"));
        let print = canvas_print_html(&page, &lib);
        assert!(print.contains("notes.txt"));
        assert!(!print.contains("download="));
    }

    #[test]
    fn pictures_inside_a_note_travel_with_the_export_and_the_print() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Sketch");
        let attachment = lib
            .save_attachment_bytes(&page.id, "png", "image/png", b"png", "sketch.png")
            .unwrap();
        let doc = crate::model::Document {
            blocks: vec![
                crate::model::Block::paragraph("Before the sketch"),
                crate::model::Block::new(crate::model::BlockKind::Image {
                    attachment: attachment.id.clone(),
                    width: 1.0,
                    meta: crate::model::PictureMeta::default(),
                }),
            ],
        };
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text { doc },
            48.0,
            136.0,
            400.0,
            200.0,
        ));
        for html in [
            canvas_export_html(&page, &lib),
            canvas_print_html(&page, &lib),
        ] {
            assert!(html.contains("<img src=\"data:image/png;base64,"), "{html}");
            assert!(
                !html.contains(&format!("src=\"{}\"", attachment.id)),
                "{html}"
            );
        }
    }

    /// A page with one note holding `block` under a line of text.
    fn note_holding(page: &mut Page, block: crate::model::Block) {
        let doc = crate::model::Document {
            blocks: vec![crate::model::Block::paragraph("In the note"), block],
        };
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text { doc },
            48.0,
            136.0,
            400.0,
            120.0,
        ));
    }

    #[test]
    fn line_spacing_at_least_is_a_minimum_in_exports() {
        use crate::model::{Block, Document, ParaSpacing};
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Spaced");
        let at_least = |line_at_least: f32, block: Block| Block {
            spacing: ParaSpacing {
                line_at_least,
                ..ParaSpacing::default()
            },
            ..block
        };
        // Body text with a 40 pt run and an 8 pt one.
        let mut runs = Block::paragraph("Big and tiny");
        runs.text
            .apply_style_chars(0..3, |style| style.size = Some(40));
        runs.text
            .apply_style_chars(8..12, |style| style.size = Some(8));
        let doc = Document {
            blocks: vec![
                at_least(12.0, Block::heading(2, "Big")),
                at_least(6.0, Block::paragraph("Small")),
                at_least(30.0, Block::paragraph("Roomy")),
                runs,
            ],
        };
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text { doc },
            48.0,
            136.0,
            400.0,
            120.0,
        ));
        for html in [
            canvas_export_html(&page, &lib),
            canvas_print_html(&page, &lib),
        ] {
            // Each line is the taller of the paragraph's own height and the least: the
            // heading keeps its 1.25 lines over 12 pt, body text its 18 px over 6 pt, and
            // 30 pt lifts body lines to 30 pt.
            assert!(
                html.contains(r#"<h2 style="line-height:max(1.25em,12pt);">Big</h2>"#),
                "{html}"
            );
            assert!(
                html.contains(r#"<p style="line-height:max(18px,6pt);">Small</p>"#),
                "{html}"
            );
            assert!(
                html.contains(r#"<p style="line-height:max(18px,30pt);">Roomy</p>"#),
                "{html}"
            );
            // A run in its own size takes lines 1.22 times that size, as in the editor: the
            // 40 pt one grows its line, and the paragraph's 18 px line still holds the 8 pt
            // one.
            assert!(
                html.contains(
                    r#"<p><span style="font-size:40pt;line-height:1.22;">Big</span> and <span style="font-size:8pt;line-height:1.22;">tiny</span></p>"#
                ),
                "{html}"
            );
            // Those natural heights are the ones the stylesheet gives notes and headings.
            assert!(
                html.contains("font-size: 14.67px; line-height: 18px;"),
                "{html}"
            );
            assert!(
                html.contains("font-weight: normal; line-height: 1.25;"),
                "{html}"
            );
        }
    }

    #[test]
    fn file_blocks_in_notes_export_as_links() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Lab");
        let attachment = lib
            .save_attachment_bytes(&page.id, "txt", "text/plain", b"0123456789", "sheet.txt")
            .unwrap();
        note_holding(
            &mut page,
            crate::model::Block::new(crate::model::BlockKind::File {
                attachment: attachment.id.clone(),
                name: "sheet.txt".into(),
                size: 10,
            }),
        );
        let export = canvas_export_html(&page, &lib);
        assert!(export.contains("href=\"data:"), "{export}");
        assert!(export.contains("download=\"sheet.txt\""), "{export}");
        assert!(export.contains("sheet.txt"), "{export}");
        // Print shows the card only.
        let print = canvas_print_html(&page, &lib);
        assert!(print.contains("sheet.txt"), "{print}");
        assert!(!print.contains("href=\"data:"), "{print}");
    }

    #[test]
    fn recording_blocks_in_notes_play_in_exports() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Lecture");
        let bytes = crate::audio::wav_bytes(crate::audio::TARGET_RATE, &[0; 16_000]);
        let attachment = lib
            .save_attachment_bytes(&page.id, "wav", "audio/wav", &bytes, "clip.wav")
            .unwrap();
        note_holding(
            &mut page,
            crate::model::Block::new(crate::model::BlockKind::Audio {
                attachment: attachment.id,
                duration_ms: 1000,
            }),
        );
        let export = canvas_export_html(&page, &lib);
        assert!(
            export.contains("<audio controls preload=\"none\" src=\"data:audio/wav;base64,"),
            "{export}"
        );
        let print = canvas_print_html(&page, &lib);
        assert!(print.contains("Audio recording · 0:01"), "{print}");
        assert!(!print.contains("<audio"), "{print}");
    }

    #[test]
    fn recordings_play_in_exports_and_print_as_a_label() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Lecture");
        let bytes = crate::audio::wav_bytes(crate::audio::TARGET_RATE, &[0; 16_000]);
        let attachment = lib
            .save_attachment_bytes(&page.id, "wav", "audio/wav", &bytes, "clip.wav")
            .unwrap();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Audio {
                attachment: attachment.id,
                duration_ms: 0,
            },
            0.0,
            0.0,
            300.0,
            64.0,
        ));
        let export = canvas_export_html(&page, &lib);
        assert!(export.contains("<audio controls"));
        assert!(export.contains("data:audio/wav;base64,"));
        let print = canvas_print_html(&page, &lib);
        assert!(print.contains("Audio recording · 0:01"));
        assert!(!print.contains("<audio"));
    }

    #[test]
    fn equations_export_as_mathml() {
        let (lib, mut page) = sample_page();
        page.canvas.add(CanvasItem::new(
            CanvasKind::Equation {
                source: "x^2+1/2".into(),
                size: 22.0,
            },
            40.0,
            40.0,
            120.0,
            60.0,
        ));
        let html = canvas_export_html(&page, &lib);
        assert!(html.contains("<math display=\"block\">"));
        assert!(html.contains("<mfrac><mn>1</mn><mn>2</mn></mfrac>"));
    }

    /// A page picture carries its alt text, escaped, and a turned one keeps its own
    /// proportions inside its turned box.
    #[test]
    fn page_pictures_export_their_alt_text_and_turn() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Flags");
        let mut kind = CanvasKind::image("missing");
        if let CanvasKind::Image { meta, .. } = &mut kind {
            meta.alt = "A \"red\" flag".into();
            meta.turns = 1;
        }
        page.canvas
            .add(CanvasItem::new(kind, 48.0, 136.0, 100.0, 200.0));
        let html = canvas_export_html(&page, &lib);
        assert!(html.contains("alt=\"A &quot;red&quot; flag\""), "{html}");
        assert!(
            html.contains("width:200px;height:100px;max-width:none;max-height:none;margin-left:-100px;margin-top:-50px;transform:rotate(90deg);"),
            "{html}"
        );
    }

    /// A cropped page picture is exported and printed as its kept part only.
    #[test]
    fn cropped_pictures_export_cropped() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Flags");
        // 40×20: the left half red, the right half blue.
        let picture = image::RgbaImage::from_fn(40, 20, |x, _| {
            if x < 20 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let attachment = lib
            .save_attachment_bytes(&page.id, "png", "image/png", &png, "flag.png")
            .unwrap();
        let mut kind = CanvasKind::image(attachment.id.clone());
        if let CanvasKind::Image { meta, .. } = &mut kind {
            meta.crop = Some([0.5, 0.0, 1.0, 1.0]);
        }
        page.canvas
            .add(CanvasItem::new(kind, 48.0, 136.0, 100.0, 100.0));
        for html in [
            canvas_export_html(&page, &lib),
            canvas_print_html(&page, &lib),
        ] {
            let start = html.find("data:image/png;base64,").expect("the picture")
                + "data:image/png;base64,".len();
            let end = start + html[start..].find('"').unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&html[start..end])
                .unwrap();
            let shown = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert_eq!(shown.dimensions(), (20, 20));
            assert!(
                shown
                    .pixels()
                    .all(|pixel| *pixel == image::Rgba([0, 0, 255, 255])),
                "only the blue half"
            );
        }
    }

    /// A picture set as the background after the notes were typed lies under them in the
    /// export and the print, as on the page.
    #[test]
    fn a_later_background_picture_exports_under_the_notes() {
        let mut lib = Library::open_in_memory().unwrap();
        let mut page = page_with(&mut lib, "Worksheet");
        page.canvas.add(CanvasItem::new(
            CanvasKind::Text {
                doc: crate::model::Document {
                    blocks: vec![crate::model::Block::paragraph("My answer")],
                },
            },
            60.0,
            150.0,
            300.0,
            40.0,
        ));
        let mut kind = CanvasKind::image("missing");
        if let CanvasKind::Image { meta, .. } = &mut kind {
            meta.background = true;
        }
        page.canvas
            .add(CanvasItem::new(kind, 48.0, 136.0, 400.0, 300.0));
        for html in [
            canvas_export_html(&page, &lib),
            canvas_print_html(&page, &lib),
        ] {
            let picture = html.find("class=\"picture\"").expect("the picture");
            let note = html.find("My answer").expect("the note");
            assert!(picture < note, "the background comes first: {html}");
        }
    }

    /// File › Export of a section: a folder named after it, one numbered web page per page
    /// (its pictures inside), and an index linking them in order. Exporting again makes a
    /// second folder and leaves the first as it was.
    #[test]
    fn a_section_exports_as_a_folder_with_an_index() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let mut lib = Library::open_in_memory().unwrap();
        lib.create_notebook("Test", "#745cf5").unwrap();
        let section = lib.workspace().unwrap().notebooks[0].sections[0].id.clone();
        let first = lib.create_canvas_page(&section, "Week 1: Plans").unwrap();
        let mut second = lib.create_canvas_page(&section, "Cells & tissues").unwrap();
        let picture = lib
            .save_attachment_bytes(&second.id, "png", "image/png", b"png", "cell.png")
            .unwrap();
        second.canvas.add(CanvasItem::new(
            CanvasKind::image(picture.id.clone()),
            48.0,
            136.0,
            100.0,
            100.0,
        ));
        let pages = [first, second];
        let out = tempfile::tempdir().unwrap();
        let folder = export_pages_to_folder(out.path(), "Biology", &pages, &lib).unwrap();
        assert_eq!(folder, out.path().join("biology"));
        assert!(folder.is_dir());
        let one = std::fs::read_to_string(folder.join("01-week-1-plans.html")).unwrap();
        assert!(one.contains("<title>Week 1: Plans</title>"), "{one}");
        let two = std::fs::read_to_string(folder.join("02-cells-tissues.html")).unwrap();
        assert!(two.contains("<title>Cells &amp; tissues</title>"), "{two}");
        assert!(two.contains("<img src=\"data:image/png;base64,"), "{two}");
        let index = std::fs::read_to_string(folder.join("index.html")).unwrap();
        assert!(index.contains("<html lang=\"en\">"), "{index}");
        assert!(index.contains("<h1>Biology</h1>"), "{index}");
        let at = |link: &str| {
            index
                .find(link)
                .unwrap_or_else(|| panic!("{link}: {index}"))
        };
        assert!(
            at("<li><a href=\"01-week-1-plans.html\">Week 1: Plans</a></li>")
                < at("<li><a href=\"02-cells-tissues.html\">Cells &amp; tissues</a></li>")
        );
        // A second export never writes over the first.
        std::fs::write(folder.join("index.html"), "kept").unwrap();
        let again = export_pages_to_folder(out.path(), "Biology", &pages, &lib).unwrap();
        assert_eq!(again, out.path().join("biology (2)"));
        assert!(again.join("index.html").is_file());
        assert!(again.join("02-cells-tissues.html").is_file());
        assert_eq!(
            std::fs::read_to_string(folder.join("index.html")).unwrap(),
            "kept"
        );
        // A very long title (accented letters take two bytes each) still makes a file
        // name the file system takes, cut at a word break.
        let long = lib
            .create_canvas_page(&section, &"Épisode très très long ".repeat(20))
            .unwrap();
        let folder = export_pages_to_folder(out.path(), &"Énorme ".repeat(80), &[long], &lib)
            .expect("a long title exports");
        let name = folder.file_name().unwrap().to_str().unwrap().to_string();
        assert!(name.chars().count() <= 60, "{name}");
        let files: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        let page = files
            .iter()
            .find(|file| file.starts_with("01-"))
            .unwrap_or_else(|| panic!("{files:?}"));
        assert!(page.contains("pisode-très-très-long-"), "{page}");
        assert!(!page.ends_with("-.html"), "{page}");
        assert!(page.chars().count() <= "01-".len() + 60 + ".html".len());
    }

    #[test]
    fn printouts_export_page_by_page_at_full_width() {
        let (lib, mut page) = sample_page();
        page.canvas = crate::model::CanvasDoc::default();
        for number in 1..=3u32 {
            let mut kind = CanvasKind::image("missing");
            if let CanvasKind::Image { printout, .. } = &mut kind {
                *printout = Some(Box::new(crate::model::PrintoutPage {
                    source: "pdf".into(),
                    page: number,
                    text: String::new(),
                    size: None,
                }));
            }
            page.canvas.add(CanvasItem::new(
                kind,
                48.0,
                100.0 + (number - 1) as f32 * 1068.0,
                816.0,
                1056.0,
            ));
        }
        let html = canvas_print_html(&page, &lib);
        assert!(
            html.contains("transform:scale(1.0000)"),
            "not shrunk to one sheet: {html}"
        );
        assert_eq!(html.matches("class=\"picture printout\"").count(), 3);
        assert!(html.contains("alt=\"Printout page 2\""), "{html}");

        // A printout page's own alt text wins over "Printout page 3".
        if let CanvasKind::Image { meta, .. } = &mut page.canvas.items[2].kind {
            meta.alt = "Answer key".into();
        }
        let html = canvas_print_html(&page, &lib);
        assert!(html.contains("alt=\"Answer key\""), "{html}");
        assert!(!html.contains("alt=\"Printout page 3\""), "{html}");
        assert_eq!(html.matches("class=\"picture printout\"").count(), 3);
    }

    #[test]
    #[ignore]
    fn write_print_sample() {
        let (lib, page) = sample_page();
        std::fs::write("/tmp/folio-print-real.html", canvas_print_html(&page, &lib)).unwrap();
    }
}
