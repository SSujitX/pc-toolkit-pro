# PC Toolkit Pro — Contribution Guidelines

These rules apply to the entire repository. More specific guidance lives in [`src/AGENTS.md`](src/AGENTS.md) and [`src-tauri/AGENTS.md`](src-tauri/AGENTS.md); a child file adds only rules for its own subtree and does not replace this file.

Thin Cursor rules under `.cursor/rules/` reinforce matching globs only. Prefer these `AGENTS.md` files over third-party-named skills or inspiration clones.

## Product and architecture

PC Toolkit Pro is a **Windows-first** Tauri 2 desktop toolkit for cleanup, memory optimization, monitoring, power control, and system information. The Vue frontend is an adapter for user interaction. Rust owns filesystem access, Win32 capabilities, scan/cleanup orchestration, memory optimization, power actions, history persistence, and typed operational results.

App name is **PC Toolkit Pro** only. Logo source of truth is root `pctoolkitpro.png` (copied/generated into `src/assets/brand/logo.png`, `public/icon.*`, and `src-tauri/icons/*` via `pnpm exec tauri icon pctoolkitpro.png`). Never ship another product’s name, logo, copy, or GPL source/assets.

Default branch is `master` (Tauri 2 + Vue 3 + Rust). Do not revive the retired Python/PyQt tree here.

Keep these domain boundaries stable:

