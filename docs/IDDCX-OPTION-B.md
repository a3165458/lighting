# LightingIdd — Option B roadmap

## Goal

Own an IddCx virtual display (`Root\LightingIdd`) like ASUS GlideX owns theirs, instead of depending only on community MttVDD.

## Done in repo (v0.1.12 scaffold)

| Item | Status |
|------|--------|
| Fork Microsoft Indirect Display sample → `driver-idd/` | Done (source) |
| INF `Root\LightingIdd` | Done |
| `scripts/idd/provision.ps1` + stage into `host-ui/resources/idd` | Done (stage **fails** without `LightingIdd.dll` + `nefconc.exe`; INF-only packs are rejected) |
| Host prefers LightingIdd, falls back to MttVDD | Done |
| Default share mode = mirror until driver is signed/reliable | Done (0.1.11) |

## You must do on a Windows + WDK machine

1. Install VS2022 + WDK matching SDK.
2. Open `driver-idd/LightingIdd.sln`, build **Release | x64**.
3. Confirm `LightingIdd.dll` is produced.
4. `pwsh scripts/idd/build-idd.ps1` then `pwsh scripts/idd/stage-idd-bundle.ps1` (must produce `LightingIdd.dll` + `nefconc.exe`; INF-only is a hard error). If the portable still lacks the DLL, host skips Idd and uses the complete MttVDD bundle.
5. Dev PC: `bcdedit /set testsigning on` and reboot.
6. Run Lighting elevated once → extend mode should create a virtual display via LightingIdd.

## Production (end users)

- Submit UMDF driver for **Microsoft Attestation signing** (Partner Center).
- Ship signed catalog (`.cat`) with the INF/DLL.
- Without this, normal PCs with Secure Boot **will not load** the driver — that is why GlideX “just works” and unsigned community packages struggle.

## Later enhancements

- Named-pipe / IOCTL to hot-plug monitor and set tablet-native mode (closer to GlideX session start).
- Optionally consume IddCx swapchain frames inside the driver for encode (bigger change).
