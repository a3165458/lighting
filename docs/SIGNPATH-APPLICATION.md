# SignPath Foundation application (copy-paste)

Open https://signpath.org/apply while signed in with the GitHub account
**a3165458** (MFA required). Paste the following.

## Project / repository URL

https://github.com/a3165458/lighting

## License

MIT. File: https://github.com/a3165458/lighting/blob/main/LICENSE

## Download / release URL

https://github.com/a3165458/lighting/releases/latest

## Project description

Lighting turns an Android tablet into a wired USB second display for Windows
(mirror, extend, or tablet-only). The Windows host captures the desktop,
encodes H.264/HEVC, and streams over adb reverse; the tablet decodes with
MediaCodec. Optional LAN is off by default (loopback only).

We need Authenticode signing because unsigned GitHub Release installers are
flagged by Windows SmartScreen and 360 as unknown/malware. We do not collect
telemetry. Source, CI, and releases are public.

## Code signing policy

https://github.com/a3165458/lighting/blob/main/docs/CODE-SIGNING.md

## What will be signed

Windows NSIS installer, portable exe, and bundled lighting-host.exe from
GitHub Actions `release.yml` on GitHub-hosted runners.

## Privacy

This program will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or
operating it.
