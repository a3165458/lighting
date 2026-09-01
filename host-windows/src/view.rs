//! The host window layout.
//!
//! Pure egui: it reads a [`Snapshot`] of the session, mutates [`Settings`] the
//! user owns, and hands back [`Action`]s for the shell to run. Keeping it free
//! of Win32 calls lets the layout be rendered and reviewed off Windows.

use egui::{Color32, Margin, Pos2, Rect, Vec2};

use crate::theme::{self, Glyph};
use crate::ui_text::{self, Tone};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ShareMode {
    /// Stream the primary monitor, scaled to the tablet panel (product path A).
    #[default]
    Mirror,
    /// Legacy wire values — coerced to Mirror. Kept so old settings/UI still parse.
    Extend,
    /// Legacy wire values — coerced to Mirror.
    External,
}

impl ShareMode {
    /// Product path A: only mirror (tablet-native encode). Extend/External stay for wire compat.
    pub const ALL: [ShareMode; 1] = [ShareMode::Mirror];

    pub fn label(self) -> &'static str {
        match self {
            ShareMode::Mirror => "按平板分辨率输出",
            ShareMode::Extend | ShareMode::External => "按平板分辨率输出",
        }
    }

    pub fn hint(self) -> &'static str {
        "镜像电脑主屏，并按平板物理分辨率编码推流。无需虚拟显示驱动，开箱即用。"
    }

    pub fn as_wire(self) -> &'static str {
        // Always advertise mirror on the wire for product path A.
        "mirror"
    }

    pub fn from_wire(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            // Path A: any historical extend/external preference becomes mirror.
            "mirror" | "clone" | "duplicate" | "extend" | "extended" | "external"
            | "second" | "externalonly" => Some(ShareMode::Mirror),
            _ => None,
        }
    }

    /// Lighting maps both extend modes to `/extend`. `/external` blanks the PC.
    pub fn display_switch_arg(self) -> &'static str {
        match self {
            ShareMode::Mirror => "/clone",
            ShareMode::Extend | ShareMode::External => "/extend",
        }
    }

    /// Path A never uses a virtual display driver.
    pub fn uses_virtual_display(self) -> bool {
        false
    }

    pub fn coerce_product(self) -> Self {
        ShareMode::Mirror
    }
}

/// Heuristic for virtual / IDD monitors used as Lighting extend targets.
pub fn looks_virtual_display(name: &str, friendly: &str) -> bool {
    let blob = format!("{name} {friendly}").to_ascii_lowercase();
    blob.contains("virtual")
        || blob.contains("lightingidd")
        || blob.contains("lighting virtual")
        || blob.contains("iddsample")
        || blob.contains("idd ")
        || blob.contains("mttvdd")
        || blob.contains("mtt vdd")
        || blob.contains("usb-mobile-monitor")
        || blob.contains("usb mobile")
        || blob.contains("spacedesk")
        || blob.contains("deskreen")
        || blob.contains("sunshine")
        || blob.contains("parsec")
        || blob.contains("amyuni")
        || blob.contains("usbmmidd")
        || blob.contains("vdd")
        || blob.contains("indirect display")
}

#[cfg(test)]
mod share_mode_tests {
    use super::*;

    #[test]
    fn share_mode_wire_roundtrip() {
        for mode in ShareMode::ALL {
            assert_eq!(ShareMode::from_wire(mode.as_wire()), Some(mode));
        }
    }

    #[test]
    fn path_a_coerces_legacy_extend_to_mirror() {
        assert_eq!(ShareMode::from_wire("extend"), Some(ShareMode::Mirror));
        assert_eq!(ShareMode::from_wire("external"), Some(ShareMode::Mirror));
        assert_eq!(ShareMode::Extend.coerce_product(), ShareMode::Mirror);
        assert!(!ShareMode::Extend.uses_virtual_display());
        assert_eq!(ShareMode::Mirror.as_wire(), "mirror");
    }

    #[test]
    fn virtual_display_heuristics() {
        assert!(looks_virtual_display(r"\\.\DISPLAY2", "Virtual Display Driver"));
        assert!(!looks_virtual_display(r"\\.\DISPLAY1", "Generic PnP Monitor"));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ResCap {
    #[default]
    Device,
    Fhd,
    Uhd2k,
    Uhd4k,
}

impl ResCap {
    pub const ALL: [ResCap; 4] = [ResCap::Device, ResCap::Fhd, ResCap::Uhd2k, ResCap::Uhd4k];

    pub fn label(self) -> &'static str {
        match self {
            ResCap::Device => "跟随平板",
            ResCap::Fhd => "最高 1080p",
            ResCap::Uhd2k => "最高 2K",
            ResCap::Uhd4k => "最高 4K",
        }
    }

    pub fn as_wire(self) -> &'static str {
        match self {
            ResCap::Device => "device",
            ResCap::Fhd => "fhd",
            ResCap::Uhd2k => "uhd2k",
            ResCap::Uhd4k => "uhd4k",
        }
    }

    pub fn from_wire(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "device" => Some(ResCap::Device),
            "fhd" | "1080p" => Some(ResCap::Fhd),
            "uhd2k" | "2k" => Some(ResCap::Uhd2k),
            "uhd4k" | "4k" => Some(ResCap::Uhd4k),
            _ => None,
        }
    }
}

