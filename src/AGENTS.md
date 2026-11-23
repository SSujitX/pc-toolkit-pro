# Frontend Guidelines

This file applies only to `src/` and inherits the repository-wide rules in [`../AGENTS.md`](../AGENTS.md).

## Boundaries and layout

- `pages/<domain>/` contains route-level views and components used only by that page.
- `layouts/` contains the application shell and cross-page layout components; layouts are not pages. Shell pieces live under `layouts/components/` (`pt-sidebar`, `pt-window-titlebar`, etc.).
- `components/custom/` contains project-owned reusable UI primitives (`pt-*`).
- `components/icons/` contains project-owned icon wrappers when Lucide/Tabler alone is not enough.
- `components/ui/` is generated shadcn-vue (New York) code. Do not edit it for project-wide behavior; configure it or wrap it in `components/custom/`.
- `stores/` owns UI and workflow state for one domain. A Store may coordinate services but must not copy another Store’s complete state.
- `lib/services/` owns side effects: Tauri `invoke` / `listen`, plugins (opener, process, updater, tray), dialogs, and OS APIs.
- `lib/utils/` owns deterministic functions only. Utilities must not read Stores, invoke Tauri, access storage, or mutate external state.
- `lib/models/` contains frontend-owned protocols and constants split by concrete domain. Business code imports the owning file directly.
- `locales/` owns all user-facing strings. Pages and Stores render from locale keys + typed codes.

Pages may present several domains together (for example Settings + About updater), but shared product orchestration must remain explicit; do not create a frontend `manager` or a global Store containing every workflow.

Current page domains: `cleaner`, `deep-cleaner`, `memory-cleaner`, `care`, `uninstaller`, `monitor`, `power`, `information`, `history`, `settings`.

Current Stores: `app-store` (shell/theme/navigation), `cleaner-store`, `deep-cleaner-store`, `memory-cleaner-store`, `care-store`, `startup-store`, `uninstaller-store`, `monitor-store`, `power-store`, `system-info-store`, `history-store`, `app-update-store`.

Deep Cleanup (`deep-cleaner`): category sidebar (system / application / browser / development / projects / ai; a group shows only when it has findings), analyzing workspace with files/data/elapsed, Smart recommendation above results, Clean confirm dialog with selected-item list + optional close/force-close of related apps. Elevation rules show an Admin badge; clean is skipped honestly when not elevated. Status `unmeasured` (needs admin / timed out) rows show "Size unknown" with the typed `reason`. Item rows (projects, AI models, toolchains) render `displayName` / `pathHint` / `tags` chips / `artifacts` / `lastModifiedMs` via `ruleTitle` / `ruleDetail` in `lib/models/deep-cleaner.ts` instead of locale keys; active projects get an "Active project" badge and are excluded from Smart and All. Uses `DeepCleanerService` → `scan_deep_cleanup` / `execute_deep_cleanup_command` / process probe-close (not the coarse 4-category Cleaner scan).

PC Care (`care`): tabs **Scan** / **Startup** (`pt-startup-manager.vue`). Home = circular Scan button with rotating conic ring, category checklist (selection + Auto fix + after-action persisted in `localStorage` as a per-viewer convenience), last-scan summary. Scanning/fixing view (`pt-care-run`) = left rail in `CareCategoryId` order (spinner → count badge / ✓ / ⓘ / — / skipped / failed), weighted percent ring (`pt-care-ring`, registered `--care-pct` transition), current-path line (middle ellipsis, monospace), Skip (first running category) and Stop (confirm). Results = collapsible category sections with item checkboxes, risk + Admin badges, allowlisted info actions; Fix → destructive confirm → fixing → summary (fixed / partial / failed / skipped / freed, undo hint). Items render from `titleKey`/`detailKey` (+ `params`) with literal fallbacks; key rows by `category:id`. Uses `CareService` → `scan_care` / `fix_care` / `stop_care` / `skip_care_category` / `open_care_action`, event `care-progress` (full snapshot, ~50 ms). History category `care`.

Startup Manager (`pages/care/pt-startup-manager.vue`, `startup-store`, `lib/models/startup.ts`): self-contained panel inside the Care Startup tab (no own `PtPageShell`). Boot header (last boot seconds or typed `needsAdmin` / `unavailable`), counts, search, filter chips All / Enabled / Disabled / New (Delayed counts as enabled), "Show Microsoft tasks & services" (read-only rows). Rows: icon (lazy `get_startup_icons_command` batches, initials fallback), name + New / Missing file / Admin / Windows / Policy (policy-locked Store app task, `isPolicyLocked`) badges, publisher, source (`packagedApp` = "Store app") + scope, impact chip, role=switch toggle, kebab (Delay / Start normally / Open file location / Delete broken entry → destructive confirm). Admin-only rows in a non-elevated process open the Restart as administrator prompt instead of calling Rust; results render from typed `StartupActionStatus` (`care.startup.notice.*`). Uses `StartupService` (ids only, never paths).

