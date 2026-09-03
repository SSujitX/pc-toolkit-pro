export type DeepCleanupGroup = 'system' | 'application' | 'browser' | 'development';

export type DeepCleanupSelectionMode = 'smart' | 'all' | 'none' | 'manual';

export interface DeepCleanupProgress {
  phase: string;
  currentPath: string;
  itemsScanned: number;
  bytesScanned: number;
  elapsedMs: number;
  message: string;
}

export interface DeepCleanupRule {
  id: string;
  group: DeepCleanupGroup;
  nameKey: string;
  detailKey: string;
  risk: 'safe' | 'recoverable' | string;
  bytes: number;
  itemCount: number;
  recommended: boolean;
  selected: boolean;
  status: 'found' | 'clean' | 'notApplicable' | string;
}

export interface DeepCleanupScan {
  rules: DeepCleanupRule[];
  isAdmin: boolean;
}

export interface DeepCleanupResult {
  freedBytes: number;
  filesRemoved: number;
  log: string[];
}

export const DEEP_CLEANUP_GROUPS: DeepCleanupGroup[] = [
  'system',
  'application',
  'browser',
  'development',
];

export function foundRules(rules: DeepCleanupRule[]): DeepCleanupRule[] {
  return rules.filter((r) => r.status === 'found' && r.bytes > 0);
}

export function recommendedIds(rules: DeepCleanupRule[]): string[] {
  return foundRules(rules)
    .filter((r) => r.recommended)
    .map((r) => r.id);
}

export function groupBytes(rules: DeepCleanupRule[], group: DeepCleanupGroup): number {
  return foundRules(rules)
    .filter((r) => r.group === group)
    .reduce((n, r) => n + r.bytes, 0);
}

export function groupCount(rules: DeepCleanupRule[], group: DeepCleanupGroup): number {
  return foundRules(rules).filter((r) => r.group === group).length;
}
