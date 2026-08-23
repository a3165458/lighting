//! The host window layout.
//!
//! Pure egui: it reads a [`Snapshot`] of the session, mutates [`Settings`] the
//! user owns, and hands back [`Action`]s for the shell to run. Keeping it free
//! of Win32 calls lets the layout be rendered and reviewed off Windows.

use egui::{Color32, Margin, Vec2};

use crate::theme::{self, Glyph};
use crate::ui_text::{self, Tone};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ResCap {
    #[default]
    Device,
    Fhd,
    Uhd2k,
}

impl ResCap {
    pub const ALL: [ResCap; 3] = [ResCap::Device, ResCap::Fhd, ResCap::Uhd2k];

    pub fn label(self) -> &'static str {
        match self {
            ResCap::Device => "跟随平板",
            ResCap::Fhd => "最高 1080p",
            ResCap::Uhd2k => "最高 2K",
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
            quality_pct: 75,
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
    TouchRelayChanged,
}

pub fn render(ctx: &egui::Context, snap: &Snapshot, settings: &mut Settings) -> Vec<Action> {
    let mut actions = Vec::new();

    egui::TopBottomPanel::bottom("status_strip")
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(0xEF, 0xED, 0xF8))
                .inner_margin(Margin::symmetric(16, 6)),
        )
        .show(ctx, |ui| status_strip(ui, snap));

    egui::TopBottomPanel::bottom("action_bar")
        .frame(
            egui::Frame::new()
                .fill(theme::CARD)
                .inner_margin(Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| action_bar(ui, snap, settings, &mut actions));

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(Margin::symmetric(16, 14)),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    hero(ui);
                    connection_card(ui, snap, settings, &mut actions);
                    display_card(ui, snap, settings);
                    transport_card(ui, snap, settings);
                    controls_card(ui, settings, &mut actions);
                    if settings.show_advanced {
                        advanced_card(ui, snap, settings, &mut actions);
                    }
                    if settings.show_about {
                        about_card(ui);
                    }
                });
        });

    actions
}

fn hero(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(
                egui::RichText::new("Lighting 副屏")
                    .font(theme::bold(24.0))
                    .color(theme::TEXT),
            );
            ui.label(
                egui::RichText::new("将你的平板 / 手机变成电脑扩展屏")
                    .size(12.5)
                    .color(theme::MUTED),
            );
            ui.label(
                egui::RichText::new("低延迟 · 高画质 · 触控回传")
                    .size(12.0)
                    .color(theme::ACCENT),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    theme::glyph(ui, Glyph::Wifi, theme::ACCENT, 15.0);
                });
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(130.0, 70.0), egui::Sense::hover());
                theme::hero_art(ui.painter(), rect);
            });
        });
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
            "插上数据线，点下面的「开始共享」".into()
        };
        ui.horizontal(|ui| {
            if streaming {
                theme::glyph_badge(ui, Glyph::Check, theme::OK, theme::OK_SOFT, 22.0);
            } else if snap.running {
                theme::glyph_badge(ui, Glyph::Monitor, theme::ACCENT, theme::ACCENT_SOFT, 22.0);
            } else {
                theme::glyph_badge(ui, Glyph::Monitor, theme::DIM, theme::TRACK, 22.0);
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    egui::RichText::new(ui_text::connection_title(
                        snap.running,
                        &snap.phase,
                        &snap.client_name,
                    ))
                    .font(theme::bold(13.0))
                    .color(theme::TEXT),
                );
                ui.label(egui::RichText::new(&second).size(11.5).color(theme::DIM));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if snap.running && ui.add(outline_button("断开连接")).clicked() {
                    actions.push(Action::Stop);
                }
            });
        });

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

        let detail = ui_text::human_detail_text(&snap.phase, &snap.detail);
        if !detail.is_empty() && !second.contains(&detail) {
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
    theme::card(ui, "扩展屏设置", |ui| {
        setting_row(ui, Glyph::Monitor, "显示器", |ui| {
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
                    236.0,
                    &snap.displays,
                    &mut settings.selected_display,
                );
            }
        });
        slider_row(
            ui,
            Glyph::Gauge,
            "画质",
            format!("{}%", settings.quality_pct),
            |ui| {
                ui.add(
                    egui::Slider::new(&mut settings.quality_pct, 40..=100)
                        .show_value(false)
                        .trailing_fill(true),
                );
            },
        );
        slider_row(
            ui,
            Glyph::Film,
            "帧率",
            format!("{} fps", settings.fps),
            |ui| {
                ui.add(
                    egui::Slider::new(&mut settings.fps, 30..=120)
                        .show_value(false)
                        .trailing_fill(true),
                );
            },
        );
        slider_row(
            ui,
            Glyph::Meter,
            "码率",
            format!("{} kbps", settings.bitrate_kbps),
            |ui| {
                ui.add(
                    egui::Slider::new(&mut settings.bitrate_kbps, 5_000..=40_000)
                        .show_value(false)
                        .trailing_fill(true)
                        .step_by(1_000.0),
                );
            },
        );
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
    theme::card(ui, "传输与性能", |ui| {
        ui.columns(4, |cols| {
            metric_tile(
                &mut cols[0],
                Glyph::Route,
                "传输协议",
                ui_text::transport_text(snap.running, &snap.transport),
                theme::TEXT,
            );
            metric_tile(
                &mut cols[1],
                Glyph::Chip,
                "编码方式",
                ui_text::codec_text(&snap.codec, settings.prefer_hevc),
                theme::TEXT,
            );
            metric_tile(
                &mut cols[2],
                Glyph::Bolt,
                "延迟",
                ui_text::latency_text(snap.latency_ms),
                latency_color(snap.latency_ms),
            );
            metric_tile(
                &mut cols[3],
                Glyph::Target,
                "丢包率",
                ui_text::loss_text(snap.running, snap.loss_permille),
                theme::TEXT,
            );
        });
    });
}

