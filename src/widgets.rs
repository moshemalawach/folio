use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{
    self, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, RichText,
    Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
};

use crate::icons;
use crate::theme::{self, Tokens};

/// The width of one line of `text` in `font`. It goes through egui's galley cache, so
/// measuring every frame (the ribbon does, to fit the window in the current language) is
/// cheap.
pub fn text_width(ctx: &egui::Context, text: &str, font: FontId) -> f32 {
    ctx.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER))
        .size()
        .x
}

/// A desktop notebook card: white, rounded, with a soft shadow, on the grey window. The ribbon,
/// the page list, the page and the task pane each sit in one.
pub fn card(t: Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(t.raised)
        .corner_radius(CornerRadius::same(8))
        .shadow(egui::Shadow {
            offset: [0, 1],
            blur: 6,
            spread: 0,
            color: t.card_shadow,
        })
}

pub fn hairline(ui: &mut Ui, color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, color);
}

/// `Painter::text` on one line, cut with "…" where it would run past `max_width`. The cut
/// is measured (not a count of characters), so the text always ends inside its row.
pub fn fitted_text(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    text: impl Into<String>,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Rect {
    let mut job = LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    let galley = painter.layout_job(job);
    let rect = anchor.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

pub fn vline(ui: &mut Ui, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, color);
}

/// A small heading over a group of controls ("Appearance", "Language"). Office writes
/// these in sentence case in the interface font.
pub fn caption(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).font(caption_font()).color(color));
}

/// How wide `caption` draws `text`, for rows that measure before they lay out.
pub fn caption_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), caption_font(), Color32::PLACEHOLDER)
        .size()
        .x
}

fn caption_font() -> FontId {
    theme::semibold(12.0)
}

pub fn icon_button_sized(
    ui: &mut Ui,
    glyph: &str,
    tooltip: &str,
    box_size: f32,
    glyph_size: f32,
    t: Tokens,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(box_size), Sense::click());
    let response = response.on_hover_text(tooltip);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), tooltip));
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let fill = if pressed {
        t.sunken
    } else if hovered || response.has_focus() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect.shrink(1.0), CornerRadius::same(3), fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(3),
            Stroke::new(1.0, t.accent),
            StrokeKind::Inside,
        );
    }
    let color = if response.enabled() {
        if hovered {
            t.ink
        } else {
            t.muted
        }
    } else {
        t.faint
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        theme::icon(glyph_size),
        color,
    );
    response
}

pub fn text_button(ui: &mut Ui, glyph: Option<&str>, label: &str, t: Tokens) -> Response {
    let mut text = String::new();
    if let Some(glyph) = glyph {
        text.push_str(glyph);
        text.push(' ');
    }
    text.push_str(label);
    let response = ui.add(
        egui::Button::new(RichText::new(text).font(theme::semibold(12.0)).color(t.ink))
            .frame(false),
    );
    response.on_hover_text(label)
}

/// A small toggle in a pane's row of choices, as the ribbon draws its toggles: a grey
/// fill with a grey frame when on, a lighter grey under the pointer.
pub fn toggle_chip(ui: &mut Ui, on: bool, label: &str, t: Tokens) -> Response {
    let font = theme::ui(11.5);
    let width = text_width(ui.ctx(), label, font.clone()) + 16.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    let radius = CornerRadius::same(4);
    if on {
        ui.painter().rect_filled(rect, radius, t.nav_selected);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, t.container_border),
            StrokeKind::Inside,
        );
    }
    if response.hovered() {
        ui.painter().rect_filled(rect, radius, t.hover);
    }
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, label, font, t.ink);
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, on, label));
    response
}

