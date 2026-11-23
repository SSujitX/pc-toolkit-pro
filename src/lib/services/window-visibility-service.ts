import { getCurrentWindow } from '@tauri-apps/api/window';

/** How often to re-check while nothing else signals a change (hide-to-tray emits no event). */
const PROBE_MS = 5000;

/**
 * Report whether the main window is actually on screen (not hidden in the tray, not minimized),
 * so the shell can pause its 1s polls. Combines `document.visibilityState`, Tauri focus/resize
 * events, and a slow probe. Returns a stop function. Browser preview always reports visible.
 */
export async function watchWindowVisibility(
  onChange: (visible: boolean) => void
): Promise<() => void> {
  let last: boolean | null = null;
  let stopped = false;
  const unlisteners: Array<() => void> = [];

  let win: ReturnType<typeof getCurrentWindow> | null = null;
  try {
    win = getCurrentWindow();
  } catch {
    win = null;
  }

  async function check() {
    if (stopped) return;
    let visible = document.visibilityState === 'visible';
    if (visible && win) {
      try {
        visible = (await win.isVisible()) && !(await win.isMinimized());
      } catch {
        // No Tauri bridge (browser preview): trust the document state.
      }
    }
    if (stopped || visible === last) return;
    last = visible;
    onChange(visible);
  }

  const onDocumentVisibility = () => void check();
  document.addEventListener('visibilitychange', onDocumentVisibility);
  unlisteners.push(() => document.removeEventListener('visibilitychange', onDocumentVisibility));

  if (win) {
    try {
      unlisteners.push(await win.onFocusChanged(() => void check()));
      unlisteners.push(await win.onResized(() => void check()));
    } catch {
      // Events unavailable: the probe below still converges.
    }
  }

  const probe = window.setInterval(() => void check(), PROBE_MS);
  unlisteners.push(() => window.clearInterval(probe));
  void check();

  return () => {
    stopped = true;
    for (const unlisten of unlisteners) unlisten();
  };
}
