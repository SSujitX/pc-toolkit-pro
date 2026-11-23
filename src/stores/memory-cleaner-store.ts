import { defineStore } from 'pinia';
import { MemoryCleanerService } from '@/lib/services/api-services';
import {
  AUTO_INTERVAL_STEPS_MINUTES,
  defaultMemoryAreas,
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
import { useMonitorStore } from './monitor-store';

/** Settings write ordering (module scope: not UI state, never serialized). */
let settingsWriteSeq = 0;
let settingsWriteChain: Promise<void> = Promise.resolve();

export const useMemoryCleanerStore = defineStore('memoryCleaner', {
  state: () => ({
    loading: false,
    /** Epoch ms when the running optimize started (drives the elapsed clock). */
    busySince: null as number | null,
    progress: null as MemoryProgress | null,
    result: null as MemoryOptimizeResult | null,
    stats: null as MemoryStats | null,
    settings: {
      areas: defaultMemoryAreas(),
      autoIntervalMinutes: 0,
      autoFreeBelowPercent: 0,
    } as MemoryCleanerSettings,
    settingsLoaded: false,
    isElevated: false,
    elevationLoaded: false,
    restartingElevated: false,
    elevationPromptOpen: false,
    pendingOptimizeReason: null as OptimizeReason | null,
    /** Listener for runs made by the Rust auto-clean scheduler. */
    autoUnlisten: null as (() => void) | null,
    statsTimer: null as number | null,
    statsHolders: 0,
    statsPaused: false,
    statsSeq: 0,
    statsAppliedSeq: 0,
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
    async loadElevation() {
      try {
        const status = await MemoryCleanerService.getElevationStatus();
        this.isElevated = status.elevated;
      } catch {
        this.isElevated = false;
      } finally {
        this.elevationLoaded = true;
      }
    },
    async restartAsAdministrator() {
      if (this.isElevated || this.restartingElevated) return;
      const app = useAppStore();
      this.restartingElevated = true;
      try {
        await MemoryCleanerService.restartAsAdministrator();
        // Process should exit after UAC approval; keep busy state if it does not.
      } catch (error) {
        this.restartingElevated = false;
        if (!isCancelledError(error)) app.reportError(error);
      }
    },
    /** Interactive optimize (page / titlebar): prompt to elevate when needed. */
    async requestOptimize(reason: OptimizeReason = 'manual') {
      if (this.loading || this.restartingElevated) return;
      if (!this.settingsLoaded) await this.loadSettings();
      if (!this.elevationLoaded) await this.loadElevation();
      if (!this.isElevated) {
        this.pendingOptimizeReason = reason;
        this.elevationPromptOpen = true;
        return;
      }
      await this.run(reason);
    },
    async confirmElevationRestart() {
      this.elevationPromptOpen = false;
      this.pendingOptimizeReason = null;
      await this.restartAsAdministrator();
    },
    /** Explicit "continue without admin": optimize with the reason that opened the prompt. */
    async declineElevationContinue() {
      const reason = this.pendingOptimizeReason ?? 'manual';
      this.elevationPromptOpen = false;
      this.pendingOptimizeReason = null;
      await this.run(reason);
    },
    /** Overlay click / Escape: close the prompt without optimizing. */
    dismissElevationPrompt() {
      this.elevationPromptOpen = false;
      this.pendingOptimizeReason = null;
    },
    /**
     * v-model setter only. It deliberately keeps `pendingOptimizeReason`: the dialog closes
     * itself before emitting confirm/cancel, and those handlers still need the reason.
     */
    setElevationPromptOpen(open: boolean) {
      this.elevationPromptOpen = open;
    },
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
    /**
     * Writes are serialized and coalesced: rapid toggles queue behind the in-flight write, a
     * superseded write is skipped, and only the newest reply may replace local settings — so a
     * slow earlier reply can never revert a later toggle.
     */
    persistSettings(): Promise<void> {
      const seq = ++settingsWriteSeq;
      settingsWriteChain = settingsWriteChain.then(async () => {
        if (seq !== settingsWriteSeq) return;
        try {
          const saved = await MemoryCleanerService.setSettings(this.settings);
          if (seq === settingsWriteSeq) this.settings = saved;
        } catch (error) {
          useAppStore().reportError(error);
        }
      });
      return settingsWriteChain;
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
      const seq = ++this.statsSeq;
      try {
        const next = await MemoryCleanerService.stats();
        // Drop out-of-order replies so a slow poll cannot pin an older RAM %.
        if (seq < this.statsAppliedSeq) return;
        this.stats = next;
        this.statsAppliedSeq = seq;
      } catch {
        // Browser / unsupported — leave last stats.
      }
    },
    /**
     * Reference-counted 1s stats poll (titlebar circle + Memory Cleaner page). Every acquire must
     * be paired with one release; the poll runs while any holder exists and the window is visible.
     */
    acquireStatsPolling() {
      this.statsHolders += 1;
      this.syncStatsTimer(true);
    },
    releaseStatsPolling() {
      this.statsHolders = Math.max(0, this.statsHolders - 1);
      this.syncStatsTimer(false);
    },
    /** Pause while the window is hidden in the tray; resumes with an immediate refresh. */
    setStatsPollingPaused(paused: boolean) {
      if (this.statsPaused === paused) return;
      this.statsPaused = paused;
      this.syncStatsTimer(!paused);
    },
    syncStatsTimer(refreshNow: boolean) {
      const wanted = this.statsHolders > 0 && !this.statsPaused;
      if (wanted && refreshNow) void this.refreshStats();
      if (wanted && this.statsTimer == null) {
        // 1s — keep titlebar circle aligned with Physical memory / WMC / IObit.
        this.statsTimer = window.setInterval(() => {
          void this.refreshStats();
        }, 1000);
      } else if (!wanted && this.statsTimer != null) {
        window.clearInterval(this.statsTimer);
        this.statsTimer = null;
      }
    },
    /**
     * Auto-clean (interval + low free memory) is scheduled by Rust on its own thread so it keeps
     * working while the window is hidden in the tray. The page only refreshes after each run.
     */
    async startAutoClean() {
      if (this.autoUnlisten) return;
      try {
        this.autoUnlisten = await MemoryCleanerService.onAutoOptimized((event) => {
          if (event.result) {
            this.result = event.result;
            this.isElevated = event.result.adminOptimizations || this.isElevated;
          }
          void useHistoryStore().load({ reportError: false });
          void this.refreshStats();
          void useMonitorStore().refreshQuiet();
        });
      } catch {
        // Browser preview: no Tauri events.
      }
    },
    stopAutoClean() {
      this.autoUnlisten?.();
      this.autoUnlisten = null;
    },
    async run(reason: OptimizeReason = 'manual') {
      if (this.loading) return;
      const app = useAppStore();
      app.setBusy(PAGE_IDS.memoryCleaner, true);
      this.loading = true;
      this.busySince = Date.now();
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
        this.isElevated = this.result.adminOptimizations || this.isElevated;
        void useHistoryStore().load({ reportError: false });
        void this.refreshStats();
        // Titlebar / tray gauge share monitor snapshot — refresh immediately like WMC.
        void useMonitorStore().refreshQuiet();
      } catch (error) {
        if (!isCancelledError(error)) app.reportError(error);
      } finally {
        this.loading = false;
        this.busySince = null;
        this.progress = null;
        app.setBusy(PAGE_IDS.memoryCleaner, false);
      }
    },
    async cancel() {
      try {
        await MemoryCleanerService.cancel();
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
  },
});