/// More Colors…' picker: every hue from left to right, from white at the top through the
/// full colour (half way) to black at the bottom. A click or a drag picks the colour under
/// the pointer into `color`; returns whether it changed.
pub fn color_plane(ui: &mut Ui, color: &mut Color32, size: Vec2, t: Tokens) -> bool {
    const COLUMNS: usize = 36;
    const ROWS: usize = 16;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let at = |x: f32, y: f32| -> Color32 {
        let full = Color32::from(egui::ecolor::Hsva::new(x.clamp(0.0, 1.0), 1.0, 1.0, 1.0));
        let y = y.clamp(0.0, 1.0);
        if y < 0.5 {
            Color32::WHITE.lerp_to_gamma(full, y * 2.0)
        } else {
            full.lerp_to_gamma(Color32::BLACK, (y - 0.5) * 2.0)
        }
    };
    if ui.is_rect_visible(rect) {
        let mut mesh = egui::Mesh::default();
        for row in 0..=ROWS {
            for column in 0..=COLUMNS {
                let (x, y) = (column as f32 / COLUMNS as f32, row as f32 / ROWS as f32);
                mesh.colored_vertex(rect.lerp_inside(vec2(x, y)), at(x, y));
            }
        }
        let index = |row: usize, column: usize| (row * (COLUMNS + 1) + column) as u32;
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let (a, b) = (index(row, column), index(row, column + 1));
                let (c, d) = (index(row + 1, column), index(row + 1, column + 1));
                mesh.add_triangle(a, b, c);
                mesh.add_triangle(b, d, c);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, t.line_strong),
            StrokeKind::Outside,
        );
    }
    let picked = (response.clicked() || response.dragged())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|pos| {
            let local = (pos - rect.min) / rect.size();
            at(local.x, local.y)
        });
    match picked {
        Some(picked) if picked != *color => {
            *color = picked;
            true
        }
        _ => false,
    }
}

fn primary_text(glyph: Option<&str>, label: &str) -> String {
    let mut text = String::new();
    if let Some(glyph) = glyph {
        text.push_str(glyph);
        text.push(' ');
    }
    text.push_str(label);
    text
}

/// How wide `primary_button` draws, for rows that measure before they lay out.
pub fn primary_button_width(ui: &Ui, glyph: Option<&str>, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            primary_text(glyph, label),
            theme::semibold(12.0),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x
}

pub fn primary_button(ui: &mut Ui, glyph: Option<&str>, label: &str, t: Tokens) -> Response {
    let text = primary_text(glyph, label);
    let mut style = ui.style().as_ref().clone();
    style.visuals.widgets.inactive.weak_bg_fill = t.accent;
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.hovered.weak_bg_fill = t.accent.gamma_multiply(0.88);
    style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    style.visuals.widgets.active.weak_bg_fill = t.accent.gamma_multiply(0.75);
    style.visuals.widgets.active.bg_stroke = Stroke::NONE;
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add(
            egui::Button::new(
                RichText::new(text)
                    .font(theme::semibold(12.0))
                    .color(theme::on_fill(t.accent)),
            )
            .corner_radius(CornerRadius::same(4))
            .min_size(vec2(0.0, 30.0)),
        )
    })
    .inner
}

pub fn danger_button(ui: &mut Ui, label: &str, t: Tokens) -> Response {
    let mut style = ui.style().as_ref().clone();
    style.visuals.widgets.inactive.weak_bg_fill = t.danger;
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.hovered.weak_bg_fill = t.danger.gamma_multiply(0.88);
    style.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    style.visuals.widgets.active.weak_bg_fill = t.danger.gamma_multiply(0.75);
    style.visuals.widgets.active.bg_stroke = Stroke::NONE;
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add(
            egui::Button::new(
                RichText::new(label)
                    .font(theme::semibold(12.0))
                    .color(theme::on_fill(t.danger)),
            )
            .corner_radius(CornerRadius::same(4))
            .min_size(vec2(0.0, 30.0)),
        )
    })
    .inner
}

pub fn secondary_button(ui: &mut Ui, glyph: Option<&str>, label: &str, t: Tokens) -> Response {
    let mut text = String::new();
    if let Some(glyph) = glyph {
        text.push_str(glyph);
        text.push(' ');
    }
    text.push_str(label);
    let mut style = ui.style().as_ref().clone();
    style.visuals.widgets.inactive.weak_bg_fill = t.raised;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, t.line_strong);
    style.visuals.widgets.hovered.weak_bg_fill = t.sunken;
    style.visuals.widgets.active.weak_bg_fill = t.sunken;
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add(
            egui::Button::new(RichText::new(text).font(theme::semibold(12.0)).color(t.ink))
                .corner_radius(CornerRadius::same(4))
                .min_size(vec2(0.0, 30.0)),
        )
    })
    .inner
}

