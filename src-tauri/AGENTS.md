# Rust and Tauri Guidelines

This file applies only to `src-tauri/` and inherits the repository-wide rules in [`../AGENTS.md`](../AGENTS.md).

## Workspace boundaries

- `crates/pctoolkit-core`: platform-neutral product domains, use cases, safety policy, and persistence. It must not depend on Tauri or a WebView.
- `crates/pctoolkit-platform`: OS contracts and Windows implementations. It reports typed capabilities, samples, and safe fallbacks; it does not decide product workflows (no auto-clean policy, no history orchestration, no UI strings).
- `src/` (under `src-tauri`): thin Tauri adapter. Commands validate transport input, call Core, translate typed errors, and publish events.
- Plugins (`tauri-plugin-*`): isolated integration only (opener, process, single-instance, updater, tray via Tauri features). Register plugin init, capabilities, and frontend bindings in the same change. Tray icon itself is bootstrapped in `lib.rs` (`pctoolkit-main-tray`); frontend sets the menu.

Core modules today: `cleaner`, `memory`, `power`, `monitor`, `system_info`, `history`, `shared`. Platform modules include `memory`, `monitor`, `power`, `system_info`, `recycle`, `launch`, `process`, `gpu`, and related helpers. Adapter commands also include `settings::open_app_data_folder` (shell helper, not a new Core domain).

Do not create a giant `ToolkitService` that owns every domain. Keep cleaner, memory, power, and history as separate implementations that collaborate through typed requests/results.

## Rust organization and naming

- Files and modules use `snake_case`; types use precise domain nouns; functions use verbs that state observable behavior.
- Avoid `utils` for domain behavior. Put a helper beside its owner or in a narrowly named shared/infrastructure module (`shared` for cross-domain error types only).
- Keep visibility minimal. A new `pub` or `pub(crate)` API must represent a stable collaboration boundary, not an expedient way around module ownership.
- Prefer small typed request/result structures over long parameter lists and unrelated tuples.
- Prefer `deny_unknown_fields` + camelCase serde for frontend payloads unless an existing protocol already differs.
- Source comments, logs, errors, tests, and assertions must be clear and consistent. Explain safety assumptions, elevation requirements, performance tradeoffs, and fallback reasons.

## Errors, logs, and protocols

