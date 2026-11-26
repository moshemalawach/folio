//! File printouts (the desktop notebook's Insert › File Printout): each page of a PDF becomes a
//! picture on the page. hayro renders PDFs in pure Rust, so nothing has to be installed.

use std::sync::Arc;

use eframe::egui;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{DecryptionError, LoadPdfError, Pdf};
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

use crate::i18n::t;

/// Printout pages are stored at this resolution: sharp up to about 150 % zoom, and a
/// page of text is 0.2–0.5 MB as lossless WebP.
pub const PRINTOUT_DPI: f32 = 150.0;
/// The longest side of a stored page, in pixels, as for pictures: a poster-sized page
/// can't make Folio allocate gigabytes.
pub const MAX_SIDE_PX: f32 = 4096.0;
/// Page units per PDF point: a printout shows at its printed size, 96 units an inch.
pub const UNITS_PER_POINT: f32 = 96.0 / 72.0;

/// Why a PDF can't become a printout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfError {
    /// It has a password, and none (or a wrong one) was given.
    Password,
    /// It is encrypted in a way hayro can't read.
    Encryption,
    /// Not a PDF, or damaged beyond repair.
    Invalid,
    /// It has no pages.
    Empty,
}

impl PdfError {
    /// What to tell the user about the file called `name`.
    pub fn message(self, name: &str) -> String {
        let name = name.to_string();
        match self {
            PdfError::Password => t!("printout-password-body", name = name),
            PdfError::Encryption => t!("printout-error-encryption", name = name),
            PdfError::Invalid => t!("printout-error-invalid", name = name),
            PdfError::Empty => t!("printout-error-empty", name = name),
        }
        .into_owned()
    }
}

/// Opens a PDF. A PDF without a password expects the empty one.
pub fn open(bytes: Arc<Vec<u8>>, password: Option<&str>) -> Result<Pdf, PdfError> {
    let pdf =
        Pdf::new_with_password(bytes, password.unwrap_or("")).map_err(|error| match error {
            LoadPdfError::Decryption(DecryptionError::PasswordProtected) => PdfError::Password,
            LoadPdfError::Decryption(_) => PdfError::Encryption,
            LoadPdfError::Invalid => PdfError::Invalid,
        })?;
    if pdf.pages().is_empty() {
        return Err(PdfError::Empty);
    }
    Ok(pdf)
}

/// A page's size in PDF points, after its rotation and crop box.
pub fn page_size(pdf: &Pdf, index: usize) -> Option<(f32, f32)> {
    pdf.pages().get(index).map(|page| page.render_dimensions())
}

/// A rendered page: opaque RGBA pixels, row by row.
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Renders page `index` at `dpi` on white paper. `cache` is shared by the pages of one
/// document (it keeps parsed fonts). `None` for a page that doesn't exist, or that
/// hayro fails on: that is caught, so one bad page never stops a printout.
pub fn render_page<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    index: usize,
    dpi: f32,
) -> Option<Rendered> {
    let page = pdf.pages().get(index)?;
    let (width, height) = page.render_dimensions();
    let mut scale = dpi / 72.0;
    let longest = width.max(height) * scale;
    if longest > MAX_SIDE_PX {
        scale *= MAX_SIDE_PX / longest;
    }
    let pixmap = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render(
            page,
            cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                // Paper: every pixel comes out opaque.
                bg_color: WHITE,
            },
        )
    }))
    .ok()?;
    let (width, height) = (pixmap.width() as u32, pixmap.height() as u32);
    (width > 0 && height > 0).then(|| Rendered {
        width,
        height,
        rgba: pixmap.data_as_u8_slice().to_vec(),
    })
}

/// A rendered page as lossless WebP. Its alpha is always opaque, so only RGB is kept.
pub fn encode_webp(page: &Rendered) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let rgb: Vec<u8> = page
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, _]| [r, g, b])
        .collect();
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(
            &rgb,
            page.width,
            page.height,
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(out)
}

