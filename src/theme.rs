use std::borrow::Cow;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, Style, TextStyle, Vec2,
};

use crate::i18n::t;

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub raised: Color32,
    pub sunken: Color32,
    pub panel: Color32,
    pub ink: Color32,
    pub ink_strong: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub selection: Color32,
    pub hover: Color32,
    /// Section, page and page-title text: one step lighter than `ink` (#424242).
    pub text2: Color32,
    /// The selected section row in the navigation pane.
    pub nav_selected: Color32,
    /// The selected page row in the page list.
    pub page_selected: Color32,
    /// A note container's grab bar.
    pub grab_bar: Color32,
    /// A note container's frame while hovered or edited.
    pub container_border: Color32,
    /// The soft shadow under the white cards (ribbon, page list, page).
    pub card_shadow: Color32,
}

impl Tokens {
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: Color32::from_rgb(0xf3, 0xf3, 0xf3),
            raised: Color32::WHITE,
            sunken: Color32::from_rgb(0xf5, 0xf5, 0xf5),
            panel: Color32::from_rgb(0xf3, 0xf3, 0xf3),
            ink: Color32::from_rgb(0x24, 0x24, 0x24),
            ink_strong: Color32::BLACK,
            muted: Color32::from_rgb(0x61, 0x61, 0x61),
            faint: Color32::from_rgb(0x9e, 0x9e, 0x9e),
            line: Color32::from_rgb(0xe6, 0xe6, 0xe6),
            line_strong: Color32::from_rgb(0xd1, 0xd1, 0xd1),
            accent: Color32::from_rgb(0x77, 0x19, 0xaa),
            accent_soft: Color32::from_rgb(0xf1, 0xe7, 0xf8),
            danger: Color32::from_rgb(0xc4, 0x2b, 0x1c),
            warning: Color32::from_rgb(0xf7, 0x63, 0x0c),
            selection: Color32::from_rgb(0xcc, 0xe4, 0xf7),
            // Translucent, so one value darkens both the grey window and the white cards.
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 0x0f),
            text2: Color32::from_rgb(0x42, 0x42, 0x42),
            nav_selected: Color32::from_rgb(0xe7, 0xe7, 0xe7),
            page_selected: Color32::from_rgb(0xeb, 0xeb, 0xeb),
            grab_bar: Color32::from_rgb(0xde, 0xde, 0xde),
            container_border: Color32::from_rgb(0xc8, 0xc8, 0xc8),
            card_shadow: Color32::from_black_alpha(26),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: Color32::from_rgb(0x1f, 0x1f, 0x1f),
            raised: Color32::from_rgb(0x29, 0x29, 0x29),
            sunken: Color32::from_rgb(0x33, 0x33, 0x33),
            panel: Color32::from_rgb(0x1f, 0x1f, 0x1f),
            ink: Color32::from_rgb(0xf5, 0xf5, 0xf5),
            ink_strong: Color32::WHITE,
            muted: Color32::from_rgb(0xad, 0xad, 0xad),
            faint: Color32::from_rgb(0x85, 0x85, 0x85),
            line: Color32::from_rgb(0x3d, 0x3d, 0x3d),
            line_strong: Color32::from_rgb(0x57, 0x57, 0x57),
            accent: Color32::from_rgb(0xc0, 0x8a, 0xe6),
            accent_soft: Color32::from_rgb(0x3d, 0x2a, 0x4d),
            danger: Color32::from_rgb(0xff, 0x99, 0xa4),
            warning: Color32::from_rgb(0xfa, 0xa0, 0x6b),
            selection: Color32::from_rgb(0x26, 0x4f, 0x78),
            hover: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x14),
            text2: Color32::from_rgb(0xd6, 0xd6, 0xd6),
            nav_selected: Color32::from_rgb(0x38, 0x38, 0x38),
            page_selected: Color32::from_rgb(0x3d, 0x3d, 0x3d),
            grab_bar: Color32::from_rgb(0x47, 0x47, 0x47),
            container_border: Color32::from_rgb(0x5c, 0x5c, 0x5c),
            card_shadow: Color32::from_black_alpha(90),
        }
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

