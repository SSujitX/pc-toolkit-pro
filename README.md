# PC Toolkit Pro

**Windows PC cleaner · RAM / memory optimizer · junk file cleaner · system monitor · power tools**

<p align="center">
  <img src="src/assets/brand/logo.png" alt="PC Toolkit Pro — Windows system cleaner and memory optimizer logo" width="128" height="128" />
</p>

<p align="center">
  <strong>Free Windows system utility</strong> to clean junk files, free RAM, monitor CPU/disk/GPU, control power options, and manage your PC from a soft modern desktop app + system tray.
</p>

<p align="center">
  <a href="https://github.com/SSujitX/pc-toolkit-pro/releases"><img src="https://img.shields.io/github/v/release/SSujitX/pc-toolkit-pro?style=for-the-badge&label=Release&color=0ea5e9" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/SSujitX/pc-toolkit-pro?style=for-the-badge&color=22c55e" alt="MIT License" /></a>
  <a href="https://github.com/SSujitX/pc-toolkit-pro/actions/workflows/tauri-build.yml"><img src="https://img.shields.io/github/actions/workflow/status/SSujitX/pc-toolkit-pro/tauri-build.yml?branch=feat%2Ftauri-rewrite&style=for-the-badge&label=Windows%20Build" alt="Windows build status" /></a>
  <a href="https://github.com/SSujitX/pc-toolkit-pro/stargazers"><img src="https://img.shields.io/github/stars/SSujitX/pc-toolkit-pro?style=for-the-badge&color=f59e0b" alt="GitHub stars" /></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Platform-Windows_10%2F11-0078D6?style=flat-square&logo=windows&logoColor=white" alt="Windows 10 and Windows 11" />
  <img src="https://img.shields.io/badge/Tauri-2-FFC131?style=flat-square&logo=tauri&logoColor=black" alt="Tauri 2" />
  <img src="https://img.shields.io/badge/Vue-3-42b883?style=flat-square&logo=vuedotjs&logoColor=white" alt="Vue 3" />
  <img src="https://img.shields.io/badge/Rust-stable-DEA584?style=flat-square&logo=rust&logoColor=black" alt="Rust" />
  <img src="https://img.shields.io/badge/TypeScript-5-3178C6?style=flat-square&logo=typescript&logoColor=white" alt="TypeScript" />
  <img src="https://img.shields.io/badge/UI-Tailwind_CSS_4-38BDF8?style=flat-square&logo=tailwindcss&logoColor=white" alt="Tailwind CSS 4" />
  <img src="https://img.shields.io/badge/Downloads-GitHub_Releases-181717?style=flat-square&logo=github" alt="Download from GitHub Releases" />
</p>

<p align="center">
  <a href="https://github.com/SSujitX/pc-toolkit-pro/releases/latest"><strong>Download PC Toolkit Pro for Windows</strong></a>
  ·
  <a href="#features">Features</a>
  ·
  <a href="#develop">Develop</a>
  ·
  <a href="#build--release">Build</a>
</p>

---

## Why PC Toolkit Pro?

**PC Toolkit Pro** is a lightweight **Windows PC cleaner** and **system manager** for everyday maintenance: remove temp/junk files, **optimize memory / free RAM**, watch live resource usage, run quick Windows tools, schedule shutdown, and keep a full hardware report ready to copy.

Rebuilt from the ground up with **Tauri 2 + Vue 3 + Rust** for a fast, native-feeling desktop app — soft dense UI, hide-to-tray, signed auto-updates, and no heavy Electron runtime.

