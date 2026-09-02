# PC Toolkit Pro v3.0 — Tauri parity checklist

Source of truth: PyQt v2.9 feature matrix vs Tauri rewrite on `feat/tauri-rewrite`.

| ID | Capability | Implemented | Verified on Windows build |
|----|------------|-------------|---------------------------|
| M1 | CPU/RAM/Disk/uptime live | Yes (`get_monitor_snapshot`) | CI / manual |
| M2 | NVIDIA GPU optional | Yes (`nvidia-smi`) | CI / manual |
| M3 | OS label | Yes | CI / manual |
| Q1–Q15 | Quick Actions | Yes (`open_quick_action`) | CI / manual |
| Q-Admin | Elevated CMD/PS | Yes | CI / manual |
| C1 | Temp/prefetch clean | Yes | CI / manual |
| C2 | Recycle bin | Yes | CI / manual |
| C3 | Disk Cleanup launch | Yes | CI / manual |
| C4 | Free memory (measured) | Yes | CI / manual |
| P1–P6 | Power actions | Yes | CI / manual |
| P7–P8 | Schedule/cancel | Yes | CI / manual |
| I1–I10 | Information + copy | Yes | CI / manual |
| T1–T5 | Tray | Yes | CI / manual |
| U1 | Dark/light dense UI | Yes | CI / manual |
| R1 | Tauri Windows CI | Yes (`tauri-build.yml`) | CI |

## Performance gate

- [x] Window starts hidden; shown after Vue mount
- [x] No filesystem scan on startup
- [x] Heavy work via `run_blocking`
- [x] Cleaner progress events + cancel
- [x] Lazy secondary pages + idle preload
- [x] Buttons do not translate/scale

## Sign-off

Parity implemented in code on `feat/tauri-rewrite`. Manual verification against a downloaded CI artifact completes Phase Y for release candidates.