- Domain and platform code return typed errors or stable error codes (`CoreError` / `PlatformError`). Convert to Tauri transport errors only in the adapter (`commands/error` and friends).
- Logs use stable domain/event/field names. Log operation IDs, counts, timings, area skip reasons, and error digests — not private full paths or file contents.
- Persisted structures (Memory Cleaner settings, history) require a clear location under `%LOCALAPPDATA%\PC Toolkit Pro\` and a documented read / default / migrate / reject policy.
- Keep command names and event payloads versionable. Do not retain permanent old/new aliases after a migration window.

## Cleaner safety

- Default cleaner scan must work **without admin**. Denied paths skip-and-continue; do not fail the whole scan for one inaccessible folder.
- Destructive flows preserve preview/scan results, explicit user intent, progress, cancel, and honest result counts.
- Empty recycle bin and similar privileged/special operations must report failure clearly rather than pretending success.
- Missing permission, locked files, and platform uncertainty must fail closed or skip with typed reasons.

## Memory cleaner

- Stats and optimize areas use real Win32 / NT APIs in `pctoolkit-platform` (for example `GlobalMemoryStatusEx`, `NtSetSystemInformation` and related calls).
- Core owns settings persistence, auto-interval steps (from **5 minutes** upward), free-RAM threshold / cooldown policy, history writes, and orchestration of `optimize_memory`.
- When elevation is missing, skip or fail **per area** with honest status — never fake success via PowerShell or empty stubs.
- Cleaner `freeMemory` (if exposed) must wire through the same memory engine so tray, page, and cleaner stay consistent.
- Auto-clean from tray and UI must share the same settings file and optimize entry point.

## Power, monitor, system info

- Power actions and scheduled shutdown live in platform + Core; adapter commands stay thin. Prefer `System32\shutdown.exe`, wait for exit status (Python-era `check=True`), and cancel-then-reschedule before a new `/t` timer. Lock/sleep use Win32 (`LockWorkStation` / `SetSuspendState`), not rundll32 stubs.
- Quick-action GUI tools launch via System32 + `cmd /C start` so PATH/console flags from the Tauri host do not block Task Manager and peers.
- Monitor samples feed the titlebar and Monitor page; keep sampling cheap and non-blocking on the UI path.
- System information collection must avoid flashing a console window. Prefer quiet Win32 / WMI-style collection with typed fields (CPU, disks, RAM, GPU, monitors, motherboard, OS, PSU name when available). Emit staged `SystemInfoProgress` (`metrics` → `hardware` → `gpu` → `assemble`) for the Information UI.
- Memory privilege enable must treat `ERROR_NOT_ALL_ASSIGNED` as failure; map `STATUS_PRIVILEGE_NOT_HELD` to `skippedNeedAdmin`, not fake `ok` / vague `failed`.

## Platform code

- Define the contract before moving an implementation. Platform facts must not import cleaner UI concepts or frontend locale strings.
- Keep `cfg` at narrow module or item boundaries. This product is Windows-first; do not pretend full macOS/Linux support without real implementations.
- Native fast paths require a correct fallback and diagnostics that distinguish success, skip, and failure.
- Privileged operations require a separately reviewed capability boundary. Do not expand Tauri permissions “just in case,” and do not simulate a privileged helper inside an ordinary cleaner path.

## Tauri adapter and plugins

- Command handlers remain async adapters and contain no scan, cleanup, memory-optimize, or persistence algorithms.
- Register every command, permission, capability, frontend service binding, and plugin initialization in the same change.
- Capability scopes stay minimal (`capabilities/default.json`). Tray/menu needs `core:tray:default` + `core:menu:default` + `core:image:default`; updater needs `updater:default` only when wired end-to-end. Do not assume `opener:default` includes `allow-open-path`.
- App startup must not perform long scans or blocking filesystem work before the first window is shown. Tray icon bootstrap is allowed at setup (cheap); full menus attach from Vue after mount.
- Window close on `main` hides to tray; Exit from tray quits. Preserve single-instance focus behavior.
- Updater: `createUpdaterArtifacts` + pubkey/endpoints in `tauri.conf.json`; CI signs with `TAURI_SIGNING_PRIVATE_KEY` secrets and publishes `latest.json`. Write the secret to a temp file, then set `TAURI_SIGNING_PRIVATE_KEY` from that file in the build step (avoid `Out-File` UTF-8 BOM into `GITHUB_ENV`). Never commit private keys under `.tauri/`.
- `tauri` Cargo features for tray icons include `tray-icon`, `image-png`, and `image-ico` when embedding/loading tray images.

## Blocking work and cancel

- Run blocking Core/platform work only via `spawn_blocking` / `run_blocking` (or equivalent).
- Emit progress events from Core callbacks; never invent UI strings in Rust for control flow.
- Support cancel for long operations; return typed `operationBusy` instead of deadlocking when a second operation starts.

## Validation

For Rust changes, run what the local toolchain allows:

```sh
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml -p pctoolkit-core
```

When MSVC/`link.exe` is missing locally, do not claim a full Windows link succeeded — use GitHub Actions (`.github/workflows/tauri-build.yml` / `release-windows.yml`) for real binaries and state the unvalidated local scope.

Changes to cleaner safety, memory optimize areas, persistence, elevation paths, or performance require tests or a reproducible observation appropriate to the affected behavior. Keep raw machine evidence and private datasets outside the repository.

When a change adds a Core/platform module, Tauri command/event/plugin, persistence path, or Win32 honesty/elevation rule, update **this** file (and root [`AGENTS.md`](../AGENTS.md) if cross-cutting) in the same change. See `.cursor/rules/agents-md-sync.mdc`.
