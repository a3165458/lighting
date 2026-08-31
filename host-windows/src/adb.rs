use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
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
    cmd
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
}

impl AdbDevice {
    pub fn label(&self) -> String {
        format!("{} ({})", self.serial, self.state)
    }
}

pub async fn list_devices(adb: &Path) -> Result<Vec<AdbDevice>> {
    let output = adb_command(adb)
        .arg("devices")
        .output()
        .await
        .context("adb devices")?;
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
            });
        }
    }
    for device in &mut devices {
        if device.state == "device" {
            device.client_installed =
                Some(package_installed(adb, &device.serial, CLIENT_PACKAGE).await);
        }
    }
    Ok(devices)
}

/// `adb shell pm path <package>` — empty stdout means not installed.
pub async fn package_installed(adb: &Path, serial: &str, package: &str) -> bool {
    let output = adb_command(adb)
        .args(["-s", serial, "shell", "pm", "path", package])
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().any(|l| l.trim().starts_with("package:"))
        }
        _ => false,
    }
}

pub async fn install_apk(adb: &Path, serial: &str, apk: &Path) -> Result<()> {
    let output = adb_command(adb)
        .args(["-s", serial, "install", "-r"])
        .arg(apk)
        .output()
        .await
        .context("adb install")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "安装失败: {}",
            if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            }
        );
    }
    Ok(())
}

pub async fn reverse_port(adb: &Path, serial: &str, port: u16) -> Result<()> {
    let spec = format!("tcp:{port}");
    let output = adb_command(adb)
        .args(["-s", serial, "reverse", &spec, &spec])
        .output()
        .await
        .context("adb reverse")?;
    if !output.status.success() {
        anyhow::bail!(
            "adb reverse 失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub async fn remove_reverse(adb: &Path, serial: &str, port: u16) -> Result<()> {
    let spec = format!("tcp:{port}");
    let _ = adb_command(adb)
        .args(["-s", serial, "reverse", "--remove", &spec])
        .output()
        .await;
    Ok(())
}