/// Everything the window shows but does not own.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub running: bool,
    pub phase: String,
    pub detail: String,
    pub transport: String,
    pub client_name: String,
    pub client_addr: String,
    pub codec: String,
    pub frames: u64,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
    pub loss_permille: u32,
    pub bytes_sent: u64,
    pub connected_secs: u64,
    pub usb_hint: String,
    pub usb_tone: Tone,
    pub client_app_missing: bool,
    pub can_install_apk: bool,
    pub install_inflight: bool,
    pub displays: Vec<String>,
    pub devices: Vec<String>,
    pub multi_device: bool,
    pub adb_path: String,
    pub last_error: String,
}

/// Everything the user can change from the window.
#[derive(Clone)]
pub struct Settings {
    pub selected_display: usize,
    pub selected_device: usize,
    pub share_mode: ShareMode,
    pub quality_pct: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub send_audio: bool,
    pub prefer_hevc: bool,
    pub res_cap: ResCap,
    pub touch_relay: bool,
    pub keyboard_relay: bool,
    pub clipboard_share: bool,
    pub bind_host: String,
    pub bind_port: u16,
    pub show_advanced: bool,
    pub show_about: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_display: 0,
            selected_device: 0,
            // Mirror needs no virtual display driver. GlideX ships a private signed
            // IddCx driver; until we have equivalent, first-run must succeed without it.
            share_mode: ShareMode::Mirror,
            quality_pct: 100,
            fps: 60,
            bitrate_kbps: 25_000,
            send_audio: true,
            prefer_hevc: false,
            res_cap: ResCap::Device,
            touch_relay: true,
            keyboard_relay: true,
            clipboard_share: false,
            bind_host: "0.0.0.0".into(),
            bind_port: 17400,
            show_advanced: false,
            show_about: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Start,
    Stop,
    Refresh,
    InstallClient,
    TouchRelayChanged,
}

pub fn render(ctx: &egui::Context, snap: &Snapshot, settings: &mut Settings) -> Vec<Action> {
    let mut actions = Vec::new();
    title_bar(ctx);

    // Keep left/right page gutters identical everywhere (cards, action bar, status).
    const SIDE: i8 = 24;

    egui::TopBottomPanel::bottom("status_strip")
        .frame(
            egui::Frame::new()
                .fill(theme::STATUS_STRIP)
                .inner_margin(Margin {
                    left: SIDE,
                    right: SIDE,
                    top: 10,
                    bottom: 12,
                }),
        )
        .show(ctx, |ui| status_strip(ui, snap));

    egui::TopBottomPanel::bottom("action_bar")
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(Margin {
                    left: SIDE,
                    right: SIDE,
                    top: 8,
                    bottom: 8,
                }),
        )
        .show(ctx, |ui| action_bar(ui, snap, settings, &mut actions));

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(Margin {
                    left: SIDE,
                    right: SIDE,
                    top: 8,
                    bottom: 8,
                }),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // Prevent wide children (combo / slider rows) from stretching
                    // the scroll content past the panel and eating the right gutter.
                    let content_w = ui.available_width();
                    ui.set_width(content_w);
                    ui.set_max_width(content_w);

                    hero(ui);
                    connection_card(ui, snap, settings, &mut actions);
                    display_card(ui, snap, settings);
                    transport_card(ui, snap, settings);
                    controls_card(ui, settings, &mut actions);
                });
        });

    if settings.show_advanced {
        advanced_modal(ctx, snap, settings, &mut actions);
    }
    if settings.show_about {
        about_modal(ctx, settings);
    }

    actions
}