Uninstaller (`uninstaller`): "removed by another uninstaller" banner (scan leftovers / dismiss via `dismiss_removed_program_command`) above the program list (first load shows a live loading card: stage-weighted percent, program being read, found / read counts, ticking elapsed, fading feed of recent programs with their real icons (listing events seed `store.icons`, so the list does not refetch them) via the `PtOperationWorkspace` default slot; search, filter chips All / Desktop / Store / Recent 30 days / Large ≥500 MB, sort, runtimes hidden unless toggled) → confirm dialog (quick uninstall, scan leftovers, restore point when elevated, force remove for one desktop program, admin hint for machine-wide picks) → modal progress dialog (`pt-uninstall-progress-dialog`, same visual language as `PtOperationWorkspace`: program icon inside the conic rotating ring, steps restore point → uninstall → scan → clean, current-path card, progress bar (sweep while the vendor uninstaller runs, percent while scanning/cleaning), stat tiles with a locally ticking elapsed clock, "The uninstaller needs you" + **Show uninstaller window** when the vendor window is visible, scanner checklist with current path, cleaning bar + live per-item log) → leftover review (group tabs, medium = "Review" unselected, Admin badge) → destructive confirm → summary. Uses `UninstallerService` → `list_installed_programs` / `uninstall_program_command` / `scan_program_leftovers_command` / `clean_uninstall_leftovers` / `get_program_icons_command` (lazy batches of 24, PNG data URLs, initials fallback via `pt-program-icon`; Store logos drawn on a tile) / `focus_uninstaller_window` / `cancel_uninstaller`, progress event `uninstaller-progress`. Leftover item ids are only unique per scan — key rows by `scanId:id`. History category `uninstaller`.

## Vue and TypeScript

- Use Vue 3 `<script setup lang="ts">` and strict TypeScript. Do not introduce `any`.
- Project-owned Vue files use `kebab-case`; reusable custom components use the `pt-` prefix.
- Prefer props and emits for component communication. Do not use provide/inject as a hidden event bus.
- Pinia stores use the Options API (`state`, `getters`, `actions`).
- Prefer exported module functions or static service methods for stateless adapters. Use an owned service instance when it has lifecycle state (tray menus, pending updater handle) or needs test isolation.
- Avoid new composables and generic `use*` helpers when a named Store, static service, or pure utility gives clearer IDE navigation.
- Import business code from its concrete file. Minimal indexes required by generated shadcn code are not public project APIs.
- Do not duplicate Rust protocol types by guessing. When bindings are maintained manually, update Rust, TypeScript models/services, Stores, and any compatibility tests in one change.

## Services and events

- Progress listeners: **listen before invoke**; always unlisten in `finally`.
- Long-running operations belong in Stores that call services; pages stay presentational.
- Tray (`tray-service.ts`) must reuse Memory Cleaner settings and the same optimize path as the Memory page — do not fork “tray-only” clean logic. Prefer attaching the menu to the Rust-created tray id rather than creating a second icon without a fallback icon.
- Titlebar memory circle click runs Memory Cleaner optimize (`reason: tray`); the circle shows a rotating ring while busy and animates to the new % via registered `--mem` (no popover). The monitor snapshot must refresh quietly afterward so the gauge matches page stats. Monitor store polls ~1s with ordered `refreshQuiet` (drop stale replies). Titlebar RAM% prefers Memory Cleaner `physicalLoadPercent` via `displayedMemoryPercent` (`lib/models/memory-cleaner.ts`, shared with the tray); it must track the page / WMC / IObit physical load, not a stuck higher `dwMemoryLoad`-style value.
- Both 1s polls are **reference-counted**: `monitor.acquirePolling()` / `releasePolling()` and `memory.acquireStatsPolling()` / `releaseStatsPolling()`. The shell holds one share for the titlebar; pages acquire on mount and release on unmount — never stop a shared poll directly. While the window is hidden or minimized (`window-visibility-service`), the shell pauses both polls; the tray keeps a ~5 s tick that fetches its own samples, uses locale text (`tray.tooltip*`), and skips `setTooltip` / `setText` when the text is unchanged.
- Memory Cleaner shows **Restart as administrator** when Optimize / titlebar circle runs without elevation; declining continues and skips privileged areas. No persistent admin banner on the page. Tray / auto-clean stay quiet (clean what is possible). The store does not time auto-clean itself; `startAutoClean` only listens to `memory-auto-optimized` to refresh stats, history, and the titlebar gauge.
- Long operations (Cleaner, Deep Cleaner, Memory, Information) share `PtOperationWorkspace` with a **circular** indeterminate spinner (always rotating while busy — not progress-tied); the horizontal bar shows percent.
- Updater flow lives in `app-update-service` + `app-update-store`. About UI shows checking / up-to-date / available / download progress / install / restart states. Do not regress Check for Updates to only opening the GitHub releases URL.
- Settings **Keep activity history** (presets 10–500 + Custom 1–1000) goes through `history-store.saveSettings` → `HistoryService.updateSettings` → `update_history_settings`; reload the list afterwards because Core trims immediately.
- Settings **Open Folder** goes through `SettingsApi.openAppDataFolder` → `open_app_data_folder` (not frontend `openPath`).
- Information uses `SystemInfoService.loadWithProgress` + `system-info-progress` events; Power schedule UI shows live countdown + cancel while `hasActiveSchedule`.
- Window show/hide/close/tray hide behavior goes through the window/application services and matching Rust commands — preserve hide-to-tray until Exit.

