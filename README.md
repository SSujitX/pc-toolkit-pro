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

CI: [`.github/workflows/tauri-build.yml`](.github/workflows/tauri-build.yml) builds Windows artifacts on `feat/tauri-rewrite`.

## Features

- Live System monitor (CPU / RAM / Disk / NVIDIA GPU)
- Quick Actions (15 Windows tools, admin shells)
- Cleaner (scan → confirm → clean, measured memory free)
- Power (immediate + scheduled shutdown)
- Information panel + clipboard export
- System tray with live tooltip

## Branch

Active rewrite lives on `feat/tauri-rewrite`. Do not treat this README as the published PyQt v2.9 release docs on `master` until merge.
