//! Policy for shipping / replacing the Android client.
//!
//! CI `assembleDebug` marks the APK test-only, so `adb install` needs `-t`.
//! Always uninstalling first left the pad with no app when install then hung
//! (incremental session, OEM "USB 安装", or `dumpsys package` never returning).
//! Cover in place; uninstall only on signature mismatch. Never report a version
//! the pad did not actually dump. `versionName` in dumpsys can list a hidden
//! 0.1.0 package *before* the active one — parse the `Packages:` block.

pub const CLIENT_PACKAGE: &str = "app.lighting.display";

pub fn expected_client_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn launcher_component() -> &'static str {
    "app.lighting.display/app.lighting.display.MainActivity"
}

/// `adb install` flags after `-s <serial> install`.
/// `-r` replace, `-d` downgrade, `-g` grant, `-t` test-only (CI debug APK).
pub fn install_replace_flags() -> &'static [&'static str] {
    &["-r", "-d", "-g", "-t"]
}

/// Flag sets tried in order. Newer platform-tools hang on incremental install
/// of a debug APK; older GlideX `adb.exe` does not know `--no-incremental`.
pub fn install_replace_attempts() -> &'static [&'static [&'static str]] {
    &[
        &["--no-incremental", "-r", "-d", "-g", "-t"],
        &["-r", "-d", "-g", "-t"],
        &["-r", "-d", "-g"],
    ]
}

pub fn install_fresh_attempts() -> &'static [&'static [&'static str]] {
    &[
        &["--no-incremental", "-g", "-t"],
        &["-g", "-t"],
        &["-g"],
    ]
}

/// Do not uninstall the working client before the new APK is known to install.
pub fn uninstall_before_install() -> bool {
    false
}

pub fn install_timeout_secs() -> u64 {
    90
}

pub fn adb_probe_timeout_secs() -> u64 {
    8
}

pub fn uninstall_timeout_secs() -> u64 {
    30
}

pub fn wait_for_device_timeout_secs() -> u64 {
    20
}

pub fn settle_after_uninstall_ms() -> u64 {
    1500
}

pub fn needs_uninstall_reinstall(output: &str) -> bool {
    let upper = output.to_ascii_uppercase();
    upper.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE")
        || upper.contains("INSTALL_FAILED_VERSION_DOWNGRADE")
        || upper.contains("INSTALL_FAILED_ALREADY_EXISTS")
        || upper.contains("INCONSISTENT CERTIFICATES")
        || upper.contains("SIGNATURES ARE INCONSISTENT")
}

pub fn unknown_adb_option(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("unknown option")
        || lower.contains("unrecognized")
        || lower.contains("not a valid option")
        || lower.contains("unexpected argument")
        || lower.contains("unknown command")
}

pub fn user_action_required(output: &str) -> bool {
    let upper = output.to_ascii_uppercase();
    upper.contains("INSTALL_FAILED_USER_RESTRICTED")
        || upper.contains("INSTALL_FAILED_ABORTED")
        || upper.contains("INSTALL_CANCELED_BY_USER")
        || upper.contains("INSTALL_FAILED_USER_RESTRICTED")
}

pub fn timeout_hint() -> &'static str {
    "安装超时。多数平板不会弹「允许安装」。请点亮屏幕，打开开发者选项里的「USB安装 / 通过USB安装应用」，然后重试。这次不会先卸载旧客户端。"
}

pub fn user_restricted_hint() -> &'static str {
    "平板禁止 USB 安装。请点亮屏幕，打开开发者选项里的「USB安装 / USB调试（安全设置）」，再点重新安装。"
}

pub fn install_succeeded(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("success") && !lower.contains("failure [") && !lower.contains("failed to")
}

/// `pm path` stdout: ignore staged copies under `/data/local/tmp`.
pub fn pm_path_means_installed(stdout: &str) -> bool {
    stdout.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("package:") && !trimmed.contains("/data/local/tmp")
    })
}

/// Active `versionName` from `dumpsys package <id>`.
pub fn parse_version_name(dumpsys: &str) -> Option<String> {
    let mut section = Section::Unknown;
    let mut fallback: Option<String> = None;
    for line in dumpsys.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("Packages:") {
            section = Section::Packages;
            continue;
        }
        if trimmed.eq_ignore_ascii_case("Hidden system packages:")
            || trimmed.eq_ignore_ascii_case("Disabled Packages:")
        {
            section = Section::Hidden;
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("versionName=") else {
            continue;
        };
        let ver = rest.trim().trim_matches('"').trim();
        if ver.is_empty() {
            continue;
        }
        match section {
            Section::Packages => return Some(ver.to_string()),
            Section::Hidden => {}
            Section::Unknown => {
                if fallback.is_none() {
                    fallback = Some(ver.to_string());
                }
            }
        }
    }
    fallback
}

