import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { CleanerCategory, QuickActionId, PowerAction } from '@/lib/models/actions';

export interface MonitorSnapshot {
  cpu: number;
  memoryPercent: number;
  memoryUsed: number;
  memoryTotal: number;
  diskPercent: number;
  diskUsed: number;
  diskTotal: number;
  uptimeSeconds: number;
  osLabel: string;
  gpuAvailable: boolean;
  gpuUtilization?: number | null;
  gpuMemoryUsed?: number | null;
  gpuMemoryTotal?: number | null;
  gpuTemperature?: number | null;
}

export interface CleanupScanItem {
  id: CleanerCategory;
  titleKey: string;
  estimatedBytes: number;
  requiresAdmin: boolean;
  selected: boolean;
}

export interface CleanupScan {
  items: CleanupScanItem[];
  isAdmin: boolean;
}

export interface CleanupProgress {
  phase: string;
  current: number;
  total: number;
  message: string;
}

export interface CleanupResult {
  freedBytes: number;
  filesRemoved: number;
  log: string[];
  memory?: {
    freeBefore: number;
    freeAfter: number;
    freedBytes: number;
    adminOptimizations: boolean;
  } | null;
}

export interface SystemInformation {
  uptime: string;
  cpuName: string;
  cpuCores: number;
  cpuThreads: number;
  cpuUsage: number;
  memoryTotal: number;
  memoryUsed: number;
  memoryPercent: number;
  diskTotal: number;
  diskUsed: number;
  diskPercent: number;
  gpuName: string;
  gpuUsage?: number | null;
  gpuMemoryUsed?: number | null;
  gpuMemoryTotal?: number | null;
  gpuTemperature?: number | null;
  motherboard: string;
  bios: string;
  osEdition: string;
  osVersion: string;
  osBuild: string;
  hostname: string;
  username: string;
  monitors: string[];
  storageDevices: string[];
  copyText: string;
}

export class MonitorService {
  static snapshot(): Promise<MonitorSnapshot> {
    return invoke('get_monitor_snapshot');
  }
}

export class QuickActionService {
  static open(action: QuickActionId): Promise<void> {
    return invoke('open_quick_action', { request: { action } });
  }
}

export class CleanerService {
  static scan(): Promise<CleanupScan> {
    return invoke('scan_cleanup_candidates');
  }

  static cancel(): Promise<void> {
    return invoke('cancel_cleanup');
  }

  static async executeWithProgress(
    categories: CleanerCategory[],
    handler: (progress: CleanupProgress) => void
  ): Promise<CleanupResult> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<CleanupProgress>('cleanup-progress', (e) => handler(e.payload));
      return await invoke('execute_cleanup', { request: { categories } });
    } finally {
      unlisten?.();
    }
  }
}

export class PowerService {
  static execute(action: PowerAction): Promise<void> {
    return invoke('execute_power_action', { action });
  }

  static schedule(seconds: number): Promise<void> {
    return invoke('schedule_shutdown', { request: { seconds } });
  }

  static cancelSchedule(): Promise<void> {
    return invoke('cancel_scheduled_shutdown');
  }
}

export class SystemInfoService {
  static load(): Promise<SystemInformation> {
    return invoke('get_system_information');
  }
}

export class HistoryService {
  static list(): Promise<
    Array<{ id: string; domain: string; action: string; success: boolean; detail?: string }>
  > {
    return invoke('list_history');
  }
}
