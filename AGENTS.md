## Learned User Preferences

- Prefer a full rewrite to Rust + Tauri 2 over Go/Wails for PC Toolkit Pro’s long-term desktop product.
- Match MangoDisk’s UI closely (shell, density, soft look, sidebar, tables, dialogs, light/dark); only branding should be PC Toolkit Pro (name, logo, accent)—do not invent a different layout when unsure.
- Follow MangoDisk’s tech stack: Tauri 2, Vue 3 + TypeScript, Pinia, Tailwind 4, shadcn-vue, thin Tauri commands, and a Rust core with no Tauri dependency.
- Prioritize fast load and no UI hangs using MangoDisk-style patterns (deferred window show, no heavy work on startup, background workers, progress events + cancel, busy guards).
- Keep `master` stable for the shipped Python app; do Tauri rewrite and experiments on a side branch and merge only after solid testing.
- Prefer avoiding a full Visual Studio install locally; use GitHub Actions for real `.exe` builds and browser/`npm run dev` for UI-only preview when a local C++ toolchain is missing.

## Learned Workspace Facts

- Production PyQt app historically lived on `master`; the Tauri rewrite on `feat/tauri-rewrite` replaces Python with root `src/` + `src-tauri/` (Vue 3 + Rust core/platform).
- Active rewrite branch is `feat/tauri-rewrite`; Python sources are removed on this branch after parity implementation.
- UI/architecture reference is harry0703/MangoDisk (Tauri 2 + Vue + Rust core); use patterns only—do not copy GPL source or mango branding.
- Project Cursor guidance lives under `.cursor/skills/mangodisk-style-desktop-ui/` and `.cursor/rules/mangodisk-style-ui.mdc` plus `tauri-rust-adapter.mdc`.
- Windows CI build: `.github/workflows/tauri-build.yml` (MSVC available on `windows-latest`).
