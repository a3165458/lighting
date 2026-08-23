//! Light visual language for the host window: palette, card container, iOS-style
//! switch and the small vector glyphs used as row/tile icons.
//!
//! Icons are painted rather than shipped as an icon font so the window keeps
//! working on a bare Windows install with no extra assets.

use egui::{
    epaint::PathShape, Color32, CornerRadius, FontFamily, FontId, Margin, Pos2, Rect, Stroke,
    StrokeKind, Vec2,
};

pub const BG: Color32 = Color32::from_rgb(0xF5, 0xF4, 0xFA);
pub const CARD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const CARD_LINE: Color32 = Color32::from_rgb(0xEA, 0xE7, 0xF5);
pub const TEXT: Color32 = Color32::from_rgb(0x1D, 0x1B, 0x2C);
pub const MUTED: Color32 = Color32::from_rgb(0x6D, 0x69, 0x84);
pub const DIM: Color32 = Color32::from_rgb(0x9C, 0x97, 0xAF);
pub const ACCENT: Color32 = Color32::from_rgb(0x6C, 0x4C, 0xE0);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x59, 0x3B, 0xCE);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xF1, 0xED, 0xFE);
pub const ACCENT_LINE: Color32 = Color32::from_rgb(0xD8, 0xCD, 0xFB);
pub const OK: Color32 = Color32::from_rgb(0x1D, 0xA1, 0x5A);
pub const OK_SOFT: Color32 = Color32::from_rgb(0xE6, 0xF7, 0xEE);
pub const WARN: Color32 = Color32::from_rgb(0xB3, 0x76, 0x0A);
pub const BAD: Color32 = Color32::from_rgb(0xD2, 0x43, 0x43);
pub const TRACK: Color32 = Color32::from_rgb(0xE4, 0xE1, 0xF0);