fn title_bar(ctx: &egui::Context) {
    const BTN_W: f32 = 46.0;
    // Keep the whole caption strip clear of Win11's ~8px rounded corner clip.
    const EDGE_PAD: f32 = 10.0;

    egui::TopBottomPanel::top("title_bar")
        .exact_height(40.0)
        .frame(
            egui::Frame::new()
                .fill(Color32::WHITE)
                .inner_margin(Margin {
                    left: 12,
                    right: EDGE_PAD as i8,
                    top: 0,
                    bottom: 0,
                }),
        )
        .show(ctx, |ui| {
            let full = ui.max_rect();
            // Hairline under the caption, matching the mockup separator.
            ui.painter().hline(
                full.x_range(),
                full.bottom() - 0.5,
                egui::Stroke::new(1.0, theme::CARD_LINE),
            );
            let strip_w = BTN_W * 3.0;
            let strip = Rect::from_min_max(
                Pos2::new(full.right() - strip_w, full.top()),
                full.right_bottom(),
            );
            let drag = Rect::from_min_max(full.left_top(), Pos2::new(strip.left(), full.bottom()));
            let drag_resp =
                ui.interact(drag, ui.id().with("title_drag"), egui::Sense::click_and_drag());
            if drag_resp.drag_started() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            // Branding on the left — capped so it never runs under the buttons.
            ui.scope_builder(egui::UiBuilder::new().max_rect(drag), |ui| {
                ui.horizontal_centered(|ui| {
                    let (mark, _) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(mark, egui::CornerRadius::same(6), theme::ACCENT);
                    theme::paint_glyph(
                        ui.painter(),
                        mark.shrink(4.0),
                        Glyph::Bolt,
                        Color32::WHITE,
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Lighting 副屏")
                            .font(theme::bold(13.0))
                            .color(theme::TEXT),
                    );
                });
            });

            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            let min_r = Rect::from_min_size(strip.left_top(), Vec2::new(BTN_W, strip.height()));
            let max_r = Rect::from_min_size(
                Pos2::new(strip.left() + BTN_W, strip.top()),
                Vec2::new(BTN_W, strip.height()),
            );
            let close_r = Rect::from_min_size(
                Pos2::new(strip.left() + BTN_W * 2.0, strip.top()),
                Vec2::new(BTN_W, strip.height()),
            );

            if caption_btn(ui, min_r, CaptionIcon::Minimize).clicked() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            if caption_btn(
                ui,
                max_r,
                if maximized {
                    CaptionIcon::Restore
                } else {
                    CaptionIcon::Maximize
                },
            )
            .clicked()
            {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if caption_btn(ui, close_r, CaptionIcon::Close).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
}

#[derive(Clone, Copy)]
enum CaptionIcon {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Win11-style caption button: fixed hit box, vector glyph (no font tofu).
fn caption_btn(ui: &mut egui::Ui, rect: Rect, icon: CaptionIcon) -> egui::Response {
    let id = ui.id().with(match icon {
        CaptionIcon::Minimize => "cap_min",
        CaptionIcon::Maximize => "cap_max",
        CaptionIcon::Restore => "cap_restore",
        CaptionIcon::Close => "cap_close",
    });
    let response = ui.interact(rect, id, egui::Sense::click());
    let is_close = matches!(icon, CaptionIcon::Close);

    if response.hovered() || response.is_pointer_button_down_on() {
        let fill = if is_close {
            if response.is_pointer_button_down_on() {
                Color32::from_rgb(0xC4, 0x0E, 0x1A)
            } else {
                Color32::from_rgb(0xE8, 0x11, 0x23)
            }
        } else if response.is_pointer_button_down_on() {
            Color32::from_rgb(0xE4, 0xE4, 0xEA)
        } else {
            Color32::from_rgb(0xF0, 0xF0, 0xF5)
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, fill);
    }

    let fg = if is_close && response.hovered() {
        Color32::WHITE
    } else {
        theme::MUTED
    };
    paint_caption_icon(ui.painter(), rect, icon, fg);
    response
}

fn paint_caption_icon(painter: &egui::Painter, rect: Rect, icon: CaptionIcon, color: Color32) {
    let c = rect.center();
    let stroke = egui::Stroke::new(1.25, color);
    match icon {
        CaptionIcon::Minimize => {
            let half = 5.0;
            painter.line_segment(
                [Pos2::new(c.x - half, c.y), Pos2::new(c.x + half, c.y)],
                stroke,
            );
        }
        CaptionIcon::Maximize => {
            let r = Rect::from_center_size(c, Vec2::splat(10.0));
            painter.rect_stroke(r, egui::CornerRadius::ZERO, stroke, egui::StrokeKind::Outside);
        }
        CaptionIcon::Restore => {
            // Two overlapping squares, matching the Win11 restore glyph.
            let back = Rect::from_min_max(
                Pos2::new(c.x - 2.0, c.y - 5.5),
                Pos2::new(c.x + 5.5, c.y + 2.0),
            );
            let front = Rect::from_min_max(
                Pos2::new(c.x - 5.5, c.y - 2.0),
                Pos2::new(c.x + 2.0, c.y + 5.5),
            );
            painter.rect_stroke(back, egui::CornerRadius::ZERO, stroke, egui::StrokeKind::Outside);
            painter.rect_filled(front, egui::CornerRadius::ZERO, Color32::WHITE);
            painter.rect_stroke(front, egui::CornerRadius::ZERO, stroke, egui::StrokeKind::Outside);
        }
        CaptionIcon::Close => {
            let half = 4.5;
            painter.line_segment(
                [
                    Pos2::new(c.x - half, c.y - half),
                    Pos2::new(c.x + half, c.y + half),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + half, c.y - half),
                    Pos2::new(c.x - half, c.y + half),
                ],
                stroke,
            );
        }
    }
}

fn hero(ui: &mut egui::Ui) {
    // Text first (capped width), then art gets the *remaining* width exactly —
    // never a fixed 260px that overflows and clips the laptop's left edge.
    let art_h = 140.0;
    ui.horizontal(|ui| {
        ui.set_min_height(art_h);
        ui.spacing_mut().item_spacing.x = 12.0;

        ui.vertical(|ui| {
            // Leave at least ~200px for the devices on the right.
            let text_max = (ui.available_width() - 200.0).max(180.0);
            ui.set_max_width(text_max);
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                egui::RichText::new("Lighting 副屏")
                    .font(theme::bold(30.0))
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new("将你的平板 / 手机变成电脑扩展屏")
                    .size(13.5)
                    .color(theme::MUTED),
            );
            ui.label(
                egui::RichText::new("低延迟 · 高画质 · 触控回传")
                    .size(13.0)
                    .color(theme::ACCENT_MUTED),
            );
        });

        let art_w = ui.available_width().max(160.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(art_w, art_h), egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);
        theme::paint_hero(painter, rect);

        let wifi = Rect::from_center_size(
            Pos2::new(rect.right() - 16.0, rect.top() + 16.0),
            Vec2::splat(26.0),
        );
        let p = ui.painter().with_clip_rect(rect);
        p.circle_filled(wifi.center() + Vec2::new(0.0, 1.0), 12.5, Color32::from_black_alpha(18));
        p.circle_filled(wifi.center(), 12.0, Color32::WHITE);
        p.circle_stroke(wifi.center(), 12.0, egui::Stroke::new(1.0, theme::CARD_LINE));
        theme::paint_glyph(&p, wifi.shrink(5.5), Glyph::Wifi, theme::ACCENT);
        p.circle_filled(
            Pos2::new(wifi.right() - 3.0, wifi.top() + 5.0),
            3.4,
            theme::OK,
        );
    });
    ui.add_space(14.0);
}

fn connection_card(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    settings: &mut Settings,
    actions: &mut Vec<Action>,
) {
    theme::card(ui, "连接状态", |ui| {
        let streaming = ui_text::is_streaming(&snap.phase);
        let second = if streaming {
            ui_text::client_display_addr(&snap.client_addr)
        } else if snap.running {
            "已开始共享，请在平板点「USB 一键连接」".into()
        } else {
            "插上数据线后，点下面的「开始共享」".into()
        };
        ui.horizontal(|ui| {
            if streaming {
                theme::glyph_badge(ui, Glyph::Tablet, theme::OK, theme::OK_SOFT, 44.0);
            } else if snap.running {
                theme::glyph_badge(ui, Glyph::Tablet, theme::ACCENT, theme::ACCENT_SOFT, 44.0);
            } else {
                theme::glyph_badge(ui, Glyph::Tablet, theme::DIM, theme::TRACK, 44.0);
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if streaming {
                    ui.label(
                        egui::RichText::new(format!(
                            "已连接：{}",
                            ui_text::peer_name(&snap.client_name)
                        ))
                        .font(theme::bold(15.0))
                        .color(theme::LINK),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(ui_text::connection_title(
                            snap.running,
                            &snap.phase,
                            &snap.client_name,
                        ))
                        .font(theme::bold(14.5))
                        .color(theme::TEXT),
                    );
                }
                ui.label(egui::RichText::new(&second).size(11.5).color(theme::DIM));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if snap.running && ui.add(outline_button("断开连接")).clicked() {
                    actions.push(Action::Stop);
                }
            });
        });

        if !streaming && !snap.usb_hint.is_empty() {
            ui.add_space(8.0);
            let color = tone_color(snap.usb_tone);
            ui.horizontal(|ui| {
                theme::glyph(ui, Glyph::Plug, color, 14.0);
                ui.add_space(1.0);
                ui.label(
                    egui::RichText::new(&snap.usb_hint)
                        .size(11.5)
                        .color(color),
                );
            });
            if snap.client_app_missing && snap.can_install_apk {
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("安装到平板")
                                .size(13.0)
                                .color(Color32::WHITE),
                        )
                        .fill(theme::ACCENT)
                        .corner_radius(egui::CornerRadius::same(14))
                        .min_size(Vec2::new(120.0, 34.0)),
                    )
                    .clicked()
                {
                    actions.push(Action::InstallClient);
                }
            } else if snap.install_inflight {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("安装进行中，请稍候…")
                        .size(11.5)
                        .color(theme::DIM),
                );
            }
        }

        let detail = ui_text::human_detail_text(&snap.phase, &snap.detail);
        if !streaming && !detail.is_empty() && !second.contains(&detail) {
            ui.label(egui::RichText::new(detail).size(11.5).color(theme::MUTED));
        }
        if !snap.last_error.is_empty() {
            ui.label(
                egui::RichText::new(ui_text::human_last_error(&snap.last_error))
                    .size(11.5)
                    .color(theme::BAD),
            );
        }
        if snap.multi_device {
            ui.add_space(4.0);
            setting_row(ui, Glyph::Plug, "选择设备", |ui| {
                combo(
                    ui,
                    "device_main",
                    200.0,
                    &snap.devices,
                    &mut settings.selected_device,
                );
            });
        }
    });
}

