use std::borrow::Cow;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Id, Key, Layout, Margin, Modifiers, RichText, ScrollArea,
    Stroke, Ui,
};

use crate::i18n::t;
use crate::icons;
use crate::model::{display_title, Page};
use crate::speech;
use crate::theme::{self, Tokens};
use crate::widgets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderTheme {
    Paper,
    White,
    Sepia,
    Dark,
}

impl ReaderTheme {
    const ALL: [ReaderTheme; 4] = [
        ReaderTheme::Paper,
        ReaderTheme::White,
        ReaderTheme::Sepia,
        ReaderTheme::Dark,
    ];

    fn label(self) -> Cow<'static, str> {
        match self {
            ReaderTheme::Paper => t!("reader-theme-paper"),
            ReaderTheme::White => t!("reader-theme-white"),
            ReaderTheme::Sepia => t!("reader-theme-sepia"),
            ReaderTheme::Dark => t!("reader-theme-dark"),
        }
    }

    fn colors(self) -> (Color32, Color32, Color32) {
        match self {
            ReaderTheme::Paper => (
                Color32::from_rgb(0xfa, 0xf9, 0xf7),
                Color32::from_rgb(0x10, 0x10, 0x14),
                Color32::from_rgb(0x77, 0x19, 0xaa),
            ),
            ReaderTheme::White => (
                Color32::WHITE,
                Color32::from_rgb(0x10, 0x10, 0x14),
                Color32::from_rgb(0x77, 0x19, 0xaa),
            ),
            ReaderTheme::Sepia => (
                Color32::from_rgb(0xf4, 0xec, 0xd8),
                Color32::from_rgb(0x33, 0x2b, 0x20),
                Color32::from_rgb(0x8a, 0x58, 0x00),
            ),
            ReaderTheme::Dark => (
                Color32::from_rgb(0x0d, 0x0c, 0x11),
                Color32::from_rgb(0xf2, 0xf1, 0xee),
                Color32::from_rgb(0x9d, 0x8d, 0xff),
            ),
        }
    }
}

pub struct ReaderState {
    pub open: bool,
    pub font_size: f32,
    pub line_focus: usize,
    pub focus_line: usize,
    pub theme: ReaderTheme,
    pub page_id: String,
    /// The page's text, line by line, built once per opening (a long printout's text is
    /// too much to rebuild every frame), and the page and version it was built from.
    lines: Vec<String>,
    lines_of: Option<(String, i64, i64)>,
}

impl Default for ReaderState {
    fn default() -> Self {
        Self {
            open: false,
            font_size: 24.0,
            line_focus: 3,
            focus_line: 0,
            theme: ReaderTheme::Paper,
            page_id: String::new(),
            lines: Vec::new(),
            lines_of: None,
        }
    }
}

impl ReaderState {
    pub fn begin(&mut self, page: &Page) {
        if self.page_id != page.id {
            self.focus_line = 0;
        }
        self.page_id = page.id.clone();
        self.open = true;
        self.lines_of = None;
    }
}

pub enum ReaderOutcome {
    None,
    Close,
    Toast(String),
}