/// Forms exported by LibreOffice (and some web forms) ask the viewer to draw their
/// fields (`/NeedAppearances`). hayro draws only what is stored, so typed-in values
/// may not show, and Folio says so.
pub fn needs_form_appearances(pdf: &Pdf) -> bool {
    use hayro::hayro_syntax::object::Dict;
    let xref = pdf.xref();
    xref.get::<Dict<'_>>(xref.root_id())
        .and_then(|catalog| catalog.get::<Dict<'_>>(b"AcroForm"))
        .and_then(|form| form.get::<bool>(b"NeedAppearances"))
        .unwrap_or(false)
}

/// The text of page `index`, line by line from the top: what search indexes and
/// "Copy text from this page" gives. Empty for a scan. A failure inside hayro is
/// caught, as for rendering.
pub fn page_text(pdf: &Pdf, index: usize) -> String {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| collect_text(pdf, index)))
        .ok()
        .flatten()
        .map(|text| unligature(text.trim_end()))
        .unwrap_or_default()
}

/// Glyphs as they are drawn: x, baseline y (down the page), the x where the glyph
/// ends, and its text.
#[derive(Default)]
struct TextSink {
    glyphs: Vec<(f64, f64, f64, String)>,
}

impl<'a> hayro::hayro_interpret::Device<'a> for TextSink {
    fn draw_path(
        &mut self,
        _: &hayro::kurbo::BezPath,
        _: hayro::hayro_interpret::DrawProps<'a>,
        _: &hayro::hayro_interpret::DrawMode,
    ) {
    }
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(
        &mut self,
        _: f32,
        _: Option<hayro::hayro_interpret::SoftMask<'a>>,
        _: hayro::hayro_interpret::BlendMode,
    ) {
    }
    fn draw_image(
        &mut self,
        _: hayro::hayro_interpret::Image<'a, '_>,
        _: hayro::hayro_interpret::ImageDrawProps<'a>,
    ) {
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
    fn draw_glyph_run(
        &mut self,
        run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>,
        props: hayro::hayro_interpret::DrawProps<'a>,
        _: &hayro::hayro_interpret::DrawMode,
    ) {
        use hayro::hayro_interpret::font::Glyph;
        use hayro::hayro_interpret::hayro_cmap::BfString;
        use hayro::kurbo::Point;
        for glyph in run.glyphs() {
            let transform = props.transform * glyph.transform();
            let start = transform * Point::ORIGIN;
            // The advance is in glyph space (1/1000 em), like the outline.
            let advance = match &**glyph {
                Glyph::Outline(outline) => outline.advance_width().unwrap_or(500.0),
                _ => 500.0,
            };
            let end = (transform * Point::new(f64::from(advance), 0.0)).x;
            let text = match glyph.as_unicode() {
                Some(BfString::Char(ch)) => ch.to_string(),
                Some(BfString::String(text)) => text,
                None => continue,
            };
            self.glyphs.push((start.x, start.y, end, text));
        }
    }
}

fn collect_text(pdf: &Pdf, index: usize) -> Option<String> {
    use hayro::hayro_interpret::util::TransformExt;
    use hayro::hayro_interpret::{interpret_page, Context, InterpreterCache};
    let page = pdf.pages().get(index)?;
    let (width, height) = page.render_dimensions();
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        hayro::kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut sink = TextSink::default();
    interpret_page(page, &mut context, &mut sink);
    // Lines by baseline (2 pt apart or more), each read left to right. Columns are not
    // told apart: good enough for search and for a worksheet's text.
    let mut glyphs = sink.glyphs;
    glyphs.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
    let mut out = String::new();
    let mut line: Vec<(f64, f64, String)> = Vec::new();
    let mut baseline = f64::NAN;
    for (x, y, end, text) in glyphs {
        if !baseline.is_nan() && (y - baseline).abs() > 2.0 {
            flush_line(&mut line, &mut out);
        }
        baseline = y;
        line.push((x, end, text));
    }
    flush_line(&mut line, &mut out);
    Some(out)
}

/// Appends one line of glyphs, with a space wherever the gap between two glyphs is a
/// word break (wider than about a fifth of an em).
fn flush_line(line: &mut Vec<(f64, f64, String)>, out: &mut String) {
    if line.is_empty() {
        return;
    }
    line.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut last_end = f64::NAN;
    for (x, end, text) in line.drain(..) {
        let em = (end - x).abs().max(1.0) * 2.0;
        if !last_end.is_nan() && x - last_end > 0.2 * em && !out.ends_with(' ') && text != " " {
            out.push(' ');
        }
        out.push_str(&text);
        last_end = end;
    }
    out.push('\n');
}

/// Typeset ligatures come out as one character ("ﬁ"); search wants the letters.
fn unligature(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            _ => out.push(ch),
        }
    }
    out
}