fn display_card(ui: &mut egui::Ui, snap: &Snapshot, settings: &mut Settings) {
    settings.share_mode = ShareMode::Mirror;
    theme::card(ui, "投屏设置", |ui| {
        ui.label(
            egui::RichText::new(ShareMode::Mirror.hint())
                .size(11.5)
                .color(theme::MUTED),
        );
        ui.add_space(6.0);
        // Reserve the same trailing column as sliders so the dropdown's right
        // edge lines up with the slider tracks.
        form_row(
            ui,
            Glyph::Monitor,
            "显示器",
            FORM_ROW_H,
            FORM_TRAIL,
            |ui, w| {
                if snap.displays.is_empty() {
                    ui.label(
                        egui::RichText::new("未检测到显示器")
                            .size(12.0)
                            .color(theme::WARN),
                    );
                } else {
                    combo(
                        ui,
                        "display",
                        w.max(80.0),
                        &snap.displays,
                        &mut settings.selected_display,
                    );
                }
            },
            |_| {},
        );
        slider_row(ui, Glyph::Gauge, "画质", format!("{}%", settings.quality_pct), settings.quality_pct as f32, 40.0, 100.0, 1.0, |v| {
            settings.quality_pct = v.round() as u32;
        });
        slider_row(ui, Glyph::Film, "帧率", format!("{} fps", settings.fps), settings.fps as f32, 30.0, 120.0, 1.0, |v| {
            settings.fps = v.round() as u32;
        });
        slider_row(ui, Glyph::Meter, "码率", format!("{} kbps", settings.bitrate_kbps), settings.bitrate_kbps as f32, 5_000.0, 40_000.0, 1_000.0, |v| {
            settings.bitrate_kbps = v.round() as u32;
        });
        toggle_row(
            ui,
            Glyph::Speaker,
            "系统声音同步",
            None,
            &mut settings.send_audio,
            true,
        );
    });
}