pub fn tokens(ctx: &egui::Context) -> Tokens {
    Tokens::for_dark(ctx.theme() == egui::Theme::Dark)
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    let light = style(Tokens::light());
    let dark = style(Tokens::dark());
    ctx.set_style_of(egui::Theme::Light, light);
    ctx.set_style_of(egui::Theme::Dark, dark);
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    // Selawik is Microsoft's open stand-in for Segoe UI, the Office interface font.
    add("ui", include_bytes!("../assets/fonts/Selawik-Regular.ttf"));
    add(
        "ui-light",
        include_bytes!("../assets/fonts/Selawik-Light.ttf"),
    );
    add(
        "ui-semilight",
        include_bytes!("../assets/fonts/Selawik-Semilight.ttf"),
    );
    add(
        "ui-semibold",
        include_bytes!("../assets/fonts/Selawik-Semibold.ttf"),
    );
    add(
        "ui-bold",
        include_bytes!("../assets/fonts/Selawik-Bold.ttf"),
    );
    // Selawik has no U+00A0, U+202F, º, ª, ◦ or ▪ (French spacing needs the first two).
    add(
        "ui-fallback",
        include_bytes!("../assets/fonts/SourceSans3-Regular.ttf"),
    );
    // Carlito has Calibri's metrics: the desktop notebook's note text.
    add(
        "body",
        include_bytes!("../assets/fonts/Carlito-Regular.ttf"),
    );
    add(
        "body-bold",
        include_bytes!("../assets/fonts/Carlito-Bold.ttf"),
    );
    add(
        "body-italic",
        include_bytes!("../assets/fonts/Carlito-Italic.ttf"),
    );
    add(
        "body-bold-italic",
        include_bytes!("../assets/fonts/Carlito-BoldItalic.ttf"),
    );
    add(
        "serif",
        include_bytes!("../assets/fonts/SourceSerif4-Regular.ttf"),
    );
    add(
        "serif-italic",
        include_bytes!("../assets/fonts/SourceSerif4-It.ttf"),
    );
    add(
        "serif-bold",
        include_bytes!("../assets/fonts/SourceSerif4-Bold.ttf"),
    );
    add(
        "mono",
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    );
    add(
        "icons",
        include_bytes!("../assets/fonts/fluent/FluentIcons-Regular.ttf"),
    );
    add(
        "icons-filled",
        include_bytes!("../assets/fonts/fluent/FluentIcons-Filled.ttf"),
    );
    add(
        "readable",
        include_bytes!("../assets/fonts/AtkinsonHyperlegible-Regular.ttf"),
    );
    add(
        "readable-bold",
        include_bytes!("../assets/fonts/AtkinsonHyperlegible-Bold.ttf"),
    );
    add(
        "readable-italic",
        include_bytes!("../assets/fonts/AtkinsonHyperlegible-Italic.ttf"),
    );
    add(
        "hand",
        include_bytes!("../assets/fonts/PatrickHand-Regular.ttf"),
    );

    let proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let monospace = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    // The icons come straight after the text faces: egui's own fallback fonts map some of
    // the Private Use Area code points the Fluent icons use, and would win otherwise.
    // egui's monospace default (Hack) comes last: of all the fonts, only it draws ↵, the
    // macOS Return key on keycaps.
    let mut ui = ["ui", "ui-fallback", "icons"].map(str::to_owned).to_vec();
    for fallback in proportional.iter().chain(&monospace) {
        if !ui.contains(fallback) {
            ui.push(fallback.clone());
        }
    }
    fonts.families.insert(FontFamily::Proportional, ui.clone());
    for name in [
        "ui-light",
        "ui-semilight",
        "ui-semibold",
        "ui-bold",
        "body",
        "body-bold",
        "body-italic",
        "body-bold-italic",
        "serif",
        "serif-italic",
        "serif-bold",
        "readable",
        "readable-bold",
        "readable-italic",
        "hand",
    ] {
        add_family(&mut fonts, name, &[name], &ui);
    }
    add_family(&mut fonts, "icons", &["icons"], &ui);
    add_family(&mut fonts, "icons-filled", &["icons-filled", "icons"], &ui);

    let mut mono = ["mono", "icons"].map(str::to_owned).to_vec();
    for fallback in &monospace {
        if !mono.contains(fallback) {
            mono.push(fallback.clone());
        }
    }
    fonts.families.insert(FontFamily::Monospace, mono.clone());
    add_family(&mut fonts, "mono", &["mono"], &mono);

    ctx.set_fonts(fonts);
}

