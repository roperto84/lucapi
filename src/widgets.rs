//! Reusable painted widgets: icons, method badges, tables and JSON rendering.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Layout, Rect, Response, Sense,
    Stroke, StrokeKind, TextEdit, TextFormat, Ui, Vec2,
};
use egui::text::LayoutJob;

use crate::model::{Assertion, Capture, CaptureSource, KV, Method};
use crate::theme::Palette;

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Api,
    History,
    Globe,
    Flask,
    Server,
    Book,
    Gear,
    Search,
    Plus,
    Folder,
    ChevronDown,
    ChevronRight,
    Close,
    Send,
    Copy,
    Trash,
    Check,
    Alert,
    Moon,
    Sun,
    More,
    Lock,
    Play,
    Sparkle,
    Code,
}

pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32, width: f32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let p = |x: f32, y: f32| c + egui::vec2(x * s, y * s);
    let stroke = Stroke::new(width, color);
    let line = |points: Vec<egui::Pos2>| {
        painter.add(egui::Shape::line(points, stroke));
    };
    let closed = |points: Vec<egui::Pos2>| {
        painter.add(egui::Shape::closed_line(points, stroke));
    };

    match icon {
        Icon::Api => {
            line(vec![
                p(-0.12, -0.88),
                p(-0.42, -0.74),
                p(-0.42, -0.18),
                p(-0.66, 0.0),
                p(-0.42, 0.18),
                p(-0.42, 0.74),
                p(-0.12, 0.88),
            ]);
            line(vec![
                p(0.12, -0.88),
                p(0.42, -0.74),
                p(0.42, -0.18),
                p(0.66, 0.0),
                p(0.42, 0.18),
                p(0.42, 0.74),
                p(0.12, 0.88),
            ]);
        }
        Icon::History => {
            painter.circle_stroke(c, 0.78 * s, stroke);
            line(vec![c, p(0.0, -0.4)]);
            line(vec![c, p(0.35, 0.12)]);
        }
        Icon::Globe => {
            painter.circle_stroke(c, 0.8 * s, stroke);
            line(vec![p(-0.8, 0.0), p(0.8, 0.0)]);
            let mut pts = Vec::new();
            for i in 0..=28 {
                let a = i as f32 / 28.0 * std::f32::consts::TAU;
                pts.push(p(0.42 * a.cos(), 0.8 * a.sin()));
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
        Icon::Flask => {
            closed(vec![
                p(-0.32, -0.8),
                p(-0.32, -0.18),
                p(-0.82, 0.72),
                p(0.82, 0.72),
                p(0.32, -0.18),
                p(0.32, -0.8),
            ]);
            line(vec![p(-0.44, -0.8), p(0.44, -0.8)]);
            line(vec![p(-0.56, 0.32), p(0.56, 0.32)]);
        }
        Icon::Server => {
            painter.rect_stroke(
                Rect::from_min_max(p(-0.8, -0.78), p(0.8, -0.08)),
                CornerRadius::same(2),
                stroke,
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(-0.8, 0.08), p(0.8, 0.78)),
                CornerRadius::same(2),
                stroke,
                StrokeKind::Middle,
            );
            painter.circle_filled(p(-0.5, -0.43), 0.07 * s, color);
            painter.circle_filled(p(-0.5, 0.43), 0.07 * s, color);
        }
        Icon::Book => {
            closed(vec![
                p(-0.62, -0.8),
                p(0.62, -0.8),
                p(0.62, 0.8),
                p(-0.62, 0.8),
            ]);
            line(vec![p(-0.2, -0.8), p(-0.2, 0.8)]);
        }
        Icon::Gear => {
            for (y, knob) in [(-0.52, 0.28), (0.0, -0.34), (0.52, 0.42)] {
                line(vec![p(-0.8, y), p(0.8, y)]);
                painter.circle_filled(p(knob, y), 0.17 * s, color);
            }
        }
        Icon::Search => {
            painter.circle_stroke(p(-0.15, -0.15), 0.5 * s, stroke);
            line(vec![p(0.24, 0.24), p(0.72, 0.72)]);
        }
        Icon::Plus => {
            line(vec![p(-0.55, 0.0), p(0.55, 0.0)]);
            line(vec![p(0.0, -0.55), p(0.0, 0.55)]);
        }
        Icon::Folder => {
            closed(vec![
                p(-0.82, -0.5),
                p(-0.28, -0.5),
                p(-0.08, -0.24),
                p(0.82, -0.24),
                p(0.82, 0.62),
                p(-0.82, 0.62),
            ]);
        }
        Icon::ChevronDown => line(vec![p(-0.4, -0.22), p(0.0, 0.24), p(0.4, -0.22)]),
        Icon::ChevronRight => line(vec![p(-0.22, -0.4), p(0.24, 0.0), p(-0.22, 0.4)]),
        Icon::Close => {
            line(vec![p(-0.42, -0.42), p(0.42, 0.42)]);
            line(vec![p(0.42, -0.42), p(-0.42, 0.42)]);
        }
        Icon::Send => {
            closed(vec![
                p(-0.72, -0.66),
                p(0.78, 0.0),
                p(-0.72, 0.66),
                p(-0.35, 0.0),
            ]);
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(p(-0.75, -0.45), p(0.3, 0.78)),
                CornerRadius::same(2),
                stroke,
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(-0.25, -0.78), p(0.78, 0.42)),
                CornerRadius::same(2),
                stroke,
                StrokeKind::Middle,
            );
        }
        Icon::Trash => {
            line(vec![p(-0.55, -0.5), p(0.55, -0.5)]);
            line(vec![
                p(-0.2, -0.5),
                p(-0.2, -0.7),
                p(0.2, -0.7),
                p(0.2, -0.5),
            ]);
            line(vec![
                p(-0.42, -0.5),
                p(-0.32, 0.72),
                p(0.32, 0.72),
                p(0.42, -0.5),
            ]);
            line(vec![p(-0.12, -0.25), p(-0.08, 0.45)]);
            line(vec![p(0.12, -0.25), p(0.08, 0.45)]);
        }
        Icon::Check => line(vec![p(-0.6, 0.02), p(-0.14, 0.48), p(0.66, -0.48)]),
        Icon::Alert => {
            painter.circle_stroke(c, 0.78 * s, stroke);
            line(vec![p(0.0, -0.42), p(0.0, 0.14)]);
            painter.circle_filled(p(0.0, 0.44), 0.09 * s, color);
        }
        Icon::Moon => {
            let mut pts = Vec::new();
            for i in 0..=20 {
                let a = 0.6 + i as f32 / 20.0 * (std::f32::consts::TAU - 2.4);
                pts.push(p(0.55 * a.cos(), 0.55 * a.sin()));
            }
            for i in 0..=20 {
                let a =
                    std::f32::consts::TAU - 0.6 - i as f32 / 20.0 * (std::f32::consts::TAU - 2.4);
                pts.push(p(0.32 * a.cos() + 0.3, 0.32 * a.sin()));
            }
            closed(pts);
        }
        Icon::Sun => {
            painter.circle_stroke(c, 0.34 * s, stroke);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                line(vec![
                    p(0.55 * a.cos(), 0.55 * a.sin()),
                    p(0.85 * a.cos(), 0.85 * a.sin()),
                ]);
            }
        }
        Icon::More => {
            for dx in [-0.5, 0.0, 0.5] {
                painter.circle_filled(p(dx, 0.0), 0.09 * s, color);
            }
        }
        Icon::Lock => {
            painter.rect_stroke(
                Rect::from_min_max(p(-0.55, -0.08), p(0.55, 0.72)),
                CornerRadius::same(2),
                stroke,
                StrokeKind::Middle,
            );
            let mut pts = Vec::new();
            for i in 0..=18 {
                let a = std::f32::consts::PI + i as f32 / 18.0 * std::f32::consts::PI;
                pts.push(p(0.36 * a.cos(), 0.36 * a.sin() - 0.08));
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
        Icon::Play => {
            closed(vec![p(-0.5, -0.62), p(0.62, 0.0), p(-0.5, 0.62)]);
        }
        Icon::Sparkle => {
            closed(vec![
                p(0.0, -0.85),
                p(0.2, -0.2),
                p(0.85, 0.0),
                p(0.2, 0.2),
                p(0.0, 0.85),
                p(-0.2, 0.2),
                p(-0.85, 0.0),
                p(-0.2, -0.2),
            ]);
        }
        Icon::Code => {
            line(vec![p(-0.5, -0.5), p(-0.9, 0.0), p(-0.5, 0.5)]);
            line(vec![p(0.5, -0.5), p(0.9, 0.0), p(0.5, 0.5)]);
            line(vec![p(0.22, -0.7), p(-0.22, 0.7)]);
        }
    }
}

