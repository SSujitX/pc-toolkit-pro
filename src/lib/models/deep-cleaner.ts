export type DeepCleanupGroup =
  | 'system'
  | 'application'
  | 'browser'
  | 'development'
  | 'projects'
  | 'ai';

export type DeepCleanupSelectionMode = 'smart' | 'all' | 'none' | 'manual';

export type DeepCleanupRisk = 'safe' | 'recoverable';

/** Scan status from Core (`DeepCleanupRuleStatus`). */
export type DeepCleanupRuleStatus =
  | 'found'
  | 'clean'
  | 'notApplicable'
  | 'limited'
  | 'requiresClose'
  /** Target exists but its size is unknown (needs admin / timed out); see `reason`. */
  | 'unmeasured';

/** Typed reason on a scan result (`DeepCleanupScanReason`). */
export type DeepCleanupScanReason =
  | 'needsAdmin'
  | 'toolUnavailable'
  | 'daemonUnavailable'
  | 'handlerMissing'
  | 'timedOut'
  | 'failed'
  | 'discoveryLimited';

/** Per-rule execution status from Core (`DeepCleanupExecStatus`). */
export type DeepCleanupExecStatus = 'completed' | 'partial' | 'blocked' | 'skipped' | 'failed';

/** Typed reason attached to a non-completed rule (`DeepCleanupExecReason`). */
export type DeepCleanupExecReason =
  | 'runningProcesses'
  | 'needsAdmin'
  | 'itemsBusy'
  | 'itemsFailed'
  | 'cancelled'
  | 'unknownRule'
  | 'notApplicable'
  | 'changedSinceScan'
  | 'toolFailed';

export interface DeepCleanupProgress {
  phase: 'scanning' | 'executing' | 'complete' | string;
  /** Path being read (display only; never logged). */
  currentPath: string;
  itemsScanned: number;
  bytesScanned: number;
  elapsedMs: number;
  /** Locale key of the active rule, or a phase marker. */
  message: string;
  /** Rules finished. */
  current?: number;
  /** Rules in this run. */
  total?: number;
  ruleId?: string | null;
}

export interface DeepCleanupRule {
  id: string;
  group: DeepCleanupGroup;
  nameKey: string;
  detailKey: string;
  risk: DeepCleanupRisk;
  bytes: number;
  itemCount: number;
  recommended: boolean;
  selected: boolean;
  status: DeepCleanupRuleStatus;
  /** Process image names that must be closed before cleaning this rule. */
  relatedProcesses: string[];
  /** Subset of `relatedProcesses` running at scan time. */
  runningProcesses: string[];
  /** Clean needs administrator; Core skips honestly when not elevated. */
  requiresElevation: boolean;
  /** `requiresElevation` and the app is not elevated. */
  needsAdmin: boolean;
  /** The entry safety limit stopped the walk; size is a lower bound. */
  limited: boolean;
  /** Roots refused by the safety policy (diagnostic count). */
  refusedRoots: number;
  /** Literal item name (project folder, model id); replaces `nameKey` when set. */
  displayName?: string;
  /** UI-only location hint such as `~\code\app`. */
  pathHint?: string;
  /** Typed codes: project ecosystems (`node`, `rust`) or the AI model provider. */
  tags?: string[];
  /** What will be removed, relative to the item (`node_modules`) or toolchain names. */
  artifacts?: string[];
  lastModifiedMs?: number;
  /** Project changed in the last 7 days; never preselected. */
  active?: boolean;
  reason?: DeepCleanupScanReason;
}

type Translate = (key: string, named?: Record<string, unknown>) => string;

/** Display title: the literal item name when Core sends one, else the rule's locale key. */
export function ruleTitle(rule: DeepCleanupRule, t: Translate): string {
  return rule.displayName ?? t(rule.nameKey);
}

/** Locale key for a tag of this rule (ecosystem or AI provider). */
export function tagLabelKey(rule: DeepCleanupRule, tag: string): string {
  return rule.group === 'ai'
    ? `deepCleaner.ai.providers.${tag}`
    : `deepCleaner.projects.ecosystems.${tag}`;
}

