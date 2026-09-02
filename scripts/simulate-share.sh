#!/usr/bin/env bash
# Dry-run the share-start policy on Linux (no DXGI / DisplaySwitch).
set -euo pipefail
cd "$(dirname "$0")/../host-windows"
exec cargo test --lib share_flow -- --nocapture