/// Office-like widget skin. Standard buttons and drop-downs are white boxes with a #D1D1D1
/// hairline and a 4 px radius; menus and dialogs are white cards with a hairline and a soft
/// shadow. The text caret is ink, not purple, and egui-selected buttons use the selection blue.
pub fn style(t: Tokens) -> Style {
    let mut style = Style::default();
    let mut visuals = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.dark_mode = t.dark;
    visuals.override_text_color = Some(t.ink);
    visuals.weak_text_color = Some(t.muted);
    visuals.panel_fill = t.bg;
    // Dialogs, menus and tooltips are white cards with a hairline and a soft shadow.
    visuals.window_fill = t.raised;
    visuals.window_stroke = Stroke::new(1.0, t.line_strong);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.extreme_bg_color = t.raised;
    visuals.text_edit_bg_color = Some(t.raised);
    visuals.code_bg_color = t.sunken;
    visuals.faint_bg_color = t.sunken;
    visuals.hyperlink_color = t.accent;
    visuals.warn_fg_color = t.warning;
    visuals.error_fg_color = t.danger;
    visuals.selection.bg_fill = t.selection;
    visuals.selection.stroke = Stroke::new(1.0, t.ink);
    visuals.text_cursor.stroke = Stroke::new(1.5, t.ink);
    visuals.text_cursor.preview = true;
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 120 } else { 46 }),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if t.dark { 110 } else { 36 }),
    };
    visuals.resize_corner_size = 10.0;
    visuals.indent_has_left_vline = false;
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.slider_trailing_fill = true;

    // Office buttons and boxes: white with a grey hairline, a light grey on hover, and the
    // accent tint while pressed. Frameless buttons (most of Folio's) only show hover.
    let radius = CornerRadius::same(4);
    visuals.widgets.noninteractive.bg_fill = t.bg;
    visuals.widgets.noninteractive.weak_bg_fill = t.bg;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.ink);
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = t.raised;
    visuals.widgets.inactive.weak_bg_fill = t.raised;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, t.line_strong);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, t.ink);
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = t.sunken;
    visuals.widgets.hovered.weak_bg_fill = t.sunken;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, t.container_border);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, t.ink);
    visuals.widgets.hovered.corner_radius = radius;

    visuals.widgets.active.bg_fill = t.accent_soft;
    visuals.widgets.active.weak_bg_fill = t.accent_soft;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, t.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, t.ink_strong);
    visuals.widgets.active.corner_radius = radius;

    visuals.widgets.open.bg_fill = t.sunken;
    visuals.widgets.open.weak_bg_fill = t.sunken;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, t.container_border);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, t.ink);
    visuals.widgets.open.corner_radius = radius;

    style.visuals = visuals;
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.indent = 16.0;
    style.spacing.interact_size = Vec2::new(30.0, 28.0);
    style.spacing.icon_width = 16.0;
    style.spacing.icon_width_inner = 9.0;
    style.spacing.icon_spacing = 7.0;
    style.spacing.text_edit_width = 180.0;
    style.spacing.combo_width = 130.0;
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating_width = 4.0;
    style.interaction.tooltip_delay = 0.35;
    style.interaction.selectable_labels = false;
    style.animation_time = 0.16;

    style
        .text_styles
        .insert(TextStyle::Heading, FontId::new(20.0, named("ui-semibold")));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(11.5, FontFamily::Monospace),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.5, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style
}

pub fn named(name: &str) -> FontFamily {
    FontFamily::Name(Arc::from(name))
}

fn add_family(fonts: &mut FontDefinitions, name: &str, first: &[&str], fallbacks: &[String]) {
    let mut list: Vec<String> = first.iter().map(|value| (*value).to_owned()).collect();
    for fallback in fallbacks {
        if !list.contains(fallback) {
            list.push(fallback.clone());
        }
    }
    fonts
        .families
        .insert(FontFamily::Name(Arc::from(name)), list);
}

pub fn color(hex: &str) -> Color32 {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0x64);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0x6a);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0x7a);
        Color32::from_rgb(r, g, b)
    } else {
        Color32::from_rgb(0x64, 0x6a, 0x7a)
    }
}

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// The 16 section and notebook colours, in the Section Color menu's order.
pub const SECTION_COLORS: [&str; 16] = [
    "#0166AC", "#FABE14", "#00B24A", "#D13438", "#8764B8", "#00B7C3", "#F2610C", "#BE0151",
    "#6E9BD1", "#9A8BC9", "#C39F74", "#B5CC18", "#6BB700", "#038387", "#D47E7E", "#A0A0A0",
];
/// Section Color › None: no colour of its own, drawn in a neutral grey.
pub const NO_COLOR: &str = "";

