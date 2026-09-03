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
  /** Process image names associated with this rule (may need closing before clean). */
  relatedProcesses?: string[];
  /** Clean typically needs administrator; Core skips honestly when not elevated. */
  requiresElevation?: boolean;
}

export interface RunningProcessGroup {
  imageName: string;
  processCount: number;
}

export interface ProcessCloseTargetResult {
  imageName: string;
  status: string;
  remainingCount: number;
}

export interface ProcessCloseBatchResult {
  targets: ProcessCloseTargetResult[];
}

export interface DeepCleanupCloseAppGroup {
  id: string;
  name: string;
  processes: string[];
  processCount: number;
  ruleIds: string[];
}

/** Group selected rules that share running processes for the confirm dialog. */
export function closeAppGroupsForRules(
  rules: DeepCleanupRule[],
  running: RunningProcessGroup[]
): DeepCleanupCloseAppGroup[] {
  const runningMap = new Map(
    running.map((item) => [item.imageName.toLowerCase(), item.processCount] as const)
  );
  const groups: DeepCleanupCloseAppGroup[] = [];

  for (const rule of rules) {
    const processes = (rule.relatedProcesses ?? []).filter((name) =>
      runningMap.has(name.toLowerCase())
    );
    if (!processes.length) continue;

    const normalized = new Set(processes.map((p) => p.toLowerCase()));
    const overlap = groups.find((group) =>
      group.processes.some((p) => normalized.has(p.toLowerCase()))
    );
    if (!overlap) {
      groups.push({
        id: rule.id,
        name: rule.nameKey,
        processes: [...new Set(processes)],
        processCount: processes.reduce(
          (n, name) => n + (runningMap.get(name.toLowerCase()) ?? 0),
          0
        ),
        ruleIds: [rule.id],
      });
      continue;
    }
    overlap.processes = [...new Set([...overlap.processes, ...processes])];
    overlap.ruleIds = [...new Set([...overlap.ruleIds, rule.id])];
    overlap.processCount = overlap.processes.reduce(
      (n, name) => n + (runningMap.get(name.toLowerCase()) ?? 0),
      0
    );
  }

  return groups.map((group) => ({
    ...group,
    id: [...group.ruleIds].sort().join(':'),
  }));
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
