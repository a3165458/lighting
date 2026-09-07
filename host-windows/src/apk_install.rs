//! Policy for shipping / replacing the Android client.
//!
//! CI `assembleDebug` signs with the runner's debug keystore. `adb install -r`
//! then hits `INSTALL_FAILED_UPDATE_INCOMPATIBLE` against an older pad APK
//! (often still advertised as v0.1.0) and the UI looked like "重新安装" was a
//! no-op. Uninstall-then-install plus `-r -d -g` is the GlideX-style force
//! replace. `versionName` in dumpsys can also list a hidden 0.1.0 package
//! *before* the active one — parse the `Packages:` block, not the first hit.

pub const CLIENT_PACKAGE: &str = "app.lighting.display";

/// `adb install` flags after `-s <serial> install`.
/// `-r` replace, `-d` allow versionCode downgrade, `-g` grant permissions.
pub fn install_replace_flags() -> &'static [&'static str] {
    &["-r", "-d", "-g"]
}

pub fn needs_uninstall_reinstall(output: &str) -> bool {
    let upper = output.to_ascii_uppercase();
    upper.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE")
        || upper.contains("INSTALL_FAILED_VERSION_DOWNGRADE")
        || upper.contains("INSTALL_FAILED_ALREADY_EXISTS")
        || upper.contains("INCONSISTENT CERTIFICATES")
        || upper.contains("SIGNATURES ARE INCONSISTENT")
        || upper.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE")
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

#[derive(Clone, Copy)]
enum Section {
    Unknown,
    Packages,
    Hidden,
}

pub fn versions_match(installed: &str, expected: &str) -> bool {
    normalize_ver(installed) == normalize_ver(expected)
}

fn normalize_ver(v: &str) -> String {
    v.trim().trim_start_matches('v').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_flags_force_replace_and_downgrade() {
        let flags = install_replace_flags();
        assert!(flags.contains(&"-r"));
        assert!(flags.contains(&"-d"));
        assert!(flags.contains(&"-g"));
    }

    #[test]
    fn signature_mismatch_needs_uninstall() {
        assert!(needs_uninstall_reinstall(
            "Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE: Package signatures do not match]"
        ));
        assert!(needs_uninstall_reinstall(
            "INSTALL_FAILED_VERSION_DOWNGRADE"
        ));
        assert!(!needs_uninstall_reinstall("Success"));
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
}
