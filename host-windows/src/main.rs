#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adb;
mod audio;
mod displays;
mod encoder;
mod input;
mod ipc;
mod protocol;
mod service;
mod session;

use std::time::Duration;

use eframe::egui;
use lighting_host::host_ipc::{DEFAULT_PORT, PORT_ENV};
use lighting_host::view::{self, Action};
use lighting_host::theme;
use service::HostService;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

fn enable_dpi_awareness() {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

fn log_writer() -> BoxMakeWriter {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("lighting-host.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("lighting-host.log"));
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => BoxMakeWriter::new(std::sync::Mutex::new(file)),
        Err(_) => BoxMakeWriter::new(std::io::sink),
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lighting_host=info,warn".into()),
        )
        .with_writer(log_writer())
        .with_ansi(false)
        .init();
}

fn wants_ipc_only() -> bool {
    std::env::args().any(|a| a == "--ipc-only" || a == "--headless")
}

fn main() -> eframe::Result<()> {
    enable_dpi_awareness();
    init_tracing();

    let service = HostService::new();
    let port = ipc::resolve_port();
    ipc::write_port_file(port);
    ipc::spawn_background(service.clone_handle(), port);
    tracing::info!(
        "Lighting host IPC on 127.0.0.1:{port} (override with {PORT_ENV})"
    );

    if wants_ipc_only() {
        tracing::info!("running in --ipc-only mode (no egui window)");
        loop {
            service.tick();
            service.set_running_flag_from_status();
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 1024.0])
            .with_min_inner_size([540.0, 920.0])
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(true)
            .with_title("Lighting 副屏"),
        ..Default::default()
    };
    eframe::run_native(
        "Lighting",
        native,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(LightingApp { service }))
        }),
    )
}

struct LightingApp {
    service: HostService,
}

impl eframe::App for LightingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.service.tick();
        self.service.set_running_flag_from_status();

        let actions = self.service.with_ui(|settings, snapshot| {
            view::render(ctx, &snapshot, settings)
        });

        for action in actions {
            match action {
                Action::Start => {
                    if let Err(err) = self.service.start_share() {
                        tracing::warn!("start share failed: {err}");
                    }
                }
                Action::Stop => self.service.stop_share(),
                Action::Refresh => self.service.refresh(),
                Action::InstallClient => {
                    let _ = self.service.install_client();
                }
                Action::TouchRelayChanged => {
                    // Settings already mutated by the view; push touch flag live.
                    let state = self.service.state();
                    self.service.patch_settings(lighting_host::host_ipc::SettingsPatchDto {
                        touch_relay: Some(state.settings.touch_relay),
                        ..Default::default()
                    });
                }
            }
        }

        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

// Silence unused import when someone greps DEFAULT_PORT from main.
#[allow(dead_code)]
fn _port_doc() -> u16 {
    DEFAULT_PORT
}