fn paint_rect(painter: &egui::Painter, rect: Rect, radius: u8, fill: Color32, stroke: Stroke) {
    painter.rect_filled(rect, CornerRadius::same(radius), fill);
    if stroke.width > 0.0 {
        painter.rect_stroke(rect, CornerRadius::same(radius), stroke, StrokeKind::Inside);
    }
}

pub fn icon_button(ui: &mut Ui, p: &Palette, icon: Icon, size: f32, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), p.hover);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size * 0.58));
    paint_icon(
        ui.painter(),
        icon_rect,
        icon,
        if resp.hovered() { p.text } else { p.muted },
        1.4,
    );
    if !tooltip.is_empty() {
        resp.clone().on_hover_text(tooltip);
    }
    resp
}

/// Brand mark: a geometric "L" with round terminals on a gradient tile.
/// The artwork is a bundled asset so it renders identically at 22 px in the
/// top bar and at 56 px on the welcome screen.
pub fn logo(painter: &egui::Painter, rect: Rect) {
    let s = rect.width().min(rect.height());
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(s));
    let texture = crate::theme::logo_texture(painter.ctx());
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Wordmark: the product name set as a code token — "Luc" in text colour and
/// "API" in the accent, both in SemiBold Plex Mono. It ties the brand to the
/// monospace world the app lives in.
pub fn wordmark(ui: &mut Ui, p: &Palette, size: f32) -> Response {
    let font = crate::theme::brand(size);
    let luc = ui.fonts(|f| f.layout_no_wrap("Luc".into(), font.clone(), p.text));
    let api = ui.fonts(|f| f.layout_no_wrap("API".into(), font, p.accent));
    let gap = 0.5;
    let width = luc.size().x + api.size().x + gap;
    let height = luc.size().y.max(api.size().y);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let top = rect.top() + (height - luc.size().y) * 0.5;
    ui.painter()
        .galley(egui::pos2(rect.left(), top), luc.clone(), p.text);
    ui.painter().galley(
        egui::pos2(rect.left() + luc.size().x + gap, top),
        api,
        p.accent,
    );
    resp
}

