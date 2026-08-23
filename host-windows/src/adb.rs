use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

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

    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = std::env::var(key) {
            push(PathBuf::from(root).join("platform-tools").join("adb.exe"));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // target/release/lighting-host.exe → repo/.runtime/...
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
}

impl AdbDevice {
    pub fn label(&self) -> String {
        format!("{} ({})", self.serial, self.state)
    }
}

pub async fn list_devices(adb: &Path) -> Result<Vec<AdbDevice>> {
    let output = Command::new(adb)
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
            devices.push(AdbDevice { serial, state });
        }
    }
    Ok(devices)
}

pub async fn reverse_port(adb: &Path, serial: &str, port: u16) -> Result<()> {
    let spec = format!("tcp:{port}");
    let output = Command::new(adb)
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
    let _ = Command::new(adb)
        .args(["-s", serial, "reverse", "--remove", &spec])
        .output()
        .await;
    Ok(())
}