fn transport_card(ui: &mut egui::Ui, snap: &Snapshot, settings: &Settings) {
    ui.label(
        egui::RichText::new("传输与性能")
            .font(theme::bold(15.5))
            .color(theme::TEXT),
    );
    ui.add_space(8.0);

    let gap = 8.0;
    let row_h = 102.0;
    let total = ui.available_width();
    ui.set_max_width(total);
    let (row, _) = ui.allocate_exact_size(Vec2::new(total, row_h), egui::Sense::hover());
    let tile_w = ((row.width() - gap * 3.0) / 4.0).max(1.0);
    let tiles = [
        (
            Glyph::Route,
            "传输协议",
            ui_text::transport_text(snap.running, &snap.transport),
            theme::TEXT,
        ),
        (
            Glyph::Chip,
            "编码方式",
            ui_text::codec_text(&snap.codec, settings.prefer_hevc),
            theme::TEXT,
        ),
        (
            Glyph::Bolt,
            "延迟",
            ui_text::latency_text(snap.latency_ms),
            latency_color(snap.latency_ms),
        ),
        (
            Glyph::Target,
            "丢包率",
            ui_text::loss_text(snap.running, snap.loss_permille),
            theme::TEXT,
        ),
    ];
    for (i, (g, label, value, color)) in tiles.into_iter().enumerate() {
        let x = row.left() + i as f32 * (tile_w + gap);
        let tile = Rect::from_min_size(Pos2::new(x, row.top()), Vec2::new(tile_w, row_h));
        metric_tile(ui, tile, g, label, value, color);
    }
    ui.add_space(10.0);
}

fn controls_card(ui: &mut egui::Ui, settings: &mut Settings, actions: &mut Vec<Action>) {
    theme::card(ui, "控制与交互", |ui| {
        if toggle_row(
            ui,
            Glyph::Touch,
            "触控回传 (模拟鼠标)",
            None,
            &mut settings.touch_relay,
            true,
        ) {
            actions.push(Action::TouchRelayChanged);
        }
        toggle_row(
            ui,
            Glyph::Keyboard,
            "键盘输入回传",
            Some("平板端支持后自动生效"),
            &mut settings.keyboard_relay,
            true,
        );
        toggle_row(
            ui,
            Glyph::Clipboard,
            "剪贴板共享",
            Some("即时支持"),
            &mut settings.clipboard_share,
            false,
        );
    });
}

fn advanced_modal(
    ctx: &egui::Context,
    snap: &Snapshot,
    settings: &mut Settings,
    actions: &mut Vec<Action>,
) {
    let response = egui::Modal::new(egui::Id::new("advanced_modal"))
        .backdrop_color(Color32::from_black_alpha(90))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_width(420.0);
            modal_header(ui, "高级设置", &mut settings.show_advanced);
            ui.add_space(12.0);

            ui.label(
                egui::RichText::new("局域网绑定（一般不用改）")
                    .size(11.5)
                    .color(theme::DIM),
            );
            ui.add_space(4.0);
            setting_row(ui, Glyph::Route, "地址", |ui| {
                ui.add_sized(
                    Vec2::new(200.0, 24.0),
                    egui::TextEdit::singleline(&mut settings.bind_host)
                        .font(egui::FontId::proportional(12.0)),
                );
            });
            setting_row(ui, Glyph::Plug, "端口", |ui| {
                ui.add(egui::DragValue::new(&mut settings.bind_port).range(1..=65535));
            });

            ui.add_space(8.0);
            setting_row(ui, Glyph::Monitor, "输出上限", |ui| {
                egui::ComboBox::from_id_salt("res_cap")
                    .width(160.0)
                    .selected_text(egui::RichText::new(settings.res_cap.label()).size(12.0))
                    .show_ui(ui, |ui| {
                        for cap in ResCap::ALL {
                            ui.selectable_value(&mut settings.res_cap, cap, cap.label());
                        }
                    });
            });
            toggle_row(
                ui,
                Glyph::Chip,
                "优先 HEVC",
                Some("设备支持时文字更清晰"),
                &mut settings.prefer_hevc,
                true,
            );

            ui.add_space(6.0);
            setting_row(ui, Glyph::Plug, "Android 设备", |ui| {
                if snap.devices.is_empty() {
                    ui.label(
                        egui::RichText::new("未检测到设备")
                            .size(12.0)
                            .color(theme::DIM),
                    );
                } else {
                    combo(
                        ui,
                        "device_advanced",
                        220.0,
                        &snap.devices,
                        &mut settings.selected_device,
                    );
                }
            });

            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "阶段 {} · {}",
                    ui_text::display_phase(&snap.phase),
                    ui_text::metrics_line(snap.frames, snap.bitrate_kbps)
                ))
                .size(11.0)
                .color(theme::DIM),
            );
            if !snap.adb_path.is_empty() {
                ui.label(
                    egui::RichText::new(format!("adb：{}", snap.adb_path))
                        .size(11.0)
                        .color(theme::DIM),
                );
            }
            if !snap.last_error.is_empty() {
                ui.collapsing("详情", |ui| {
                    ui.label(
                        egui::RichText::new(&snap.last_error)
                            .monospace()
                            .color(theme::MUTED),
                    );
                });
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add(outline_button("刷新显示器 / 设备")).clicked() {
                    actions.push(Action::Refresh);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary_pill("完成")).clicked() {
                        settings.show_advanced = false;
                    }
                });
            });
        });

    if response.backdrop_response.clicked() {
        settings.show_advanced = false;
    }
}