/// Single-line editor that highlights `{{variables}}` and completes them.
#[allow(clippy::too_many_arguments)]
pub fn var_singleline(
    ui: &mut Ui,
    p: &Palette,
    defined: &std::collections::HashSet<String>,
    text: &mut String,
    font: FontId,
    hint: &str,
    size: [f32; 2],
    framed: bool,
    margin: egui::Margin,
    id: egui::Id,
) -> Response {
    let snapshot = defined.clone();
    let layout_font = font.clone();
    let palette = *p;
    let mut layouter = |ui: &Ui, content: &str, wrap_width: f32| {
        let mut job = crate::vars::layout_job(
            content,
            &snapshot,
            layout_font.clone(),
            palette.text,
            palette.accent,
            palette.danger,
        );
        job.wrap.max_width = wrap_width;
        ui.fonts(|fonts| fonts.layout_job(job))
    };
    let mut edit = TextEdit::singleline(text)
        .hint_text(hint)
        .font(font)
        .margin(margin)
        .desired_width(size[0])
        .layouter(&mut layouter);
    if !framed {
        edit = edit.frame(false);
    }
    let response = ui.add_sized(size, edit);
    if response.has_focus() {
        show_var_suggestions(ui, p, defined, text, &response, id);
    }
    response
}

/// Multi-line editor with the same variable highlighting and completion.
#[allow(clippy::too_many_arguments)]
pub fn var_multiline(
    ui: &mut Ui,
    p: &Palette,
    defined: &std::collections::HashSet<String>,
    text: &mut String,
    font: FontId,
    hint: &str,
    size: [f32; 2],
    margin: egui::Margin,
    id: egui::Id,
) -> Response {
    let snapshot = defined.clone();
    let layout_font = font.clone();
    let palette = *p;
    let mut layouter = |ui: &Ui, content: &str, wrap_width: f32| {
        let mut job = crate::vars::layout_job(
            content,
            &snapshot,
            layout_font.clone(),
            palette.text,
            palette.accent,
            palette.danger,
        );
        job.wrap.max_width = wrap_width;
        ui.fonts(|fonts| fonts.layout_job(job))
    };
    let response = ui.add_sized(
        size,
        TextEdit::multiline(text)
            .hint_text(hint)
            .font(font)
            .margin(margin)
            .desired_width(size[0])
            .frame(false)
            .layouter(&mut layouter),
    );
    if response.has_focus() {
        show_var_suggestions(ui, p, defined, text, &response, id);
    }
    response
}