pub fn show(ctx: &egui::Context, page: &Page, state: &mut ReaderState, t: Tokens) -> ReaderOutcome {
    if !state.open {
        return ReaderOutcome::None;
    }
    state.page_id = page.id.clone();
    let (bg, ink, _accent) = state.theme.colors();
    let muted = Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 110);
    let faint = Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 55);
    let version = (page.id.clone(), page.updated_at, page.word_count);
    if state.lines_of.as_ref() != Some(&version) {
        state.lines = page
            .plain_text()
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
        state.lines_of = Some(version);
    }
    // Lent out while the reader is drawn, which changes `state`; put back below.
    let lines = std::mem::take(&mut state.lines);
    let line_count = lines.len();
    state.focus_line = state.focus_line.min(line_count.saturating_sub(1));
    let mut outcome = ReaderOutcome::None;
    let mut close = false;

    let response = egui::Modal::new(Id::unique(("folio-reader", page.id.as_str())))
        .backdrop_color(Color32::from_black_alpha(if t.dark { 170 } else { 110 }))
        .frame(
            egui::Frame::new()
                .fill(bg)
                .stroke(Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 24),
                ))
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::same(26)),
        )
        .show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 96.0).clamp(560.0, 980.0);
            ui.set_width(width);
            let height = (ctx.content_rect().height() - 250.0).clamp(240.0, 560.0);
            ui.set_min_height(height + 100.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(display_title(&page.title))
                            .font(theme::light(30.0))
                            .color(ink),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  {}  ·  {}",
                            t!("common-immersive-reading"),
                            t!("common-word-count", count = page.word_count()),
                            page.section_name
                        ))
                        .font(theme::ui(12.0))
                        .color(muted),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if reader_button(ui, icons::X, &t!("app-reader-close"), false, ink, muted, bg)
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let text_hud = t!("app-reader-text");
                let smaller = t!("app-reader-smaller");
                let larger = t!("app-reader-larger");
                let size = format!("{:.0}", state.font_size);
                let focus_hud = t!("app-reader-line-focus");
                let off = t!("app-reader-line-focus-off");
                let focus = [(0usize, &*off), (1, "1"), (3, "3"), (5, "5")];
                let themes = ReaderTheme::ALL.map(|theme| (theme, theme.label()));
                let read_aloud = t!("app-reader-read-aloud");
                // The text-size buttons keep their labels only where the whole row fits
                // beside Read aloud (French runs longer than the reader at any width);
                // otherwise they fold to their glyphs, labelled by tooltip. Each widget
                // below is measured the way it is drawn, plus the item spacing after it.
                let spacing = ui.spacing().item_spacing.x;
                let row = [
                    widgets::caption_width(ui, &text_hud),
                    reader_button_width(ui, icons::MINUS, &smaller),
                    ui.painter()
                        .layout_no_wrap(size.clone(), theme::ui(12.0), muted)
                        .size()
                        .x,
                    reader_button_width(ui, icons::PLUS, &larger),
                    widgets::caption_width(ui, &focus_hud),
                    // The two rules.
                    1.0,
                    1.0,
                ]
                .into_iter()
                .chain(
                    focus
                        .iter()
                        .map(|(_, label)| reader_button_width(ui, "", label)),
                )
                .chain(
                    themes
                        .iter()
                        .map(|(_, label)| reader_button_width(ui, "", label)),
                )
                .map(|width| width + spacing)
                .sum::<f32>()
                    // The 10 px either side of the two rules.
                    + 4.0 * 10.0
                    + widgets::primary_button_width(ui, Some(icons::VOLUME_HIGH), &read_aloud);
                let labelled = row <= ui.available_width();

                widgets::caption(ui, &text_hud, muted);
                if size_button(ui, icons::MINUS, &smaller, labelled, ink, muted, bg).clicked() {
                    state.font_size = (state.font_size - 2.0).max(16.0);
                }
                ui.label(RichText::new(size).font(theme::ui(12.0)).color(muted));
                if size_button(ui, icons::PLUS, &larger, labelled, ink, muted, bg).clicked() {
                    state.font_size = (state.font_size + 2.0).min(42.0);
                }
                ui.add_space(10.0);
                widgets::vline(
                    ui,
                    Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 35),
                    20.0,
                );
                ui.add_space(10.0);
                widgets::caption(ui, &focus_hud, muted);
                for (count, label) in focus {
                    let active = state.line_focus == count;
                    if reader_button(ui, "", label, active, ink, muted, bg).clicked() {
                        state.line_focus = count;
                    }
                }
                ui.add_space(10.0);
                widgets::vline(
                    ui,
                    Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 35),
                    20.0,
                );
                ui.add_space(10.0);
                for (theme, label) in &themes {
                    if reader_button(ui, "", label, state.theme == *theme, ink, muted, bg).clicked()
                    {
                        state.theme = *theme;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::primary_button(ui, Some(icons::VOLUME_HIGH), &read_aloud, t)
                        .clicked()
                    {
                        match speech::speak(&page.plain_text()) {
                            Ok(()) => {
                                outcome = ReaderOutcome::Toast(
                                    t!("app-reader-reading-aloud").into_owned(),
                                )
                            }
                            Err(error) => outcome = ReaderOutcome::Toast(error),
                        }
                    }
                });
            });
            ui.add_space(10.0);
            ui.painter().line_segment(
                [ui.cursor().left_top(), ui.cursor().right_top()],
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 26),
                ),
            );
            ui.add_space(8.0);
            ScrollArea::vertical()
                .id_salt(("reader-scroll", page.id.as_str()))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width() - 24.0);
                    ui.add_space(8.0);
                    if lines.is_empty() {
                        ui.label(
                            RichText::new(t!("app-reader-no-text"))
                                .font(theme::body_italic(state.font_size))
                                .color(muted),
                        );
                    }
                    for (index, line) in lines.iter().enumerate() {
                        let focused = state.line_focus == 0
                            || (index as isize - state.focus_line as isize).unsigned_abs()
                                <= state.line_focus / 2;
                        let response = ui.add(
                            egui::Label::new(
                                RichText::new(if line.is_empty() { " " } else { line })
                                    .font(theme::body(state.font_size))
                                    .color(if focused { ink } else { faint }),
                            )
                            .wrap(),
                        );
                        if response.clicked() {
                            state.focus_line = index;
                        }
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                        }
                        ui.add_space(5.0);
                    }
                    ui.add_space(28.0);
                });
            let _ = faint;
        });
    state.lines = lines;
    if response.should_close() || close {
        state.open = false;
        outcome = ReaderOutcome::Close;
    }
    if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
        state.open = false;
        outcome = ReaderOutcome::Close;
    }
    let _ = t;
    outcome
}

