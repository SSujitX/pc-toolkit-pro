/** Startup Manager protocol (mirrors `pctoolkit_core::startup`). */

export type StartupSource = 'run' | 'runOnce' | 'startupFolder' | 'scheduledTask' | 'service' | 'packagedApp';
export type StartupScope = 'user' | 'machine';
export type StartupState = 'enabled' | 'disabled' | 'delayed';
export type StartupImpact = 'high' | 'medium' | 'low' | 'notFlagged' | 'unknown';
export type ServiceStartKind = 'automatic' | 'automaticDelayed' | 'manual' | 'disabled' | 'other';
export type BootPerfAccess = 'available' | 'needsAdmin' | 'unavailable';
export type StartupActionKind = 'enable' | 'disable' | 'delay' | 'undelay' | 'delete';
export type StartupActionStatus = 'done' | 'skippedNeedAdmin' | 'notFound' | 'unsupported' | 'failed';

export interface StartupItem {
  id: string;
  name: string;
  entryName: string;
  publisher: string | null;
  description: string | null;
  /** Display only. */
  command: string;
  /** Display only. */
  targetPath: string | null;
  source: StartupSource;
  scope: StartupScope;
  state: StartupState;
  serviceStart: ServiceStartKind | null;
  isMicrosoft: boolean;
  requiresAdmin: boolean;
  canToggle: boolean;
  canDelay: boolean;
  canDelete: boolean;
  broken: boolean;
  isNew: boolean;
  impact: StartupImpact;
  impactMs: number | null;
  disabledAtMs: number | null;
  delaySeconds: number | null;
}

export interface BootSummary {
  access: BootPerfAccess;
  lastBootMs: number | null;
  lastBootAtMs: number | null;
  bootsRecorded: number;
}

export interface StartupInventory {
  items: StartupItem[];
  isAdmin: boolean;
  boot: BootSummary;
  hiddenMicrosoftCount: number;
}

export interface StartupActionResult {
  id: string;
  action: StartupActionKind;
  status: StartupActionStatus;
  reason: string | null;
  item: StartupItem | null;
}

export interface StartupIcon {
  id: string;
  dataUrl: string | null;
}

export const STARTUP_FILTERS = ['all', 'enabled', 'disabled', 'new'] as const;
export type StartupFilter = (typeof STARTUP_FILTERS)[number];

/** A packaged (Store) app startup task Windows reports as controlled by group policy. */
export function isPolicyLocked(item: StartupItem): boolean {
  return item.source === 'packagedApp' && !item.canToggle;
}

/** "Delayed" still starts with Windows, so it counts as enabled for filters and counts. */
export function startsWithWindows(item: StartupItem): boolean {
  return item.state !== 'disabled';
}

export function matchesStartupFilter(item: StartupItem, filter: StartupFilter): boolean {
  switch (filter) {
    case 'enabled':
      return startsWithWindows(item);
    case 'disabled':
      return item.state === 'disabled';
    case 'new':
      return item.isNew;
    default:
      return true;
  }
}

export function matchesStartupQuery(item: StartupItem, query: string, sourceLabel = ''): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [item.name, item.publisher ?? '', item.entryName, item.description ?? '', sourceLabel].some((v) =>
    v.toLowerCase().includes(q)
  );
}

const IMPACT_ORDER: Record<StartupImpact, number> = { high: 0, medium: 1, low: 2, notFlagged: 3, unknown: 4 };

/** New first, then enabled, then by boot impact, then by name. */
export function sortStartupItems(items: StartupItem[]): StartupItem[] {
  return [...items].sort(
    (a, b) =>
      Number(b.isNew) - Number(a.isNew) ||
      Number(startsWithWindows(b)) - Number(startsWithWindows(a)) ||
      IMPACT_ORDER[a.impact] - IMPACT_ORDER[b.impact] ||
      a.name.localeCompare(b.name)
  );
}
