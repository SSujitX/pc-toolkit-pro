import { TrayIcon } from '@tauri-apps/api/tray';
import { Menu } from '@tauri-apps/api/menu';
import { defaultWindowIcon } from '@tauri-apps/api/app';
import { ApplicationWindowService } from './application-window-service';
import { MonitorService } from './api-services';
import { PowerService } from './api-services';
import { exit } from '@tauri-apps/plugin-process';
import { useAppStore } from '@/stores/app-store';
import { PAGE_IDS } from '@/lib/models/application-shell';

let tray: TrayIcon | null = null;
let tooltipTimer: number | null = null;

export async function setupTray(): Promise<void> {
  try {
    const menu = await Menu.new({
      items: [
        {
          id: 'show',
          text: 'Show PC Toolkit Pro',
          action: async () => {
            await ApplicationWindowService.showAfterMount();
          },
        },
        {
          id: 'cleaner',
          text: 'System Cleaner',
          action: async () => {
            useAppStore().navigate(PAGE_IDS.cleaner);
            await ApplicationWindowService.showAfterMount();
          },
        },
        {
          id: 'shutdown',
          text: 'Shutdown',
          action: async () => {
            await PowerService.execute('shutdown');
          },
        },
        {
          id: 'restart',
          text: 'Restart',
          action: async () => {
            await PowerService.execute('restart');
          },
        },
        {
          id: 'sleep',
          text: 'Sleep',
          action: async () => {
            await PowerService.execute('sleep');
          },
        },
        {
          id: 'lock',
          text: 'Lock',
          action: async () => {
            await PowerService.execute('lock');
          },
        },
        {
          id: 'exit',
          text: 'Exit',
          action: async () => {
            await exit(0);
          },
        },
      ],
    });

    const icon = await defaultWindowIcon();
    tray = await TrayIcon.new({
      icon: icon ?? undefined,
      menu,
      tooltip: 'PC Toolkit Pro',
      action: async (event) => {
        if (event.type === 'Click' && event.button === 'Left') {
          await ApplicationWindowService.showAfterMount();
        }
      },
    });

    tooltipTimer = window.setInterval(async () => {
      try {
        const snap = await MonitorService.snapshot();
        const gpu = snap.gpuAvailable ? ` GPU ${snap.gpuUtilization?.toFixed(0)}%` : '';
        await tray?.setTooltip(
          `CPU ${snap.cpu.toFixed(0)}% · RAM ${snap.memoryPercent.toFixed(0)}%${gpu}`
        );
      } catch {
        // ignore tray tooltip failures
      }
    }, 2000);
  } catch {
    // Browser / unsupported environments skip tray.
  }
}

export function disposeTray() {
  if (tooltipTimer != null) clearInterval(tooltipTimer);
  tooltipTimer = null;
  void tray?.close();
  tray = null;
}
