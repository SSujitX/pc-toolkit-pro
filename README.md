# PC Toolkit Pro

Windows system manager rebuilt with **Tauri 2 + Vue 3 + Rust**.

## Stack

- Tauri 2 desktop shell (hidden window until Vue mounts)
- Vue 3 + TypeScript + Pinia + Tailwind 4
- `pctoolkit-core` / `pctoolkit-platform` Rust crates (no Tauri in core)
- MangoDisk-style UI chrome with **PC Toolkit Pro** branding

## Develop

```bash
pnpm install
pnpm tauri:dev
```

Requires Rust stable and Windows C++ build tools (or use GitHub Actions).

UI-only preview without Rust:

```bash
pnpm dev
```

## Build

```bash
pnpm tauri:build
```

### GitHub Actions (recommended for `.exe`)

Workflow: [`.github/workflows/tauri-build.yml`](.github/workflows/tauri-build.yml)

1. Push to `feat/tauri-rewrite` (or run **Actions → Build Windows EXE → Run workflow**)
2. Open the finished run → **Artifacts**
3. Download `pc-toolkit-pro-windows-<sha>` — contains the NSIS installer and portable EXE

### Version + release

Single source of truth: root [`VERSION`](VERSION) (`X.Y.Z`).

```bash
# bump then sync into package.json / tauri.conf / Cargo.toml / APP_VERSION
echo 3.0.1 > VERSION
pnpm version:sync
pnpm version:check

git commit -am "chore: bump version to 3.0.1"
git tag v3.0.1
git push origin v3.0.1
```

Tag `vX.Y.Z` (must match `VERSION`) triggers [`.github/workflows/release-windows.yml`](.github/workflows/release-windows.yml), which builds the Windows EXE and publishes a GitHub Release.

## Features

- Live System monitor (CPU / RAM / Disk / NVIDIA GPU)
- Quick Actions (15 Windows tools, admin shells)
- Cleaner (scan → confirm → clean, measured memory free)
- Power (immediate + scheduled shutdown)
- Information panel + clipboard export
- System tray with live tooltip

## Branch

Active rewrite lives on `feat/tauri-rewrite`. Do not treat this README as the published PyQt v2.9 release docs on `master` until merge.