/// The name of a colour from `SECTION_COLORS` (any case), for its tooltip and menu row;
/// `NO_COLOR` is "None", and any other colour is its own hex.
pub fn color_name(hex: &str) -> Cow<'static, str> {
    if hex == NO_COLOR {
        return t!("app-color-none");
    }
    let Some(index) = SECTION_COLORS
        .iter()
        .position(|color| color.eq_ignore_ascii_case(hex))
    else {
        return Cow::Owned(hex.to_owned());
    };
    match index {
        0 => t!("app-section-color-blue"),
        1 => t!("app-section-color-yellow"),
        2 => t!("app-section-color-green"),
        3 => t!("app-section-color-red"),
        4 => t!("app-section-color-purple"),
        5 => t!("app-section-color-cyan"),
        6 => t!("app-section-color-orange"),
        7 => t!("app-section-color-magenta"),
        8 => t!("app-section-color-blue-mist"),
        9 => t!("app-section-color-purple-mist"),
        10 => t!("app-section-color-tan"),
        11 => t!("app-section-color-lemon-lime"),
        12 => t!("app-section-color-apple"),
        13 => t!("app-section-color-teal"),
        14 => t!("app-section-color-red-chalk"),
        _ => t!("app-section-color-silver"),
    }
}

/// `color` mixed toward `base` in sRGB: `amount` 0 is the colour itself and 1 is `base`.
/// The Horizontal Tabs layout draws its section tabs, band and page list this way.
pub fn pastel(color: Color32, base: Color32, amount: f32) -> Color32 {
    color.lerp_to_gamma(base, amount.clamp(0.0, 1.0))
}

/// WCAG's relative luminance of an opaque colour.
fn luminance(color: Color32) -> f32 {
    let channel = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.039_28 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// WCAG's contrast ratio between two opaque colours, from 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The text colour on `fill` (a primary button, a ticked box): white, unless near-black
/// reads better, as on dark mode's light purple.
pub fn on_fill(fill: Color32) -> Color32 {
    let dark = Color32::from_rgb(0x1b, 0x1b, 0x1b);
    if contrast(Color32::WHITE, fill) >= contrast(dark, fill) {
        Color32::WHITE
    } else {
        dark
    }
}

/// `color`, mixed toward white (`pastel`) in steps until it has `min` contrast on
/// `background`: a tag's dark blue on dark mode's page. A colour that already reads is
/// kept as it is.
pub fn lifted_on(color: Color32, background: Color32, min: f32) -> Color32 {
    let mut lifted = color;
    let mut amount = 0.0;
    while contrast(lifted, background) < min && amount < 1.0 {
        amount += 0.05;
        lifted = pastel(color, Color32::WHITE, amount);
    }
    lifted
}

/// Backstage's left column: the desktop notebook's purple, as Office's colourful theme paints it, and a
/// deeper purple in dark mode. White text reads on both.
pub fn backstage_fill(t: Tokens) -> Color32 {
    if t.dark {
        Color32::from_rgb(0x5b, 0x2a, 0x86)
    } else {
        Color32::from_rgb(0x77, 0x19, 0xaa)
    }
}

/// Horizontal Tabs' tints, as measured on the desktop notebook: how far a section's colour is mixed
/// toward white (`pastel`) for its tabs and the band under them, the page list's header
/// and the page list itself. The selected tab is not tinted differently: it is raised and
/// joins the band.
pub const TAB_TINT: f32 = 0.6;
pub const TAB_HOVER_TINT: f32 = 0.5;
pub const LIST_HEADER_TINT: f32 = 0.45;
pub const LIST_TINT: f32 = 0.86;

/// A section's (or group's) colour as the navigation pane and the section tabs draw it:
/// `NO_COLOR` is a neutral grey.
pub fn section_color(hex: &str, t: Tokens) -> Color32 {
    if hex == NO_COLOR {
        t.line_strong
    } else {
        color(hex)
    }
}

/// Background behind find-on-page matches; the current match is stronger. Dark mode
/// uses translucent fills so light ink stays readable.
pub fn find_highlight(t: Tokens, current: bool) -> Color32 {
    match (t.dark, current) {
        (false, false) => Color32::from_rgb(0xff, 0xe8, 0x7a),
        (false, true) => Color32::from_rgb(0xff, 0xb3, 0x47),
        (true, false) => Color32::from_rgba_unmultiplied(0xff, 0xc1, 0x4d, 0x45),
        (true, true) => Color32::from_rgba_unmultiplied(0xff, 0xa0, 0x2e, 0x9a),
    }
}

pub fn ui(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, named("ui-semibold"))
}

pub fn light(size: f32) -> FontId {
    FontId::new(size, named("ui-light"))
}

