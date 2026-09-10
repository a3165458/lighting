# Code signing policy

Windows release installers and the bundled `lighting-host.exe` are intended
to be Authenticode-signed.

**Status:** applying to the [SignPath Foundation](https://signpath.org/)
open-source program. After approval, builds from this repository’s GitHub
Actions release workflow will be signed.

Free code signing provided by [SignPath.io](https://about.signpath.io),
certificate by [SignPath Foundation](https://signpath.org).

## Team roles

This is a personal repository. The same person fills every signing role:

- **Authors / committers / reviewers:** [@a3165458](https://github.com/a3165458)
- **Approvers** (each signing request): [@a3165458](https://github.com/a3165458)

GitHub and SignPath access must use multi-factor authentication.

## What will be signed

- `Lighting-*-setup.exe` (NSIS installer)
- `Lighting-*-portable.exe` (portable launcher)
- bundled `lighting-host.exe`

Unsigned third-party pieces that may ship *inside* a signed package (not
re-signed with this project’s certificate): FFmpeg, adb, and the Microsoft /
MttVDD virtual-display driver catalog.

## Privacy

This program will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or
operating it.

USB / LAN screen sharing only starts after the user clicks **开始共享**
and the tablet connects. The default listener is USB loopback
(`127.0.0.1`), not the LAN.

## Uninstall

Use Windows **Apps & features** for the setup build, or delete the portable
folder. Sharing stop restores the PC display layout when possible.