fn controls_card(ui: &mut egui::Ui, settings: &mut Settings, actions: &mut Vec<Action>) {
    theme::card(ui, "控制与交互", |ui| {
        if toggle_row(
            ui,
            Glyph::Touch,
            "触控回传（模拟鼠标）",
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
            Some("即将支持"),
            &mut settings.clipboard_share,
            false,
        );
    });
}

fn advanced_card(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    settings: &mut Settings,
    actions: &mut Vec<Action>,
) {
    theme::card(ui, "高级设置", |ui| {
        ui.label(
            egui::RichText::new("局域网绑定（一般不用改）")
                .size(11.5)
                .color(theme::DIM),
        );
        ui.add_space(2.0);
        setting_row(ui, Glyph::Route, "地址", |ui| {
            ui.add_sized(
                Vec2::new(170.0, 22.0),
                egui::TextEdit::singleline(&mut settings.bind_host)
                    .font(egui::FontId::proportional(12.0)),
            );
        });
        setting_row(ui, Glyph::Plug, "端口", |ui| {
            ui.add(egui::DragValue::new(&mut settings.bind_port).range(1..=65535));
        });

        ui.add_space(6.0);
        setting_row(ui, Glyph::Monitor, "输出上限", |ui| {
            egui::ComboBox::from_id_salt("res_cap")
                .width(140.0)
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
                    200.0,
                    &snap.devices,
                    &mut settings.selected_device,
                );
            }
        });

        ui.add_space(6.0);
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
        ui.add_space(4.0);
        if ui.add(outline_button("刷新显示器 / 设备")).clicked() {
            actions.push(Action::Refresh);
        }
    });
}

fn about_card(ui: &mut egui::Ui) {
    theme::card(ui, "关于", |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(
            egui::RichText::new(format!("Lighting 副屏 v{}", env!("CARGO_PKG_VERSION")))
                .font(theme::bold(12.5))
                .color(theme::TEXT),
        );
        for line in [
            "扩展模式：winget install VirtualDrivers.Virtual-Display-Driver，然后在 Windows 显示设置里设为「扩展」并选这块虚拟屏。",
            "平板触控：单击 / 拖动、长按右键、双指滚动。",
            "局域网连接：在两边的「高级」里填写电脑地址即可。",
        ] {
            ui.label(egui::RichText::new(line).size(11.5).color(theme::MUTED));
        }
    });
}