pub fn semilight(size: f32) -> FontId {
    FontId::new(size, named("ui-semilight"))
}

/// Note text: Carlito, which has Calibri's metrics (the desktop notebook's default note font).
pub fn body(size: f32) -> FontId {
    FontId::new(size, named("body"))
}

pub fn body_italic(size: f32) -> FontId {
    FontId::new(size, named("body-italic"))
}

/// The page title: 20 pt Selawik Light (see PLAN contract C1), at 4/3 px per pt (R2).
pub const TITLE_SIZE: f32 = 20.0 * 4.0 / 3.0;

pub fn serif(size: f32) -> FontId {
    FontId::new(size, named("serif"))
}

pub fn serif_italic(size: f32) -> FontId {
    FontId::new(size, named("serif-italic"))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn icon(size: f32) -> FontId {
    FontId::new(size, named("icons"))
}

/// The filled style of a Fluent icon (same names, its own code points: look them up in
/// assets/fonts/fluent/codepoints.txt, style "filled").
pub fn icon_filled(size: f32) -> FontId {
    FontId::new(size, named("icons-filled"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character a message may use (`i18n::tests::messages_only_use_characters_the_
    /// fonts_have`), plus the spaces French typography inserts, has a glyph in the
    /// interface and note faces, so nothing shows as an empty box.
    #[test]
    fn the_interface_and_note_fonts_draw_every_character_messages_use() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut missing = Vec::new();
        ctx.run_ui(egui::RawInput::default(), |frame| {
            frame.fonts_mut(|fonts| {
                let chars = (' '..='~')
                    .chain('\u{a0}'..='\u{ff}')
                    .chain("œŒŸ‘’“”…–—€•↑↓←→↵⌘⇧⌥⌃\u{202f}◦▪".chars());
                for ch in chars {
                    for font in [ui(13.0), semibold(13.0), light(20.0), body(14.0)] {
                        if !fonts.has_glyph(&font, ch) {
                            missing.push((font.family.clone(), ch));
                        }
                    }
                }
            });
        })
        .drop_without_applying_deltas();
        assert!(missing.is_empty(), "no glyph: {missing:?}");
    }

    #[test]
    fn color_names_cover_every_notebook_colour() {
        crate::i18n::pin_thread_lang(crate::i18n::Lang::En);
        let names: Vec<String> = SECTION_COLORS
            .iter()
            .map(|hex| color_name(hex).into_owned())
            .collect();
        let distinct: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(distinct.len(), SECTION_COLORS.len(), "{names:?}");
        assert!(
            names
                .iter()
                .all(|name| !name.is_empty() && !name.starts_with('#')),
            "{names:?}"
        );
        assert_eq!(color_name(NO_COLOR), "None");
        assert_eq!(color_name("#0166ac"), "Blue");
        assert_eq!(color_name("#a0a0a0"), "Silver");
        // A colour from before the desktop notebook palette names itself.
        assert_eq!(color_name("#745cf5"), "#745cf5");
        assert_eq!(
            section_color(NO_COLOR, Tokens::light()),
            Tokens::light().line_strong
        );
        assert_eq!(
            section_color(SECTION_COLORS[0], Tokens::light()),
            Color32::from_rgb(0x01, 0x66, 0xac)
        );
    }

    #[test]
    fn pastel_mixes_toward_the_base() {
        let blue = color(SECTION_COLORS[0]);
        let white = Color32::WHITE;
        assert_eq!(pastel(blue, white, 0.0), blue);
        assert_eq!(pastel(blue, white, 1.0), white);
        let half = pastel(blue, white, 0.5);
        assert_eq!(half, Color32::from_rgb(0x80, 0xb3, 0xd6));
        // Out-of-range amounts stay between the two.
        assert_eq!(pastel(blue, white, 2.0), white);
        assert_eq!(pastel(blue, white, -1.0), blue);
    }

    #[test]
    fn buttons_and_ticks_read_on_their_fill_in_both_themes() {
        for t in [Tokens::light(), Tokens::dark()] {
            for fill in [t.accent, t.danger] {
                assert!(
                    contrast(on_fill(fill), fill) >= 4.5,
                    "{fill:?} in dark={}",
                    t.dark
                );
            }
            assert!(contrast(Color32::WHITE, backstage_fill(t)) >= 4.5);
        }
        // The light theme keeps the desktop notebook's white text on purple.
        assert_eq!(on_fill(Tokens::light().accent), Color32::WHITE);
        assert!((contrast(Color32::WHITE, Color32::BLACK) - 21.0).abs() < 0.01);
    }
}
