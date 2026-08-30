//! Light visual language for the host window: palette, card container, iOS-style
//! switch and the small vector glyphs used as row/tile icons.
//!
//! Icons are painted rather than shipped as an icon font so the window keeps
//! working on a bare Windows install with no extra assets.

use egui::{
    epaint::PathShape, Color32, CornerRadius, FontFamily, FontId, Margin, Pos2, Rect, Stroke,
    StrokeKind, Vec2,
};

// Palette sampled from the product mockup (669×1024).
pub const BG: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xFC);
pub const BG_TOP: Color32 = Color32::from_rgb(0xF7, 0xF3, 0xFF);
pub const BG_BOTTOM: Color32 = Color32::from_rgb(0xE8, 0xEC, 0xFB);
pub const CARD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const CARD_LINE: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xF5);
pub const TILE: Color32 = Color32::from_rgb(0xF0, 0xF4, 0xFD);
pub const TEXT: Color32 = Color32::from_rgb(0x19, 0x1E, 0x32);
pub const MUTED: Color32 = Color32::from_rgb(0x6A, 0x6A, 0x74);
pub const DIM: Color32 = Color32::from_rgb(0x9A, 0x9A, 0xA8);
pub const ACCENT: Color32 = Color32::from_rgb(0x73, 0x57, 0xFA);
pub const ACCENT_MID: Color32 = Color32::from_rgb(0x78, 0x6C, 0xFD);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x7C, 0x7A, 0xFF);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0x5F, 0x45, 0xE8);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xEF, 0xEC, 0xFF);
pub const ACCENT_LINE: Color32 = Color32::from_rgb(0xD4, 0xD0, 0xFB);
pub const ACCENT_MUTED: Color32 = Color32::from_rgb(0x86, 0x7B, 0xCA);
pub const LINK: Color32 = Color32::from_rgb(0x53, 0x3F, 0xBC);
pub const OK: Color32 = Color32::from_rgb(0x27, 0xAC, 0x7F);
pub const OK_SOFT: Color32 = Color32::from_rgb(0xD2, 0xF3, 0xEC);
pub const WARN: Color32 = Color32::from_rgb(0xB3, 0x76, 0x0A);
pub const BAD: Color32 = Color32::from_rgb(0xD2, 0x43, 0x43);
pub const TRACK: Color32 = Color32::from_rgb(0xE4, 0xE6, 0xF2);
pub const STATUS_STRIP: Color32 = Color32::from_rgb(0xE8, 0xEC, 0xFB);

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
    visuals.window_corner_radius = CornerRadius::same(16);
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
    style.spacing.interact_size = Vec2::new(40.0, 24.0);
    style.spacing.icon_width = 16.0;
    style.spacing.combo_width = 200.0;
    style.spacing.slider_rail_height = 8.0;
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating_allocated_width = 0.0;
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
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(18, 14))
        .outer_margin(Margin::symmetric(0, 0))
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(12),
        })
}

/// One titled section. Titles use the section wording from the design, so pass
/// an empty string for untitled cards.
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let inner = card_frame()
        .show(ui, |ui| {
            // Lock inner width so combo/slider children cannot grow past the
            // page gutter (which was collapsing the right margin).
            let w = ui.available_width();
            ui.set_width(w);
            ui.set_max_width(w);
            if !title.is_empty() {
                ui.label(egui::RichText::new(title).font(bold(15.5)).color(TEXT));
                ui.add_space(10.0);
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
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(44.0, 26.0), sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.14);
    let track = if !enabled {
        TRACK
    } else {
        mix(TRACK, ACCENT, t)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(13), track);
    let cx = egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), t);
    let knob = if enabled {
        Color32::WHITE
    } else {
        Color32::from_rgb(0xF6, 0xF5, 0xFA)
    };
    // Soft knob shadow for depth matching the mockup.
    painter.circle_filled(Pos2::new(cx, rect.center().y + 0.6), 10.2, Color32::from_black_alpha(18));
    painter.circle_filled(Pos2::new(cx, rect.center().y), 10.0, knob);
    response
}