pub fn empty_state(ui: &mut Ui, glyph: &str, title: &str, body: &str, t: Tokens) {
    ui.vertical_centered(|ui| {
        ui.add_space(58.0);
        ui.label(RichText::new(glyph).font(theme::icon(34.0)).color(t.faint));
        ui.add_space(10.0);
        ui.label(
            RichText::new(title)
                .font(theme::semilight(20.0))
                .color(t.ink),
        );
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(body).font(theme::ui(12.5)).color(t.muted)).wrap());
        ui.add_space(14.0);
    });
}

pub fn keycap(ui: &mut Ui, label: &str, t: Tokens) {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::ui(11.0), t.muted);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 10.0, 18.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), t.sunken);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, t.muted);
}

/// A menu row, 28 px high and as wide as the menu.
const MENU_ROW: f32 = 28.0;

pub fn menu_item(ui: &mut Ui, glyph: Option<&str>, label: &str, t: Tokens) -> Response {
    tinted_menu_item(ui, glyph, None, label, t)
}

/// `menu_item` for a choice, as the desktop notebook ticks its menus: a check mark at its left (in the
/// glyph column) when it is the current choice, or its glyph on a highlighted square.
pub fn checked_menu_item(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: &str,
    checked: bool,
    t: Tokens,
) -> Response {
    menu_entry(ui, glyph, None, label, "", Some(checked), t)
}

/// `checked_menu_item` with a tinted glyph (a notebook's colour, a tag's).
pub fn tinted_checked_menu_item(
    ui: &mut Ui,
    glyph: Option<&str>,
    glyph_color: Option<Color32>,
    label: &str,
    checked: bool,
    t: Tokens,
) -> Response {
    menu_entry(ui, glyph, glyph_color, label, "", Some(checked), t)
}

/// `menu_row` for a choice: a check mark at its left (in the glyph column) when it is the
/// current choice, or its glyph on a highlighted square; the shortcut right-aligned, and
/// greyed when it can't run.
pub fn checked_menu_row(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: &str,
    shortcut: &str,
    enabled: bool,
    checked: bool,
    t: Tokens,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        menu_entry(ui, glyph, None, label, shortcut, Some(checked), t)
    })
    .inner
}

/// A menu row as the desktop notebook lays its menus out: the glyph at the left and the label after
/// it, both left-aligned, so a menu's glyphs make one column and its labels another
/// (every Fluent glyph has the same advance). They sit where egui's own submenu rows,
/// written "{glyph}    {label}", put theirs, so those line up too. The glyph takes
/// `glyph_color` when given (a tag's colour). The row's name is "{glyph}    {label}".
pub fn tinted_menu_item(
    ui: &mut Ui,
    glyph: Option<&str>,
    glyph_color: Option<Color32>,
    label: &str,
    t: Tokens,
) -> Response {
    menu_entry(ui, glyph, glyph_color, label, "", None, t)
}

