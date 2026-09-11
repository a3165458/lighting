#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adb;
mod audio;
mod cursor;
mod display_topology;
mod displays;
mod encoder;
mod input;
mod ipc;
mod protocol;
mod service;
mod session;
mod suspend;

use std::time::Duration;

use eframe::egui;
use lighting_host::host_ipc::{DEFAULT_PORT, PORT_ENV, TOKEN_ENV};
use lighting_host::theme;
use lighting_host::view::{self, Action};
use service::HostService;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

fn enable_dpi_awareness() {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

/// Sunshine / GlideX: 1 ms timer so 1 ms cursor / HID sleeps are not 15.6 ms.
fn enable_timer_resolution() {
    unsafe {
        let _ = windows::Win32::Media::timeBeginPeriod(1);
    }
}

/// ffmpeg is HIGH_PRIORITY_CLASS. A NORMAL host loses the pointer sampler
/// (and the TCP write loop) whenever NVENC is busy — that's the tablet
/// mouse sitting a refresh behind the laptop.
fn enable_process_priority() {
    unsafe {
        let _ = windows::Win32::System::Threading::SetPriorityClass(
            windows::Win32::System::Threading::GetCurrentProcess(),
            windows::Win32::System::Threading::HIGH_PRIORITY_CLASS,
        );
    }
}

/// Windows 11 EcoQoS / timer-resolution ignore kicks in when the game owns
/// the virtual display and this window is in the background. 1 ms cursor
/// sleeps and ffmpeg's pipe reads then sit on a 15.6 ms grid next to the
/// laptop panel. Opt out of both: keep P-cores and honor timeBeginPeriod.
fn disable_power_throttling() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
    };
    unsafe {
        let process = GetCurrentProcess();
        let mut state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: 0,
        };
        if SetProcessInformation(
            process,
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&state) as u32,
        )
        .is_err()
        {
            state.ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
            let _ = SetProcessInformation(
                process,
                ProcessPowerThrottling,
                &state as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&state) as u32,
            );
        }
    }
}

/// Sunshine: DWM on MMCSS so the IddCx virtual panel keeps 120 Hz while a
/// game loads the GPU. Without this the laptop (independent flip) stays
/// snappy and the tablet, which only sees DWM's copy, sits a refresh behind.
fn enable_dwm_mmcss() {
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmEnableMMCSS(true);
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
    disable_power_throttling();
    enable_timer_resolution();
    enable_process_priority();
    enable_dwm_mmcss();
    init_tracing();
    displays::raise_gpu_scheduling(unsafe {
        windows::Win32::System::Threading::GetCurrentProcess()
    });

    let service = HostService::new();
    if wants_ipc_only() {
        let port = ipc::resolve_port();
        let Some(token) = ipc::resolve_token() else {
            tracing::error!("--ipc-only requires a 32+ character {TOKEN_ENV}");
            return Ok(());
        };
        ipc::write_port_file(port);
        ipc::spawn_background(service.clone_handle(), port, token);
        tracing::info!("Lighting host IPC on 127.0.0.1:{port} (override with {PORT_ENV})");
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

        let actions = self
            .service
            .with_ui(|settings, snapshot| view::render(ctx, &snapshot, settings));

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
                    self.service
                        .patch_settings(lighting_host::host_ipc::SettingsPatchDto {
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
