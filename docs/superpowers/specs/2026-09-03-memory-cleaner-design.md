# Memory Cleaner — Design

**Date:** 2026-09-03  
**Status:** Approved — implementing  
**Branch:** `feat/tauri-rewrite`  
**References (patterns only, do not copy GPL source):** local trees under `test_inspiration/` if present

## Goal

Upgrade PC Toolkit Pro’s **Memory Cleaner** into a full memory workspace: selectable areas, live physical/virtual stats, auto-optimize, tray parity — with soft PC Toolkit UI and honest results.

## Non-goals (pass 1)

- Global hotkey  
- Process exclusion list  
- Painting memory % onto the tray icon bitmap  
- Copying third-party layouts, chrome, colors, or source  
- Claiming standby/combined/file-cache purges when those APIs were skipped  

## Current state (facts)

Today `pctoolkit-platform::memory::optimize_memory()` historically trimmed this process and optionally used a PowerShell nudge. That path is replaced by real area-based Win32 APIs (see implementation).

## Success criteria

1. User can select memory **areas** and Optimize runs only those.  
2. Page shows live **physical** and **virtual (page file)** used/free/%.  
3. Auto-clean: interval from **5 minutes** upward (and multi-hour), plus free-physical-% threshold.  
4. Tray Clean Memory / middle-click use the **same** settings (areas + engine).  
5. Result log lists **per-area** ok / skipped / failed — no silent lies.  
6. UI matches soft PC Toolkit shell (`AGENTS.md`).  
7. Settings persist without requiring admin to *save* preferences.

---

## Architecture

```
Vue Memory Cleaner page
  → Pinia memory-cleaner-store
  → lib/services
  → Tauri commands (thin)
  → pctoolkit-core (settings + history + orchestration)
  → pctoolkit-platform (Win32 stats + per-area optimize)
```

Follow root **`AGENTS.md`** for stack and UI rules.

Keep Cleaner / Deep Cleaner `freeMemory` on the **same** engine + saved areas.

---

## Memory areas

Stable string IDs (camelCase): `combinedPageList`, `modifiedFileCache`, `modifiedPageList`, `registryCache`, `standbyList`, `standbyListLowPriority`, `systemFileCache`, `workingSet`.

Defaults: all on except `standbyListLowPriority`. Standby ↔ low-priority mutually exclusive.

Optimize order: Working set → System file cache → Modified page list → Standby (or low) → Combined → Registry → Modified file cache.

Elevation: skip + log when privileges/OS block an area. Never report skipped areas as optimized.

---

## Stats / settings / auto

- Stats via `GlobalMemoryStatusEx`; poll 2s on page.  
- Settings: `%LOCALAPPDATA%\PC Toolkit Pro\memory-cleaner.json`  
- Interval steps: 0, 5…1440 minutes (max 24h). Threshold 0–100% with 5-minute low-memory cooldown.  
- Reasons: `manual` | `schedule` | `lowMemory` | `tray`  
- Auto tick every 30–60s while app (incl. tray) is alive.

## UI

Soft meters, area tiles, auto control cards, Optimize primary — see `src/pages/memory-cleaner/`. Not a dark third-party clone.

## Better product deltas

- Minute-based auto interval (5 min floor)  
- App-local settings (no machine-wide admin registry required to save)  
- Soft shell + Activity History + full toolkit tray  
- Honest skip/log when not elevated  

## Risks

- Some APIs fail without elevation → skip cleanly.  
- Standby purge can cause short disk churn.  
- Freed bytes = measured free-RAM delta (approximate).
