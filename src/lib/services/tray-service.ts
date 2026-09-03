import { TrayIcon } from '@tauri-apps/api/tray';
import { Menu, MenuItem, PredefinedMenuItem, Submenu } from '@tauri-apps/api/menu';
import { defaultWindowIcon } from '@tauri-apps/api/app';
import { Image } from '@tauri-apps/api/image';
import { exit } from '@tauri-apps/plugin-process';
import { ApplicationWindowService } from './application-window-service';
import { MonitorService, PowerService } from './api-services';
import { useAppStore } from '@/stores/app-store';
import { useMemoryCleanerStore } from '@/stores/memory-cleaner-store';
import { useMonitorStore } from '@/stores/monitor-store';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { formatBytes } from '@/lib/utils/format';
import { i18n } from '@/i18n';

/** Must match `TRAY_ID` in `src-tauri/src/lib.rs`. */
const TRAY_ID = 'pctoolkit-main-tray';

let tray: TrayIcon | null = null;
let tooltipTimer: number | null = null;
let memoryStatusItem: MenuItem | null = null;
let cleanMemoryItem: MenuItem | null = null;
let cleaningFromTray = false;

function t(key: string, params?: Record<string, unknown>): string {
  return (params ? i18n.global.t(key, params) : i18n.global.t(key)) as string;
}

function memoryStatusLabel(percent: number): string {
  return t('tray.memoryStatus', { percent: percent.toFixed(0) });
}

async function openPage(pageId: (typeof PAGE_IDS)[keyof typeof PAGE_IDS]) {
  useAppStore().navigate(pageId);
  await ApplicationWindowService.showAfterMount();
}

