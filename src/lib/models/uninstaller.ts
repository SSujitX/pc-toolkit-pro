/** Protocol types for the Uninstaller domain (mirror `pctoolkit_core::uninstaller`). */

export type ProgramSource = 'desktop' | 'store';

export type InstallerKind = 'msi' | 'inno' | 'nsis' | 'squirrel' | 'store' | 'other';

export interface InstalledProgram {
  id: string;
  name: string;
  publisher: string;
  version: string;
  installDate: string | null;
  sizeBytes: number;
  installLocation: string;
  source: ProgramSource;
  installerKind: InstallerKind;
  machineWide: boolean;
  quietSupported: boolean;
  isRuntime: boolean;
  registryPath: string | null;
}

/** A desktop program removed by another uninstaller since the last listing. */
export interface RemovedProgram {
  id: string;
  name: string;
  publisher: string;
  version: string;
  keyName: string;
  installLocation: string;
  removedAtMs: number;
}

export interface ProgramInventory {
  programs: InstalledProgram[];
  isAdmin: boolean;
  storeUnavailable: boolean;
  removedElsewhere: RemovedProgram[];
}

/** Minimal program row for a program that is already gone (dialog header, review). */
export function programFromRemoved(removed: RemovedProgram): InstalledProgram {
  return {
    id: removed.id,
    name: removed.name,
    publisher: removed.publisher,
    version: removed.version,
    installDate: null,
    sizeBytes: 0,
    installLocation: removed.installLocation,
    source: 'desktop',
    installerKind: 'other',
    machineWide: removed.id.startsWith('d:lm:'),
    quietSupported: false,
    isRuntime: false,
    registryPath: null,
  };
}

export type UninstallerPhase = 'listing' | 'restorePoint' | 'uninstalling' | 'scanning' | 'cleaning';

export type LeftoverArea =
  | 'folders'
  | 'registry'
  | 'shortcuts'
  | 'startup'
  | 'services'
  | 'tasks'
  | 'firewall'
  | 'traces';

/** Stage of the program listing (`listing` events): uninstall keys → each program → Store apps. */
export type ListingStage = 'registry' | 'desktop' | 'store' | 'done';

export interface UninstallerProgress {
  phase: UninstallerPhase;
  programName: string;
  /** Scanner area while scanning; listing stage while listing. */
  area: LeftoverArea | ListingStage | null;
  current: number;
  total: number;
  itemsFound: number;
  elapsedMs: number;
  /** Path / key being scanned or removed right now. */
  currentPath: string | null;
  /** The vendor uninstaller has a visible window (it may be waiting for the user). */
  windowVisible: boolean;
  /** While cleaning: the item that just finished and its outcome. */
  itemId: string | null;
  itemStatus: LeftoverItemStatus | null;
  /** While listing: icon (PNG data URL) of the program in `programName` / `itemId`. */
  icon: string | null;
}

export interface ProgramIcon {
  id: string;
  dataUrl: string | null;
}

/** Scanner areas in the order Core runs them (for the progress checklist). */
export const LEFTOVER_AREAS: LeftoverArea[] = [
  'folders',
  'registry',
  'shortcuts',
  'startup',
  'services',
  'tasks',
  'firewall',
  'traces',
];

export type LeftoverKind =
  | 'folder'
  | 'file'
  | 'registryKey'
  | 'registryValue'
  | 'shortcut'
  | 'startupEntry'
  | 'service'
  | 'scheduledTask'
  | 'firewallRule';

export type LeftoverConfidence = 'high' | 'medium';

export interface LeftoverItem {
  id: string;
  kind: LeftoverKind;
  location: string;
  detail: string | null;
  bytes: number;
  confidence: LeftoverConfidence;
  requiresAdmin: boolean;
  selected: boolean;
}

export interface LeftoverScan {
  scanId: string;
  programId: string;
  programName: string;
  items: LeftoverItem[];
  totalBytes: number;
  isAdmin: boolean;
}

export type UninstallStatus =
  | 'completed'
  | 'completedRebootRequired'
  | 'stillInstalled'
  | 'cancelled'
  | 'launchDenied'
  | 'failed';

export type RestorePointStatus =
  | 'notRequested'
  | 'created'
  | 'notCreated'
  | 'skippedNeedAdmin'
  | 'failed';