## Text, status, and logging

- All user-facing strings belong in locale resources. Update every supported locale in the same change.
- Constants are domain-owned. Do not move every unrelated constant into a new global constants file.
- Render behavior from typed status, risk, capability, and reason codes. Free-form backend messages are diagnostics, not UI control flow.
- Prefer toast / store-reported errors over raw `console.*` in production paths. `app-store.reportError` maps the thrown value to a typed `AppErrorCode` (`lib/utils/errors.ts`, Rust `CommandError { code, details, retryable }`); the shell toast (`layouts/components/pt-error-toast.vue`) renders `errors.codes.<code>` (cancelled is never shown), auto-dismisses after ~6 s, and never shows raw messages or paths.
- Frontend logs and toasts must not contain raw filesystem paths, file contents, or unrelated user-specific metadata. Prefer typed events, counts, timings, and redacted diagnostics.

## Styling and interaction

- The project uses Tailwind CSS 4 + CSS variables (oklch) under `assets/` / themes. Prefer semantic tokens (`--background`, `--card`, `--primary`, `--muted-foreground`, …). No random hex in page templates.
- Soft, dense utility chrome: restrained borders, ~8–14px radius, warm soft surfaces, light + dark (+ system when wired).
- Use `PtPageShell` for page framing (`document` | `workspace`). Typical operation pages: toolbar → results → action bar.
- Density targets: page title ~22–28px · section ~0.9375rem · row text ~0.8125rem · result rows ~44px · sidebar item ~40px.
- Buttons and hover states must not translate, scale, or change layout dimensions. Use color, border, or shadow feedback only.
- Confirm destructive actions with project-owned confirm UI (`pt-confirm-dialog` or equivalent). Modal dialogs use `trapDialogFocus` (`lib/services/dialog-focus-service.ts`): initial focus on `[data-autofocus]` or the first button, Tab trapped, Escape = dismiss (ignored while busy). `pt-confirm-dialog` emits `cancel` for its cancel button and a separate `dismiss` for overlay / Escape, which must never run an action.
- Large selectable lists keep selection O(1): mutate `selected` in place or use a Set getter (`selectedIdSet`); do not rebuild item arrays per checkbox.
- Every looping animation needs a `prefers-reduced-motion` fallback.
- Never place raw SVG markup in a template. Use Lucide / Tabler icon components or wrappers under `components/icons/`.
- Native `<select>` popups on Windows cannot be fully rounded; prefer soft custom selects (`PtSoftSelect`) when the design requires rounded menus.
- When layout choices conflict, keep the existing soft shell — do not invent a new product layout.

## Fast / no hang (frontend)

- Main window stays hidden until the Vue app has mounted and is ready to show.
- Lazy-load secondary pages; idle-preload chunks when practical.
- Do not kick off long cleaner/memory/system scans on app boot.
- Sidebar and pages must show busy/disabled state while an operation is running; cancel must be reachable for long work.

## Validation

For frontend changes, run:

```sh
pnpm exec vue-tsc --noEmit
```

Also run `pnpm build` when a change touches packaging, assets, routing, or production-only code paths. Test affected UI flows in light and dark (and system theme when relevant). Verify About updater states, Memory Cleaner auto settings, and tray actions when those areas change.

When a change adds a page/store/service pattern, shell/tray/updater UI invariant, or locale workflow rule, update **this** file (and root [`AGENTS.md`](../AGENTS.md) if cross-cutting) in the same change. See `.cursor/rules/agents-md-sync.mdc`.
