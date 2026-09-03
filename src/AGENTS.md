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

Current page domains: `cleaner`, `deep-cleaner`, `memory-cleaner`, `monitor`, `power`, `information`, `history`, `settings`.

Current Stores: `app-store` (shell/theme/navigation), `cleaner-store`, `deep-cleaner-store`, `memory-cleaner-store`, `monitor-store`, `power-store`, `system-info-store`, `history-store`, `app-update-store`.

Deep Cleanup (`deep-cleaner`): category sidebar (system / application / browser / development), analyzing workspace with files/data/elapsed, Smart recommendation selection, Clean confirm. Uses `DeepCleanerService` → `scan_deep_cleanup` / `execute_deep_cleanup_command` (not the coarse 4-category Cleaner scan).

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
- Titlebar memory circle click runs Memory Cleaner optimize (`reason: tray`) and the monitor snapshot must refresh quietly afterward so the gauge matches page stats. Monitor store polls ~1s with ordered `refreshQuiet` (drop stale replies). Titlebar RAM% prefers Memory Cleaner `physicalLoadPercent` (shell starts stats polling); it must track the page / WMC / IObit physical load, not a stuck higher `dwMemoryLoad`-style value.
- Memory Cleaner shows **Restart as administrator** when Optimize / titlebar circle runs without elevation; declining continues and skips privileged areas. No persistent admin banner on the page. Tray / auto-clean stay quiet (clean what is possible).
- Long operations (Cleaner, Deep Cleaner, Memory, Information) share `PtOperationWorkspace` with a **circular** indeterminate spinner (always rotating while busy — not progress-tied); the horizontal bar shows percent.
- Updater flow lives in `app-update-service` + `app-update-store`. About UI shows checking / up-to-date / available / download progress / install / restart states. Do not regress Check for Updates to only opening the GitHub releases URL.
- Settings **Open Folder** goes through `SettingsApi.openAppDataFolder` → `open_app_data_folder` (not frontend `openPath`).
- Information uses `SystemInfoService.loadWithProgress` + `system-info-progress` events; Power schedule UI shows live countdown + cancel while `hasActiveSchedule`.
- Window show/hide/close/tray hide behavior goes through the window/application services and matching Rust commands — preserve hide-to-tray until Exit.

## Text, status, and logging

- All user-facing strings belong in locale resources. Update every supported locale in the same change.
- Constants are domain-owned. Do not move every unrelated constant into a new global constants file.
- Render behavior from typed status, risk, capability, and reason codes. Free-form backend messages are diagnostics, not UI control flow.
- Prefer toast / store-reported errors over raw `console.*` in production paths.
- Frontend logs and toasts must not contain raw filesystem paths, file contents, or unrelated user-specific metadata. Prefer typed events, counts, timings, and redacted diagnostics.

## Styling and interaction

- The project uses Tailwind CSS 4 + CSS variables (oklch) under `assets/` / themes. Prefer semantic tokens (`--background`, `--card`, `--primary`, `--muted-foreground`, …). No random hex in page templates.
- Soft, dense utility chrome: restrained borders, ~8–14px radius, warm soft surfaces, light + dark (+ system when wired).
- Use `PtPageShell` for page framing (`document` | `workspace`). Typical operation pages: toolbar → results → action bar.
- Density targets: page title ~22–28px · section ~0.9375rem · row text ~0.8125rem · result rows ~44px · sidebar item ~40px.
- Buttons and hover states must not translate, scale, or change layout dimensions. Use color, border, or shadow feedback only.
- Confirm destructive actions with project-owned confirm UI (`pt-confirm-dialog` or equivalent).
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
