//! Temporary tablet-only output with an owned, explicit CCD restore snapshot.

use anyhow::{bail, Context, Result};
use std::time::Duration;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, SetDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS, SDC_ALLOW_CHANGES, SDC_APPLY, SDC_TOPOLOGY_EXTEND,
    SDC_USE_SUPPLIED_DISPLAY_CONFIG,
};
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, POINTL};

const SNAPSHOT_ATTEMPTS: usize = 4;

#[derive(Default)]
pub struct TabletOnlyOutput {
    saved: Option<DisplaySnapshot>,
}

impl TabletOnlyOutput {
    pub fn enable(&mut self, device_name: &str) -> Result<()> {
        if self.is_active() {
            bail!("tablet-only output already has a pending desktop restoration");
        }

        let snapshot = DisplaySnapshot::capture()?;
        let mut paths = Vec::with_capacity(snapshot.paths.len());
        for path in &snapshot.paths {
            if source_matches(path, device_name)? {
                paths.push(*path);
            }
        }
        if paths.is_empty() {
            bail!("no active QueryDisplayConfig source matches {device_name:?}");
        }

        // Keep every mode at its original index; only selected paths are active.
        let mut tablet = DisplaySnapshot {
            paths,
            modes: snapshot.modes.clone(),
        };
        for path in &tablet.paths {
            // Capture uses non-virtual-aware indices and validates the union tags.
            let index = unsafe { path.sourceInfo.Anonymous.modeInfoIdx } as usize;
            tablet.modes[index].Anonymous.sourceMode.position = POINTL { x: 0, y: 0 };
        }

        // Even a failed apply may change the desktop. Never lose its restore data.
        self.saved = Some(snapshot);
        tablet.apply(false).context("enabling tablet-only output")
    }

    pub fn restore(&mut self) -> Result<()> {
        let Some(snapshot) = self.saved.take() else {
            return Ok(());
        };
        // Replay the pre-blank layout, then Win+P extend so IddCx is not
        // left detached. Snapshot-only restore is what left "只剩主屏".
        let snapshot_err = snapshot
            .apply(true)
            .context("restoring pre-tablet desktop")
            .err();
        let extend_err = apply_extend().err();
        if let Some(err) = snapshot_err {
            if extend_err.is_some() {
                return Err(err);
            }
            tracing::warn!("tablet-only snapshot restore failed, extend kept the desktop: {err:#}");
        }
        if let Some(err) = extend_err {
            tracing::warn!("extend after tablet-only failed: {err:#}");
        }
        Ok(())
    }

    /// True while a saved desktop still needs restoration, including failed apply.
    pub fn is_active(&self) -> bool {
        self.saved.is_some()
    }
}

impl Drop for TabletOnlyOutput {
    fn drop(&mut self) {
        if let Err(err) = self.restore() {
            tracing::warn!("restore tablet-only display topology failed: {err:#}");
        }
    }
}

struct DisplaySnapshot {
    paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    modes: Vec<DISPLAYCONFIG_MODE_INFO>,
}

impl DisplaySnapshot {
    fn capture() -> Result<Self> {
        for _ in 0..SNAPSHOT_ATTEMPTS {
            let mut path_count = 0;
            let mut mode_count = 0;
            let code = unsafe {
                GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            };
            if code != ERROR_SUCCESS {
                bail!("GetDisplayConfigBufferSizes failed with code {}", code.0);
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
            let code = unsafe {
                QueryDisplayConfig(
                    QDC_ONLY_ACTIVE_PATHS,
                    &mut path_count,
                    paths.as_mut_ptr(),
                    &mut mode_count,
                    modes.as_mut_ptr(),
                    None,
                )
            };
            if code == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if code != ERROR_SUCCESS {
                bail!("QueryDisplayConfig failed with code {}", code.0);
            }
            if path_count as usize > paths.len() || mode_count as usize > modes.len() {
                bail!("QueryDisplayConfig returned counts beyond the supplied buffers");
            }
            paths.truncate(path_count as usize);
            modes.truncate(mode_count as usize);
            let snapshot = Self { paths, modes };
            snapshot.validate()?;
            return Ok(snapshot);
        }
        bail!(
            "QueryDisplayConfig failed with code {} after {SNAPSHOT_ATTEMPTS} snapshot attempts",
            ERROR_INSUFFICIENT_BUFFER.0
        );
    }

    fn validate(&self) -> Result<()> {
        if self.paths.is_empty() {
            bail!("QueryDisplayConfig returned no active desktop paths");
        }
        for path in &self.paths {
            // Without QDC_VIRTUAL_MODE_AWARE these unions contain full modeInfoIdx.
            let source_index = unsafe { path.sourceInfo.Anonymous.modeInfoIdx };
            let target_index = unsafe { path.targetInfo.Anonymous.modeInfoIdx };
            for (index, kind, adapter, id) in [
                (
                    source_index,
                    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
                    path.sourceInfo.adapterId,
                    path.sourceInfo.id,
                ),
                (
                    target_index,
                    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
                    path.targetInfo.adapterId,
                    path.targetInfo.id,
                ),
            ] {
                let Some(mode) = self.modes.get(index as usize) else {
                    bail!("QueryDisplayConfig returned invalid mode index {index}");
                };
                if mode.infoType != kind || mode.adapterId != adapter || mode.id != id {
                    bail!("QueryDisplayConfig returned mismatched mode at index {index}");
                }
            }
        }
        Ok(())
    }

    fn apply(&self, allow_changes: bool) -> Result<()> {
        // No database writes: tablet-only must not persist "second screen only".
        let flags = if allow_changes {
            SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES
        } else {
            SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG
        };
        let code = unsafe { SetDisplayConfig(Some(&self.paths), Some(&self.modes), flags) };
        if code != 0 {
            bail!("SetDisplayConfig failed with code {code}");
        }
        Ok(())
    }
}

fn apply_extend() -> Result<()> {
    let code = unsafe { SetDisplayConfig(None, None, SDC_APPLY | SDC_TOPOLOGY_EXTEND) };
    if code != 0 {
        bail!("SetDisplayConfig extend failed with code {code}");
    }
    std::thread::sleep(Duration::from_millis(400));
    Ok(())
}

fn source_matches(path: &DISPLAYCONFIG_PATH_INFO, device_name: &str) -> Result<bool> {
    let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
            adapterId: path.sourceInfo.adapterId,
            id: path.sourceInfo.id,
        },
        ..Default::default()
    };
    let code = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) };
    if code != 0 {
        bail!("DisplayConfigGetDeviceInfo(GET_SOURCE_NAME) failed with code {code}");
    }
    // GDI display names are ASCII; compare UTF-16 without allocating a string.
    let fold = |unit: u16| {
        if (b'A' as u16..=b'Z' as u16).contains(&unit) {
            unit + (b'a' - b'A') as u16
        } else {
            unit
        }
    };
    Ok(source
        .viewGdiDeviceName
        .iter()
        .copied()
        .take_while(|&unit| unit != 0)
        .map(fold)
        .eq(device_name.encode_utf16().map(fold)))
}