fn action_bar(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    settings: &mut Settings,
    actions: &mut Vec<Action>,
) {
    let line_y = ui.max_rect().top() - 10.0;
    ui.painter().hline(
        ui.max_rect().x_range().expand(16.0),
        line_y,
        egui::Stroke::new(1.0_f32, theme::CARD_LINE),
    );
    ui.columns(3, |cols| {
        cols[0].with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if ghost_button(ui, Glyph::Gear, "高级设置").clicked() {
                settings.show_advanced = !settings.show_advanced;
            }
        });
        let width = cols[1].available_width();
        let label = ui_text::share_button_label(snap.running);
        let clicked = cols[1]
            .add_sized(
                Vec2::new(width, 40.0),
                egui::Button::new(
                    egui::RichText::new(label)
                        .font(theme::bold(14.5))
                        .color(Color32::WHITE),
                )
                .fill(if snap.running {
                    theme::ACCENT_DARK
                } else {
                    theme::ACCENT
                })
                .corner_radius(egui::CornerRadius::same(10)),
            )
            .clicked();
        if clicked {
            actions.push(if snap.running {
                Action::Stop
            } else {
                Action::Start
            });
        }
        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ghost_button(ui, Glyph::Info, "关于").clicked() {
                settings.show_about = !settings.show_about;
            }
        });
    });
}

fn status_strip(ui: &mut egui::Ui, snap: &Snapshot) {
    let (text, tone) = ui_text::health_text(snap.running, &snap.phase, snap.latency_ms);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, tone_color(tone));
        ui.label(egui::RichText::new(text).size(11.5).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(ui_text::format_duration(snap.connected_secs))
                    .size(11.5)
                    .color(theme::MUTED),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!(
                    "已传输 {}",
                    ui_text::format_bytes(snap.bytes_sent)
                ))
                .size(11.5)
                .color(theme::MUTED),
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

/// Left icon + label, right-aligned control.
fn setting_row(ui: &mut egui::Ui, g: Glyph, label: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        theme::glyph(ui, g, theme::MUTED, 16.0);
        ui.add_space(1.0);
        ui.label(egui::RichText::new(label).size(12.5).color(theme::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

/// Like [`setting_row`], but reserves a fixed value column so the 画质 / 帧率 /
/// 码率 sliders start and end on the same pixels.
fn slider_row(
    ui: &mut egui::Ui,
    g: Glyph,
    label: &str,
    value: String,
    add: impl FnOnce(&mut egui::Ui),
) {
    setting_row(ui, g, label, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(80.0, 18.0), egui::Sense::hover());
        ui.painter().text(
            rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            value,
            egui::FontId::proportional(12.0),
            theme::TEXT,
        );
        ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(60.0);
        add(ui);
    });
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
    ui.horizontal(|ui| {
        theme::glyph(
            ui,
            g,
            if enabled { theme::MUTED } else { theme::DIM },
            16.0,
        );
        ui.add_space(1.0);
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = theme::switch(ui, on, enabled).changed();
        });
    });
    ui.add_space(2.0);
    changed
}

fn metric_tile(ui: &mut egui::Ui, g: Glyph, label: &str, value: String, value_color: Color32) {
    ui.horizontal(|ui| {
        theme::glyph_badge(ui, g, theme::ACCENT, theme::ACCENT_SOFT, 22.0);
        ui.add_space(1.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(label).size(10.5).color(theme::DIM));
            ui.label(egui::RichText::new(value).size(11.5).color(value_color));
        });
    });
}

fn outline_button(label: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label.to_owned())
            .size(12.0)
            .color(theme::ACCENT),
    )
    .fill(Color32::WHITE)
    .stroke(egui::Stroke::new(1.0_f32, theme::ACCENT_LINE))
    .corner_radius(egui::CornerRadius::same(8))
}

fn ghost_button(ui: &mut egui::Ui, g: Glyph, label: &str) -> egui::Response {
    ui.horizontal(|ui| {
        theme::glyph(ui, g, theme::MUTED, 15.0);
        ui.add(
            egui::Button::new(egui::RichText::new(label).size(12.0).color(theme::MUTED))
                .frame(false),
        )
    })
    .inner
}

fn combo(
    ui: &mut egui::Ui,
    id: &'static str,
    width: f32,
    items: &[String],
    selected: &mut usize,
) {
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(
            egui::RichText::new(items.get(*selected).cloned().unwrap_or_default()).size(12.0),
        )
        .show_ui(ui, |ui| {
            for (i, item) in items.iter().enumerate() {
                ui.selectable_value(selected, i, item);
            }
        });
}