/// One page of a printout, ready to store.
pub struct PageImage {
    /// 0-based.
    pub index: usize,
    /// Its size in PDF points.
    pub size_pt: (f32, f32),
    pub webp: Vec<u8>,
    pub text: String,
}

/// What a printout job reports, in order: `Opened`, then one `Page` or `PageFailed`
/// per page, then `Done`. `Failed` instead when the PDF can't be read at all.
pub enum JobEvent {
    Opened {
        pages: usize,
        form_warning: bool,
    },
    Page(PageImage),
    /// A page hayro couldn't draw (its index); the others carry on. The printout only
    /// counts them, so nothing reads the index yet.
    PageFailed(#[allow(dead_code)] usize),
    Failed(PdfError),
    Done,
}

/// Renders a printout on its own thread, page by page, so a long PDF never freezes
/// the window. Dropping the job stops it after the page in hand.
pub struct PrintoutJob {
    events: std::sync::mpsc::Receiver<JobEvent>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    finished: bool,
}

impl PrintoutJob {
    /// Starts rendering `bytes`; `ctx` is asked to repaint as each page arrives.
    pub fn start(bytes: Arc<Vec<u8>>, password: Option<String>, ctx: egui::Context) -> Self {
        let (sender, events) = std::sync::mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        // If the thread can't start, the closed channel reports the PDF as unreadable.
        let _ = std::thread::Builder::new()
            .name("folio-printout".into())
            .spawn(move || {
                run(&bytes, password.as_deref(), &stop, &mut |event| {
                    let _ = sender.send(event);
                    ctx.request_repaint();
                });
            });
        Self {
            events,
            cancel,
            finished: false,
        }
    }

