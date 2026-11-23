/** Care protocol (mirrors `pctoolkit_core::care` + `care::engine`). */

export const CARE_CATEGORY_IDS = [
  'privacy',
  'junk',
  'shortcuts',
  'registry',
  'startup',
  'security',
  'protection',
  'windowsUpdate',
  'software',
  'diskHealth',
  'diskOptimize',
] as const;

export type CareCategoryId = (typeof CARE_CATEGORY_IDS)[number];

export type CareCategoryState =
  | 'clean'
  | 'issues'
  | 'attention'
  | 'notApplicable'
  | 'skipped'
  | 'failed';

export type CareRisk = 'safe' | 'review' | 'info';

export interface CareItem {
  id: string;
  category: CareCategoryId;
  titleKey: string | null;
  title: string | null;
  params: Record<string, string>;
  detailKey: string | null;
  detail: string | null;
  /** Display-only location hint. Never log it. */
  location: string | null;
  bytes: number | null;
  count: number;
  risk: CareRisk;
  selected: boolean;
  fixable: boolean;
  needsAdmin: boolean;
  action: string | null;
}

export interface CareCategoryScan {
  category: CareCategoryId;
  state: CareCategoryState;
  items: CareItem[];
  reason: string | null;
  elapsedMs: number;
}

export interface CareScanReport {
  scanId: string;
  categories: CareCategoryScan[];
  isAdmin: boolean;
  stopped: boolean;
  startedAtMs: number;
  elapsedMs: number;
}

export type CarePhase = 'scanning' | 'fixing' | 'complete';
export type CareRunStatus = 'queued' | 'running' | 'done';

export interface CareCategoryProgress {
  category: CareCategoryId;
  status: CareRunStatus;
  state: CareCategoryState | null;
  count: number;
  elapsedMs: number;
}

export interface CareProgress {
  phase: CarePhase;
  percent: number;
  categories: CareCategoryProgress[];
  currentCategory: CareCategoryId | null;
  /** Display-only path / item text. */
  currentText: string;
  elapsedMs: number;
}

export type CareAfterAction = 'none' | 'shutdown' | 'restart' | 'sleep' | 'hibernate';

export const CARE_AFTER_ACTIONS: CareAfterAction[] = ['none', 'shutdown', 'restart', 'sleep'];

export interface CareFixSelection {
  category: CareCategoryId;
  itemIds: string[];
}

export interface CareFixRequest {
  items: CareFixSelection[];
  auto: boolean;
  afterAction: CareAfterAction;
}

export type CareFixStatus = 'fixed' | 'partial' | 'skipped' | 'failed';

export interface CareFixResult {
  itemId: string;
  category: CareCategoryId;
  status: CareFixStatus;
  reason: string | null;
  freedBytes: number;
  undoable: boolean;
}

export interface CareFixReport {
  results: CareFixResult[];
  fixed: number;
  partial: number;
  failed: number;
  skipped: number;
  freedBytes: number;
  undoable: number;
  stopped: boolean;
  elapsedMs: number;
  afterAction: CareAfterAction;
  afterActionError: string | null;
  /** Seconds until a scheduled shutdown / restart (cancellable). */
  afterActionDelaySecs: number | null;
}

/** Item actions that open a Windows page (mirrors Rust `CARE_ACTIONS`); others are fix hints. */
export const CARE_OPEN_ACTIONS = [
  'openWindowsUpdate',
  'openWindowsSecurity',
  'openVirusProtection',
  'openFirewall',
  'openStartupApps',
  'openStorage',
  'openAppsFeatures',
  'openPrivacy',
  'openRecovery',
  'openBackup',
] as const;

export function isOpenAction(action: string | null | undefined): boolean {
  return !!action && (CARE_OPEN_ACTIONS as readonly string[]).includes(action);
}

export interface CareUndoRecord {
  id: string;
  category: CareCategoryId;
  itemId: string;
  createdAtMs: number;
  needsAdmin: boolean;
}

export interface CareUndoOutcome {
  recordId: string;
  status: 'restored' | 'skipped' | 'failed';
  reason: string | null;
}

/** Engine-level typed reasons that have their own locale text (`care.engine.reasons.*`). */
export const CARE_KNOWN_REASONS = [
  'skipped',
  'stopped',
  'busy',
  'needsAdmin',
  'failed',
  'panicked',
  'notImplemented',
  'toolUnavailable',
  'unknownItem',
  'notFixable',
  'noResult',
  'cancelled',
  'runningProcesses',
  'itemsBusy',
  'itemsFailed',
  'itemChanged',
  'notApplicable',
] as const;

/** Item key that stays unique across categories. */
export function careItemKey(item: Pick<CareItem, 'category' | 'id'>): string {
  return `${item.category}:${item.id}`;
}

/** Badge count of a category (underlying entries of fixable items, else item rows). */
export function careIssueCount(scan: CareCategoryScan): number {
  const fixable = scan.items.filter((i) => i.fixable).reduce((sum, i) => sum + Math.max(1, i.count), 0);
  return fixable > 0 ? fixable : scan.items.length;
}

export function careBytes(items: CareItem[]): number {
  return items.reduce((sum, i) => sum + (i.bytes ?? 0), 0);
}

export type CareSelectionMode = 'smart' | 'all' | 'none' | 'manual';

/**
 * Smart recommendation: only items that cannot hurt — marked Safe (rebuildable caches, broken
 * shortcuts, settings with an undo record), fixable by this process, and not blocked by a running
 * app. Review / Info items (history, cookies, startup apps, registry removals, updates, disk
 * repairs) are never smart-picked.
 */
export function isSmartPick(item: CareItem, isAdmin: boolean): boolean {
  return (
    item.fixable &&
    item.risk === 'safe' &&
    (!item.needsAdmin || isAdmin) &&
    item.params?.running !== 'true'
  );
}

/** Everything this process can fix (includes Review items). */
export function isSelectableNow(item: CareItem, isAdmin: boolean): boolean {
  return item.fixable && (!item.needsAdmin || isAdmin);
}