> **Inspiration (patterns only):** dense desktop cleanup chrome akin to [MangoDisk](https://github.com/harry0703/MangoDisk), plus Windows memory-clean workflows akin to [Win Memory Cleaner](https://github.com/IgorMundstein/WinMemoryCleaner) — original PC Toolkit Pro code, branding, and MIT license.

---

## Screenshots

<p align="center">
  <img src="images/screenshot1.png" alt="PC Toolkit Pro System monitor — CPU RAM disk GPU metrics and Quick Actions" width="720" />
</p>
<p align="center">
  <img src="images/screenshot2.png" alt="PC Toolkit Pro Memory Cleaner — RAM optimizer with selectable memory areas and auto-clean" width="720" />
</p>
<p align="center">
  <img src="images/screenshot3.png" alt="PC Toolkit Pro Cleaner — junk file temp prefetch recycle bin disk cleanup" width="720" />
</p>
<p align="center">
  <img src="images/screenshot4.png" alt="PC Toolkit Pro Information — full Windows hardware system report CPU GPU PSU" width="720" />
</p>

---

## Features

| Area | What you get |
|------|----------------|
| **Junk / disk cleaner** | Scan → confirm → clean temp, prefetch, recycle, and related junk with progress + cancel. Default scan works without admin (skip-and-continue on denied paths). |
| **Deep cleaner** | Broader cleanup categories for deeper Windows disk cleanup sessions. |
| **Memory cleaner / RAM optimizer** | Selectable memory areas, live physical & virtual RAM stats, auto-clean (from 5 minutes + free-RAM threshold). Real Win32 APIs — honest skip when elevation is missing. |
| **System monitor** | Live CPU, RAM, disk, and NVIDIA GPU metrics in-app and in the titlebar. |
| **System tray** | Hide-to-tray, live RAM tooltip, Clean Memory, power shortcuts — same settings as the Memory page. |
| **Power tools** | Shutdown, restart, sleep, lock, and scheduled shutdown. |
| **Quick Actions** | One-click launch of common Windows tools and admin shells. |
| **System information** | Full hardware report (CPU, disks, RAM, GPU, monitors, motherboard, OS, PSU when available) with clipboard export. |
| **Activity history** | Local history of cleaner / memory / power operations. |
| **Settings & updates** | Theme (light / dark / system), About dialog, and signed in-app updates from GitHub Releases. |

**Keywords this project targets:** Windows cleaner, PC cleaner, junk file cleaner, temp file cleaner, disk cleanup, RAM cleaner, memory cleaner, free RAM, memory optimizer, Windows optimizer, system utility, system tray cleaner, Tauri Windows app, Vue desktop app.

---

## Stack

- **Shell:** Tauri 2 (window starts hidden until Vue mounts — no startup hang)
- **UI:** Vue 3 + TypeScript + Pinia + Tailwind CSS 4 + soft dense **PC Toolkit Pro** chrome
- **Backend:** `pctoolkit-core` (product logic, no Tauri) + `pctoolkit-platform` (Windows OS facts / Win32)
- **Updates:** `@tauri-apps/plugin-updater` + signed NSIS artifacts on GitHub Releases

---

## Download

1. Open **[Releases](https://github.com/SSujitX/pc-toolkit-pro/releases/latest)**
2. Install with the **NSIS setup** (`PC-Toolkit-Pro-*-setup.exe`), or run the portable EXE
3. Optional: keep the app in the tray for quick **Clean Memory** and power actions

CI artifacts (unsigned / pre-release builds) are also available from **Actions → Build Windows EXE**.

---

## Develop

```bash
pnpm install
pnpm tauri:dev
```

Needs **Rust stable** and **Windows C++ / MSVC** build tools. If `link.exe` is missing locally, use GitHub Actions for real `.exe` builds.

UI-only preview (no Rust link):

```bash
pnpm dev
```

---

## Build & release

```bash
pnpm tauri:build
```

### GitHub Actions (recommended)

Workflow: [`.github/workflows/tauri-build.yml`](.github/workflows/tauri-build.yml)

1. Push to `feat/tauri-rewrite` (or **Actions → Build Windows EXE → Run workflow**)
2. Open the finished run → **Artifacts**
3. Download `pc-toolkit-pro-windows-<sha>` (NSIS installer + portable EXE)

### Version + tagged release

Single source of truth: root [`VERSION`](VERSION) (`X.Y.Z`).

```bash
echo 3.0.1 > VERSION
pnpm version:sync
pnpm version:check

git commit -am "chore: bump version to 3.0.1"
git tag v3.0.1
git push origin v3.0.1
```

Tag `vX.Y.Z` (must match `VERSION`) runs [`.github/workflows/release-windows.yml`](.github/workflows/release-windows.yml), publishes a GitHub Release, and (with signing secrets) attaches updater `latest.json` for in-app updates.

---

## Branch note

Active Tauri rewrite: **`feat/tauri-rewrite`**.  
Legacy PyQt docs/release notes on **`master`** stay until this branch is merged and published as the mainline Windows build.

---

## License

[MIT](LICENSE) © PC Toolkit Pro / [SSujitX](https://github.com/SSujitX)

---

<p align="center">
  <sub>
    PC Toolkit Pro — Windows junk cleaner, RAM memory optimizer, system monitor, and power toolkit.
    Built with Tauri, Vue, and Rust.
  </sub>
</p>