fn show_var_suggestions(
    ui: &mut Ui,
    p: &Palette,
    defined: &std::collections::HashSet<String>,
    text: &mut String,
    response: &Response,
    id: egui::Id,
) {
    let Some((start, prefix)) = crate::vars::open_suggestion(text) else {
        return;
    };
    let needle = prefix.to_lowercase();
    let mut names: Vec<&String> = defined
        .iter()
        .filter(|name| name.to_lowercase().starts_with(&needle))
        .collect();
    if names.is_empty() {
        return;
    }
    names.sort();
    let mut chosen: Option<String> = None;
    let position = egui::pos2(response.rect.left(), response.rect.bottom() + 4.0);
    egui::Area::new(id.with("var_suggestions"))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.label(egui::RichText::new("Variables").size(9.0).color(p.faint));
                ui.add_space(2.0);
                for name in names.iter().take(8) {
                    let button = egui::Button::new(
                        egui::RichText::new(format!("{{{{{name}}}}}"))
                            .font(FontId::monospace(11.0)),
                    )
                    .frame(false);
                    if ui.add(button).clicked() {
                        chosen = Some((*name).clone());
                    }
                }
            });
        });
    if let Some(name) = chosen {
        text.truncate(start);
        text.push_str("{{");
        text.push_str(&name);
        text.push_str("}}");
        ui.ctx().memory_mut(|memory| memory.request_focus(response.id));
    }
}

/// Painted text with tracking, for small uppercase labels where a full label
/// widget is overkill. The text is drawn left-aligned and vertically centred on
/// `pos`.
pub fn tracked_text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    tracking: f32,
) {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: tracking,
            ..Default::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    let galley = painter.layout_job(job);
    painter.galley(
        pos - egui::vec2(0.0, galley.size().y * 0.5),
        galley,
        color,
    );
}

// ---------------------------------------------------------------------------
// Badges and buttons
// ---------------------------------------------------------------------------

pub fn method_badge(ui: &mut Ui, p: &Palette, method: Method) -> Response {
    let color = p.method_color(method);
    let font = FontId::new(9.0, crate::theme::mono_medium_family());
    let width = if method.as_str().len() > 6 {
        56.0
    } else {
        46.0
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), mix_soft(color, p));
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        method.as_str(),
        font,
        color,
    );
    resp
}

fn mix_soft(color: Color32, p: &Palette) -> Color32 {
    crate::theme::mix(color, p.panel, if p.dark { 0.86 } else { 0.9 })
}

pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
}

pub fn status_pill(ui: &mut Ui, p: &Palette, status: u16) -> Response {
    let color = p.status_color(status);
    let text = if reason_phrase(status).is_empty() {
        format!("{status}")
    } else {
        format!("{} {}", status, reason_phrase(status))
    };
    chip(ui, p, &text, color, mix_soft(color, p), true)
}

pub fn chip(
    ui: &mut Ui,
    _p: &Palette,
    text: &str,
    fg: Color32,
    bg: Color32,
    _bold: bool,
) -> Response {
    let font = FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), fg);
    let size = Vec2::new(galley.size().x + 14.0, 21.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(11.0),
        fg,
    );
    resp
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Btn {
    Primary,
    Secondary,
    Ghost,
    Danger,
    DangerGhost,
}