const BOLD_FAMILY: &str = "cjk_bold";

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()))
}

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.faint_bg_color = ACCENT_SOFT;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.error_fg_color = BAD;
    visuals.warn_fg_color = WARN;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_stroke = Stroke::new(1.0_f32, CARD_LINE);

    visuals.widgets.noninteractive.bg_fill = CARD;
    visuals.widgets.noninteractive.weak_bg_fill = CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, CARD_LINE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);

    visuals.widgets.inactive.bg_fill = ACCENT_SOFT;
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xF4, 0xF2, 0xFC);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, CARD_LINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    visuals.widgets.hovered.bg_fill = ACCENT_SOFT;
    visuals.widgets.hovered.weak_bg_fill = ACCENT_SOFT;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_LINE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, ACCENT_DARK);

    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT_SOFT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, ACCENT_DARK);

    visuals.widgets.open.bg_fill = Color32::WHITE;
    visuals.widgets.open.weak_bg_fill = Color32::WHITE;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT_LINE);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);

    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(8);
        w.expansion = 0.0;
    }

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.text_styles = [
        (egui::TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
        (egui::TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (egui::TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
        (egui::TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (egui::TextStyle::Monospace, FontId::new(11.5, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.interact_size = Vec2::new(36.0, 22.0);
    style.spacing.icon_width = 16.0;
    style.spacing.combo_width = 200.0;
    style.spacing.slider_rail_height = 5.0;
    style.spacing.scroll.bar_width = 8.0;
    ctx.set_style(style);
}

fn install_fonts(ctx: &egui::Context) {
    // Last entries are Linux paths so the same UI can be rendered for review on
    // a build machine without Windows fonts.
    let regular = read_first(&[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\NotoSansSC-Regular.otf",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ]);
    let heavy = read_first(&[
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\msyhbd.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    ]);
    if regular.is_none() && heavy.is_none() {
        tracing::warn!("未找到中文字体，界面汉字可能显示为方框");
    }

    let mut fonts = egui::FontDefinitions::default();
    let mut bold_family: Vec<String> = Vec::new();
    if let Some(bytes) = heavy {
        fonts.font_data.insert(
            "cjk_heavy".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        bold_family.push("cjk_heavy".into());
    }
    if let Some(bytes) = regular {
        fonts.font_data.insert(
            "cjk".into(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.insert(0, "cjk".into());
            }
        }
        bold_family.push("cjk".into());
    }
    if let Some(list) = fonts.families.get(&FontFamily::Proportional) {
        for name in list {
            if !bold_family.contains(name) {
                bold_family.push(name.clone());
            }
        }
    }
    fonts
        .families
        .insert(FontFamily::Name(BOLD_FAMILY.into()), bold_family);
    ctx.set_fonts(fonts);
}

fn read_first(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, CARD_LINE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(14, 12))
        .shadow(egui::epaint::Shadow {
            offset: [0, 1],
            blur: 5,
            spread: 0,
            color: Color32::from_black_alpha(8),
        })
}

/// One titled section. Titles use the section wording from the design, so pass
/// an empty string for untitled cards.
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = card_frame()
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if !title.is_empty() {
                ui.label(egui::RichText::new(title).font(bold(13.0)).color(TEXT));
                ui.add_space(6.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(10.0);
    inner
}

pub fn switch(ui: &mut egui::Ui, on: &mut bool, enabled: bool) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(38.0, 21.0), sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let track = if !enabled {
        TRACK
    } else {
        mix(TRACK, ACCENT, t)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(11), track);
    let cx = egui::lerp((rect.left() + 10.5)..=(rect.right() - 10.5), t);
    let knob = if enabled {
        Color32::WHITE
    } else {
        Color32::from_rgb(0xF6, 0xF5, 0xFA)
    };
    painter.circle_filled(Pos2::new(cx, rect.center().y), 8.0, knob);
    response
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let a = egui::Rgba::from(a);
    let b = egui::Rgba::from(b);
    Color32::from(a * (1.0 - t) + b * t)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Monitor,
    Gauge,
    Film,
    Meter,
    Route,
    Chip,
    Bolt,
    Target,
    Touch,
    Keyboard,
    Clipboard,
    Speaker,
    Gear,
    Info,
    Check,
    Wifi,
    Plug,
}

/// Allocate a square and paint `glyph` into it.
pub fn glyph(ui: &mut egui::Ui, g: Glyph, color: Color32, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_glyph(ui.painter(), rect, g, color);
    response
}

/// Glyph inside a soft round badge, used by the connection card and metric tiles.
pub fn glyph_badge(ui: &mut egui::Ui, g: Glyph, color: Color32, bg: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, bg);
    paint_glyph(ui.painter(), rect.shrink(size * 0.26), g, color);
}

pub fn paint_glyph(painter: &egui::Painter, rect: Rect, g: Glyph, color: Color32) {
    let s = Stroke::new((rect.width() / 12.0).clamp(1.1, 1.8), color);
    let r = rect.shrink(rect.width() * 0.08);
    let c = r.center();
    let w = r.width();
    match g {
        Glyph::Monitor => {
            let screen = Rect::from_min_max(r.left_top(), Pos2::new(r.right(), r.bottom() - w * 0.2));
            painter.rect_stroke(screen, CornerRadius::same(2), s, StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.18, r.bottom()),
                    Pos2::new(c.x + w * 0.18, r.bottom()),
                ],
                s,
            );
            painter.line_segment([Pos2::new(c.x, screen.bottom()), Pos2::new(c.x, r.bottom())], s);
        }
        Glyph::Gauge => {
            painter.circle_stroke(c, w * 0.42, s);
            painter.line_segment([c, Pos2::new(c.x + w * 0.26, c.y - w * 0.26)], s);
        }
        Glyph::Film => {
            painter.rect_stroke(r, CornerRadius::same(2), s, StrokeKind::Inside);
            for i in 0..2 {
                let x = r.left() + w * (0.32 + 0.36 * i as f32);
                painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], s);
            }
        }
        Glyph::Meter => {
            for i in 0..3 {
                let x = r.left() + w * (0.2 + 0.3 * i as f32);
                let h = w * (0.3 + 0.24 * i as f32);
                painter.line_segment([Pos2::new(x, r.bottom()), Pos2::new(x, r.bottom() - h)], s);
            }
        }
        Glyph::Route => {
            painter.circle_stroke(Pos2::new(r.left() + w * 0.18, r.bottom() - w * 0.18), w * 0.16, s);
            painter.circle_stroke(Pos2::new(r.right() - w * 0.18, r.top() + w * 0.18), w * 0.16, s);
            painter.line_segment(
                [
                    Pos2::new(r.left() + w * 0.18, r.bottom() - w * 0.4),
                    Pos2::new(r.left() + w * 0.18, r.top() + w * 0.18),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(r.left() + w * 0.34, r.top() + w * 0.18),
                    Pos2::new(r.right() - w * 0.34, r.top() + w * 0.18),
                ],
                s,
            );
        }
        Glyph::Chip => {
            painter.rect_stroke(r.shrink(w * 0.16), CornerRadius::same(2), s, StrokeKind::Inside);
            for i in 0..2 {
                let x = r.left() + w * (0.36 + 0.28 * i as f32);
                painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.top() + w * 0.16)], s);
                painter.line_segment(
                    [Pos2::new(x, r.bottom() - w * 0.16), Pos2::new(x, r.bottom())],
                    s,
                );
            }
        }
        Glyph::Bolt => {
            let points = vec![
                Pos2::new(c.x + w * 0.16, r.top()),
                Pos2::new(c.x - w * 0.2, c.y + w * 0.04),
                Pos2::new(c.x + w * 0.02, c.y + w * 0.04),
                Pos2::new(c.x - w * 0.14, r.bottom()),
                Pos2::new(c.x + w * 0.22, c.y - w * 0.06),
                Pos2::new(c.x, c.y - w * 0.06),
            ];
            painter.add(PathShape::convex_polygon(points, color, Stroke::NONE));
        }
        Glyph::Target => {
            painter.circle_stroke(c, w * 0.42, s);
            painter.circle_filled(c, w * 0.12, color);
        }
        Glyph::Touch => {
            // Mouse pointer: concave, so stroke the outline instead of filling.
            let p = |x: f32, y: f32| Pos2::new(r.left() + w * x, r.top() + w * y);
            painter.add(PathShape::closed_line(
                vec![
                    p(0.30, 0.06),
                    p(0.30, 0.80),
                    p(0.46, 0.62),
                    p(0.58, 0.94),
                    p(0.72, 0.87),
                    p(0.59, 0.56),
                    p(0.80, 0.50),
                ],
                s,
            ));
        }
        Glyph::Keyboard => {
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(r.left(), r.top() + w * 0.16),
                    Pos2::new(r.right(), r.bottom() - w * 0.16),
                ),
                CornerRadius::same(2),
                s,
                StrokeKind::Inside,
            );
            for i in 0..3 {
                let x = r.left() + w * (0.26 + 0.24 * i as f32);
                painter.circle_filled(Pos2::new(x, c.y - w * 0.08), w * 0.05, color);
            }
            painter.line_segment(
                [
                    Pos2::new(r.left() + w * 0.28, c.y + w * 0.14),
                    Pos2::new(r.right() - w * 0.28, c.y + w * 0.14),
                ],
                s,
            );
        }
        Glyph::Clipboard => {
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(r.left() + w * 0.14, r.top() + w * 0.12),
                    Pos2::new(r.right() - w * 0.14, r.bottom()),
                ),
                CornerRadius::same(2),
                s,
                StrokeKind::Inside,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(c.x - w * 0.16, r.top()),
                    Pos2::new(c.x + w * 0.16, r.top() + w * 0.16),
                ),
                CornerRadius::same(1),
                color,
            );
        }
        Glyph::Speaker => {
            let points = vec![
                Pos2::new(r.left() + w * 0.1, c.y - w * 0.14),
                Pos2::new(r.left() + w * 0.3, c.y - w * 0.14),
                Pos2::new(r.left() + w * 0.52, r.top() + w * 0.08),
                Pos2::new(r.left() + w * 0.52, r.bottom() - w * 0.08),
                Pos2::new(r.left() + w * 0.3, c.y + w * 0.14),
                Pos2::new(r.left() + w * 0.1, c.y + w * 0.14),
            ];
            painter.add(PathShape::convex_polygon(points, color, Stroke::NONE));
            arc(painter, Pos2::new(r.left() + w * 0.52, c.y), w * 0.24, -1.0, 1.0, s);
            arc(painter, Pos2::new(r.left() + w * 0.52, c.y), w * 0.42, -1.0, 1.0, s);
        }
        Glyph::Gear => {
            painter.circle_stroke(c, w * 0.24, s);
            for i in 0..6 {
                let a = i as f32 * std::f32::consts::TAU / 6.0;
                let (sin, cos) = a.sin_cos();
                painter.line_segment(
                    [
                        Pos2::new(c.x + cos * w * 0.32, c.y + sin * w * 0.32),
                        Pos2::new(c.x + cos * w * 0.46, c.y + sin * w * 0.46),
                    ],
                    s,
                );
            }
        }
        Glyph::Info => {
            painter.circle_stroke(c, w * 0.44, s);
            painter.circle_filled(Pos2::new(c.x, c.y - w * 0.2), w * 0.06, color);
            painter.line_segment(
                [
                    Pos2::new(c.x, c.y - w * 0.04),
                    Pos2::new(c.x, c.y + w * 0.22),
                ],
                s,
            );
        }
        Glyph::Check => {
            painter.line_segment(
                [
                    Pos2::new(r.left() + w * 0.16, c.y + w * 0.02),
                    Pos2::new(c.x - w * 0.06, r.bottom() - w * 0.18),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.06, r.bottom() - w * 0.18),
                    Pos2::new(r.right() - w * 0.12, r.top() + w * 0.18),
                ],
                s,
            );
        }
        Glyph::Wifi => {
            let base = Pos2::new(c.x, r.bottom() - w * 0.12);
            painter.circle_filled(base, w * 0.07, color);
            arc(painter, base, w * 0.28, -2.36, -0.78, s);
            arc(painter, base, w * 0.48, -2.36, -0.78, s);
            arc(painter, base, w * 0.68, -2.36, -0.78, s);
        }
        Glyph::Plug => {
            painter.line_segment([Pos2::new(c.x, r.bottom()), Pos2::new(c.x, c.y + w * 0.08)], s);
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(c.x - w * 0.24, c.y - w * 0.16),
                    Pos2::new(c.x + w * 0.24, c.y + w * 0.1),
                ),
                CornerRadius::same(2),
                s,
                StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.12, r.top()),
                    Pos2::new(c.x - w * 0.12, c.y - w * 0.16),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + w * 0.12, r.top()),
                    Pos2::new(c.x + w * 0.12, c.y - w * 0.16),
                ],
                s,
            );
        }
    }
}

