# Lighting Indirect Display Driver (IddCx) — Option B

Own IddCx UMDF driver forked from Microsoft
[`Windows-driver-samples/video/IndirectDisplay`](https://github.com/microsoft/Windows-driver-samples/tree/main/video/IndirectDisplay)
(MIT). Same class of tech as ASUS GlideX; difference is **we own install + lifecycle**.

## What this driver does

- Root-enumerated device `Root\LightingIdd`
- On D0/start: creates **one** virtual monitor (EDID + modes from sample)
- Swap-chain thread consumes OS frames (encode still happens in `host-windows` via DXGI today; later we can pull frames here like GlideX)

## Build (Windows + WDK required)

This Linux CI agent **cannot** link UMDF/IddCx. Build on a Windows 11 machine:

1. Install **Visual Studio 2022** (C++ desktop) + **Windows 11 SDK** + **WDK** matching the SDK
2. Open `driver-idd/LightingIdd.sln`
3. Configuration: `Release | x64`
4. Build → output `LightingIdd.dll` under WDK/UMDF out dir (check project Output Directory)
5. Run `scripts/idd/stage-idd-bundle.ps1` to copy `LightingIdd.dll` + `LightingIdd.inf` + `nefconw.exe` into `host-ui/resources/idd/`

### Test signing (dev machines only)

```powershell
bcdedit /set testsigning on   # reboot
# Then provision.ps1 will pnputil /add-driver LightingIdd.inf /install
```

Production: **Microsoft Attestation signing** (Hardware Dev Center) for UMDF IddCx. Self-signed / test-signed will not load on normal end-user PCs with Secure Boot.

## Install / enable (runtime)

`scripts/idd/provision.ps1`:

- `Full` — add driver package + create `Root\LightingIdd` via nefcon + enable
- `EnableOnly` — enable/restart existing device

ASCII result file: `OK|...` / `FAIL|CODE` (same protocol as VDD provision).

## Host integration

`host-windows` `ensure_secondary_display`:

1. Try **LightingIdd** bundle (`resources/idd/`)
2. Fall back to legacy **MttVDD** (`resources/vdd/`) if IDD missing or fails

## License

Driver sources retain Microsoft sample copyright headers where applicable; Lighting modifications are Apache-2.0 / MIT consistent with this repo. See Microsoft sample license in upstream tree.
