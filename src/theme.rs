//! Visual language: palette, fonts and egui style for LucAPI.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    Style, TextStyle, Visuals,
};
use std::sync::Arc;

use crate::model::Method;

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub panel_alt: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warn: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub code_bg: Color32,
    pub json_key: Color32,
    pub json_string: Color32,
    pub json_number: Color32,
    pub json_literal: Color32,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: Color32::from_rgb(0xF4, 0xF6, 0xF9),
            panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            panel_alt: Color32::from_rgb(0xF8, 0xF9, 0xFB),
            border: Color32::from_rgb(0xE6, 0xE9, 0xEF),
            border_strong: Color32::from_rgb(0xD3, 0xD8, 0xE2),
            text: Color32::from_rgb(0x1C, 0x20, 0x27),
            muted: Color32::from_rgb(0x74, 0x7C, 0x8B),
            faint: Color32::from_rgb(0xA2, 0xA9, 0xB5),
            accent: Color32::from_rgb(0x2B, 0x5C, 0xE6),
            accent_hover: Color32::from_rgb(0x1F, 0x4B, 0xCC),
            accent_soft: Color32::from_rgb(0xEB, 0xF1, 0xFE),
            on_accent: Color32::WHITE,
            hover: Color32::from_rgb(0xEF, 0xF2, 0xF7),
            selected: Color32::from_rgb(0xE7, 0xEF, 0xFE),
            success: Color32::from_rgb(0x0E, 0x8F, 0x62),
            success_soft: Color32::from_rgb(0xE6, 0xF6, 0xEF),
            warn: Color32::from_rgb(0xB4, 0x7A, 0x0B),
            danger: Color32::from_rgb(0xC8, 0x3A, 0x3F),
            danger_soft: Color32::from_rgb(0xFC, 0xEC, 0xEC),
            code_bg: Color32::from_rgb(0xFB, 0xFB, 0xFD),
            json_key: Color32::from_rgb(0x6B, 0x41, 0xC9),
            json_string: Color32::from_rgb(0x0B, 0x79, 0x54),
            json_number: Color32::from_rgb(0x1A, 0x60, 0xC8),
            json_literal: Color32::from_rgb(0xA8, 0x5A, 0x06),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: Color32::from_rgb(0x0F, 0x11, 0x15),
            panel: Color32::from_rgb(0x16, 0x19, 0x1E),
            panel_alt: Color32::from_rgb(0x1B, 0x1F, 0x25),
            border: Color32::from_rgb(0x25, 0x2A, 0x32),
            border_strong: Color32::from_rgb(0x33, 0x3A, 0x44),
            text: Color32::from_rgb(0xE8, 0xEA, 0xEF),
            muted: Color32::from_rgb(0x96, 0x9E, 0xAB),
            faint: Color32::from_rgb(0x6A, 0x72, 0x80),
            accent: Color32::from_rgb(0x55, 0x90, 0xFF),
            accent_hover: Color32::from_rgb(0x74, 0xA4, 0xFF),
            accent_soft: Color32::from_rgb(0x1C, 0x2A, 0x44),
            on_accent: Color32::from_rgb(0x0B, 0x12, 0x22),
            hover: Color32::from_rgb(0x1E, 0x23, 0x2A),
            selected: Color32::from_rgb(0x21, 0x30, 0x4C),
            success: Color32::from_rgb(0x3E, 0xD6, 0x99),
            success_soft: Color32::from_rgb(0x14, 0x2A, 0x22),
            warn: Color32::from_rgb(0xE8, 0xB0, 0x4B),
            danger: Color32::from_rgb(0xFF, 0x6B, 0x6B),
            danger_soft: Color32::from_rgb(0x32, 0x1B, 0x1D),
            code_bg: Color32::from_rgb(0x12, 0x15, 0x1A),
            json_key: Color32::from_rgb(0xC0, 0xA3, 0xFF),
            json_string: Color32::from_rgb(0x7E, 0xD6, 0xA8),
            json_number: Color32::from_rgb(0x7C, 0xB8, 0xFF),
            json_literal: Color32::from_rgb(0xFF, 0xB8, 0x6B),
        }
    }

    pub fn of(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn method_color(&self, method: Method) -> Color32 {
        match method {
            Method::Get => {
                if self.dark {
                    Color32::from_rgb(0x34, 0xD3, 0x99)
                } else {
                    Color32::from_rgb(0x0B, 0x8A, 0x5E)
                }
            }
            Method::Post => {
                if self.dark {
                    Color32::from_rgb(0xF5, 0xB9, 0x4B)
                } else {
                    Color32::from_rgb(0xB2, 0x6D, 0x05)
                }
            }
            Method::Put => {
                if self.dark {
                    Color32::from_rgb(0x74, 0xA8, 0xFF)
                } else {
                    Color32::from_rgb(0x1F, 0x5F, 0xC9)
                }
            }
            Method::Patch => {
                if self.dark {
                    Color32::from_rgb(0xB4, 0x9B, 0xFF)
                } else {
                    Color32::from_rgb(0x6D, 0x3F, 0xC4)
                }
            }
            Method::Delete => {
                if self.dark {
                    Color32::from_rgb(0xFF, 0x7B, 0x7B)
                } else {
                    Color32::from_rgb(0xC0, 0x35, 0x3A)
                }
            }
            Method::Head | Method::Options => self.muted,
        }
    }

    pub fn status_color(&self, status: u16) -> Color32 {
        match status {
            200..=299 => self.success,
            300..=399 => self.accent,
            400..=499 => self.warn,
            _ => self.danger,
        }
    }
}