- `cleaner`: junk/temp scan, preview, execute, cancel, progress;
- `deep-cleaner`: Deep Cleanup UI (system / application / browser / developer caches) over `cleaner` + `cleaner_deep` Core rules — Smart selection above results, confirm dialog with optional close/force-close of related apps; Core ownership stays in cleaner/shared safety (skip-and-continue, no GPL rule copies);
- `memory`: physical/virtual stats, selectable optimize areas, auto-clean settings, tray-aligned clean;
- `power`: shutdown / restart / sleep / lock and scheduled shutdown;
- `monitor`: live titlebar/monitor snapshot (uptime, disk, memory);
- `system_info`: full hardware/OS report for Information / export;
- `history`: operation records under app local data;
- `quick_actions`: launch common Windows tools/settings (platform launch helpers);
- `window` / tray / updater: shell adapters over Core + plugins — not new product domains.
- `settings` (presentation): theme/language/About/updater UI; **Open Folder** opens `%LOCALAPPDATA%\PC Toolkit Pro\` via a Rust command (`open_app_data_folder`) — do not rely on frontend `opener:allow-open-path` alone (`opener:default` does not include it).

Do not create broad modules such as `common`, `misc`, `manager`, `optimization`, or a new service that aggregates unrelated domains. Product pages may combine domain results without moving that coordination into a giant Core service.

```
Vue pages / shell     → presentation only
lib/services/*        → invoke(), listen(), dialogs, OS/plugin APIs
Pinia stores          → one domain workflow each
src-tauri/commands/*  → validate input → call Core → map errors/events
pctoolkit-core        → scan, safety, orchestration, persistence (no Tauri)
pctoolkit-platform    → OS facts/capabilities (no product orchestration)
```

## Implementation principles

1. Prefer the smallest practical design that preserves a clear boundary. Do not add abstractions for hypothetical reuse.
2. Keep side effects at adapters and domain boundaries. Pure classification, formatting, and calculation must remain deterministic.
3. Prefer typed status, risk, capability, and reason codes across process and persistence boundaries. Free-form messages are diagnostics, not UI control flow.
4. Blocking or long work must not run on the UI/async adapter thread. Use `spawn_blocking` / `run_blocking`, emit progress, support cancel, and return typed `operationBusy` instead of deadlocking.
5. Preserve safety invariants: skip-and-continue on denied paths for default cleaner scans, protected-path awareness, explicit confirmation for destructive actions, honest elevation/skip reporting for privileged memory APIs, and safe fallback when capabilities are unavailable.
6. Never hide a compatibility or honesty regression with fake success (for example PowerShell “memory clean”), silent privilege elevation, or undocumented fallbacks.
7. Startup must stay fast: main window starts **hidden** and `show()` only after Vue mounts; no long scans or blocking filesystem work before first paint.

## Naming and text

- Rust files and modules use `snake_case`; frontend, documentation, and resource file names use `kebab-case` unless an external tool requires another format.
- Project-owned Vue components use the `pt-` prefix so ownership is recognizable as PC Toolkit Pro code.
- Source comments, logs, diagnostic codes, test names, and assertions must be clear and consistent. All code comments must use idiomatic, professional English. Comments explain reasons, risks, and non-obvious boundaries rather than restating code.
- User-facing text belongs in locale resources (`src/locales/`). Update every supported locale when contributor-visible behavior changes.
- Use stable typed enums or codes across process and persistence boundaries. Do not make UI logic infer behavior from free-form messages.

## Desktop product invariants

Soft, dense utility chrome — warm soft background, soft sidebar, restrained borders, ~8–14px radius, light + dark (+ system theme where implemented).

- **Shell:** custom titlebar (uptime / disk / memory circle / window controls) · collapsible sidebar · `PtPageShell` (`document` | `workspace`).
- **Density:** page title ~22–28px · section ~0.9375rem · row text ~0.8125rem · result rows ~44px · sidebar item ~40px.
- **Interaction:** buttons change color/border/shadow only — never translate/scale. Confirm destructive actions. Semantic CSS tokens only (no random hex in pages).
- **When unsure:** keep the existing soft shell patterns; do not invent a different product layout.

### Cleaner / Memory

- Default cleaner scan: **no admin required**; skip-and-continue on denied paths (temp, prefetch, recycle, related junk). Recycle Bin size matches Explorer (`SHQueryRecycleBinW`); never walk `$Recycle.Bin`.
- Memory Cleaner: selectable areas, live physical/virtual stats, auto-clean from **5 min** upward + free-RAM threshold; tray uses the same settings. Real Win32 APIs; honest skip/log when not elevated; **no PowerShell fake**; settings in `%LOCALAPPDATA%\PC Toolkit Pro\`.
- Optimize / titlebar circle prompts **Restart as administrator** only when needed; declining continues with honest skips. No always-on admin banner. Tray and auto-clean do not UAC-nag.

### Tray / Information / Updates / Power / Shell

- Tray + hide-to-tray until Exit (Python-era behavior). Close / Alt+F4 hides; Exit quits.
- Tray icon is created at **Rust startup** (stable id `pctoolkit-main-tray`) with a real icon; Vue attaches the menu after mount. Capabilities need `core:tray:default`, `core:menu:default`, and `core:image:default`.
- Titlebar memory circle is **click-to-optimize** (same Memory Cleaner settings/path as tray clean) and must refresh immediately after optimize. Live RAM % uses the same Task-Manager-consistent load (`GetPerformanceInfo` via `memory_stats()`) as Memory Cleaner / tray / WMC / IObit and polls about once per second. Do not spawn `nvidia-smi` on every titlebar poll (late replies can pin a stale higher %).
- No flashing console when collecting system info.
- Information load emits staged progress (metrics → hardware → GPU → assemble); UI uses the shared operation workspace spinner (same as Cleaner / Memory).
- Power schedule: after confirm, show a live countdown with **Cancel Shutdown** (`shutdown /a`); power actions check Windows exit status (no fire-and-forget success).
- App updates use `@tauri-apps/plugin-updater` with signed release artifacts and GitHub `latest.json` — not “open releases URL” as the primary Check for Updates path.
- Windows NSIS installer uses **`currentUser`** install mode: `%LOCALAPPDATA%\PC Toolkit Pro\` with Start Menu entry + `uninstall.exe` (no admin required to install). Main binary name `PCToolkitPro.exe`.
- GitHub Release title uses **`PC Toolkit Pro vX.Y.Z`**. Uploaded files keep that display label, but GitHub stores names with spaces as dots (`PC.Toolkit.Pro.vX.Y.Z.Setup.exe`). `latest.json` must use the stored name or the in-app updater 404s.
- Operator release: **Actions → Release → Run workflow** with bump `current | patch | minor | major` (from root `VERSION`, no free-typed semver). Workflow syncs packages, commits, tags `vX.Y.Z`, builds Windows setup + portable, and publishes categorized commit notes plus `latest.json`.

## Repository hygiene

- Existing and untracked changes belong to the user unless proven otherwise. Do not overwrite, delete, reformat, or include unrelated changes in a commit.
- Do not edit generated or third-party source to implement application-wide behavior.
- Import project-owned business modules from concrete files. Do not add business barrel files unless a tool genuinely requires one.
- Do not commit credentials, private signing keys (`.tauri/*.key`), personal file contents, raw private paths in Markdown, build outputs, or local dependency directories.
- Logs and diagnostics must not expose raw filesystem paths, file contents, installation identifiers, or unrelated user-specific metadata. Prefer operation IDs, counts, timings, typed reason codes, and error digests.
- Do not push commits or open PRs unless the user explicitly asks. A request to commit does not authorize push.
- Optional reference trees under `test_inspiration/` (gitignored) are for **behavior/UX patterns only**. Do **not** copy GPL source, names, logos, or assets into the product.

## Workflow

1. Read the nearest `AGENTS.md` before changing code.
2. Inspect the worktree and establish a behavior baseline proportional to risk.
3. Write a concise implementation plan for multi-step work and keep it updated when scope changes.
4. Keep structural moves separate from behavior changes whenever review would otherwise become ambiguous.
5. Validate on Windows when the change touches Win32, NSIS, tray, elevation, or installer behavior. If the environment lacks MSVC/`link.exe`, use GitHub Actions for real `.exe` builds and document what was not validated locally.
6. Review the final diff for correctness, safety, honesty of privileged paths, naming, locales, and stale documentation before committing.
7. **Keep AGENTS.md current:** when the change adds a domain, command/event, store, safety/honesty rule, persistence path, tray/updater/shell invariant, or validation requirement, update root [`AGENTS.md`](AGENTS.md) and the matching child ([`src/AGENTS.md`](src/AGENTS.md) and/or [`src-tauri/AGENTS.md`](src-tauri/AGENTS.md)) in the **same** change. Skip AGENTS edits for typo-only, format-only, or one-off fixes that do not change documented invariants. Cursor rule: `.cursor/rules/agents-md-sync.mdc`.

## Required validation

Run the smallest applicable checks during development and the complete checks before submitting a change or preparing a release:

```sh
pnpm exec vue-tsc --noEmit
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml -p pctoolkit-core
```

UI-only preview: `pnpm dev`. Full desktop: `pnpm tauri:dev` / `pnpm tauri:build` when the local toolchain can link.

Prefer GitHub Actions for real Windows installers when local MSVC is missing (`.github/workflows/tauri-build.yml`, `.github/workflows/release-windows.yml`).

Tests are required for high-risk logic, persistence, safety boundaries, memory optimize outcomes, and regressions. Ordinary presentation-only changes may rely on type, build, and interaction checks when an automated test would not add meaningful confidence.

## Public guidance

- Keep contributor guidance concise and place domain-specific instructions near the code they govern. Child `AGENTS.md` files own subtree detail.
- Do not commit private research, raw machine reports, credentials, or private release tooling. Durable architecture decisions may be documented publicly when they help contributors.
- Update public guidance in the same change when contributor-visible behavior or validation changes (new domains, commands, safety rules, persistence, tray/updater, or required checks). Typo-only and one-off bug fixes without invariant changes do not require AGENTS edits.

## Learned User Preferences

- When the `github-accurate-commits` skill is attached or requested, prefer one short conventional commit per file (a source file may share a commit with its matching test for the same change).
- Do not add `Co-authored-by` Cursor/Codex trailers to commits; strip them if a hook injects them when the user objects.
- Keep the product MIT-licensed; do not switch to GPL/GNU to match inspiration clones unless the user explicitly relicenses.
- Match inspiration soft-density UI and professionalism, but keep PC Toolkit Pro branding and aim to exceed the reference apps rather than pixel-clone them.
- Shared operation/loading spinners should be circular and continuously rotating (activity indicator), not a rounded square frame or progress-stepped spin.

## Learned Workspace Facts

- The project license is MIT, not GPL (unlike MangoDisk / some inspiration trees under `test_inspiration/`).
- With `createUpdaterArtifacts` enabled, CI must provide `TAURI_SIGNING_PRIVATE_KEY` (and password if configured) to both `release-windows.yml` and `tauri-build.yml`, or the build fails after packaging despite a successful EXE/NSIS step.
- On Windows runners, write the key to a temp file and set `TAURI_SIGNING_PRIVATE_KEY` from that file in the **build** step. Do not use PowerShell `Out-File -Encoding utf8` for `GITHUB_ENV` (UTF-8 BOM breaks the variable name); use `Add-Content -Encoding utf8NoBOM` or append without a BOM.
- The CI updater private key must match `plugins.updater.pubkey` in Tauri config; a wrong password fails key decode, and a mismatched keypair warns and will not verify updates at runtime.
- Dual macOS support is feasible for shared shell/updater patterns, but Memory Cleaner Win32 optimize areas stay Windows-only; do not fake Mac feature parity for those APIs.