/// A text-size button: its glyph and label where the toolbar has room, else its glyph
/// alone, with the label as its tooltip and accessible name.
fn size_button(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    labelled: bool,
    ink: Color32,
    muted: Color32,
    bg: Color32,
) -> egui::Response {
    if labelled {
        return reader_button(ui, glyph, label, false, ink, muted, bg);
    }
    let response = reader_button(ui, glyph, "", false, ink, muted, bg);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label)
}

fn reader_button_text(glyph: &str, label: &str) -> String {
    if glyph.is_empty() {
        label.to_string()
    } else if label.is_empty() {
        glyph.to_string()
    } else {
        format!("{glyph}  {label}")
    }
}

fn reader_button_font(glyph: &str) -> egui::FontId {
    if glyph.is_empty() {
        theme::ui(11.0)
    } else {
        theme::ui(12.0)
    }
}

/// How wide `reader_button` draws: its old fixed size, or its text and padding where that
/// runs longer (French does), so the text never spills past the button's frame.
fn reader_button_width(ui: &Ui, glyph: &str, label: &str) -> f32 {
    let base: f32 = if glyph.is_empty() {
        38.0
    } else if label.is_empty() {
        28.0
    } else {
        84.0
    };
    let text = ui
        .painter()
        .layout_no_wrap(
            reader_button_text(glyph, label),
            reader_button_font(glyph),
            Color32::PLACEHOLDER,
        )
        .size()
        .x;
    base.max(text + 2.0 * ui.spacing().button_padding.x)
}

fn reader_button(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    active: bool,
    ink: Color32,
    muted: Color32,
    bg: Color32,
) -> egui::Response {
    let text = reader_button_text(glyph, label);
    let font = reader_button_font(glyph);
    let width = reader_button_width(ui, glyph, label);
    let mut style = ui.style().as_ref().clone();
    style.visuals.widgets.inactive.weak_bg_fill = if active {
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 18)
    } else {
        bg
    };
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(
        1.0,
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 28),
    );
    style.visuals.widgets.hovered.weak_bg_fill =
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 25);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(
        1.0,
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 60),
    );
    style.visuals.widgets.active.weak_bg_fill =
        Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 35);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ink);
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add_sized(
            [width, 25.0],
            egui::Button::new(RichText::new(text).font(font).color(if active {
                ink
            } else {
                muted
            })),
        )
    })
    .inner
}