// ---------------------------------------------------------------------------
// Typography
//
// One superfamily carries the product: IBM Plex Sans for the interface and
// IBM Plex Mono for anything that is an identifier (URLs, headers, JSON and
// the wordmark itself). Using the two Plex cuts instead of unrelated faces is
// what makes the app read as one designed system rather than a font salad.
// ---------------------------------------------------------------------------

pub const UI_FONT: &str = "plex-sans";
pub const UI_MEDIUM_FONT: &str = "plex-sans-medium";
pub const UI_SEMIBOLD_FONT: &str = "plex-sans-semibold";
pub const MONO_FONT: &str = "plex-mono";
pub const MONO_MEDIUM_FONT: &str = "plex-mono-medium";
pub const BRAND_FONT: &str = "plex-mono-semibold";

pub const DISPLAY_FAMILY: &str = "display";
pub const MEDIUM_FAMILY: &str = "medium";
pub const MONO_MEDIUM_FAMILY: &str = "mono-medium";
pub const BRAND_FAMILY: &str = "brand";

/// SemiBold sans for page titles and the few large headings.
pub fn display_family() -> FontFamily {
    FontFamily::Name(DISPLAY_FAMILY.into())
}

/// Medium sans for labels, buttons and anything that needs quiet emphasis.
pub fn medium_family() -> FontFamily {
    FontFamily::Name(MEDIUM_FAMILY.into())
}

/// Medium mono for table headings and identifiers that should read as data.
pub fn mono_medium_family() -> FontFamily {
    FontFamily::Name(MONO_MEDIUM_FAMILY.into())
}

/// SemiBold mono used by the wordmark and the app's brand lockup.
pub fn brand_family() -> FontFamily {
    FontFamily::Name(BRAND_FAMILY.into())
}

pub fn display(size: f32) -> FontId {
    FontId::new(size, display_family())
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, medium_family())
}

pub fn brand(size: f32) -> FontId {
    FontId::new(size, brand_family())
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // Bundled open-source faces (OFL) keep the typography identical everywhere.
    for (name, bytes) in [
        (
            UI_FONT,
            &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..],
        ),
        (
            UI_MEDIUM_FONT,
            &include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf")[..],
        ),
        (
            UI_SEMIBOLD_FONT,
            &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
        ),
        (
            MONO_FONT,
            &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..],
        ),
        (
            MONO_MEDIUM_FONT,
            &include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")[..],
        ),
        (
            BRAND_FONT,
            &include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, UI_FONT.to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, MONO_FONT.to_owned());

    // Named families only contain the Plex cuts; borrow the system fallbacks
    // (emoji and symbol fonts) from the default families so user-typed names
    // never render as missing glyphs.
    let ui_fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mono_fallbacks = fonts
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let with_fallbacks = |primary: &[&str], fallbacks: &[String]| {
        let mut list: Vec<String> = primary.iter().map(|name| (*name).to_owned()).collect();
        for name in fallbacks {
            if !list.contains(name) {
                list.push(name.clone());
            }
        }
        list
    };

    fonts.families.insert(
        FontFamily::Name(DISPLAY_FAMILY.into()),
        with_fallbacks(&[UI_SEMIBOLD_FONT, UI_FONT], &ui_fallbacks),
    );
    fonts.families.insert(
        FontFamily::Name(MEDIUM_FAMILY.into()),
        with_fallbacks(&[UI_MEDIUM_FONT, UI_FONT], &ui_fallbacks),
    );
    fonts.families.insert(
        FontFamily::Name(MONO_MEDIUM_FAMILY.into()),
        with_fallbacks(&[MONO_MEDIUM_FONT, MONO_FONT], &mono_fallbacks),
    );
    fonts.families.insert(
        FontFamily::Name(BRAND_FAMILY.into()),
        with_fallbacks(&[BRAND_FONT, MONO_FONT], &mono_fallbacks),
    );

    ctx.set_fonts(fonts);
}