fn about_modal(ctx: &egui::Context, settings: &mut Settings) {
    let response = egui::Modal::new(egui::Id::new("about_modal"))
        .backdrop_color(Color32::from_black_alpha(90))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_width(400.0);
            modal_header(ui, "关于", &mut settings.show_about);
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(
                egui::RichText::new(format!("Lighting 副屏 v{}", env!("CARGO_PKG_VERSION")))
                    .font(theme::bold(16.0))
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new("将 Android 平板 / 手机变成 Windows 扩展屏")
                    .size(12.5)
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);
            for line in [
                "扩展模式：winget install VirtualDrivers.Virtual-Display-Driver，然后在 Windows 显示设置里设为「扩展」并选这块虚拟屏。",
                "平板触控：单击 / 拖动、长按右键、双指滚动。",
                "局域网连接：在两边的「高级」里填写电脑地址即可。",
            ] {
                ui.label(egui::RichText::new(line).size(12.0).color(theme::MUTED));
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(primary_pill("知道了")).clicked() {
                    settings.show_about = false;
                }
            });
        });

    if response.backdrop_response.clicked() {
        settings.show_about = false;
    }
}

fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1.0, theme::CARD_LINE))
        .corner_radius(egui::CornerRadius::same(18))
        .inner_margin(Margin::symmetric(20, 18))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(40),
        })
}

fn modal_header(ui: &mut egui::Ui, title: &str, open: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(theme::bold(17.0))
                .color(theme::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(8),
                    Color32::from_rgb(0xF0, 0xF0, 0xF5),
                );
            }
            let c = rect.center();
            let s = egui::Stroke::new(1.3, theme::MUTED);
            let half = 4.5;
            ui.painter().line_segment(
                [
                    Pos2::new(c.x - half, c.y - half),
                    Pos2::new(c.x + half, c.y + half),
                ],
                s,
            );
            ui.painter().line_segment(
                [
                    Pos2::new(c.x + half, c.y - half),
                    Pos2::new(c.x - half, c.y + half),
                ],
                s,
            );
            if response.clicked() {
                *open = false;
            }
        });
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }
}

fn primary_pill(label: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label.to_owned())
            .size(13.0)
            .color(Color32::WHITE),
    )
    .fill(theme::ACCENT)
    .stroke(egui::Stroke::NONE)
    .corner_radius(egui::CornerRadius::same(16))
    .min_size(Vec2::new(88.0, 34.0))
}

fn action_bar(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    settings: &mut Settings,
    actions: &mut Vec<Action>,
) {
    // Three slots in one row: side buttons keep a fixed width so the primary
    // CTA shrinks instead of shoving 「关于」 off the right edge.
    let full_w = ui.available_width();
    let side_w = 108.0;
    let gap = 8.0;
    let share_w = (full_w - side_w * 2.0 - gap * 2.0).max(140.0);

    ui.horizontal(|ui| {
        // Disable default item spacing — we place explicit `gap`s so the row
        // width stays exactly `full_w` (otherwise the right gutter collapses).
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.set_width(full_w);
        ui.set_max_width(full_w);
        if ghost_button(ui, side_w, Glyph::Gear, "高级设置").clicked() {
            settings.show_about = false;
            settings.show_advanced = true;
        }
        ui.add_space(gap);
        let label = ui_text::share_button_label(snap.running);
        let share = share_button(ui, share_w, &label, snap.running);
        if share.clicked() {
            actions.push(if snap.running {
                Action::Stop
            } else {
                Action::Start
            });
        }
        ui.add_space(gap);
        if ghost_button(ui, side_w, Glyph::Info, "关于").clicked() {
            settings.show_advanced = false;
            settings.show_about = true;
        }
    });
}

fn status_strip(ui: &mut egui::Ui, snap: &Snapshot) {
    let (text, tone) = ui_text::health_text(snap.running, &snap.phase, snap.latency_ms);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, tone_color(tone));
        ui.label(egui::RichText::new(text).size(11.0).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(ui_text::format_duration(snap.connected_secs))
                    .size(11.0)
                    .color(theme::DIM),
            );
            ui.add_space(36.0);
            ui.label(
                egui::RichText::new(format!(
                    "已传输：{}",
                    ui_text::format_bytes(snap.bytes_sent)
                ))
                .size(11.0)
                .color(theme::DIM),
            );
        });
    });
}

fn tone_color(tone: Tone) -> Color32 {
    match tone {
        Tone::Ok => theme::OK,
        Tone::Warn => theme::WARN,
        Tone::Bad => theme::BAD,
        Tone::Info => theme::ACCENT,
        Tone::Muted => theme::DIM,
    }
}

fn latency_color(latency_ms: u32) -> Color32 {
    match latency_ms {
        0 => theme::TEXT,
        1..=60 => theme::OK,
        61..=140 => theme::WARN,
        _ => theme::BAD,
    }
}

/// Shared column widths so 显示器 / 画质 / 帧率 / 码率 / 开关 的左右边缘对齐。
const FORM_ICON: f32 = 22.0;
const FORM_LABEL: f32 = 88.0;
const FORM_TRAIL: f32 = 100.0;
const FORM_GAP: f32 = 10.0;
const FORM_ROW_H: f32 = 36.0;

