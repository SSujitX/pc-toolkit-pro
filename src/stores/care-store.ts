import { defineStore } from 'pinia';
import { CareService } from '@/lib/services/api-services';
import {
  CARE_CATEGORY_IDS,
  careBytes,
  careIssueCount,
  careItemKey,
  type CareAfterAction,
  type CareCategoryId,
  type CareCategoryScan,
  type CareFixReport,
  type CareItem,
  type CareProgress,
  type CareScanReport,
  isSelectableNow,
  isSmartPick,
  type CareSelectionMode,
} from '@/lib/models/care';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

export type CareView = 'home' | 'scanning' | 'results' | 'fixing' | 'summary';

/** Compact summary of the previous scan (per-viewer convenience, counts only). */
export interface CareLastSummary {
  atMs: number;
  issues: number;
  bytes: number;
  categories: number;
}

const PREFS_KEY = 'pt-care-preferences';
const LAST_KEY = 'pt-care-last-scan';

interface CarePreferences {
  categories: CareCategoryId[];
  autoFix: boolean;
  afterAction: CareAfterAction;
}

function readJson<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

function writeJson(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage can be unavailable (private mode); preferences are a convenience only.
  }
}

function isCategoryId(value: unknown): value is CareCategoryId {
  return typeof value === 'string' && (CARE_CATEGORY_IDS as readonly string[]).includes(value);
}

function loadPreferences(): CarePreferences {
  const saved = readJson<Partial<CarePreferences>>(PREFS_KEY);
  const categories = Array.isArray(saved?.categories) ? saved.categories.filter(isCategoryId) : null;
  const after = saved?.afterAction;
  return {
    categories: categories && categories.length ? categories : [...CARE_CATEGORY_IDS],
    autoFix: saved?.autoFix === true,
    afterAction:
      after === 'shutdown' || after === 'restart' || after === 'sleep' || after === 'hibernate'
        ? after
        : 'none',
  };
}

function selectionFor(report: CareScanReport, mode: CareSelectionMode): Record<string, boolean> {
  const map: Record<string, boolean> = {};
  for (const cat of report.categories) {
    for (const item of cat.items) {
      if (!item.fixable) continue;
      map[careItemKey(item)] =
        mode === 'smart'
          ? isSmartPick(item, report.isAdmin)
          : mode === 'all'
            ? isSelectableNow(item, report.isAdmin)
            : false;
    }
  }
  return map;
}

