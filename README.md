# PC Toolkit Pro

**Windows PC cleaner · RAM / memory optimizer · junk file cleaner · system monitor · power tools**

![PC Toolkit Pro — Windows system cleaner and memory optimizer logo](src/assets/brand/logo.png)

**Free Windows system utility** to clean junk files, free RAM, monitor CPU/disk/GPU, control power options, and manage your PC from a soft modern desktop app + system tray.

![Latest release](https://img.shields.io/github/v/release/SSujitX/pc-toolkit-pro?style=for-the-badge&label=Release&color=0ea5e9)![MIT License](https://img.shields.io/github/license/SSujitX/pc-toolkit-pro?style=for-the-badge&color=22c55e)![Windows build status](https://img.shields.io/github/actions/workflow/status/SSujitX/pc-toolkit-pro/tauri-build.yml?branch=feat%2Ftauri-rewrite&style=for-the-badge&label=Windows%20Build)![GitHub stars](https://img.shields.io/github/stars/SSujitX/pc-toolkit-pro?style=for-the-badge&color=f59e0b)

![Windows 10 and Windows 11](https://img.shields.io/badge/Platform-Windows_10%2F11-0078D6?style=flat-square&logo=windows&logoColor=white)![Tauri 2](https://img.shields.io/badge/Tauri-2-FFC131?style=flat-square&logo=tauri&logoColor=black)![Vue 3](https://img.shields.io/badge/Vue-3-42b883?style=flat-square&logo=vuedotjs&logoColor=white)![Rust](https://img.shields.io/badge/Rust-stable-DEA584?style=flat-square&logo=rust&logoColor=black)![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?style=flat-square&logo=typescript&logoColor=white)![Tailwind CSS 4](https://img.shields.io/badge/UI-Tailwind_CSS_4-38BDF8?style=flat-square&logo=tailwindcss&logoColor=white)![Download from GitHub Releases](https://img.shields.io/badge/Downloads-GitHub_Releases-181717?style=flat-square&logo=github)

**[Download PC Toolkit Pro for Windows](https://github.com/SSujitX/pc-toolkit-pro/releases/latest)** · [Features](#features) · [Develop](#develop) · [Build](#build)

---



## Why PC Toolkit Pro?

**PC Toolkit Pro** is a lightweight **Windows PC cleaner** and **system manager** for everyday maintenance: remove temp/junk files, **optimize memory / free RAM**, watch live resource usage, run quick Windows tools, schedule shutdown, and keep a full hardware report ready to copy.

Rebuilt from the ground up with **Tauri 2 + Vue 3 + Rust** for a fast, native-feeling desktop app — soft dense UI, hide-to-tray, signed auto-updates, and no heavy Electron runtime.

> **Inspiration (patterns only):** dense desktop cleanup chrome akin to [MangoDisk](https://github.com/harry0703/MangoDisk), plus Windows memory-clean workflows akin to [Win Memory Cleaner](https://github.com/IgorMundstein/WinMemoryCleaner) — original PC Toolkit Pro code, branding, and MIT license.

---



## Screenshots

![PC Toolkit Pro System monitor — CPU RAM disk GPU metrics and Quick Actions](images/screenshot1.png)

![PC Toolkit Pro Memory Cleaner — RAM optimizer with selectable memory areas and auto-clean](images/screenshot2.png)

![PC Toolkit Pro Cleaner — junk file temp prefetch recycle bin disk cleanup](images/screenshot3.png)

![PC Toolkit Pro Information — full Windows hardware system report CPU GPU PSU](images/screenshot4.png)

---



## Features


| Area                               | What you get                                                                                                                                                           |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Junk / disk cleaner**            | Scan → confirm → clean temp, prefetch, recycle, and related junk with progress + cancel. Default scan works without admin (skip-and-continue on denied paths).         |
| **Deep cleaner**                   | Broader cleanup categories for deeper Windows disk cleanup sessions.                                                                                                   |
| **Memory cleaner / RAM optimizer** | Selectable memory areas, live physical & virtual RAM stats, auto-clean (from 5 minutes + free-RAM threshold). Real Win32 APIs — honest skip when elevation is missing. |
| **System monitor**                 | Live CPU, RAM, disk, and NVIDIA GPU metrics in-app and in the titlebar.                                                                                                |
| **System tray**                    | Hide-to-tray, live RAM tooltip, Clean Memory, power shortcuts — same settings as the Memory page.                                                                      |
| **Power tools**                    | Shutdown, restart, sleep, lock, and scheduled shutdown.                                                                                                                |
| **Quick Actions**                  | One-click launch of common Windows tools and admin shells.                                                                                                             |
| **System information**             | Full hardware report (CPU, disks, RAM, GPU, monitors, motherboard, OS, PSU when available) with clipboard export.                                                      |
| **Activity history**               | Local history of cleaner / memory / power operations.                                                                                                                  |
| **Settings & updates**             | Theme (light / dark / system), About dialog, and signed in-app updates from GitHub Releases.                                                                           |


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
2. Run the **NSIS setup** (`PC Toolkit Pro vX.Y.Z Setup.exe`) — installs for the current user to `%LOCALAPPDATA%\PC Toolkit Pro\` (Start Menu + `uninstall.exe`, no admin required for install)
3. Optional: on **Memory Cleaner**, use **Restart as administrator** once so deep RAM clean / tray / auto-clean can use privileged Win32 APIs for that session
4. Optional: keep the app in the tray for quick **Clean Memory** and power actions

---



## Develop

```bash
pnpm install
pnpm tauri:dev
```

Needs **Rust stable** and **Windows C++ / MSVC** build tools.

UI-only preview (no Rust link):

```bash
pnpm dev
```

---



## Build

```bash
pnpm tauri:build
```

---



## Branch note

Active Tauri rewrite: `feat/tauri-rewrite`.  
Legacy PyQt docs/release notes on `master` stay until this branch is merged and published as the mainline Windows build.

---



## License

[MIT](LICENSE) © PC Toolkit Pro / [SSujitX](https://github.com/SSujitX)

---

PC Toolkit Pro — Windows junk cleaner, RAM memory optimizer, system monitor, and power toolkit. Built with Tauri, Vue, and Rust.

---



### **Get Help**

- 🐛 **Issues**: [GitHub Issues](https://github.com/SSujitX/pc-toolkit-pro/issues)
- 💬 **Discussions**: [GitHub Discussions](https://github.com/SSujitX/pc-toolkit-pro/discussions)



### **Stay Updated**

- ⭐ **Star** this repository for updates
- 👀 **Watch** for new releases
- 🔔 **Follow** for announcements

---

**Made with ❤️ for the Windows Community**

*PC Toolkit Pro - Empowering Windows Users Since 2025*

[⬆️ Back to Top](#-pc-toolkit-pro---advanced-windows-system-manager)