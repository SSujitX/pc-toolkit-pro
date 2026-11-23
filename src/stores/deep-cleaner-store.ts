import { defineStore } from 'pinia';
import { DeepCleanerService } from '@/lib/services/api-services';
import {
  DEEP_CLEANUP_GROUPS,
  foundRules,
  isReclaimable,
  recommendedIds,
  selectableByAll,
  type DeepCleanupGroup,
  type DeepCleanupProgress,
  type DeepCleanupRuleExecResult,
  type DeepCleanupResult,
  type DeepCleanupRule,
  type DeepCleanupSelectionMode,
} from '@/lib/models/deep-cleaner';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

export const useDeepCleanerStore = defineStore('deepCleaner', {
  state: () => ({
    rules: [] as DeepCleanupRule[],
    groups: [...DEEP_CLEANUP_GROUPS] as DeepCleanupGroup[],
    isAdmin: false,
    loading: false,
    /** Epoch ms when the running scan / clean started (drives the elapsed clock). */
    busySince: null as number | null,
    progress: null as DeepCleanupProgress | null,
    result: null as DeepCleanupResult | null,
    log: [] as string[],
    selectionMode: 'smart' as DeepCleanupSelectionMode,
    hasScanned: false,
  }),
  getters: {
    visibleRules(state): DeepCleanupRule[] {
      return foundRules(state.rules);
    },
    selectedRules(state): DeepCleanupRule[] {
      return foundRules(state.rules).filter((r) => r.selected);
    },
    selectedBytes(): number {
      return this.selectedRules.reduce((n: number, r: DeepCleanupRule) => n + r.bytes, 0);
    },
    foundBytes(): number {
      return this.visibleRules.reduce((n: number, r: DeepCleanupRule) => n + r.bytes, 0);
    },
    resultById(state): Record<string, DeepCleanupRuleExecResult> {
      const map: Record<string, DeepCleanupRuleExecResult> = {};
      for (const item of state.result?.rules ?? []) map[item.id] = item;
      return map;
    },
  },
  actions: {
    async scan() {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.deepCleaner, true);
      this.loading = true;
      this.busySince = Date.now();
      this.result = null;
      this.progress = null;
      this.log = [];
      try {
        const scan = await DeepCleanerService.scanWithProgress((p) => {
          this.progress = p;
        });
        this.rules = scan.rules;
        this.groups = scan.groups.length ? scan.groups : [...DEEP_CLEANUP_GROUPS];
        this.isAdmin = scan.isAdmin;
        this.hasScanned = true;
        this.selectionMode = 'smart';
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.busySince = null;
        this.progress = null;
        app.setBusy(PAGE_IDS.deepCleaner, false);
      }
    },
    // Selection mutates `selected` in place: rebuilding the rule array on every checkbox
    // invalidated every rule-derived computed and re-rendered the whole list.
    setRuleSelected(id: string, selected: boolean) {
      const rule = this.rules.find((r) => r.id === id);
      if (rule) rule.selected = selected;
      this.selectionMode = 'manual';
    },
    setGroupSelected(group: string, selected: boolean) {
      for (const rule of this.rules) {
        if (rule.group === group && isReclaimable(rule) && rule.selected !== selected) {
          rule.selected = selected;
        }
      }
      this.selectionMode = 'manual';
    },
    applySelectionMode(mode: DeepCleanupSelectionMode) {
      if (mode === 'manual') {
        this.selectionMode = 'manual';
        return;
      }
      const recommended = new Set(recommendedIds(this.rules));
      this.rules = this.rules.map((rule) => {
        if (!isReclaimable(rule)) {
          return { ...rule, selected: false };
        }
        if (mode === 'all') return { ...rule, selected: selectableByAll(rule) };
        if (mode === 'none') return { ...rule, selected: false };
        return { ...rule, selected: recommended.has(rule.id) };
      });
      this.selectionMode = mode;
    },
    async execute() {
      const app = useAppStore();
      const ruleIds = this.selectedRules.map((r) => r.id);
      if (!ruleIds.length) return;
      app.setBusy(PAGE_IDS.deepCleaner, true);
      this.loading = true;
      this.busySince = Date.now();
      this.progress = null;
      this.result = null;
      try {
        this.result = await DeepCleanerService.executeWithProgress(ruleIds, (p) => {
          this.progress = p;
        });
        this.log = this.result.log;
        // Reflect what was just removed so the list does not keep offering stale sizes/selection.
        const outcomes = new Map(this.result.rules.map((r) => [r.id, r] as const));
        this.rules = this.rules.map((rule) => {
          const outcome = outcomes.get(rule.id);
          if (!outcome) return rule;
          if (outcome.status === 'completed') {
            return { ...rule, selected: false, status: 'clean', bytes: 0, itemCount: 0 };
          }
          return {
            ...rule,
            selected: false,
            bytes: Math.max(0, rule.bytes - outcome.freedBytes),
            itemCount: Math.max(0, rule.itemCount - outcome.removedCount),
          };
        });
        this.selectionMode = 'manual';
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.busySince = null;
        this.progress = null;
        app.setBusy(PAGE_IDS.deepCleaner, false);
      }
    },
    dismissResult() {
      this.result = null;
      this.log = [];
    },
    async cancel() {
      try {
        await DeepCleanerService.cancel();
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
  },
});