export const useCareStore = defineStore('care', {
  state: () => {
    const prefs = loadPreferences();
    return {
      view: 'home' as CareView,
      selectedCategories: prefs.categories,
      autoFix: prefs.autoFix,
      afterAction: prefs.afterAction,
      progress: null as CareProgress | null,
      report: null as CareScanReport | null,
      /** careItemKey → checked. */
      selection: {} as Record<string, boolean>,
      selectionMode: 'smart' as CareSelectionMode,
      fixReport: null as CareFixReport | null,
      /** Wall-clock start of the last fix; undo records created after it belong to that fix. */
      fixStartedAtMs: 0,
      /** Seconds left before a scheduled Auto fix shutdown / restart (null = none pending). */
      afterActionCountdown: null as number | null,
      afterActionTimer: null as number | null,
      /** Outcome of "Undo these changes" for the summary line. */
      undoOutcome: null as { restored: number; skipped: number } | null,
      lastSummary: readJson<CareLastSummary>(LAST_KEY),
      stopping: false,
      /** Categories the user pressed Skip on during the current operation. */
      skipRequested: [] as CareCategoryId[],
    };
  },
  getters: {
    busy(state): boolean {
      return state.view === 'scanning' || state.view === 'fixing';
    },
    allCategoriesSelected(state): boolean {
      return state.selectedCategories.length === CARE_CATEGORY_IDS.length;
    },
    scans(state): CareCategoryScan[] {
      return state.report?.categories ?? [];
    },
    totalIssues(): number {
      return this.scans
        .filter((c: CareCategoryScan) => c.state === 'issues' || c.state === 'attention')
        .reduce((n: number, c: CareCategoryScan) => n + careIssueCount(c), 0);
    },
    totalBytes(): number {
      return this.scans.reduce((n: number, c: CareCategoryScan) => n + careBytes(c.items), 0);
    },
    selectedItems(state): CareItem[] {
      return this.scans.flatMap((c: CareCategoryScan) =>
        c.items.filter((i) => i.fixable && state.selection[careItemKey(i)])
      );
    },
    selectedBytes(): number {
      return careBytes(this.selectedItems);
    },
    fixableItems(): CareItem[] {
      return this.scans.flatMap((c: CareCategoryScan) => c.items.filter((i) => i.fixable));
    },
    /** First running category in display order (target of Skip). */
    runningCategory(state): CareCategoryId | null {
      return state.progress?.categories.find((c) => c.status === 'running')?.category ?? null;
    },
  },
  actions: {
    persistPreferences() {
      writeJson(PREFS_KEY, {
        categories: this.selectedCategories,
        autoFix: this.autoFix,
        afterAction: this.afterAction,
      } satisfies CarePreferences);
    },
    toggleCategory(id: CareCategoryId) {
      const set = new Set(this.selectedCategories);
      if (set.has(id)) set.delete(id);
      else set.add(id);
      this.selectedCategories = CARE_CATEGORY_IDS.filter((c) => set.has(c));
      this.persistPreferences();
    },
    setAllCategories(selected: boolean) {
      this.selectedCategories = selected ? [...CARE_CATEGORY_IDS] : [];
      this.persistPreferences();
    },
    setAutoFix(value: boolean) {
      this.autoFix = value;
      this.persistPreferences();
    },
    setAfterAction(value: CareAfterAction) {
      this.afterAction = value;
      this.persistPreferences();
    },
    async scan() {
      if (this.busy || !this.selectedCategories.length) return;
      const app = useAppStore();
      app.setBusy(PAGE_IDS.care, true);
      this.view = 'scanning';
      this.stopping = false;
      this.skipRequested = [];
      this.fixReport = null;
      this.progress = {
        phase: 'scanning',
        percent: 0,
        categories: this.selectedCategories.map((category) => ({
          category,
          status: 'queued',
          state: null,
          count: 0,
          elapsedMs: 0,
        })),
        currentCategory: null,
        currentText: '',
        elapsedMs: 0,
      };
      let autoFixNext = false;
      try {
        const report = await CareService.scanWithProgress([...this.selectedCategories], (p) => {
          this.progress = p;
        });
        this.report = report;
        this.selection = selectionFor(report, 'smart');
        this.selectionMode = 'smart';
        this.lastSummary = {
          atMs: Date.now(),
          issues: this.totalIssues,
          bytes: this.totalBytes,
          categories: report.categories.length,
        };
        writeJson(LAST_KEY, this.lastSummary);
        this.view = 'results';
        autoFixNext = this.autoFix && !report.stopped;
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
        this.view = this.report ? 'results' : 'home';
      } finally {
        this.progress = null;
        this.stopping = false;
        app.setBusy(PAGE_IDS.care, false);
      }
      if (autoFixNext) await this.fix(true);
    },
    setItemSelected(item: CareItem, selected: boolean) {
      this.selection = { ...this.selection, [careItemKey(item)]: selected };
      this.selectionMode = 'manual';
    },
    /** Smart / all / none presets; 'manual' only reflects hand-picked changes. */
    applySelectionMode(mode: CareSelectionMode) {
      if (mode === 'manual' || !this.report) return;
      this.selection = selectionFor(this.report, mode);
      this.selectionMode = mode;
    },
    setCategorySelected(category: CareCategoryId, selected: boolean) {
      const next = { ...this.selection };
      const isAdmin = this.report?.isAdmin ?? false;
      for (const item of this.scans.find((c) => c.category === category)?.items ?? []) {
        if (item.fixable) next[careItemKey(item)] = selected && (!item.needsAdmin || isAdmin);
      }
      this.selection = next;
      this.selectionMode = 'manual';
    },
    setAllSelected(selected: boolean) {
      const next: Record<string, boolean> = {};
      const isAdmin = this.report?.isAdmin ?? false;
      for (const item of this.fixableItems) {
        next[careItemKey(item)] = selected && (!item.needsAdmin || isAdmin);
      }
      this.selection = next;
      // Select all / none is a hand-made selection, not the Smart preset.
      this.selectionMode = 'manual';
    },
    async fix(auto = false) {
      if (this.busy || !this.report) return;
      const groups = new Map<CareCategoryId, string[]>();
      if (!auto) {
        for (const item of this.selectedItems) {
          const ids = groups.get(item.category) ?? [];
          ids.push(item.id);
          groups.set(item.category, ids);
        }
        if (!groups.size) return;
      }
      const app = useAppStore();
      app.setBusy(PAGE_IDS.care, true);
      this.view = 'fixing';
      this.stopping = false;
      this.skipRequested = [];
      this.progress = null;
      this.fixStartedAtMs = Date.now();
      this.undoOutcome = null;
      try {
        this.fixReport = await CareService.fixWithProgress(
          {
            items: [...groups].map(([category, itemIds]) => ({ category, itemIds })),
            auto,
            afterAction: auto ? this.afterAction : 'none',
          },
          (p) => {
            this.progress = p;
          }
        );
        this.dropFixedItems(this.fixReport);
        this.startAfterActionCountdown(this.fixReport.afterActionDelaySecs);
        this.view = 'summary';
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
        this.view = 'results';
      } finally {
        this.progress = null;
        this.stopping = false;
        app.setBusy(PAGE_IDS.care, false);
      }
    },
    startAfterActionCountdown(seconds: number | null) {
      this.clearAfterActionTimer();
      if (!seconds) return;
      this.afterActionCountdown = seconds;
      this.afterActionTimer = window.setInterval(() => {
        const left = (this.afterActionCountdown ?? 0) - 1;
        this.afterActionCountdown = Math.max(0, left);
        if (left <= 0) this.clearAfterActionTimer(false);
      }, 1000);
    },
    clearAfterActionTimer(resetCountdown = true) {
      if (this.afterActionTimer != null) window.clearInterval(this.afterActionTimer);
      this.afterActionTimer = null;
      if (resetCountdown) this.afterActionCountdown = null;
    },
    async cancelAfterAction() {
      try {
        await CareService.cancelAfterAction();
        this.clearAfterActionTimer();
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    /** Restore every undo record created by the last fix (changed-since-fix items are skipped). */
    async undoLastFix() {
      try {
        const records = await CareService.listUndoRecords();
        const ids = records.filter((r) => r.createdAtMs >= this.fixStartedAtMs - 1000).map((r) => r.id);
        if (!ids.length) {
          this.undoOutcome = { restored: 0, skipped: 0 };
          return;
        }
        const outcomes = await CareService.undo(ids);
        this.undoOutcome = {
          restored: outcomes.filter((o) => o.status === 'restored').length,
          skipped: outcomes.filter((o) => o.status !== 'restored').length,
        };
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    /** Mirror Core: fixed items leave the stored scan so the results stay honest. */
    dropFixedItems(report: CareFixReport) {
      if (!this.report) return;
      const fixed = new Set(
        report.results.filter((r) => r.status === 'fixed').map((r) => `${r.category}:${r.itemId}`)
      );
      this.report = {
        ...this.report,
        categories: this.report.categories.map((c) => {
          const items = c.items.filter((i) => !fixed.has(careItemKey(i)));
          const state =
            c.state === 'issues' && !items.some((i) => i.fixable)
              ? items.length
                ? 'attention'
                : 'clean'
              : c.state;
          return { ...c, items, state };
        }),
      };
      const next: Record<string, boolean> = {};
      for (const [key, value] of Object.entries(this.selection)) {
        if (!fixed.has(key)) next[key] = value;
      }
      this.selection = next;
    },
    async stop() {
      if (!this.busy) return;
      this.stopping = true;
      try {
        await CareService.stop();
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    async skip() {
      const category = this.runningCategory;
      if (!category || this.skipRequested.includes(category)) return;
      this.skipRequested = [...this.skipRequested, category];
      try {
        await CareService.skip(category);
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    async openAction(action: string) {
      try {
        await CareService.openAction(action);
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    showResults() {
      if (this.report && !this.busy) this.view = 'results';
    },
    backToHome() {
      if (!this.busy) this.view = 'home';
    },
  },
});