async function resolveTrayIcon(): Promise<Image | Uint8Array> {
  const fromApp = await defaultWindowIcon();
  if (fromApp) return fromApp;

  // Dev / fallback: public/icon.png is always available to the webview.
  const response = await fetch('/icon.png');
  if (!response.ok) {
    throw new Error(`tray icon fetch failed (${response.status})`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

async function runCleanMemoryFromTray() {
  if (cleaningFromTray || useMemoryCleanerStore().loading) return;
  cleaningFromTray = true;
  try {
    await cleanMemoryItem?.setEnabled(false);
    await cleanMemoryItem?.setText(t('tray.cleaningMemory'));
    await tray?.setTooltip(t('tray.cleaningTooltip'));

    await useMemoryCleanerStore().run('tray');
    const result = useMemoryCleanerStore().result;
    const freed = result?.freedBytes ?? 0;
    const freedLabel = formatBytes(freed);

    await cleanMemoryItem?.setText(t('tray.cleanedMemory', { bytes: freedLabel }));
    await tray?.setTooltip(t('tray.cleanedTooltip', { bytes: freedLabel }));

    try {
      const snap = await MonitorService.snapshot();
      useMonitorStore().$patch({ snapshot: snap });
      await memoryStatusItem?.setText(memoryStatusLabel(snap.memoryPercent));
    } catch {
      // keep previous status if refresh fails
    }

    window.setTimeout(() => {
      void cleanMemoryItem?.setText(t('tray.cleanMemory'));
    }, 4000);
  } catch {
    await cleanMemoryItem?.setText(t('tray.cleanMemory'));
  } finally {
    cleaningFromTray = false;
    await cleanMemoryItem?.setEnabled(true);
  }
}

export async function setupTray(): Promise<void> {
  try {
    memoryStatusItem = await MenuItem.new({
      id: 'memory-status',
      text: memoryStatusLabel(0),
      enabled: false,
    });

    cleanMemoryItem = await MenuItem.new({
      id: 'clean-memory',
      text: t('tray.cleanMemory'),
      action: () => {
        void runCleanMemoryFromTray();
      },
    });

    const powerMenu = await Submenu.new({
      id: 'power-options',
      text: t('tray.powerOptions'),
      items: [
        {
          id: 'shutdown',
          text: t('tray.shutdown'),
          action: async () => {
            await PowerService.execute('shutdown');
          },
        },
        {
          id: 'restart',
          text: t('tray.restart'),
          action: async () => {
            await PowerService.execute('restart');
          },
        },
        {
          id: 'sleep',
          text: t('tray.sleep'),
          action: async () => {
            await PowerService.execute('sleep');
          },
        },
        {
          id: 'lock',
          text: t('tray.lock'),
          action: async () => {
            await PowerService.execute('lock');
          },
        },
      ],
    });

    const systemMenu = await Submenu.new({
      id: 'system-info',
      text: t('tray.systemInfo'),
      items: [
        {
          id: 'open-monitor',
          text: t('navigation.monitor'),
          action: async () => {
            await openPage(PAGE_IDS.monitor);
          },
        },
        {
          id: 'open-information',
          text: t('navigation.information'),
          action: async () => {
            await openPage(PAGE_IDS.information);
          },
        },
      ],
    });

    const quickActionsMenu = await Submenu.new({
      id: 'quick-actions',
      text: t('tray.quickActions'),
      items: [
        {
          id: 'open-cleaner',
          text: t('navigation.cleaner'),
          action: async () => {
            await openPage(PAGE_IDS.cleaner);
          },
        },
        {
          id: 'open-deep-cleaner',
          text: t('navigation.deepCleaner'),
          action: async () => {
            await openPage(PAGE_IDS.deepCleaner);
          },
        },
        {
          id: 'open-memory-cleaner',
          text: t('navigation.memoryCleaner'),
          action: async () => {
            await openPage(PAGE_IDS.memoryCleaner);
          },
        },
      ],
    });

    const menu = await Menu.new({
      items: [
        {
          id: 'show',
          text: t('tray.show'),
          action: async () => {
            await ApplicationWindowService.showAfterMount();
          },
        },
        await PredefinedMenuItem.new({ item: 'Separator' }),
        memoryStatusItem,
        cleanMemoryItem,
        await PredefinedMenuItem.new({ item: 'Separator' }),
        powerMenu,
        systemMenu,
        quickActionsMenu,
        await PredefinedMenuItem.new({ item: 'Separator' }),
        {
          id: 'exit',
          text: t('tray.exit'),
          action: async () => {
            await exit(0);
          },
        },
      ],
    });

    // Prefer the Rust-created tray (always has a real Windows icon).
    tray = await TrayIcon.getById(TRAY_ID);
    if (!tray) {
      const icon = await resolveTrayIcon();
      tray = await TrayIcon.new({
        id: TRAY_ID,
        icon,
        menu,
        tooltip: t('app.name'),
        showMenuOnLeftClick: false,
        action: async (event) => {
          if (event.type !== 'Click' || event.buttonState !== 'Up') return;
          if (event.button === 'Left') {
            await ApplicationWindowService.showAfterMount();
            return;
          }
          if (event.button === 'Middle') {
            await runCleanMemoryFromTray();
          }
        },
      });
    } else {
      await tray.setMenu(menu);
      await tray.setTooltip(t('app.name'));
      await tray.setShowMenuOnLeftClick(false);
      await tray.setVisible(true);
      // Re-assert icon in case the host started without one.
      try {
        await tray.setIcon(await resolveTrayIcon());
      } catch {
        // keep Rust-provided icon
      }
    }

    // Reuse monitor store’s 1s live snapshot (same GlobalMemoryStatusEx % as Memory page).
    tooltipTimer = window.setInterval(async () => {
      if (cleaningFromTray) return;
      const snap = useMonitorStore().snapshot;
      if (!snap) return;
      try {
        const gpu = snap.gpuAvailable
          ? ` · GPU ${snap.gpuUtilization?.toFixed(0) ?? 0}%`
          : '';
        await tray?.setTooltip(
          `RAM ${snap.memoryPercent.toFixed(0)}% · CPU ${snap.cpu.toFixed(0)}%${gpu}`
        );
        await memoryStatusItem?.setText(memoryStatusLabel(snap.memoryPercent));
      } catch {
        // ignore tray tooltip failures
      }
    }, 1000);

    void MonitorService.snapshot()
      .then(async (snap) => {
        useMonitorStore().$patch({ snapshot: snap });
        await memoryStatusItem?.setText(memoryStatusLabel(snap.memoryPercent));
        await tray?.setTooltip(
          `RAM ${snap.memoryPercent.toFixed(0)}% · CPU ${snap.cpu.toFixed(0)}%`
        );
      })
      .catch(() => undefined);
  } catch (error) {
    // Surface tray failures — silent catch made the icon disappear with no clue.
    useAppStore().reportError(error);
  }
}

export function disposeTray() {
  if (tooltipTimer != null) clearInterval(tooltipTimer);
  tooltipTimer = null;
  memoryStatusItem = null;
  cleanMemoryItem = null;
  void tray?.close();
  tray = null;
}
