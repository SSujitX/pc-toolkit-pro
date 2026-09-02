#!/usr/bin/env bash
# Thin wrapper for CI / Git Bash. Prefer: pnpm version:sync | version:check
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
exec node ./scripts/sync-version.mjs "${1:-sync}"