/// The menu rows above. With `checked`, the row is a choice: a row without a glyph keeps
/// the glyph column for its check mark, so ticked and unticked rows line up, and a row
/// with one shows it on a highlighted square while on. A disabled `ui` greys the row as
/// it greys `menu_row` (its painter fades everything), with no hover and no click.
fn menu_entry(
    ui: &mut Ui,
    glyph: Option<&str>,
    glyph_color: Option<Color32>,
    label: &str,
    shortcut: &str,
    checked: Option<bool>,
    t: Tokens,
) -> Response {
    let font = theme::ui(12.5);
    let shortcut_font = theme::ui(11.5);
    let name = match glyph {
        Some(glyph) => format!("{glyph}    {label}"),
        None => label.to_owned(),
    };
    let left = ui.spacing().button_padding.x;
    let column = match (glyph, checked) {
        (Some(glyph), _) => Some(glyph),
        (None, Some(_)) => Some(icons::CHECK),
        (None, None) => None,
    };
    let label_x = entry_label_x(ui, column, &font);
    let mut width = label_x + text_width(ui.ctx(), label, font.clone()) + 32.0;
    if !shortcut.is_empty() {
        width += text_width(ui.ctx(), shortcut, shortcut_font.clone()) + 16.0;
    }
    let (rect, response) = ui.allocate_at_least(vec2(width, MENU_ROW), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| match checked {
        Some(on) => WidgetInfo::selected(Role::Button, enabled, on, &name),
        None => WidgetInfo::labeled(Role::Button, enabled, &name),
    });
    if ui.is_rect_visible(rect) {
        paint_entry_back(
            ui,
            rect,
            &response,
            glyph.is_none() && checked == Some(true),
            t,
        );
        let painter = ui.painter();
        let y = rect.center().y;
        if let Some(glyph) = glyph {
            let at = egui::pos2(rect.left() + left, y);
            if checked == Some(true) {
                let center = at + vec2(text_width(ui.ctx(), glyph, font.clone()) / 2.0, 0.0);
                let square = Rect::from_center_size(center, Vec2::splat(22.0));
                painter.rect_filled(square, CornerRadius::same(3), t.nav_selected);
                painter.rect_stroke(
                    square,
                    CornerRadius::same(3),
                    Stroke::new(1.0, t.container_border),
                    StrokeKind::Inside,
                );
            }
            painter.text(
                at,
                Align2::LEFT_CENTER,
                glyph,
                font.clone(),
                glyph_color.unwrap_or(t.ink),
            );
        }
        painter.text(
            egui::pos2(rect.left() + label_x, y),
            Align2::LEFT_CENTER,
            label,
            font,
            t.ink,
        );
        if !shortcut.is_empty() {
            painter.text(
                egui::pos2(rect.right() - left, y),
                Align2::RIGHT_CENTER,
                shortcut,
                shortcut_font,
                t.muted,
            );
        }
    }
    response
}

/// Where a menu row's label starts: after the glyph column, which holds the row's glyph,
/// or the check mark of a choice that has none.
fn entry_label_x(ui: &Ui, column: Option<&str>, font: &FontId) -> f32 {
    ui.spacing().button_padding.x
        + column.map_or(0.0, |glyph| {
            text_width(ui.ctx(), &format!("{glyph}    "), font.clone())
        })
}

/// A menu row's hover fill, and its check mark at the left (in the glyph column) when
/// `ticked`.
fn paint_entry_back(ui: &Ui, rect: Rect, response: &Response, ticked: bool, t: Tokens) {
    let painter = ui.painter();
    if ui.is_enabled() && (response.hovered() || response.has_focus()) {
        let hovered = &ui.visuals().widgets.hovered;
        painter.rect_filled(rect, hovered.corner_radius, hovered.weak_bg_fill);
    }
    if ticked {
        painter.text(
            egui::pos2(rect.left() + ui.spacing().button_padding.x, rect.center().y),
            Align2::LEFT_CENTER,
            icons::CHECK,
            theme::icon(12.0),
            t.ink,
        );
    }
}

/// A choice shown as a preview of itself (a style in its own font and colour): a check
/// mark at its left (in the glyph column) when it is the current choice, then `job`
/// where a row's label goes. The row is named `label`.
pub fn checked_preview_item(
    ui: &mut Ui,
    job: LayoutJob,
    label: &str,
    checked: bool,
    t: Tokens,
) -> Response {
    let label_x = entry_label_x(ui, Some(icons::CHECK), &theme::ui(12.5));
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let width = label_x + galley.size().x + 32.0;
    let (rect, response) = ui.allocate_at_least(vec2(width, MENU_ROW), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, checked, label));
    if ui.is_rect_visible(rect) {
        paint_entry_back(ui, rect, &response, checked, t);
        let at = egui::pos2(
            rect.left() + label_x,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(at, galley, t.ink);
    }
    response
}

/// A menu row as the desktop notebook draws them: glyph, label, and the shortcut right-aligned, greyed
/// when it can't run. It is `MENU_ROW` high, and its glyph and label sit where
/// `menu_item`'s do, so the two kinds of row line up in one menu.
pub fn menu_row(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: &str,
    shortcut: &str,
    enabled: bool,
    t: Tokens,
) -> Response {
    let text = match glyph {
        Some(glyph) => format!("{glyph}    {label}"),
        None => label.to_string(),
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).font(theme::ui(12.5)).color(t.ink))
            .shortcut_text(RichText::new(shortcut).font(theme::ui(11.5)).color(t.muted))
            .frame(false)
            .min_size(vec2(ui.available_width(), MENU_ROW)),
    )
}

