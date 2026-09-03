import { defineStore } from 'pinia';
import { DeepCleanerService } from '@/lib/services/api-services';
import {
  foundRules,
  recommendedIds,
  type DeepCleanupProgress,
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
    isAdmin: false,
    loading: false,
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
  },
  actions: {
    async scan() {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.deepCleaner, true);
      this.loading = true;
      this.result = null;
      this.progress = null;
      this.log = [];
      try {
        const scan = await DeepCleanerService.scanWithProgress((p) => {
          this.progress = p;
        });
        this.rules = scan.rules;
        this.isAdmin = scan.isAdmin;
        this.hasScanned = true;
        this.selectionMode = 'smart';
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.deepCleaner, false);
      }
    },
    setRuleSelected(id: string, selected: boolean) {
      this.rules = this.rules.map((rule) =>
        rule.id === id ? { ...rule, selected } : rule
      );
      this.selectionMode = 'manual';
    },
    setGroupSelected(group: string, selected: boolean) {
      this.rules = this.rules.map((rule) =>
        rule.group === group && rule.status === 'found' && rule.bytes > 0
          ? { ...rule, selected }
          : rule
      );
      this.selectionMode = 'manual';
    },
    applySelectionMode(mode: DeepCleanupSelectionMode) {
      if (mode === 'manual') {
        this.selectionMode = 'manual';
        return;
      }
      const recommended = new Set(recommendedIds(this.rules));
      this.rules = this.rules.map((rule) => {
        if (rule.status !== 'found' || rule.bytes <= 0) {
          return { ...rule, selected: false };
        }
        if (mode === 'all') return { ...rule, selected: true };
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
      this.progress = null;
      try {
        this.result = await DeepCleanerService.executeWithProgress(ruleIds, (p) => {
          this.progress = p;
        });
        this.log = this.result.log;
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.deepCleaner, false);
      }
    },
    async cancel() {
      await DeepCleanerService.cancel();
    },
  },
});
