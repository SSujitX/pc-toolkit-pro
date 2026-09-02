import { defineStore } from 'pinia';
import { MemoryCleanerService } from '@/lib/services/api-services';
import {
  AUTO_INTERVAL_STEPS_MINUTES,
  defaultMemoryAreas,
  freePhysicalPercent,
  type MemoryAreaId,
  type MemoryCleanerSettings,
  type MemoryOptimizeResult,
  type MemoryProgress,
  type MemoryStats,
  type OptimizeReason,
} from '@/lib/models/memory-cleaner';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

const LOW_MEMORY_COOLDOWN_MS = 5 * 60 * 1000;
const AUTO_TICK_MS = 30_000;

export const useMemoryCleanerStore = defineStore('memoryCleaner', {
  state: () => ({
    loading: false,
    progress: null as MemoryProgress | null,
    result: null as MemoryOptimizeResult | null,
    stats: null as MemoryStats | null,
    settings: {
      areas: defaultMemoryAreas(),
      autoIntervalMinutes: 0,
      autoFreeBelowPercent: 0,
    } as MemoryCleanerSettings,
    settingsLoaded: false,
    elevatedHint: false,
    lastOptimizeAt: 0,
    lastLowMemoryOptimizeAt: 0,
    autoTimer: null as number | null,
    statsTimer: null as number | null,
  }),
  getters: {
    selectedAreas(state): MemoryAreaId[] {
      return Object.entries(state.settings.areas)
        .filter(([, on]) => on)
        .map(([id]) => id as MemoryAreaId);
    },
    intervalSteps: () => [...AUTO_INTERVAL_STEPS_MINUTES],
  },
  actions: {
    async loadSettings() {
      try {
        this.settings = await MemoryCleanerService.getSettings();
        this.settingsLoaded = true;
      } catch {
        this.settings = {
          areas: defaultMemoryAreas(),
          autoIntervalMinutes: 0,
          autoFreeBelowPercent: 0,
        };
        this.settingsLoaded = true;
      }
    },
    async persistSettings() {
      try {
        this.settings = await MemoryCleanerService.setSettings(this.settings);
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    setArea(id: MemoryAreaId, enabled: boolean) {
      const areas = { ...this.settings.areas, [id]: enabled };
      if (enabled && id === 'standbyList') {
        areas.standbyListLowPriority = false;
      }
      if (enabled && id === 'standbyListLowPriority') {
        areas.standbyList = false;
      }
      this.settings = { ...this.settings, areas };
      void this.persistSettings();
    },
    setAreas(next: Record<string, boolean>) {
      const areas = { ...next };
      if (areas.standbyList && areas.standbyListLowPriority) {
        areas.standbyListLowPriority = false;
      }
      this.settings = { ...this.settings, areas };
      void this.persistSettings();
    },
    setAutoIntervalMinutes(minutes: number) {
      this.settings = { ...this.settings, autoIntervalMinutes: minutes };
      void this.persistSettings();
    },
    setAutoFreeBelowPercent(percent: number) {
      this.settings = {
        ...this.settings,
        autoFreeBelowPercent: Math.max(0, Math.min(100, Math.round(percent))),
      };
      void this.persistSettings();
    },
    async refreshStats() {
      try {
        this.stats = await MemoryCleanerService.stats();
      } catch {
        // Browser / unsupported — leave last stats.
      }
    },
    startStatsPolling() {
      void this.refreshStats();
      if (this.statsTimer != null) return;
      this.statsTimer = window.setInterval(() => {
        void this.refreshStats();
      }, 2000);
    },
    stopStatsPolling() {
      if (this.statsTimer != null) {
        window.clearInterval(this.statsTimer);
        this.statsTimer = null;
      }
    },
    startAutoClean() {
      if (this.autoTimer != null) return;
      this.autoTimer = window.setInterval(() => {
        void this.autoTick();
      }, AUTO_TICK_MS);
    },
    stopAutoClean() {
      if (this.autoTimer != null) {
        window.clearInterval(this.autoTimer);
        this.autoTimer = null;
      }
    },
    async autoTick() {
      if (this.loading || !this.settingsLoaded) return;
      const now = Date.now();
      const intervalMs = this.settings.autoIntervalMinutes * 60_000;

      if (intervalMs > 0 && this.lastOptimizeAt > 0 && now - this.lastOptimizeAt >= intervalMs) {
        await this.run('schedule');
        return;
      }
      // First schedule fire: wait a full interval from app start.
      if (intervalMs > 0 && this.lastOptimizeAt === 0) {
        this.lastOptimizeAt = now;
      }

      const threshold = this.settings.autoFreeBelowPercent;
      if (threshold <= 0) return;

      try {
        const stats = await MemoryCleanerService.stats();
        this.stats = stats;
        const freePct = freePhysicalPercent(stats);
        if (freePct >= threshold) return;
        if (
          this.lastLowMemoryOptimizeAt > 0 &&
          now - this.lastLowMemoryOptimizeAt < LOW_MEMORY_COOLDOWN_MS
        ) {
          return;
        }
        await this.run('lowMemory');
        this.lastLowMemoryOptimizeAt = Date.now();
      } catch {
        // ignore auto tick failures
      }
    },
    async run(reason: OptimizeReason = 'manual') {
      if (this.loading) return;
      const app = useAppStore();
      app.setBusy(PAGE_IDS.memoryCleaner, true);
      this.loading = true;
      this.progress = null;
      this.result = null;
      try {
        this.result = await MemoryCleanerService.optimizeWithProgress(
          reason,
          (p) => {
            this.progress = p;
          },
          this.selectedAreas
        );
        this.elevatedHint = !this.result.adminOptimizations;
        this.lastOptimizeAt = Date.now();
        if (reason === 'lowMemory') {
          this.lastLowMemoryOptimizeAt = this.lastOptimizeAt;
        }
        void useHistoryStore().load({ reportError: false });
        void this.refreshStats();
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.progress = null;
        app.setBusy(PAGE_IDS.memoryCleaner, false);
      }
    },
    async cancel() {
      await MemoryCleanerService.cancel();
    },
  },
});