pub fn button(ui: &mut Ui, p: &Palette, label: &str, kind: Btn) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::PLACEHOLDER);
    let desired = Vec2::new(galley.size().x + 24.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let hovered = resp.hovered();
    let (bg, fg, stroke) = match kind {
        Btn::Primary => (
            if hovered { p.accent_hover } else { p.accent },
            p.on_accent,
            Stroke::NONE,
        ),
        Btn::Secondary => (
            if hovered { p.hover } else { p.panel },
            p.text,
            Stroke::new(1.0_f32, if hovered { p.border_strong } else { p.border }),
        ),
        Btn::Ghost => (
            if hovered {
                p.hover
            } else {
                Color32::TRANSPARENT
            },
            if hovered { p.text } else { p.muted },
            Stroke::NONE,
        ),
        Btn::Danger => (p.danger, Color32::WHITE, Stroke::NONE),
        Btn::DangerGhost => (
            if hovered {
                p.danger_soft
            } else {
                Color32::TRANSPARENT
            },
            p.danger,
            Stroke::NONE,
        ),
    };
    paint_rect(ui.painter(), rect, 7, bg, stroke);
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        label,
        font,
        fg,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

pub fn wide_button(ui: &mut Ui, p: &Palette, label: &str, kind: Btn) -> Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let hovered = resp.hovered();
    let (bg, fg, stroke) = match kind {
        Btn::Primary => (
            if hovered { p.accent_hover } else { p.accent },
            p.on_accent,
            Stroke::NONE,
        ),
        _ => (
            if hovered { p.hover } else { p.panel },
            p.text,
            Stroke::new(1.0_f32, p.border),
        ),
    };
    paint_rect(ui.painter(), rect, 7, bg, stroke);
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.5),
        fg,
    );
    resp
}

