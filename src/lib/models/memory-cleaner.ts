export type MemoryAreaId =
  | 'combinedPageList'
  | 'modifiedFileCache'
  | 'modifiedPageList'
  | 'registryCache'
  | 'standbyList'
  | 'standbyListLowPriority'
  | 'systemFileCache'
  | 'workingSet';

export const MEMORY_AREA_IDS: MemoryAreaId[] = [
  'workingSet',
  'systemFileCache',
  'modifiedPageList',
  'standbyList',
  'standbyListLowPriority',
  'combinedPageList',
  'registryCache',
  'modifiedFileCache',
];

/** Discrete auto-interval steps (minutes). 0 = off. */
export const AUTO_INTERVAL_STEPS_MINUTES = [
  0, 5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360, 720, 1440,
] as const;

export type OptimizeReason = 'manual' | 'schedule' | 'lowMemory' | 'tray';

export type AreaStatus = 'ok' | 'skipped' | 'failed';

export interface MemoryStats {
  physicalTotal: number;
  physicalAvail: number;
  physicalUsed: number;
  physicalLoadPercent: number;
  virtualTotal: number;
  virtualAvail: number;
  virtualUsed: number;
  virtualLoadPercent: number;
}

export interface MemoryCleanerSettings {
  areas: Record<string, boolean>;
  autoIntervalMinutes: number;
  autoFreeBelowPercent: number;
}

export interface AreaOutcome {
  id: MemoryAreaId;
  status: AreaStatus;
  detail?: string | null;
}

export interface MemoryOptimizeResult {
  freeBefore: number;
  freeAfter: number;
  freedBytes: number;
  adminOptimizations: boolean;
  areas: AreaOutcome[];
}

export interface MemoryProgress {
  phase: string;
  current: number;
  total: number;
  message: string;
  area?: MemoryAreaId | null;
}

export function defaultMemoryAreas(): Record<string, boolean> {
  const areas: Record<string, boolean> = {};
  for (const id of MEMORY_AREA_IDS) {
    areas[id] = id !== 'standbyListLowPriority';
  }
  return areas;
}

export function formatIntervalLabel(minutes: number): string {
  if (minutes <= 0) return 'Off';
  if (minutes < 60) return `Every ${minutes} min`;
  const hours = minutes / 60;
  if (Number.isInteger(hours)) return `Every ${hours}h`;
  return `Every ${hours.toFixed(1)}h`;
}

export function freePhysicalPercent(stats: MemoryStats | null | undefined): number {
  if (!stats || stats.physicalTotal <= 0) return 0;
  return (stats.physicalAvail / stats.physicalTotal) * 100;
}