export interface UninstallReport {
  programId: string;
  programName: string;
  status: UninstallStatus;
  exitCode: number | null;
  restorePoint: RestorePointStatus;
  leftovers: LeftoverScan | null;
}

export type LeftoverItemStatus =
  | 'removed'
  | 'pendingReboot'
  | 'skippedNeedAdmin'
  | 'notFound'
  | 'backupFailed'
  | 'failed';

export interface LeftoverCleanResult {
  removedCount: number;
  freedBytes: number;
  pendingRebootCount: number;
  skippedNeedAdminCount: number;
  failedCount: number;
  registryBackupSaved: boolean;
  items: Array<{ id: string; status: LeftoverItemStatus }>;
  /** Set by Core when the clean stopped on cancel (older builds omit it). */
  cancelled?: boolean;
}

export interface UninstallOptions {
  quiet: boolean;
  createRestorePoint: boolean;
  scanLeftovers: boolean;
}

/** Program list filters shown as tabs. */
export type ProgramFilter = 'all' | 'desktop' | 'store' | 'recent' | 'large';

export type ProgramSort = 'name' | 'size' | 'date' | 'publisher';

export const PROGRAM_FILTERS: ProgramFilter[] = ['all', 'desktop', 'store', 'recent', 'large'];

/** "Large programs" threshold (same order of magnitude users expect from uninstaller tools). */
export const LARGE_PROGRAM_BYTES = 500 * 1024 * 1024;

/** "Recently installed" window. */
export const RECENT_PROGRAM_DAYS = 30;

/** Leftover groups shown as tabs in the results view. */
export type LeftoverGroup = 'files' | 'registry' | 'shortcuts' | 'startup' | 'system';

export const LEFTOVER_GROUPS: LeftoverGroup[] = ['files', 'registry', 'shortcuts', 'startup', 'system'];

export function leftoverGroup(kind: LeftoverKind): LeftoverGroup {
  switch (kind) {
    case 'folder':
    case 'file':
      return 'files';
    case 'registryKey':
    case 'registryValue':
      return 'registry';
    case 'shortcut':
      return 'shortcuts';
    case 'startupEntry':
      return 'startup';
    default:
      return 'system';
  }
}

function daysSince(isoDate: string, now: number): number {
  const time = Date.parse(`${isoDate}T00:00:00`);
  return Number.isNaN(time) ? Number.POSITIVE_INFINITY : (now - time) / 86_400_000;
}

export function matchesFilter(program: InstalledProgram, filter: ProgramFilter, now = Date.now()): boolean {
  switch (filter) {
    case 'desktop':
      return program.source === 'desktop';
    case 'store':
      return program.source === 'store';
    case 'recent':
      return program.installDate != null && daysSince(program.installDate, now) <= RECENT_PROGRAM_DAYS;
    case 'large':
      return program.sizeBytes >= LARGE_PROGRAM_BYTES;
    default:
      return true;
  }
}

export function matchesQuery(program: InstalledProgram, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    program.name.toLowerCase().includes(q) ||
    program.publisher.toLowerCase().includes(q) ||
    program.version.toLowerCase().includes(q)
  );
}

export function sortPrograms(programs: InstalledProgram[], sort: ProgramSort): InstalledProgram[] {
  const byName = (a: InstalledProgram, b: InstalledProgram) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
  const copy = [...programs];
  switch (sort) {
    case 'size':
      return copy.sort((a, b) => b.sizeBytes - a.sizeBytes || byName(a, b));
    case 'date':
      return copy.sort(
        (a, b) => (b.installDate ?? '').localeCompare(a.installDate ?? '') || byName(a, b)
      );
    case 'publisher':
      return copy.sort(
        (a, b) =>
          a.publisher.localeCompare(b.publisher, undefined, { sensitivity: 'base' }) || byName(a, b)
      );
    default:
      return copy.sort(byName);
  }
}

/** Two-letter avatar for programs (no icon extraction yet). */
export function programInitials(name: string): string {
  const words = name
    .replace(/[^\p{L}\p{N} ]/gu, ' ')
    .split(/\s+/)
    .filter(Boolean);
  if (!words.length) return '?';
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[1][0]).toUpperCase();
}

/** Stable hue per program for the avatar tint. */
export function programHue(name: string): number {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return hash % 360;
}