pub fn toggle_switch(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let bg = if *on { p.accent } else { p.border_strong };
    ui.painter().rect_filled(rect, CornerRadius::same(11), bg);
    let knob_x = if *on {
        rect.right() - 11.0
    } else {
        rect.left() + 11.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

pub fn segmented(ui: &mut Ui, p: &Palette, options: &[&str], selected: &mut usize) -> bool {
    let font = FontId::proportional(11.5);
    let widths: Vec<f32> = options
        .iter()
        .map(|opt| {
            ui.painter()
                .layout_no_wrap((*opt).to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
                + 18.0
        })
        .collect();
    let total = widths.iter().sum::<f32>() + 4.0 + 2.0 * (options.len().max(1) as f32 - 1.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 26.0), Sense::hover());
    paint_rect(
        ui.painter(),
        rect,
        7,
        p.panel_alt,
        Stroke::new(1.0_f32, p.border),
    );
    let base_id = ui
        .id()
        .with(("segmented", options.first().copied().unwrap_or("")));
    let mut changed = false;
    let mut x = rect.left() + 2.0;
    for (i, opt) in options.iter().enumerate() {
        let w = widths[i];
        let option_rect = Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), Vec2::new(w, 22.0));
        let resp = ui.interact(option_rect, base_id.with(i), Sense::click());
        let active = i == *selected;
        if active {
            paint_rect(
                ui.painter(),
                option_rect,
                5,
                p.panel,
                Stroke::new(1.0_f32, p.border),
            );
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(option_rect, CornerRadius::same(5), p.hover);
        }
        ui.painter().text(
            option_rect.center() + Vec2::new(0.0, 0.5),
            Align2::CENTER_CENTER,
            *opt,
            font.clone(),
            if active { p.text } else { p.muted },
        );
        if resp.clicked() && !active {
            *selected = i;
            changed = true;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        x += w + 2.0;
    }
    changed
}

// ---------------------------------------------------------------------------
// Key / value editor
// ---------------------------------------------------------------------------

pub fn kv_table(
    ui: &mut Ui,
    p: &Palette,
    defined: &std::collections::HashSet<String>,
    rows: &mut Vec<KV>,
    with_desc: bool,
    id_salt: &str,
) -> bool {
    let mut changed = false;
    let font = FontId::monospace(11.8);
    let width = ui.available_width();
    let del_w = 22.0;
    let check_w = 20.0;
    let key_w = 240.0_f32.min((width - 260.0).max(120.0));
    let val_w = 340.0_f32.min((width - key_w - 200.0).max(120.0));
    let desc_w = if with_desc {
        (width - key_w - val_w - del_w - check_w - 40.0).max(120.0)
    } else {
        0.0
    };

    // Header, painted at the exact offsets used by the editable rows.
    {
        let rect = ui.available_rect_before_wrap();
        let y = ui.cursor().top() + 10.0;
        let left = rect.left() + 40.0;
        let font = FontId::proportional(9.5);
        let paint = |x: f32, text: &str| {
            ui.painter().text(
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                text,
                font.clone(),
                p.faint,
            );
        };
        paint(left, "KEY");
        paint(left + key_w + 6.0, "VALUE");
        if with_desc {
            paint(left + key_w + 6.0 + val_w + 6.0, "DESCRIPTION");
        }
        let _ = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
    }

    let mut remove: Option<usize> = None;
    for (i, kv) in rows.iter_mut().enumerate() {
        let row_rect = ui.available_rect_before_wrap();
        let row_top = row_rect.top();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let mut check = kv.enabled;
            if ui
                .add(egui::Checkbox::without_text(&mut check))
                .on_hover_text("Include this entry")
                .changed()
            {
                kv.enabled = check;
                changed = true;
            }
            if var_singleline(
                ui,
                p,
                defined,
                &mut kv.key,
                font.clone(),
                "Key",
                [key_w, 26.0],
                false,
                egui::Margin::symmetric(5, 4),
                egui::Id::new((id_salt, "key", i)),
            )
            .changed()
            {
                changed = true;
            }
            if var_singleline(
                ui,
                p,
                defined,
                &mut kv.value,
                font.clone(),
                "Value",
                [val_w, 26.0],
                false,
                egui::Margin::symmetric(5, 4),
                egui::Id::new((id_salt, "value", i)),
            )
            .changed()
            {
                changed = true;
            }
            if with_desc
                && var_singleline(
                    ui,
                    p,
                    defined,
                    &mut kv.desc,
                    FontId::proportional(11.5),
                    "Description",
                    [desc_w, 26.0],
                    false,
                    egui::Margin::symmetric(5, 4),
                    egui::Id::new((id_salt, "desc", i)),
                )
                .changed()
            {
                changed = true;
            }
            let mut secret = kv.secret;
            if ui
                .add(egui::Checkbox::without_text(&mut secret))
                .on_hover_text("Keep this value in the macOS Keychain")
                .changed()
            {
                kv.secret = secret;
                changed = true;
            }
            if icon_button(ui, p, Icon::Trash, 20.0, "Remove").clicked() {
                remove = Some(i);
            }
        });
        if !kv.enabled {
            // Muted hint for disabled rows: the text fields stay editable but dim.
            let _ = i;
        }
        let _ = row_top;
        let y = ui.cursor().top();
        ui.painter().hline(
            row_rect.x_range(),
            y,
            Stroke::new(1.0_f32, p.border.gamma_multiply(0.75)),
        );
    }

    if let Some(i) = remove {
        if i < rows.len() {
            rows.remove(i);
            changed = true;
        }
    }

    ui.add_space(2.0);
    if button(ui, p, "+ Add row", Btn::Ghost).clicked() {
        rows.push(KV::new());
        changed = true;
    }
    let _ = id_salt;
    changed
}

// ---------------------------------------------------------------------------
// JSON highlighting
// ---------------------------------------------------------------------------

pub fn pretty_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

fn push_segment(job: &mut LayoutJob, text: &str, color: Color32, font: &FontId) {
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

fn push_json_line(job: &mut LayoutJob, line: &str, p: &Palette, font: &FontId) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let token: String = chars[start..i.min(chars.len())].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let is_key = j < chars.len() && chars[j] == ':';
            push_segment(
                job,
                &token,
                if is_key { p.json_key } else { p.json_string },
                font,
            );
        } else if ch.is_ascii_digit() || (ch == '-' && i + 1 < chars.len()) {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || chars[i] == '+'
                    || chars[i] == '-')
            {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            push_segment(job, &token, p.json_number, font);
        } else if ch.is_alphabetic() {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let color = match token.as_str() {
                "true" | "false" | "null" => p.json_literal,
                _ => p.text,
            };
            push_segment(job, &token, color, font);
        } else {
            let c = ch.to_string();
            push_segment(job, &c, p.muted, font);
            i += 1;
        }
    }
}