/// Horizontal purple gradient used by the primary share button.
pub fn paint_h_gradient(painter: &egui::Painter, rect: Rect, left: Color32, right: Color32, radius: u8) {
    const STEPS: i32 = 28;
    let w = rect.width();
    for i in 0..STEPS {
        let t0 = i as f32 / STEPS as f32;
        let t1 = (i + 1) as f32 / STEPS as f32;
        let color = mix(left, right, (t0 + t1) * 0.5);
        let slice = Rect::from_min_max(
            Pos2::new(rect.left() + w * t0, rect.top()),
            Pos2::new(rect.left() + w * t1 + 0.5, rect.bottom()),
        );
        let rad = if i == 0 {
            CornerRadius {
                nw: radius,
                ne: 0,
                sw: radius,
                se: 0,
            }
        } else if i == STEPS - 1 {
            CornerRadius {
                nw: 0,
                ne: radius,
                sw: 0,
                se: radius,
            }
        } else {
            CornerRadius::ZERO
        };
        painter.rect_filled(slice, rad, color);
    }
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
    Tablet,
    Share,
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
            // Landscape image / 画质.
            painter.rect_stroke(r, CornerRadius::same(2), s, StrokeKind::Inside);
            painter.circle_filled(Pos2::new(r.left() + w * 0.30, r.top() + w * 0.32), w * 0.08, color);
            painter.add(PathShape::convex_polygon(
                vec![
                    Pos2::new(r.left() + w * 0.12, r.bottom() - w * 0.18),
                    Pos2::new(r.left() + w * 0.42, c.y + w * 0.02),
                    Pos2::new(r.left() + w * 0.62, r.bottom() - w * 0.22),
                    Pos2::new(r.right() - w * 0.12, r.bottom() - w * 0.12),
                    Pos2::new(r.left() + w * 0.12, r.bottom() - w * 0.12),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Film => {
            // Signal bars / 帧率.
            for (i, h_ratio) in [0.34_f32, 0.55, 0.78, 1.0].into_iter().enumerate() {
                let x = r.left() + w * (0.14 + 0.22 * i as f32);
                let top = r.bottom() - w * 0.12 - w * 0.62 * h_ratio;
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(x, top),
                        Pos2::new(x + w * 0.14, r.bottom() - w * 0.12),
                    ),
                    CornerRadius::same(2),
                    color,
                );
            }
        }
        Glyph::Meter => {
            // Waveform / 码率.
            let pts = [
                (0.08, 0.55),
                (0.22, 0.28),
                (0.38, 0.68),
                (0.54, 0.22),
                (0.70, 0.58),
                (0.86, 0.34),
                (0.96, 0.48),
            ];
            let points: Vec<Pos2> = pts
                .into_iter()
                .map(|(x, y)| Pos2::new(r.left() + w * x, r.top() + w * y))
                .collect();
            painter.add(PathShape::line(points, s));
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
        Glyph::Tablet => {
            // Landscape phone / tablet silhouette.
            let body = Rect::from_min_max(
                Pos2::new(c.x - w * 0.30, r.top() + w * 0.06),
                Pos2::new(c.x + w * 0.30, r.bottom() - w * 0.06),
            );
            painter.rect_stroke(body, CornerRadius::same(3), s, StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(c.x - w * 0.10, body.bottom() - w * 0.10),
                    Pos2::new(c.x + w * 0.10, body.bottom() - w * 0.10),
                ],
                s,
            );
            painter.rect_filled(
                Rect::from_center_size(
                    Pos2::new(c.x, body.top() + w * 0.10),
                    Vec2::new(w * 0.10, w * 0.04),
                ),
                CornerRadius::same(1),
                color,
            );
        }
        Glyph::Share => {
            // Dual screens / cast icon for the primary CTA.
            let back = Rect::from_min_max(
                Pos2::new(r.left() + w * 0.08, r.top() + w * 0.12),
                Pos2::new(r.right() - w * 0.22, r.bottom() - w * 0.28),
            );
            let front = Rect::from_min_max(
                Pos2::new(r.left() + w * 0.28, r.top() + w * 0.30),
                Pos2::new(r.right() - w * 0.08, r.bottom() - w * 0.10),
            );
            painter.rect_stroke(back, CornerRadius::same(2), s, StrokeKind::Inside);
            painter.rect_filled(front, CornerRadius::same(2), color);
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
///
/// Drawn as vectors (not a cropped bitmap) so the full laptop — including its
/// left edge — always stays inside the allocated rect.
pub fn hero_art(ui: &mut egui::Ui, rect: Rect) {
    paint_hero(ui.painter().with_clip_rect(rect), rect);
}

pub fn paint_hero(painter: egui::Painter, rect: Rect) {
    paint_hero_devices(&painter, rect);
}

fn paint_hero_devices(painter: &egui::Painter, rect: Rect) {
    // Absolute insets so the full laptop (rounded left + base) always has
    // breathing room inside the clip, even when the art slot is narrow.
    let pad_l = 18.0_f32;
    let pad_r = 10.0_f32;
    let pad_y = 8.0_f32;
    let area = Rect::from_min_max(
        Pos2::new(rect.left() + pad_l, rect.top() + pad_y),
        Pos2::new(rect.right() - pad_r, rect.bottom() - pad_y),
    );
    let w = area.width().max(1.0);
    let h = area.height().max(1.0);

    painter.circle_filled(
        Pos2::new(area.right() - w * 0.04, area.top() + h * 0.12),
        w * 0.20,
        mix(ACCENT_SOFT, Color32::WHITE, 0.22),
    );

    // Laptop sits in the left-center of `area` — left edge = area.left (already padded).
    let laptop = Rect::from_min_size(
        Pos2::new(area.left(), area.top() + h * 0.26),
        Vec2::new(w * 0.58, h * 0.46),
    );
    painter.rect_filled(
        laptop.translate(Vec2::new(3.0, 4.0)),
        CornerRadius::same(11),
        Color32::from_rgba_unmultiplied(0x73, 0x57, 0xFA, 28),
    );
    painter.rect_filled(laptop, CornerRadius::same(10), Color32::from_rgb(0xC5, 0xC7, 0xD8));
    painter.rect_stroke(
        laptop,
        CornerRadius::same(10),
        Stroke::new(1.6_f32, Color32::from_rgb(0x8E, 0x90, 0xA8)),
        StrokeKind::Inside,
    );
    let lid = laptop.shrink2(Vec2::new(6.0, 7.0));
    paint_wallpaper(painter, lid, CornerRadius::same(5));
    painter.circle_filled(
        Pos2::new(laptop.center().x, laptop.top() + 3.5),
        1.3,
        mix(ACCENT_LINE, Color32::WHITE, 0.35),
    );
    // Base flush with laptop left; only extends to the right.
    let base = Rect::from_min_size(
        Pos2::new(laptop.left(), laptop.bottom() + 2.0),
        Vec2::new(laptop.width() + 8.0, h * 0.05),
    );
    painter.rect_filled(base, CornerRadius::same(3), Color32::from_rgb(0xB0, 0xB2, 0xC4));
    painter.rect_filled(
        Rect::from_center_size(
            Pos2::new(laptop.center().x, base.center().y),
            Vec2::new(laptop.width() * 0.18, base.height() * 0.35),
        ),
        CornerRadius::same(2),
        mix(Color32::WHITE, ACCENT_LINE, 0.45),
    );

    let tablet = Rect::from_min_size(
        Pos2::new(area.left() + w * 0.48, area.top() + h * 0.02),
        Vec2::new(w * 0.50, h * 0.90),
    );
    painter.rect_filled(
        tablet.translate(Vec2::new(2.5, 3.5)),
        CornerRadius::same(15),
        Color32::from_rgba_unmultiplied(0x73, 0x57, 0xFA, 40),
    );
    painter.rect_filled(tablet, CornerRadius::same(14), Color32::WHITE);
    painter.rect_stroke(
        tablet,
        CornerRadius::same(14),
        Stroke::new(1.3_f32, mix(ACCENT, ACCENT_LINE, 0.35)),
        StrokeKind::Inside,
    );
    let screen = tablet.shrink2(Vec2::new(6.0, 9.0));
    paint_wallpaper(painter, screen, CornerRadius::same(8));
    painter.rect_filled(
        Rect::from_center_size(
            Pos2::new(tablet.center().x, tablet.bottom() - 5.5),
            Vec2::new(tablet.width() * 0.20, 2.2),
        ),
        CornerRadius::same(1),
        mix(ACCENT_LINE, ACCENT, 0.4),
    );
}

fn paint_wallpaper(painter: &egui::Painter, rect: Rect, radius: CornerRadius) {
    painter.rect_filled(rect, radius, Color32::from_rgb(0x4F, 0x3F, 0xC8));
    // Clip blobs to the screen so they cannot paint outside and look "cropped".
    let clipped = painter.with_clip_rect(rect);
    let w = rect.width();
    let h = rect.height();
    clipped.circle_filled(
        Pos2::new(rect.left() + w * 0.05, rect.top() + h * 0.15),
        w * 0.55,
        Color32::from_rgba_unmultiplied(0x8B, 0x7A, 0xFF, 140),
    );
    clipped.circle_filled(
        Pos2::new(rect.right() + w * 0.05, rect.top() + h * 0.62),
        w * 0.62,
        Color32::from_rgba_unmultiplied(0x5C, 0x9B, 0xFF, 120),
    );
    clipped.circle_filled(
        Pos2::new(rect.left() + w * 0.62, rect.bottom() + h * 0.05),
        w * 0.48,
        Color32::from_rgba_unmultiplied(0xB9, 0xA8, 0xFF, 130),
    );
}