/// Icon + fixed label column + flexible mid + optional trailing column.
fn form_row(
    ui: &mut egui::Ui,
    g: Glyph,
    label: &str,
    row_h: f32,
    trail_w: f32,
    mid: impl FnOnce(&mut egui::Ui, f32),
    trail: impl FnOnce(&mut egui::Ui),
) {
    let full_w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(full_w, row_h), egui::Sense::hover());

    let icon_rect = Rect::from_center_size(
        Pos2::new(row.left() + FORM_ICON * 0.5, row.center().y),
        Vec2::splat(FORM_ICON),
    );
    ui.painter()
        .circle_filled(icon_rect.center(), FORM_ICON * 0.5, theme::BG);
    theme::paint_glyph(
        ui.painter(),
        icon_rect.shrink(FORM_ICON * 0.26),
        g,
        theme::MUTED,
    );

    let label_left = row.left() + FORM_ICON + FORM_GAP;
    let label_rect = Rect::from_min_size(
        Pos2::new(label_left, row.top()),
        Vec2::new(FORM_LABEL, row_h),
    );
    ui.painter().text(
        Pos2::new(label_rect.left(), label_rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        theme::TEXT,
    );

    let mid_right = if trail_w > 0.0 {
        row.right() - trail_w - FORM_GAP
    } else {
        row.right()
    };
    let mid_rect = Rect::from_min_max(
        Pos2::new(label_rect.right() + FORM_GAP, row.top()),
        Pos2::new(mid_right.max(label_rect.right() + FORM_GAP + 40.0), row.bottom()),
    );

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(mid_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.set_width(mid_rect.width());
            ui.set_max_width(mid_rect.width());
            mid(ui, mid_rect.width());
        },
    );

    if trail_w > 0.0 {
        let trail_rect = Rect::from_min_max(
            Pos2::new(row.right() - trail_w, row.top()),
            row.right_bottom(),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(trail_rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.set_width(trail_rect.width());
                ui.set_max_width(trail_rect.width());
                trail(ui);
            },
        );
    } else {
        // Silence unused if caller passed a no-op and trail_w == 0.
        let _ = trail;
    }

    ui.add_space(6.0);
}

/// Left icon + label, control fills the remaining row (no trailing column).
fn setting_row(ui: &mut egui::Ui, g: Glyph, label: &str, right: impl FnOnce(&mut egui::Ui)) {
    form_row(
        ui,
        g,
        label,
        FORM_ROW_H,
        0.0,
        |ui, _w| right(ui),
        |_| {},
    );
}

/// Icon + label + slider + right-aligned value, all columns shared with other form rows.
fn slider_row(
    ui: &mut egui::Ui,
    g: Glyph,
    label: &str,
    value: String,
    current: f32,
    min: f32,
    max: f32,
    step: f32,
    mut set: impl FnMut(f32),
) {
    form_row(
        ui,
        g,
        label,
        FORM_ROW_H,
        FORM_TRAIL,
        |ui, width| {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(width, 22.0), egui::Sense::click_and_drag());
            if response.dragged() || response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let t = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                    let mut v = min + t * (max - min);
                    if step > 0.0 {
                        v = (v / step).round() * step;
                    }
                    set(v.clamp(min, max));
                }
            }
            let t = ((current - min) / (max - min)).clamp(0.0, 1.0);
            let painter = ui.painter();
            painter.rect_filled(
                rect.shrink2(Vec2::new(0.0, 7.0)),
                egui::CornerRadius::same(5),
                theme::TRACK,
            );
            let filled = Rect::from_min_max(
                rect.left_top() + Vec2::new(0.0, 5.5),
                Pos2::new(rect.left() + rect.width() * t, rect.bottom() - 5.5),
            );
            painter.rect_filled(filled, egui::CornerRadius::same(6), theme::ACCENT);
            let knob = Pos2::new(rect.left() + rect.width() * t, rect.center().y);
            painter.circle_filled(knob, 10.0, Color32::from_black_alpha(16));
            painter.circle_filled(knob, 9.0, Color32::WHITE);
            painter.circle_stroke(knob, 9.0, egui::Stroke::new(1.0, theme::ACCENT_LINE));
        },
        |ui| {
            ui.label(egui::RichText::new(value).size(12.5).color(theme::MUTED));
        },
    );
}

fn toggle_row(
    ui: &mut egui::Ui,
    g: Glyph,
    label: &str,
    note: Option<&str>,
    on: &mut bool,
    enabled: bool,
) -> bool {
    let mut changed = false;
    let row_h = if note.is_some() { 44.0 } else { FORM_ROW_H };
    let full_w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(full_w, row_h), egui::Sense::hover());

    let icon_color = if enabled { theme::MUTED } else { theme::DIM };
    let icon_rect = Rect::from_center_size(
        Pos2::new(row.left() + FORM_ICON * 0.5, row.center().y),
        Vec2::splat(FORM_ICON),
    );
    ui.painter()
        .circle_filled(icon_rect.center(), FORM_ICON * 0.5, theme::BG);
    theme::paint_glyph(
        ui.painter(),
        icon_rect.shrink(FORM_ICON * 0.26),
        g,
        icon_color,
    );

    let trail = Rect::from_min_max(
        Pos2::new(row.right() - FORM_TRAIL, row.top()),
        row.right_bottom(),
    );
    let text_rect = Rect::from_min_max(
        Pos2::new(row.left() + FORM_ICON + FORM_GAP, row.top()),
        Pos2::new(trail.left() - FORM_GAP, row.bottom()),
    );

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    egui::RichText::new(label)
                        .size(12.5)
                        .color(if enabled { theme::TEXT } else { theme::DIM }),
                );
                if let Some(note) = note {
                    ui.label(egui::RichText::new(note).size(10.5).color(theme::DIM));
                }
            });
        },
    );

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(trail)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            changed = theme::switch(ui, on, enabled).changed();
        },
    );

    ui.add_space(6.0);
    changed
}