pub fn json_job(text: &str, p: &Palette, size: f32, line_numbers: bool) -> LayoutJob {
    let font = FontId::monospace(size);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for (idx, line) in text.split('\n').enumerate() {
        if line_numbers {
            push_segment(
                &mut job,
                &format!("{:>3}  ", idx + 1),
                p.faint.gamma_multiply(0.85),
                &font,
            );
        }
        push_json_line(&mut job, line, p, &font);
        push_segment(&mut job, "\n", p.text, &font);
    }
    job
}

pub fn code_view(ui: &mut Ui, p: &Palette, text: &str, line_numbers: bool, pretty: bool, id: &str) {
    let display = if pretty {
        pretty_json(text).unwrap_or_else(|| text.to_string())
    } else {
        text.to_string()
    };
    let lines: Vec<&str> = display.split('\n').collect();
    if lines.len() > 600 {
        // Huge payloads: paint only the visible lines instead of building a
        // galley for the whole document on every frame.
        let row_height = ui.fonts(|fonts| fonts.row_height(&FontId::monospace(12.0)));
        egui::ScrollArea::both()
            .id_salt(id)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, lines.len(), |ui, range| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for index in range {
                    let job = json_line_job(
                        lines[index],
                        if line_numbers { Some(index + 1) } else { None },
                        p,
                        12.0,
                    );
                    ui.add(
                        egui::Label::new(job)
                            .wrap_mode(egui::TextWrapMode::Extend)
                            .selectable(false),
                    );
                }
            });
        return;
    }
    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let job = json_job(&display, p, 12.0, line_numbers);
            ui.add(
                egui::Label::new(job)
                    .wrap_mode(egui::TextWrapMode::Extend)
                    .selectable(true),
            );
        });
}

fn json_line_job(line: &str, number: Option<usize>, p: &Palette, size: f32) -> LayoutJob {
    let font = FontId::monospace(size);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    if let Some(number) = number {
        push_segment(
            &mut job,
            &format!("{number:>3}  "),
            p.faint.gamma_multiply(0.85),
            &font,
        );
    }
    push_json_line(&mut job, line, p, &font);
    job
}

// ---------------------------------------------------------------------------
// Tables, cards, empty states
// ---------------------------------------------------------------------------

pub fn card_frame(p: &Palette) -> egui::Frame {
    egui::Frame::NONE
        .fill(p.panel)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(egui::Margin::same(16))
}

pub fn section_label(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .family(crate::theme::medium_family())
            .size(9.5)
            .extra_letter_spacing(0.9)
            .color(p.faint),
    );
}

pub fn column_label(ui: &mut Ui, width: f32, text: &str, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    tracked_text(
        ui.painter(),
        egui::pos2(rect.left(), rect.center().y),
        &text.to_uppercase(),
        crate::theme::medium(9.5),
        p.faint,
        0.8,
    );
}

pub fn table_header(ui: &mut Ui, p: &Palette, columns: &[&str], widths: &[f32]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, col) in columns.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(120.0);
            column_label(ui, w, col, p);
        }
    });
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, p.border));
    ui.add_space(2.0);
}

pub fn header_rows(ui: &mut Ui, p: &Palette, headers: &[(String, String)]) {
    let width = ui.available_width();
    for (k, v) in headers {
        ui.horizontal(|ui| {
            ui.add_sized(
                [width * 0.34, 22.0],
                egui::Label::new(egui::RichText::new(k).size(12.0).color(p.text).monospace()),
            );
            ui.add_sized(
                [width * 0.66, 22.0],
                egui::Label::new(egui::RichText::new(v).size(12.0).color(p.muted).monospace()),
            );
        });
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            Stroke::new(1.0_f32, p.border.gamma_multiply(0.7)),
        );
    }
}

