# MangoDisk architecture reference

Source studied: https://github.com/harry0703/MangoDisk (GPL-3.0). Use as a **pattern guide only**.

## How UI and Rust connect

```
┌─────────────────────────────────────────────────────────────┐
│  Vue App                                                     │
│  App.vue → MdAppShell → Sidebar + Page (async chunks)        │
│                                                              │
│  Store action ──► Service.invoke / listen ──► Tauri IPC      │
└───────────────────────────────┬─────────────────────────────┘
                                │ invoke / events
┌───────────────────────────────▼─────────────────────────────┐
│  src-tauri (thin adapter)                                    │
│  commands/* validate → Core use case → emit progress events  │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│  crates/*-core                                               │
│  cleanup | storage | applications | filesystem | history     │
│  platform crate: OS facts only (no product orchestration)    │
└─────────────────────────────────────────────────────────────┘
```

## Frontend layers (MangoDisk `src/AGENTS.md`)

| Path | Owns | Must not |
|------|------|----------|
| `pages/<domain>/` | Route UI for one feature | Global orchestration |
| `layouts/` | Shell, sidebar, titlebar, overlays | Feature algorithms |
| `components/custom/` | Project UI primitives | Edit generated shadcn |
| `components/ui/` | Generated shadcn-vue | Product-wide behavior edits |
| `stores/` | One domain workflow state | Copy another store wholesale |
| `lib/services/` | Tauri, dialogs, OS, persistence | Pure math (belongs in utils) |
| `lib/utils/` | Deterministic helpers | invoke / store / I/O |
| `lib/models/` | Protocols, page IDs, event names | Guess Rust types |

## Shell behavior details

- **Primary page** (cleanup) is synchronous import; other pages `defineAsyncComponent` with preload on idle.
- Navigation blocked or warned while `busyPages` includes target domain.
- Sidebar tooltips only when collapsed; expanded shows labels + section headers.
- Active nav item: accent fill + 3px primary pill on the left.
- Busy page: spinning ring around nav icon.
- Window show deferred until Vue mounts (`ApplicationWindowService.showAfterMount`) so users never see a blank WebView.

## Result workspace language

Shared CSS roles (not page-specific hacks):

- `.md-result-header` — muted toolbar header
- `.md-result-sort` / `[data-active=true]` — sort affordance
- `.md-result-primary` — medium weight row title
- `.md-workspace-toolbar` — fixed height to prevent jump on navigate
- Result rows: transparent wrapper + inset background; `--result-item-gap: 2px`

Document pages scroll; workspace pages `overflow-y: hidden` so tables own the scroll region.

## Theme skins

- Default purple-neutral skin in `themes/default.css`
- Product skin in `themes/mangodisk.css` via `html[data-skin='mangodisk']`
- Dark via `html[data-theme='dark']`
- For PC Toolkit: create `themes/pctoolkit.css` with our brand tokens; keep the same semantic variable names so components stay skin-agnostic.

## Rust adapter rules (MangoDisk `src-tauri/AGENTS.md`)

- Core must not depend on Tauri or WebView.
- Platform crate reports capabilities/fallbacks; does not decide product workflows.
- Typed errors in domain; convert to transport errors only in adapter.
- Cleanup rules preferably declarative TOML validated at build time.
- Destructive flows: dry-run, protected paths, link/reparse policy, preflight, explicit intent, verification.
- Privileged Windows/macOS ops need separate capability boundaries.

## Command / event examples (names only)

| Command | Direction | Notes |
|---------|-----------|-------|
| `scan_cleanup_candidates` | FE → Rust | emits scan progress events |
| `cancel_cleanup_scan` | FE → Rust | sync cancel |
| `execute_cleanup` | FE → Rust | dryRun flag; execution progress events |
| `list_disks` / `get_system_disk` | FE → Rust | read-only inventory |
| `delete_files_permanently` | FE → Rust | after confirmation |

Event listeners live in services, not scattered in Vue templates.

## What to match vs invent for PC Toolkit Pro

| Match MangoDisk UI | Ours only |
|--------------------|-----------|
| Shell, density, soft surfaces, workspaces, dialogs | Name, logo, primary accent |
| Service/store/command layering | Feature set (monitor, power, tray, etc.) |
| Type scale, no-motion buttons, confirm-before-clean | Locale copy, screenshots |
| Token CSS + Tailwind 4 structure | Reimplemented code (no GPL paste) |

## Validation mindset

MangoDisk gates UI with scripts like `check-style-system`, `check-source-architecture`, `check-i18n`. When PC Toolkit grows, add similar checks:

- No raw SVG in templates
- No business logic in `components/ui`
- User strings only in locales
- Buttons never use transform utilities
