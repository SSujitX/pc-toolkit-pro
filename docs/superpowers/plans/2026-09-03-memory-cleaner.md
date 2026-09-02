# Memory Cleaner Implementation Plan

> Execute task-by-task. Do **not** invent APIs; skip and log when elevation/OS blocks an area. Do **not** copy GPL third-party source. Follow root **`AGENTS.md`**.

**Goal:** Memory Cleaner with selectable areas, live physical/virtual stats, minute-based auto-clean, tray parity, and a real Windows optimize engine.

**Architecture:** platform Win32 → core settings/history → thin Tauri commands → Pinia + soft workspace page.

**Tech Stack:** Tauri 2, Vue 3, Pinia, Rust, `windows` crate.

**Spec:** `docs/superpowers/specs/2026-09-03-memory-cleaner-design.md`

## Global Constraints

- UI: soft PC Toolkit density (`AGENTS.md`)  
- Settings: `%LOCALAPPDATA%\PC Toolkit Pro\memory-cleaner.json`  
- Auto interval: 0 or ≥5 minutes through 1440  
- Honest outcomes: `ok` | `skipped` | `failed`  
- No hotkey / exclusion list / tray % bitmap in this plan  
- No git commits unless the user asks  

## Tasks (summary)

1. Platform memory engine + stats  
2. Core settings + optimize orchestration + FreeMemory wire  
3. Tauri memory commands  
4. FE store + page + locales  
5. Auto-clean runner + tray parity  
6. Verify (`vue-tsc`; cargo when MSVC available)