pub fn empty_state(ui: &mut Ui, p: &Palette, icon: Icon, title: &str, body: &str, min_height: f32) {
    let height = ui.available_height().max(min_height);
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), height),
        Layout::top_down(Align::Center),
        |ui| {
            ui.add_space((height * 0.28).clamp(28.0, 180.0));
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(12), p.panel_alt);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(12),
                Stroke::new(1.0_f32, p.border),
                StrokeKind::Inside,
            );
            paint_icon(ui.painter(), rect.shrink(12.0), icon, p.faint, 1.5);
            ui.add_space(12.0);
            ui.label(egui::RichText::new(title).size(14.0).strong().color(p.text));
            ui.add_space(3.0);
            ui.label(egui::RichText::new(body).size(12.0).color(p.muted));
        },
    );
}

pub fn divider(ui: &mut Ui, p: &Palette) {
    let rect = ui.available_rect_before_wrap();
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0_f32, p.border));
    ui.add_space(1.0);
}

pub fn capture_row(
    ui: &mut Ui,
    p: &Palette,
    index: usize,
    capture: &mut Capture,
    can_remove: bool,
) -> (bool, bool) {
    let mut changed = false;
    let mut removed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut check = capture.enabled;
        if ui.add(egui::Checkbox::without_text(&mut check)).changed() {
            capture.enabled = check;
            changed = true;
        }
        egui::ComboBox::from_id_salt(("capture_kind", index))
            .width(108.0)
            .selected_text(egui::RichText::new(capture.source.label()).size(12.0))
            .show_ui(ui, |ui| {
                for source in [CaptureSource::JsonField, CaptureSource::Header] {
                    if ui
                        .selectable_label(capture.source == source, source.label())
                        .clicked()
                    {
                        capture.source = source;
                        changed = true;
                    }
                }
            });
        if ui
            .add_sized(
                [250.0, 26.0],
                TextEdit::singleline(&mut capture.path)
                    .hint_text(capture.source.placeholder())
                    .font(FontId::monospace(11.5))
                    .margin(egui::Margin::symmetric(6, 4)),
            )
            .on_hover_text("JSON path (data.token, $.data[0].id) or header name")
            .changed()
        {
            changed = true;
        }
        ui.label(egui::RichText::new("→").size(12.0).color(p.faint));
        if ui
            .add_sized(
                [170.0, 26.0],
                TextEdit::singleline(&mut capture.variable)
                    .hint_text("variable")
                    .font(FontId::monospace(11.5))
                    .margin(egui::Margin::symmetric(6, 4)),
            )
            .on_hover_text("Referenced later as {{variable}}")
            .changed()
        {
            changed = true;
        }
        if can_remove && icon_button(ui, p, Icon::Trash, 20.0, "Remove").clicked() {
            removed = true;
        }
    });
    (changed, removed)
}

pub fn assertion_row(
    ui: &mut Ui,
    p: &Palette,
    index: usize,
    assertion: &mut Assertion,
    can_remove: bool,
) -> (bool, bool) {
    let mut changed = false;
    let mut removed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut check = assertion.enabled;
        if ui.add(egui::Checkbox::without_text(&mut check)).changed() {
            assertion.enabled = check;
            changed = true;
        }
        egui::ComboBox::from_id_salt(("assert_kind", index))
            .width(168.0)
            .selected_text(egui::RichText::new(assertion.kind.label()).size(12.0))
            .show_ui(ui, |ui| {
                for kind in crate::model::AssertKind::ALL {
                    if ui
                        .selectable_label(assertion.kind == kind, kind.label())
                        .clicked()
                    {
                        assertion.kind = kind;
                        changed = true;
                    }
                }
            });
        let (target_hint, expected_hint) = assertion.kind.placeholder();
        if !target_hint.is_empty() {
            if ui
                .add_sized(
                    [150.0, 26.0],
                    TextEdit::singleline(&mut assertion.target)
                        .hint_text(target_hint)
                        .font(FontId::monospace(11.5))
                        .margin(egui::Margin::symmetric(6, 4)),
                )
                .changed()
            {
                changed = true;
            }
        }
        if ui
            .add_sized(
                [150.0, 26.0],
                TextEdit::singleline(&mut assertion.expected)
                    .hint_text(expected_hint)
                    .font(FontId::monospace(11.5))
                    .margin(egui::Margin::symmetric(6, 4)),
            )
            .changed()
        {
            changed = true;
        }
        if can_remove && icon_button(ui, p, Icon::Trash, 20.0, "Remove").clicked() {
            removed = true;
        }
    });
    (changed, removed)
}
