---
name: mangodisk-style-desktop-ui
description: >-
  Builds a visual twin of MangoDisk's desktop UI for PC Toolkit Pro using
  Tauri 2, Vue 3, Tailwind 4, shadcn-vue, and Rust — same shell, density,
  workspaces, and polish; only PC Toolkit branding differs. Use when designing
  or implementing Tauri/Vue UI, cleaner screens, sidebar, result tables, or
  Rust↔frontend wiring.
---

# Desktop UI (same as MangoDisk, our branding)

**Design target: same UI as [MangoDisk](https://github.com/harry0703/MangoDisk)** — shell, spacing, density, soft surfaces, sidebar behavior, page chrome, dialogs, result tables, light/dark. Users should feel it's the same class of app.

**Only branding differs:** PC Toolkit Pro name, logo, and primary accent. Reimplement from scratch (GPL-3.0 — do not copy their source, README, or assets).

## Stack target (follow this)

| Layer | Choice |
|-------|--------|
| Runtime | Tauri 2 |
| UI | Vue 3 + TypeScript (`<script setup>`) |
| Style | Tailwind CSS 4 + CSS variables (oklch) |
| Components | shadcn-vue (New York) in `components/ui/` — wrap, don't fork |
| State | Pinia Options API stores per domain |
| Icons | Lucide / Tabler via icon components — never raw SVG in templates |
| Backend | Rust core crates + thin Tauri command adapter |

## Same UI vs our branding

| Match MangoDisk (yes) | Our branding only |
|-----------------------|-------------------|
| Sidebar collapse/expand, nav groups, busy rings | App name: **PC Toolkit Pro** |
| Page shell, toolbars, result rows (~44px), action bar | Own logo / window icon |
| Warm soft-neutral surfaces, dense type scale | Primary accent (ours, not mango fruit logo) |
| Custom titlebar, dialogs, empty states, progress overlays | Locale copy & marketing text |
| Light + dark theme treatment | Component prefix `pt-*` (not `md-*`) |
| Scan → select → confirm → clean UX | Our screenshots |

When unsure: prefer **looking like them** over inventing a different layout.

## Architecture (non-negotiable)

```
Vue pages / shell     → presentation only
lib/services/*        → invoke(), listen(), dialogs, OS APIs
Pinia stores          → one domain workflow each
src-tauri/commands/*  → validate input → call Core → map errors/events
pctoolkit-core        → scan, safety, filesystem, persistence (no Tauri)
```

- Frontend is an **adapter**, not the brain.
- Rust owns filesystem access, safety, scan/cleanup, and persisted history.
- Commands stay thin: no scan algorithms inside `#[tauri::command]`.
- Progress = **listen before invoke**, always unlisten in `finally`.
- UI decisions use **typed status/risk/reason codes**, not free-form error strings.

## Fast load / no hang (match MangoDisk)

Follow their performance rules so the app feels instant and never freezes the UI:

| Rule | Do this |
|------|---------|
| First paint | Start main window **hidden**; call `show()` only **after** Vue mounts (DOM ready). Never flash a white/empty WebView. |
| Startup work | **No** long scans, disk walks, or blocking FS on launch. First screen must render immediately. |
| Heavy work | Run scans/cleanup on a **background worker** (`spawn_blocking` / `run_blocking`). Never block the UI thread. |
| Progress | Emit progress events; show overlay/spinner; keep cancel working. |
| Navigation | Eager-load the primary page; **lazy-load** secondary pages; idle-preload chunks so first click isn’t a blank screen. |
| Busy ops | Mark busy pages in the sidebar; don’t let users start conflicting ops (`operationBusy`). |
| Responsiveness | Buttons don’t move layout; lists stay scrollable while work runs; overlays don’t lock the whole OS. |
| Cancel | Cancel must be cheap and idempotent; don’t hang waiting for a stuck worker without a path out. |

Anti-patterns (cause hangs / slow feel):

- Scanning on app start before showing UI
- Doing filesystem work inside the Vue main thread or sync `invoke` without a worker
- Showing the window before the shell has painted
- Loading every page/module up front
- No progress + no cancel on multi-second jobs

## Shell layout

Build one application shell:

1. **Custom titlebar** (Windows: no native decorations + window controls; macOS: drag region under traffic lights).
2. **Collapsible sidebar** — icon rail (~68px) ↔ expanded (~240px); grouped nav; busy spinner on active ops; notice dots.
3. **Page content** via a shared **page shell**: title + optional subtitle + header actions; content mode `document` | `workspace`.
4. Lazy-load secondary pages; keep the primary page (cleanup/monitor) eager.

Shared layout tokens (define once in CSS):

```css
--layout-sidebar-collapsed-width: 68px;
--layout-sidebar-expanded-width: 240px;
--layout-sidebar-brand-height: 84px;
--layout-sidebar-item-height: 40px;
--layout-page-padding-inline: 20px;
--layout-page-header-height: 58px;
--layout-page-readable-width: 1160px;
--layout-workspace-toolbar-height: 36px;
--layout-result-row-height: 44px;
--layout-action-bar-height: 48px;
--layout-dialog-standard-width: 520px;
--radius: 0.5rem; /* dense desktop, not chunky web */
```

## Visual language

### Brand / theme

- Skin via `document.documentElement.dataset.skin='pctoolkit'` + `data-theme` light/dark.
- Match MangoDisk surface language: warm soft background, soft sidebar, white/dark cards, restrained borders, ~8px radius.
- Semantic tokens: `background`, `workspace`, `card`, `sidebar`, `primary`, `muted`, `destructive`, `success`, `warning`, file-type colors.
- Swap only identity: logo mark + app title string + `--primary` accent for PC Toolkit Pro.
- Use oklch; provide solid fallbacks where `color-mix` is unavailable.

### Density & type

| Role | Size |
|------|------|
| Page title | ~22px |
| Section title | 0.9375rem |
| Primary row text | 0.8125rem |
| Body | 0.75rem |
| Meta | 0.625–0.6875rem |

- Dense result lists; medium (500) for row titles; semibold only for headings/metrics.
- Cards are for interaction containers; avoid card-soup dashboards.

### Interaction invariants

- Buttons/hover: **color, border, shadow only** — never translate/scale (no layout jump).
- Desktop app: `user-select: none` globally; inputs/contenteditable opt back in.
- Scrollbars: thin, padded thumb; `scrollbar-hidden` / `scrollbar-stable` utilities.
- Operational spinners may keep motion under `prefers-reduced-motion` but slowed.
- Confirm destructive actions with dedicated dialogs; show reclaimable size before clean.

## Page / workspace patterns

Every operational page follows:

```
PageShell (title, actions)
  └─ Toolbar (filters, search, sort) — fixed height
  └─ ResultWorkspace (scrollable table/tree)
  └─ SelectionActionBar (footer) when items selected
```

Reuse primitives (project-owned `md-*` or `pt-*` prefix):

- Empty state, status badge, confirm / destructive dialog
- Result table, row, checkbox, summary, filter toolbar
- Operation progress / delayed workspace overlay
- Inline notice, middle-ellipsis paths, file-type icon

## Frontend file map

```
src/
  assets/main.css + themes/
  layouts/          # app shell, sidebar, titlebar
  pages/<domain>/   # route views only
  components/ui/    # generated shadcn — don't edit for product behavior
  components/custom/# project wrappers
  components/icons/
  stores/           # one Pinia store per domain
  lib/services/     # Tauri + side effects
  lib/models/       # typed protocols/constants
  lib/utils/        # pure functions only
  locales/          # all user-facing strings
```

## Rust ↔ Vue wiring

```ts
// Service owns invoke + event lifecycle
static async scanWithProgress(scope, onProgress) {
  let unlisten;
  try {
    unlisten = await listen('cleanup-scan-progress', e => onProgress(e.payload));
    return await invoke('scan_cleanup_candidates', { scanScope: scope });
  } finally {
    unlisten?.();
  }
}
```

Rust adapter:

```rust
#[tauri::command]
pub async fn scan_cleanup_candidates(app: AppHandle, scan_scope: Option<...>) -> CommandResult {
  run_blocking("scan_cleanup_candidates", move || {
    let progress = |v| events::emit(&app, CLEANUP_SCAN_PROGRESS, v);
    CoreScanService::scan_with_progress(progress)
  }).await
}
```

- camelCase JSON at the wire; deny unknown fields on request DTOs.
- Startup must not block first paint with long scans.
- Minimal Tauri capabilities; privileged ops get separate review.

## Safety UX (product)

1. Scan/read-only by default — never auto-delete.
2. Preview → confirm → execute → verify → history record.
3. Protected paths / admin elevation called out explicitly.
4. Cancel must be idempotent; progress proves guard is live before retry.
5. Logs: counts, timings, operation IDs — not raw full paths or file contents.

## Implementation order for PC Toolkit Pro

1. Token CSS + shell (sidebar + titlebar + page shell)
2. Monitor / system info as first workspace page
3. Cleaner with scan → select → confirm → progress overlay
4. Power + quick actions as secondary pages
5. Thin Rust commands replacing Python `psutil`/`ctypes` gradually

## Additional resources

- Architecture detail: [reference.md](reference.md)
- Interaction / layout checklist: [ui-checklist.md](ui-checklist.md)