    /// The events since the last call. A worker that ends without `Done` (it died)
    /// reports the PDF as unreadable.
    pub fn poll(&mut self) -> Vec<JobEvent> {
        use std::sync::mpsc::TryRecvError;
        let mut events = Vec::new();
        while !self.finished {
            match self.events.try_recv() {
                Ok(event) => {
                    self.finished = matches!(event, JobEvent::Done | JobEvent::Failed(_));
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    if !self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        events.push(JobEvent::Failed(PdfError::Invalid));
                    }
                }
            }
        }
        events
    }

    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for PrintoutJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// A job's work, on the calling thread (the job's own, or a test's).
pub(crate) fn run(
    bytes: &Arc<Vec<u8>>,
    password: Option<&str>,
    cancel: &std::sync::atomic::AtomicBool,
    send: &mut dyn FnMut(JobEvent),
) {
    let pdf = match open(Arc::clone(bytes), password) {
        Ok(pdf) => pdf,
        Err(error) => {
            send(JobEvent::Failed(error));
            return;
        }
    };
    let pages = pdf.pages().len();
    send(JobEvent::Opened {
        pages,
        form_warning: needs_form_appearances(&pdf),
    });
    let cache = RenderCache::new();
    for index in 0..pages {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let size_pt = page_size(&pdf, index).unwrap_or((612.0, 792.0));
        match render_page(&pdf, &cache, index, PRINTOUT_DPI).and_then(|page| encode_webp(&page)) {
            Some(webp) => send(JobEvent::Page(PageImage {
                index,
                size_pt,
                webp,
                text: page_text(&pdf, index),
            })),
            None => send(JobEvent::PageFailed(index)),
        }
    }
    send(JobEvent::Done);
}

/// A small valid PDF for tests: one page per string, each showing the string in
/// Helvetica and a blue bar, at `size` points.
#[cfg(test)]
pub(crate) fn test_pdf(pages: &[&str], size: [f32; 2], catalog_extra: &str) -> Vec<u8> {
    let mut objects: Vec<String> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra}>>"),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            (0..pages.len())
                .map(|i| format!("{} 0 R", 4 + i * 2))
                .collect::<Vec<_>>()
                .join(" "),
            pages.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    for (i, text) in pages.iter().enumerate() {
        let content = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET 0 0 1 rg 72 600 200 40 re f");
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            size[0],
            size[1],
            5 + i * 2
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A Letter-sized test PDF, one page per string.
#[cfg(test)]
pub(crate) fn letter_pdf(pages: &[&str]) -> Vec<u8> {
    test_pdf(pages, [612.0, 792.0], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_of(bytes: Vec<u8>) -> Pdf {
        open(Arc::new(bytes), None).expect("a readable PDF")
    }

    #[test]
    fn a_pdf_opens_with_its_pages_and_sizes() {
        let pdf = pdf_of(letter_pdf(&["one", "two", "three"]));
        assert_eq!(pdf.pages().len(), 3);
        assert_eq!(page_size(&pdf, 0), Some((612.0, 792.0)));
        assert_eq!(page_size(&pdf, 3), None);
    }

    #[test]
    fn a_page_renders_at_printout_resolution_on_white_paper() {
        let pdf = pdf_of(letter_pdf(&["Folio page one"]));
        let cache = RenderCache::new();
        let page = render_page(&pdf, &cache, 0, PRINTOUT_DPI).expect("rendered");
        // 792 pt × 150/72 = 1650 rows; hayro truncates, so 1650.0 can come out 1649.
        assert_eq!(page.width, 1275);
        assert!(page.height.abs_diff(1650) <= 1, "{} rows", page.height);
        let pixel = |x: u32, y: u32| {
            let at = ((y * page.width + x) * 4) as usize;
            [
                page.rgba[at],
                page.rgba[at + 1],
                page.rgba[at + 2],
                page.rgba[at + 3],
            ]
        };
        assert_eq!(pixel(5, 5), [255, 255, 255, 255], "white paper");
        // The bar is at x 72..272 pt and y 600..640 pt from the bottom: about
        // (150..566, 317..400) px from the top at 150 DPI.
        assert_eq!(pixel(300, 360), [0, 0, 255, 255], "the blue bar");
        assert!(render_page(&pdf, &cache, 1, PRINTOUT_DPI).is_none());
    }

    #[test]
    fn a_poster_sized_page_is_capped() {
        let pdf = pdf_of(test_pdf(&["big"], [14400.0, 7200.0], ""));
        let page = render_page(&pdf, &RenderCache::new(), 0, PRINTOUT_DPI).expect("rendered");
        assert_eq!(page.width, MAX_SIDE_PX as u32);
        assert_eq!(page.height, MAX_SIDE_PX as u32 / 2);
    }

    #[test]
    fn a_rendered_page_round_trips_through_webp() {
        let pdf = pdf_of(letter_pdf(&["round trip"]));
        let page = render_page(&pdf, &RenderCache::new(), 0, 72.0).expect("rendered");
        let webp = encode_webp(&page).expect("encoded");
        let decoded = crate::images::decode(&webp, u32::MAX)
            .expect("decodes")
            .to_rgba8();
        assert_eq!(
            (decoded.width(), decoded.height()),
            (page.width, page.height)
        );
        assert_eq!(decoded.into_raw(), page.rgba, "lossless");
    }

    #[test]
    fn page_text_reads_each_page() {
        let pdf = pdf_of(letter_pdf(&["Folio page one", "Fractions page two"]));
        assert_eq!(page_text(&pdf, 0), "Folio page one");
        assert_eq!(page_text(&pdf, 1), "Fractions page two");
        assert_eq!(page_text(&pdf, 2), "");
    }

    #[test]
    fn ligatures_become_letters() {
        assert_eq!(unligature("ﬁnal ﬂow, oﬀ"), "final flow, off");
    }

    #[test]
    fn a_locked_pdf_asks_for_its_password() {
        let bytes = Arc::new(include_bytes!("../tests/fixtures/locked-aes128.pdf").to_vec());
        assert_eq!(
            open(Arc::clone(&bytes), None).err(),
            Some(PdfError::Password)
        );
        assert_eq!(
            open(Arc::clone(&bytes), Some("nope")).err(),
            Some(PdfError::Password)
        );
        let pdf = open(bytes, Some("secret")).expect("the right password opens it");
        assert_eq!(pdf.pages().len(), 1);
        assert!(page_text(&pdf, 0).contains("AES-128 encrypted page"));
    }

    #[test]
    fn other_files_are_not_pdfs() {
        for bytes in [Vec::new(), b"hello".to_vec(), b"\x89PNG\r\n\x1a\n".to_vec()] {
            assert_eq!(open(Arc::new(bytes), None).err(), Some(PdfError::Invalid));
        }
    }

    #[test]
    fn forms_that_need_their_fields_drawn_are_flagged() {
        assert!(!needs_form_appearances(&pdf_of(letter_pdf(&["plain"]))));
        let form = test_pdf(
            &["form"],
            [612.0, 792.0],
            "/AcroForm << /Fields [] /NeedAppearances true >> ",
        );
        assert!(needs_form_appearances(&pdf_of(form)));
    }

    fn run_to_end(bytes: Vec<u8>, password: Option<&str>, cancelled: bool) -> Vec<JobEvent> {
        let mut events = Vec::new();
        let cancel = std::sync::atomic::AtomicBool::new(cancelled);
        run(&Arc::new(bytes), password, &cancel, &mut |event| {
            events.push(event)
        });
        events
    }

    #[test]
    fn a_job_reports_every_page_then_done() {
        let events = run_to_end(letter_pdf(&["one", "two", "three"]), None, false);
        assert!(matches!(
            events.first(),
            Some(JobEvent::Opened {
                pages: 3,
                form_warning: false
            })
        ));
        let pages: Vec<&PageImage> = events
            .iter()
            .filter_map(|event| match event {
                JobEvent::Page(page) => Some(page),
                _ => None,
            })
            .collect();
        assert_eq!(
            pages.iter().map(|page| page.index).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(pages[1].size_pt, (612.0, 792.0));
        assert_eq!(pages[2].text, "three");
        assert!(crate::images::decode(&pages[0].webp, u32::MAX).is_some());
        assert!(matches!(events.last(), Some(JobEvent::Done)));
    }

    #[test]
    fn a_cancelled_job_stops_before_the_pages() {
        let events = run_to_end(letter_pdf(&["one", "two"]), None, true);
        assert_eq!(events.len(), 1, "only Opened");
        assert!(matches!(events[0], JobEvent::Opened { pages: 2, .. }));
    }

    #[test]
    fn a_locked_job_fails_until_the_password_comes() {
        let locked = include_bytes!("../tests/fixtures/locked-aes128.pdf").to_vec();
        let events = run_to_end(locked.clone(), None, false);
        assert!(matches!(events[..], [JobEvent::Failed(PdfError::Password)]));
        let events = run_to_end(locked, Some("secret"), false);
        assert!(matches!(events.last(), Some(JobEvent::Done)));
    }

    #[test]
    fn the_worker_thread_reports_back() {
        let mut job = PrintoutJob::start(
            Arc::new(letter_pdf(&["threaded"])),
            None,
            egui::Context::default(),
        );
        let mut pages = 0;
        let started = std::time::Instant::now();
        loop {
            let events = job.poll();
            pages += events
                .iter()
                .filter(|event| matches!(event, JobEvent::Page(_)))
                .count();
            if events.iter().any(|event| matches!(event, JobEvent::Done)) {
                break;
            }
            assert!(started.elapsed().as_secs() < 30, "the job never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(pages, 1);
        assert!(job.poll().is_empty(), "nothing after Done");
    }
}