/// A colour swatch: `fill` in a `size` square with a hairline outline, or, when
/// `selected`, a 2 px accent ring with a hairline of the card's white inside it (so the
/// ring shows on a purple swatch too). It is a button named `name`, also its tooltip. The
/// pen, text and highlight colours and `swatch_grid` all draw theirs with it.
pub fn swatch(
    ui: &mut Ui,
    fill: Color32,
    size: f32,
    selected: bool,
    name: &str,
    t: Tokens,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same(3);
        painter.rect_filled(rect, radius, fill);
        if selected {
            painter.rect_stroke(rect, radius, Stroke::new(2.0, t.accent), StrokeKind::Inside);
            painter.rect_stroke(
                rect.shrink(2.0),
                CornerRadius::same(2),
                Stroke::new(1.0, t.raised),
                StrokeKind::Inside,
            );
        } else {
            painter.rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, t.line_strong),
                StrokeKind::Inside,
            );
        }
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name));
    response.on_hover_text(name)
}

/// A swatch of `swatch_grid`, and the room between two.
const SWATCH: f32 = 22.0;
const SWATCH_GAP: f32 = 4.0;
/// Swatches in one row of `swatch_grid`.
const SWATCHES_PER_ROW: usize = 8;

/// The section colours (`theme::SECTION_COLORS`) as a grid of swatches, eight to a row, the
/// current one ringed in the accent colour; with `with_none`, a "None" chip
/// (`theme::NO_COLOR`: outlined, crossed by a diagonal) follows them. Each swatch is a
/// button named after its colour (`theme::color_name`), which is also its tooltip. A
/// click sets `color`; returns whether it changed.
pub fn swatch_grid(ui: &mut Ui, color: &mut String, with_none: bool, t: Tokens) -> bool {
    let chips: Vec<&str> = theme::SECTION_COLORS
        .iter()
        .copied()
        .chain(with_none.then_some(theme::NO_COLOR))
        .collect();
    let mut changed = false;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(SWATCH_GAP, SWATCH_GAP);
        for row in chips.chunks(SWATCHES_PER_ROW) {
            ui.horizontal(|ui| {
                for &hex in row {
                    let none = hex == theme::NO_COLOR;
                    let current = if none {
                        color.is_empty()
                    } else {
                        color.eq_ignore_ascii_case(hex)
                    };
                    let fill = if none { t.raised } else { theme::color(hex) };
                    let name = theme::color_name(hex);
                    let response = swatch(ui, fill, SWATCH, current, &name, t);
                    if none && ui.is_rect_visible(response.rect) {
                        let inner = response.rect.shrink(5.0);
                        ui.painter().line_segment(
                            [inner.left_bottom(), inner.right_top()],
                            Stroke::new(1.5, t.muted),
                        );
                    }
                    if response.clicked() && !current {
                        *color = hex.to_owned();
                        changed = true;
                    }
                }
            });
        }
    });
    changed
}

/// Paints a texture fitted inside `rect` (keeping its proportions), cropped, turned and
/// mirrored as `meta` says. Returns where the picture landed.
pub fn paint_texture_turned(
    painter: &Painter,
    texture: egui::TextureId,
    size: Vec2,
    rect: Rect,
    meta: &crate::model::PictureMeta,
) -> Rect {
    let [width, height] = meta.cropped_size([size.x, size.y]);
    let size = vec2(width, height);
    let shown = if meta.quarter_turned() {
        vec2(size.y, size.x)
    } else {
        size
    };
    let scale = (rect.width() / shown.x.max(1.0)).min(rect.height() / shown.y.max(1.0));
    let image = Rect::from_center_size(rect.center(), shown * scale);
    let corners = [
        image.left_top(),
        image.right_top(),
        image.right_bottom(),
        image.left_bottom(),
    ];
    let mut mesh = egui::Mesh::with_texture(texture);
    for (pos, [u, v]) in corners.into_iter().zip(meta.uv_corners()) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::pos2(u, v),
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
    image
}