fn arc(painter: &egui::Painter, center: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) {
    const STEPS: usize = 14;
    let points: Vec<Pos2> = (0..=STEPS)
        .map(|i| {
            let a = from + (to - from) * i as f32 / STEPS as f32;
            let (sin, cos) = a.sin_cos();
            Pos2::new(center.x + cos * radius, center.y + sin * radius)
        })
        .collect();
    painter.add(PathShape::line(points, stroke));
}

/// Laptop + tablet artwork for the hero header.
pub fn hero_art(painter: &egui::Painter, rect: Rect) {
    let w = rect.width();
    let h = rect.height();
    let screen = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + h * 0.04),
        Vec2::new(w * 0.60, h * 0.58),
    );
    painter.rect(
        screen,
        CornerRadius::same(4),
        Color32::WHITE,
        Stroke::new(1.4_f32, ACCENT_LINE),
        StrokeKind::Inside,
    );
    let inner = screen.shrink(3.0);
    painter.rect_filled(inner, CornerRadius::same(3), mix(ACCENT_SOFT, ACCENT, 0.16));
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(inner.left(), inner.center().y + h * 0.04),
            inner.right_bottom(),
        ),
        CornerRadius::same(3),
        mix(ACCENT_SOFT, ACCENT, 0.36),
    );
    let base = Rect::from_min_max(
        Pos2::new(screen.left() - w * 0.07, screen.bottom() + 1.5),
        Pos2::new(screen.right() + w * 0.07, screen.bottom() + h * 0.13),
    );
    painter.rect(
        base,
        CornerRadius::same(3),
        mix(ACCENT_SOFT, ACCENT, 0.10),
        Stroke::new(1.0_f32, ACCENT_LINE),
        StrokeKind::Inside,
    );

    let tablet = Rect::from_min_size(
        Pos2::new(rect.left() + w * 0.54, rect.top() + h * 0.20),
        Vec2::new(w * 0.44, h * 0.76),
    );
    // White halo so the tablet reads as being in front of the laptop.
    painter.rect_filled(tablet.expand(3.0), CornerRadius::same(8), BG);
    painter.rect(
        tablet,
        CornerRadius::same(6),
        Color32::WHITE,
        Stroke::new(1.4_f32, ACCENT),
        StrokeKind::Inside,
    );
    let tablet_inner = tablet.shrink(4.0);
    painter.rect_filled(tablet_inner, CornerRadius::same(4), ACCENT_SOFT);
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(tablet_inner.left(), tablet_inner.center().y + h * 0.08),
            tablet_inner.right_bottom(),
        ),
        CornerRadius::same(4),
        mix(ACCENT_SOFT, ACCENT, 0.45),
    );
    painter.circle_filled(
        Pos2::new(tablet.center().x, tablet.bottom() - 2.5),
        1.4,
        ACCENT_LINE,
    );
}
