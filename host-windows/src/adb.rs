use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Android applicationId — must match `android/app/build.gradle.kts`.
pub const CLIENT_PACKAGE: &str = "app.lighting.display";

/// Hide the console window that Windows would otherwise flash for each adb.exe.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn adb_command(adb: &Path) -> Command {
    let mut cmd = Command::new(adb);
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.kill_on_drop(true);
    cmd.stdin(Stdio::null());
    cmd
}

async fn adb_output(cmd: &mut Command, max: Duration) -> Result<std::process::Output, String> {
    match tokio::time::timeout(max, cmd.output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(err)) => Err(format!("{err:#}")),
        Err(_) => Err("超时".into()),
    }
}

async fn adb_args(adb: &Path, args: &[&str], max: Duration) -> Result<std::process::Output, String> {
    let mut cmd = adb_command(adb);
    cmd.args(args);
    adb_output(&mut cmd, max).await
}

fn probe_timeout() -> Duration {
    Duration::from_secs(lighting_host::apk_install::adb_probe_timeout_secs())
}

pub fn find_adb() -> Result<PathBuf> {
    for candidate in adb_candidates() {
        if candidate.is_file() {
            tracing::info!("using adb {}", candidate.display());
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "找不到 adb.exe。本机常见位置未加入 PATH。可把 platform-tools 加到系统 PATH，或把 adb.exe 放到仓库 .runtime\\android-sdk\\platform-tools\\"
    )
}

/// Look for a shippable APK next to the host or in the local android build tree.
pub fn find_bundled_apk() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    // Electron sets this to `resources/` (Lighting.apk lives next to lighting-host.exe).
    // LIGHTING_RUNTIME_DIR is adb/ffmpeg only — never look there first.
    if let Ok(dir) = std::env::var("LIGHTING_RESOURCES_DIR") {
        candidates.push(PathBuf::from(dir).join("Lighting.apk"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in [
                "Lighting.apk",
                "lighting.apk",
                "app-debug.apk",
                "app-release.apk",
            ] {
                candidates.push(dir.join(name));
            }
            candidates.push(
                dir.join("..")
                    .join("..")
                    .join("..")
                    .join("android")
                    .join("app")
                    .join("build")
                    .join("outputs")
                    .join("apk")
                    .join("debug")
                    .join("app-debug.apk"),
            );
        }
    }
    if let Ok(runtime) = std::env::var("LIGHTING_RUNTIME_DIR") {
        candidates.push(PathBuf::from(&runtime).join("Lighting.apk"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("android")
            .join("app")
            .join("build")
            .join("outputs")
            .join("apk")
            .join("debug")
            .join("app-debug.apk"),
    );
    candidates.into_iter().find(|p| p.is_file())
}

fn adb_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };

    if let Ok(p) = which::which("adb") {
        push(p);
    }

    if let Ok(runtime) = std::env::var("LIGHTING_RUNTIME_DIR") {
        push(PathBuf::from(&runtime).join("platform-tools").join("adb.exe"));
        push(PathBuf::from(&runtime).join("adb.exe"));
    }

    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = std::env::var(key) {
            push(PathBuf::from(root).join("platform-tools").join("adb.exe"));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push(dir.join("adb.exe"));
            push(dir.join("platform-tools").join("adb.exe"));
            push(
                dir.join("..")
                    .join("..")
                    .join("..")
                    .join(".runtime")
                    .join("android-sdk")
                    .join("platform-tools")
                    .join("adb.exe"),
            );
            push(
                dir.join("..")
                    .join("..")
                    .join("..")
                    .join(".runtime")
                    .join("platform-tools")
                    .join("adb.exe"),
            );
        }
    }

    push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".runtime")
            .join("android-sdk")
            .join("platform-tools")
            .join("adb.exe"),
    );
    push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".runtime")
            .join("platform-tools")
            .join("adb.exe"),
    );

    if let Some(home) = std::env::var_os("USERPROFILE") {
        push(PathBuf::from(&home).join("Desktop").join("platform-tools").join("adb.exe"));
        push(PathBuf::from(&home).join("Downloads").join("platform-tools").join("adb.exe"));
    }
    if let Some(appdata) = std::env::var_os("LOCALAPPDATA") {
        push(
            PathBuf::from(appdata)
                .join("Android")
                .join("Sdk")
                .join("platform-tools")
                .join("adb.exe"),
        );
    }

    push(PathBuf::from(r"C:\Program Files\ASUS\GlideX\adb.exe"));
    push(PathBuf::from(r"C:\Program Files\Software Fix\adb.exe"));
    push(PathBuf::from(r"C:\Android\platform-tools\adb.exe"));
    push(PathBuf::from(r"C:\platform-tools\adb.exe"));

    out
}