/// The brand mark as a texture. It is rasterised once and kept in egui's
/// temporary data, so every logo in the UI shares a single upload.
pub fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let key = egui::Id::new("lucapi.brand.logo");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return handle;
    }
    let image = logo_image();
    let handle = ctx.load_texture("lucapi-logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, handle.clone()));
    handle
}

/// Decoded brand mark, shared by the app icon and the texture cache.
pub fn logo_image() -> egui::ColorImage {
    let bytes = include_bytes!("../assets/logo.png");
    let image = image::load_from_memory(bytes)
        .expect("bundled logo")
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())
}

pub fn apply_style(ctx: &egui::Context, p: &Palette) {
    let mut style = Style::default();

    style.text_styles = [
        (TextStyle::Heading, display(21.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
        // Medium weight makes buttons look pressed-in rather than floating.
        (TextStyle::Button, medium(13.0)),
        (TextStyle::Small, FontId::proportional(11.0)),
    ]
    .into();

    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size = egui::vec2(24.0, 28.0);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.animation_time = 0.12;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;

    let mut visuals = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    visuals.override_text_color = Some(p.text);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.panel;
    visuals.extreme_bg_color = p.panel_alt;
    visuals.faint_bg_color = p.panel_alt;
    visuals.code_bg_color = p.code_bg;
    visuals.hyperlink_color = p.accent;
    visuals.warn_fg_color = p.warn;
    visuals.error_fg_color = p.danger;
    visuals.selection = egui::style::Selection {
        bg_fill: p.selected,
        stroke: Stroke::new(1.0_f32, p.accent),
    };
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(9);
    visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    visuals.window_shadow = Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 90 } else { 28 }),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 80 } else { 24 }),
    };

    let base = egui::style::WidgetVisuals {
        bg_fill: p.panel_alt,
        weak_bg_fill: Color32::TRANSPARENT,
        bg_stroke: Stroke::new(1.0_f32, p.border),
        corner_radius: CornerRadius::same(6),
        fg_stroke: Stroke::new(1.0_f32, p.text),
        expansion: 0.0,
    };
    visuals.widgets.noninteractive = egui::style::WidgetVisuals {
        fg_stroke: Stroke::new(1.0_f32, p.text),
        bg_fill: p.panel_alt,
        weak_bg_fill: p.panel_alt,
        bg_stroke: Stroke::NONE,
        ..base
    };
    visuals.widgets.inactive = egui::style::WidgetVisuals {
        fg_stroke: Stroke::new(1.0_f32, p.text),
        weak_bg_fill: Color32::TRANSPARENT,
        bg_fill: p.panel_alt,
        bg_stroke: Stroke::new(1.0_f32, p.border),
        ..base
    };
    visuals.widgets.hovered = egui::style::WidgetVisuals {
        fg_stroke: Stroke::new(1.0_f32, p.text),
        weak_bg_fill: p.hover,
        bg_fill: p.hover,
        bg_stroke: Stroke::new(1.0_f32, p.border_strong),
        ..base
    };
    visuals.widgets.active = egui::style::WidgetVisuals {
        fg_stroke: Stroke::new(1.0_f32, p.accent),
        weak_bg_fill: p.accent_soft,
        bg_fill: p.accent_soft,
        bg_stroke: Stroke::new(1.0_f32, p.accent),
        ..base
    };
    visuals.widgets.open = egui::style::WidgetVisuals {
        fg_stroke: Stroke::new(1.0_f32, p.text),
        weak_bg_fill: p.hover,
        bg_fill: p.panel_alt,
        bg_stroke: Stroke::new(1.0_f32, p.border_strong),
        ..base
    };
    visuals.slider_trailing_fill = true;

    style.visuals = visuals;

    // Text edits inherit a calm, borderless look; frames are drawn by widgets.
    style.visuals.text_cursor.stroke.color = p.accent;
    style.visuals.text_cursor.stroke.width = 1.5;

    ctx.set_style(style);
}
