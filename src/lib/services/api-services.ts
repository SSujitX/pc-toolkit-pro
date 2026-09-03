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
  riskKey: string;
  detailKey: string;
  itemCount: number;
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
  memory?: import('@/lib/models/memory-cleaner').MemoryOptimizeResult | null;
}

export interface SystemInformation {
  uptime: string;
  cpuName: string;
  cpuCores: number;
  cpuThreads: number;
  cpuUsage: number;
  cpuFrequency: string;
  cpuCache: string;
  cpuSocket: string;
  memoryTotal: number;
  memoryUsed: number;
  memoryAvailable: number;
  memoryPercent: number;
  ramName: string;
  ramType: string;
  ramSpeed: string;
  ramSlotsUsed: string;
  diskTotal: number;
  diskUsed: number;
  diskFree: number;
  diskPercent: number;
  diskDevice: string;
  diskType: string;
  gpuName: string;
  gpuUsage?: number | null;
  gpuMemoryUsed?: number | null;
  gpuMemoryTotal?: number | null;
  gpuTemperature?: number | null;
  motherboardProduct: string;
  motherboardManufacturer: string;
  motherboardVersion: string;
  motherboard: string;
  chipset: string;
  bios: string;
  biosVersion: string;
  biosManufacturer: string;
  biosDate: string;
  systemModel: string;
  memorySlotsTotal: string;
  maxMemoryCapacity: string;
  osEdition: string;
  osVersion: string;
  osBuild: string;
  osExperience: string;
  hostname: string;
  username: string;
  monitors: string[];
  storageDevices: string[];
  powerSupplyName: string;
  powerPlan: string;
  powerSupplies: string[];
  batteries: string[];
  acLineStatus: string;
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

  static async scanWithProgress(
    handler: (progress: CleanupProgress) => void
  ): Promise<CleanupScan> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<CleanupProgress>('cleanup-progress', (e) => handler(e.payload));
      return await invoke('scan_cleanup_candidates');
    } finally {
      unlisten?.();
    }
  }

  static cancel(): Promise<void> {
    return invoke('cancel_cleanup');
  }

  static async executeWithProgress(
    categories: CleanerCategory[],
    handler: (progress: CleanupProgress) => void,
    source: 'cleaner' | 'deepCleaner' | 'memoryCleaner' = 'cleaner'
  ): Promise<CleanupResult> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<CleanupProgress>('cleanup-progress', (e) => handler(e.payload));
      return await invoke('execute_cleanup', { request: { categories, source } });
    } finally {
      unlisten?.();
    }
  }
}

export class DeepCleanerService {
  static async scanWithProgress(
    handler: (progress: import('@/lib/models/deep-cleaner').DeepCleanupProgress) => void
  ): Promise<import('@/lib/models/deep-cleaner').DeepCleanupScan> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<import('@/lib/models/deep-cleaner').DeepCleanupProgress>(
        'deep-cleanup-progress',
        (e) => handler(e.payload)
      );
      return await invoke('scan_deep_cleanup');
    } finally {
      unlisten?.();
    }
  }

  static async executeWithProgress(
    ruleIds: string[],
    handler: (progress: import('@/lib/models/deep-cleaner').DeepCleanupProgress) => void
  ): Promise<import('@/lib/models/deep-cleaner').DeepCleanupResult> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<import('@/lib/models/deep-cleaner').DeepCleanupProgress>(
        'deep-cleanup-progress',
        (e) => handler(e.payload)
      );
      return await invoke('execute_deep_cleanup_command', { request: { ruleIds } });
    } finally {
      unlisten?.();
    }
  }

  static cancel(): Promise<void> {
    return invoke('cancel_cleanup');
  }

  static probeRunningProcesses(
    names: string[]
  ): Promise<import('@/lib/models/deep-cleaner').RunningProcessGroup[]> {
    return invoke('probe_running_processes_command', { request: { names } });
  }

  static closeRunningProcesses(
    names: string[],
    force: boolean
  ): Promise<import('@/lib/models/deep-cleaner').ProcessCloseBatchResult> {
    return invoke('close_running_processes_command', { request: { names, force } });
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

export interface SystemInfoProgress {
  phase: string;
  current: number;
  total: number;
  message: string;
}

export class SystemInfoService {
  static load(): Promise<SystemInformation> {
    return invoke('get_system_information');
  }

  static async loadWithProgress(
    handler: (progress: SystemInfoProgress) => void
  ): Promise<SystemInformation> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<SystemInfoProgress>('system-info-progress', (e) =>
        handler(e.payload)
      );
      return await invoke('get_system_information');
    } finally {
      unlisten?.();
    }
  }
}

export class HistoryService {
  static list(): Promise<import('@/lib/models/history').HistoryRecord[]> {
    return invoke('list_history');
  }

  static clear(): Promise<void> {
    return invoke('clear_history');
  }
}

export class MemoryCleanerService {
  static stats(): Promise<import('@/lib/models/memory-cleaner').MemoryStats> {
    return invoke('get_memory_stats');
  }

  static getSettings(): Promise<import('@/lib/models/memory-cleaner').MemoryCleanerSettings> {
    return invoke('get_memory_cleaner_settings');
  }

  static setSettings(
    settings: import('@/lib/models/memory-cleaner').MemoryCleanerSettings
  ): Promise<import('@/lib/models/memory-cleaner').MemoryCleanerSettings> {
    return invoke('set_memory_cleaner_settings', { settings });
  }

  static async optimizeWithProgress(
    reason: import('@/lib/models/memory-cleaner').OptimizeReason,
    handler: (progress: import('@/lib/models/memory-cleaner').MemoryProgress) => void,
    areas?: import('@/lib/models/memory-cleaner').MemoryAreaId[]
  ): Promise<import('@/lib/models/memory-cleaner').MemoryOptimizeResult> {
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<import('@/lib/models/memory-cleaner').MemoryProgress>(
        'memory-progress',
        (e) => handler(e.payload)
      );
      return await invoke('optimize_memory', {
        request: { reason, areas: areas ?? null },
      });
    } finally {
      unlisten?.();
    }
  }

  static cancel(): Promise<void> {
    return invoke('cancel_memory_optimize');
  }

  static getElevationStatus(): Promise<{ elevated: boolean }> {
    return invoke('get_elevation_status');
  }

  static restartAsAdministrator(): Promise<void> {
    return invoke('restart_as_administrator');
  }
}

export class SettingsApi {
  static openAppDataFolder(): Promise<void> {
    return invoke('open_app_data_folder');
  }
}