#[derive(Debug, Clone)]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
    /// `Some(true/false)` after a package probe; `None` if not probed.
    pub client_installed: Option<bool>,
    /// `versionName` from the installed client APK when available.
    pub client_version: Option<String>,
}

impl AdbDevice {
    pub fn label(&self) -> String {
        match &self.client_version {
            Some(v) if !v.is_empty() => format!("{} ({}) · v{v}", self.serial, self.state),
            _ => format!("{} ({})", self.serial, self.state),
        }
    }
}

pub async fn list_devices(adb: &Path) -> Result<Vec<AdbDevice>> {
    let output = adb_args(adb, &["devices"], probe_timeout())
        .await
        .map_err(|err| anyhow::anyhow!("adb devices: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let serial = parts.next().unwrap_or_default().to_string();
        let state = parts.next().unwrap_or("unknown").to_string();
        if !serial.is_empty() {
            devices.push(AdbDevice {
                serial,
                state,
                client_installed: None,
                client_version: None,
            });
        }
    }
    for device in &mut devices {
        if device.state == "device" {
            let installed = package_installed(adb, &device.serial, CLIENT_PACKAGE).await;
            device.client_installed = Some(installed);
            device.client_version = if installed {
                package_version(adb, &device.serial, CLIENT_PACKAGE).await
            } else {
                None
            };
        }
    }
    Ok(devices)
}

/// `adb shell pm path <package>` — empty stdout means not installed.
pub async fn package_installed(adb: &Path, serial: &str, package: &str) -> bool {
    let output = adb_args(
        adb,
        &["-s", serial, "shell", "pm", "path", package],
        probe_timeout(),
    )
    .await;
    match output {
        Ok(out) if out.status.success() => {
            lighting_host::apk_install::pm_path_means_installed(&String::from_utf8_lossy(
                &out.stdout,
            ))
        }
        _ => false,
    }
}

/// Best-effort `versionName` via `dumpsys package`. Must not block install forever.
pub async fn package_version(adb: &Path, serial: &str, package: &str) -> Option<String> {
    let output = adb_args(
        adb,
        &["-s", serial, "shell", "dumpsys", "package", package],
        probe_timeout(),
    )
    .await
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_version_name(&stdout)
}

/// Extract `versionName=` from `dumpsys package` output.
pub fn parse_version_name(dumpsys: &str) -> Option<String> {
    lighting_host::apk_install::parse_version_name(dumpsys)
}

async fn uninstall_package(adb: &Path, serial: &str, package: &str) -> Result<()> {
    let output = adb_args(
        adb,
        &["-s", serial, "uninstall", package],
        Duration::from_secs(lighting_host::apk_install::uninstall_timeout_secs()),
    )
    .await
    .map_err(|err| anyhow::anyhow!("卸载旧客户端失败: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success()
        || stdout.to_ascii_lowercase().contains("success")
        || stderr.to_ascii_lowercase().contains("not found")
        || stdout.to_ascii_lowercase().contains("not found")
    {
        return Ok(());
    }
    anyhow::bail!(
        "卸载旧客户端失败: {}",
        if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        }
    )
}

async fn force_stop_package(adb: &Path, serial: &str, package: &str) {
    let _ = adb_args(
        adb,
        &["-s", serial, "shell", "am", "force-stop", package],
        probe_timeout(),
    )
    .await;
}

async fn wait_for_device(adb: &Path, serial: &str) {
    let _ = adb_args(
        adb,
        &["-s", serial, "wait-for-device"],
        Duration::from_secs(lighting_host::apk_install::wait_for_device_timeout_secs()),
    )
    .await;
}

async fn launch_client(adb: &Path, serial: &str) {
    let component = lighting_host::apk_install::launcher_component();
    let _ = adb_args(
        adb,
        &[
            "-s",
            serial,
            "shell",
            "am",
            "start",
            "-n",
            component,
            "-a",
            "android.intent.action.MAIN",
            "-c",
            "android.intent.category.LAUNCHER",
        ],
        probe_timeout(),
    )
    .await;
}

/// Open the landscape stream activity on 127.0.0.1 so the user does not have
/// to tap USB 一键连接 after 开始共享. `am start` from adb shell can launch
/// the non-exported DisplayActivity. Honor sometimes rejects `--user 0` or
/// the non-exported component; fall back to the exported MainActivity.
pub async fn launch_stream_client(adb: &Path, serial: &str, port: u16) {
    wait_for_device(adb, serial).await;
    let port_s = port.to_string();
    let display = lighting_host::apk_install::display_component();
    let launcher = lighting_host::apk_install::launcher_component();
    let timeout = Duration::from_secs(lighting_host::apk_install::am_start_timeout_secs());
    let display_user = [
        "-s",
        serial,
        "shell",
        "am",
        "start",
        "--user",
        "0",
        "--activity-single-top",
        "-n",
        display,
        "--es",
        "host",
        "127.0.0.1",
        "--ei",
        "port",
        &port_s,
    ];
    if try_am_start(adb, &display_user, timeout).await {
        return;
    }
    let display_plain = [
        "-s",
        serial,
        "shell",
        "am",
        "start",
        "--activity-single-top",
        "-n",
        display,
        "--es",
        "host",
        "127.0.0.1",
        "--ei",
        "port",
        &port_s,
    ];
    if try_am_start(adb, &display_plain, timeout).await {
        return;
    }
    let launcher_auto = [
        "-s",
        serial,
        "shell",
        "am",
        "start",
        "--user",
        "0",
        "--activity-single-top",
        "-n",
        launcher,
        "--ez",
        "lightingAutoUsb",
        "true",
    ];
    if try_am_start(adb, &launcher_auto, timeout).await {
        return;
    }
    let launcher_plain = [
        "-s",
        serial,
        "shell",
        "am",
        "start",
        "-n",
        launcher,
        "-a",
        "android.intent.action.MAIN",
        "-c",
        "android.intent.category.LAUNCHER",
        "--ez",
        "lightingAutoUsb",
        "true",
    ];
    let _ = try_am_start(adb, &launcher_plain, timeout).await;
}

async fn try_am_start(adb: &Path, args: &[&str], max: Duration) -> bool {
    match adb_args(adb, args, max).await {
        Ok(out) => {
            let combined = combined_output(&out);
            let ok = lighting_host::apk_install::am_start_succeeded(&combined);
            if !ok {
                tracing::warn!("am start rejected: {}", combined.trim());
            }
            ok
        }
        Err(err) => {
            tracing::warn!("am start failed: {err}");
            false
        }
    }
}

fn combined_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!("{stdout}\n{stderr}")
}

async fn adb_install_once(
    adb: &Path,
    serial: &str,
    apk: &Path,
    flags: &[&str],
) -> Result<(), String> {
    let mut cmd = adb_command(adb);
    cmd.env("ADB_INSTALL_INCR", "0");
    cmd.args(["-s", serial, "install"]);
    cmd.args(flags);
    cmd.arg(apk);
    let output = adb_output(
        &mut cmd,
        Duration::from_secs(lighting_host::apk_install::install_timeout_secs()),
    )
    .await
    .map_err(|_| lighting_host::apk_install::timeout_hint().to_string())?;
    let combined = combined_output(&output);
    if output.status.success() || lighting_host::apk_install::install_succeeded(&combined) {
        return Ok(());
    }
    Err(if combined.trim().is_empty() {
        "安装失败，没有返回原因".into()
    } else {
        combined.trim().to_string()
    })
}

async fn adb_install_with_attempts(
    adb: &Path,
    serial: &str,
    apk: &Path,
    attempts: &'static [&'static [&'static str]],
) -> Result<(), String> {
    let mut last = String::from("安装失败");
    for flags in attempts {
        match adb_install_once(adb, serial, apk, flags).await {
            Ok(()) => return Ok(()),
            Err(err) if lighting_host::apk_install::unknown_adb_option(&err) => {
                last = err;
                continue;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last)
}

/// Cover-install the client. Uninstall only when the signatures actually
/// conflict — doing it first left the pad empty whenever `adb install` hung.
pub async fn install_apk(adb: &Path, serial: &str, apk: &Path) -> Result<String> {
    let package = CLIENT_PACKAGE;
    let expected = lighting_host::apk_install::expected_client_version();
    if package_installed(adb, serial, package).await {
        force_stop_package(adb, serial, package).await;
    }
    if let Err(err) = adb_install_with_attempts(
        adb,
        serial,
        apk,
        lighting_host::apk_install::install_replace_attempts(),
    )
    .await
    {
        if lighting_host::apk_install::user_action_required(&err) {
            anyhow::bail!("{}", lighting_host::apk_install::user_restricted_hint());
        }
        if lighting_host::apk_install::needs_uninstall_reinstall(&err) {
            uninstall_package(adb, serial, package)
                .await
                .context("签名不一致，卸载旧客户端")?;
            wait_for_device(adb, serial).await;
            tokio::time::sleep(Duration::from_millis(
                lighting_host::apk_install::settle_after_uninstall_ms(),
            ))
            .await;
            adb_install_with_attempts(
                adb,
                serial,
                apk,
                lighting_host::apk_install::install_fresh_attempts(),
            )
            .await
            .map_err(|retry| {
                if lighting_host::apk_install::user_action_required(&retry) {
                    anyhow::anyhow!("{}", lighting_host::apk_install::user_restricted_hint())
                } else {
                    anyhow::anyhow!("安装失败: {retry}")
                }
            })?;
        } else {
            anyhow::bail!("安装失败: {err}");
        }
    }
    let installed = package_installed(adb, serial, package).await;
    let version = package_version(adb, serial, package).await;
    let verified = lighting_host::apk_install::verified_install_version(
        installed,
        version.as_deref(),
        expected,
    )
    .map_err(|err| anyhow::anyhow!("{err}"))?;
    if installed {
        launch_client(adb, serial).await;
    }
    Ok(verified)
}

pub async fn reverse_port(adb: &Path, serial: &str, port: u16) -> Result<()> {
    wait_for_device(adb, serial).await;
    let spec = format!("tcp:{port}");
    let max = Duration::from_secs(lighting_host::apk_install::adb_reverse_timeout_secs());
    let mut last = String::from("adb reverse 失败");
    for _ in 0..2 {
        let output = match adb_args(adb, &["-s", serial, "reverse", &spec, &spec], max).await {
            Ok(out) => out,
            Err(err) => {
                last = err;
                continue;
            }
        };
        if output.status.success() {
            return Ok(());
        }
        last = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if last.is_empty() {
            last = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    anyhow::bail!("adb reverse 失败: {last}")
}

pub async fn remove_reverse(adb: &Path, serial: &str, port: u16) -> Result<()> {
    let spec = format!("tcp:{port}");
    let _ = adb_args(
        adb,
        &["-s", serial, "reverse", "--remove", &spec],
        probe_timeout(),
    )
    .await;
    Ok(())
}