pub fn versions_match(installed: &str, expected: &str) -> bool {
    normalize_ver(installed) == normalize_ver(expected)
}

fn normalize_ver(v: &str) -> String {
    v.trim().trim_start_matches('v').trim().to_string()
}

/// If dumpsys returned a version, it must match the bundled APK. Empty is
/// "installed but dumpsys timed out" — do not invent the host version.
pub fn verified_install_version(
    installed_ok: bool,
    dumpsys_version: Option<&str>,
    expected: &str,
) -> Result<String, String> {
    if !installed_ok {
        return Err("安装命令已返回，但平板上仍没有 Lighting 客户端".into());
    }
    match dumpsys_version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(ver) if versions_match(ver, expected) => Ok(ver.trim_start_matches('v').to_string()),
        Some(ver) => Err(format!(
            "平板上的客户端是 v{}，便携包是 v{}。没有换成功，不会谎报版本",
            normalize_ver(ver),
            normalize_ver(expected)
        )),
        None => Ok(String::new()),
    }
}

#[derive(Clone, Copy)]
enum Section {
    Unknown,
    Packages,
    Hidden,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_flags_allow_debug_apk_and_downgrade() {
        let flags = install_replace_flags();
        assert!(flags.contains(&"-r"));
        assert!(flags.contains(&"-d"));
        assert!(flags.contains(&"-g"));
        assert!(flags.contains(&"-t"));
        assert!(!uninstall_before_install());
        assert!(install_replace_attempts()[0].contains(&"--no-incremental"));
        assert!(install_timeout_secs() >= 30);
    }

    #[test]
    fn signature_mismatch_needs_uninstall() {
        assert!(needs_uninstall_reinstall(
            "Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE: Package signatures do not match]"
        ));
        assert!(needs_uninstall_reinstall("INSTALL_FAILED_VERSION_DOWNGRADE"));
        assert!(!needs_uninstall_reinstall("Success"));
        assert!(user_action_required(
            "Failure [INSTALL_FAILED_USER_RESTRICTED]"
        ));
        assert!(unknown_adb_option("unknown option `--no-incremental`"));
        assert!(install_succeeded("Performing Streamed Install\nSuccess\n"));
        assert!(!install_succeeded(
            "Failure [INSTALL_FAILED_TEST_ONLY: Failed to install]"
        ));
    }

    #[test]
    fn dumpsys_prefers_active_packages_over_hidden_0_1_0() {
        let dumpsys = r#"
Hidden system packages:
  Package [app.lighting.display] (deadbeef):
    versionCode=1 minSdk=26 targetSdk=35
    versionName=0.1.0
Packages:
  Package [app.lighting.display] (cafebabe):
    versionCode=47 minSdk=26 targetSdk=35
    versionName=0.1.47
"#;
        assert_eq!(parse_version_name(dumpsys).as_deref(), Some("0.1.47"));
    }

    #[test]
    fn dumpsys_plain_version_name() {
        let dumpsys = "    versionName=0.1.7\n";
        assert_eq!(parse_version_name(dumpsys).as_deref(), Some("0.1.7"));
    }

    #[test]
    fn host_and_apk_versions_compare_without_v_prefix() {
        assert!(versions_match("0.1.47", "v0.1.47"));
        assert!(!versions_match("0.1.0", "0.1.47"));
    }

    #[test]
    fn staged_tmp_apk_is_not_installed() {
        assert!(!pm_path_means_installed("package:/data/local/tmp/Lighting.apk\n"));
        assert!(pm_path_means_installed(
            "package:/data/app/~~x==/app.lighting.display-abc==/base.apk\n"
        ));
        assert!(!pm_path_means_installed("\n"));
    }

    #[test]
    fn verify_does_not_invent_host_version() {
        let expected = expected_client_version();
        assert_eq!(
            verified_install_version(true, Some(expected), expected).unwrap(),
            expected
        );
        assert_eq!(verified_install_version(true, None, expected).unwrap(), "");
        assert!(verified_install_version(true, Some("0.1.0"), expected).is_err());
        assert!(verified_install_version(false, Some(expected), expected).is_err());
    }
}