fn metric_tile(
    ui: &mut egui::Ui,
    rect: Rect,
    g: Glyph,
    label: &str,
    value: String,
    value_color: Color32,
) {
    // Paint into a fixed slot; do not report min-size back to the parent or the
    // tile row will stretch the page and collapse the right gutter.
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
        ui.set_max_size(rect.size());
        let frame = egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0_f32, theme::CARD_LINE))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(Margin::symmetric(8, 10))
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: Color32::from_black_alpha(10),
            });
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_max_width(ui.available_width());
            ui.set_min_height(ui.available_height());
            ui.with_layout(
                egui::Layout::top_down(egui::Align::Center).with_main_align(egui::Align::Center),
                |ui| {
                    ui.set_min_height(ui.available_height());
                    ui.spacing_mut().item_spacing.y = 5.0;
                    theme::glyph_badge(ui, g, theme::ACCENT, theme::ACCENT_SOFT, 26.0);
                    ui.label(
                        egui::RichText::new(label)
                            .size(10.5)
                            .color(theme::DIM),
                    );
                    let shown = truncate_for_width(ui, &value, ui.available_width() - 4.0, 12.5);
                    ui.label(
                        egui::RichText::new(shown)
                            .font(theme::bold(12.5))
                            .color(value_color),
                    );
                },
            );
        });
    });
}

fn outline_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label.to_owned())
            .size(12.5)
            .color(theme::ACCENT),
    )
    .fill(Color32::WHITE)
    .stroke(egui::Stroke::new(1.0_f32, theme::ACCENT_LINE))
    .corner_radius(egui::CornerRadius::same(16))
    .min_size(Vec2::new(92.0, 32.0))
}

fn ghost_button(ui: &mut egui::Ui, width: f32, g: Glyph, label: &str) -> egui::Response {
    let desired = Vec2::new(width, 46.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(24),
        Color32::WHITE,
        egui::Stroke::new(1.0_f32, theme::CARD_LINE),
        egui::StrokeKind::Inside,
    );
    // Center icon + label as one group inside the pill.
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(12.0),
        theme::MUTED,
    );
    let total = 15.0 + 6.0 + galley.size().x;
    let start = rect.center().x - total * 0.5;
    let icon = Rect::from_center_size(Pos2::new(start + 7.5, rect.center().y), Vec2::splat(15.0));
    theme::paint_glyph(ui.painter(), icon, g, theme::MUTED);
    ui.painter().galley(
        Pos2::new(icon.right() + 6.0, rect.center().y - galley.size().y * 0.5),
        galley,
        theme::MUTED,
    );
    response
}

fn share_button(ui: &mut egui::Ui, width: f32, label: &str, running: bool) -> egui::Response {
    let desired = Vec2::new(width, 46.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    if running {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(24), theme::ACCENT_DARK);
    } else {
        theme::paint_h_gradient(ui.painter(), rect, theme::ACCENT, theme::ACCENT_LIGHT, 24);
    }
    // Soft lift shadow under the primary CTA.
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(24),
        egui::Stroke::new(1.0, Color32::from_white_alpha(28)),
        egui::StrokeKind::Inside,
    );
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        theme::bold(15.0),
        Color32::WHITE,
    );
    let total = 16.0 + 8.0 + galley.size().x;
    let start = rect.center().x - total * 0.5;
    let icon = Rect::from_center_size(Pos2::new(start + 8.0, rect.center().y), Vec2::splat(16.0));
    theme::paint_glyph(ui.painter(), icon, Glyph::Share, Color32::WHITE);
    ui.painter().galley(
        Pos2::new(icon.right() + 8.0, rect.center().y - galley.size().y * 0.5),
        galley,
        Color32::WHITE,
    );
    response
}

fn combo(
    ui: &mut egui::Ui,
    id: &'static str,
    width: f32,
    items: &[String],
    selected: &mut usize,
) {
    let text = items.get(*selected).cloned().unwrap_or_default();
    let desired = Vec2::new(width.max(80.0), 32.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(16),
        if hovered {
            Color32::from_rgb(0xEC, 0xE9, 0xFC)
        } else {
            Color32::from_rgb(0xF2, 0xF0, 0xFC)
        },
        egui::Stroke::new(
            1.0_f32,
            if hovered {
                theme::ACCENT_LINE
            } else {
                theme::CARD_LINE
            },
        ),
        egui::StrokeKind::Inside,
    );
    let shown = truncate_for_width(ui, &text, rect.width() - 40.0, 12.0);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        shown,
        egui::FontId::proportional(12.0),
        theme::TEXT,
    );
    // Clearer chevron so it reads as a dropdown.
    let cx = rect.right() - 16.0;
    let cy = rect.center().y;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(cx - 5.0, cy - 2.0),
            Pos2::new(cx + 5.0, cy - 2.0),
            Pos2::new(cx, cy + 4.0),
        ],
        theme::MUTED,
        egui::Stroke::NONE,
    ));
    // `from_toggle_button_response` already toggles open state on click —
    // do not also call `Popup::toggle_id` or the menu opens and closes in
    // the same frame (appears broken / stuck on one display).
    egui::Popup::from_toggle_button_response(&response)
        .id(ui.make_persistent_id(id))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(rect.width());
            for (i, item) in items.iter().enumerate() {
                let chosen = ui.selectable_label(*selected == i, item).clicked();
                if chosen {
                    *selected = i;
                    ui.close();
                }
            }
        });
}

fn truncate_for_width(ui: &egui::Ui, text: &str, max_w: f32, size: f32) -> String {
    if max_w <= 12.0 {
        return "…".into();
    }
    let font = egui::FontId::proportional(size);
    let full = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), theme::MUTED));
    if full.size().x <= max_w {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for n in (1..chars.len()).rev() {
        let candidate: String = chars[..n].iter().collect::<String>() + "…";
        let galley = ui.fonts(|f| f.layout_no_wrap(candidate.clone(), font.clone(), theme::MUTED));
        if galley.size().x <= max_w {
            return candidate;
        }
    }
    "…".into()
}