/** Locale key of an exec reason (newer reasons live under `deepCleaner.special`). */
export function execReasonKey(reason: DeepCleanupExecReason): string {
  return reason === 'changedSinceScan' || reason === 'toolFailed'
    ? `deepCleaner.special.execReason.${reason}`
    : `deepCleaner.execReason.${reason}`;
}

/** Secondary line for item-level results: location hint and removed folders. */
export function ruleDetail(rule: DeepCleanupRule, t: Translate): string {
  if (!rule.displayName && !rule.artifacts?.length) return t(rule.detailKey);
  const parts: string[] = [];
  if (rule.pathHint) parts.push(rule.pathHint);
  if (rule.artifacts?.length) parts.push(rule.artifacts.join(', '));
  return parts.length ? parts.join(' · ') : t(rule.detailKey);
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
  /** Locale key of the first rule's name. */
  name: string;
  /** Literal name when the first rule is an item (AI model); preferred over `name`. */
  label?: string;
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
        label: rule.displayName,
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
  /** Groups that can hold rules on this build, in display order. */
  groups: DeepCleanupGroup[];
  isAdmin: boolean;
}

export interface DeepCleanupRuleExecResult {
  id: string;
  status: DeepCleanupExecStatus;
  reason: DeepCleanupExecReason | null;
  freedBytes: number;
  removedCount: number;
  /** Items not removed (includes `busyCount`). */
  failedCount: number;
  /** Items held open by another app. */
  busyCount: number;
}

export interface DeepCleanupResult {
  freedBytes: number;
  filesRemoved: number;
  failedCount: number;
  busyCount: number;
  cancelled: boolean;
  rules: DeepCleanupRuleExecResult[];
  /** Diagnostic lines (rule ids + counts only). */
  log: string[];
}

/** Fallback display order when a scan has not reported its groups yet. */
export const DEEP_CLEANUP_GROUPS: DeepCleanupGroup[] = [
  'system',
  'application',
  'browser',
  'development',
  'projects',
  'ai',
];

const RECLAIMABLE_STATUSES: ReadonlySet<DeepCleanupRuleStatus> = new Set([
  'found',
  'limited',
  'requiresClose',
]);

/** Shown and selectable: measured data, or an existing target whose size is unknown. */
export function isReclaimable(rule: DeepCleanupRule): boolean {
  if (rule.status === 'unmeasured') return true;
  return RECLAIMABLE_STATUSES.has(rule.status) && rule.bytes > 0;
}

/** `all` selection still leaves active projects alone. */
export function selectableByAll(rule: DeepCleanupRule): boolean {
  return isReclaimable(rule) && !rule.active;
}

export function foundRules(rules: DeepCleanupRule[]): DeepCleanupRule[] {
  return rules.filter(isReclaimable);
}

export function recommendedIds(rules: DeepCleanupRule[]): string[] {
  return foundRules(rules)
    .filter(
      (r) => r.recommended && !r.needsAdmin && !r.active && r.status !== 'unmeasured' && r.bytes > 0
    )
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

export interface DeepCleanupResultSummary {
  completed: number;
  partial: number;
  blocked: number;
  skipped: number;
  failed: number;
}

export function summarizeExecResults(
  results: DeepCleanupRuleExecResult[]
): DeepCleanupResultSummary {
  const summary: DeepCleanupResultSummary = {
    completed: 0,
    partial: 0,
    blocked: 0,
    skipped: 0,
    failed: 0,
  };
  for (const result of results) summary[result.status] += 1;
  return summary;
}

/** A fixed drive in the Deep Cleanup scope picker (extra drives are searched for projects). */
export interface DeepCleanupDrive {
  letter: string;
  label: string | null;
  totalBytes: number;
  freeBytes: number;
  /** The Windows drive: always searched, not toggleable. */
  system: boolean;
  selected: boolean;
}
