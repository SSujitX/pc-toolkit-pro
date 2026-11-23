import { defineStore } from 'pinia';
import { MonitorService, QuickActionService, type MonitorSnapshot } from '@/lib/services/api-services';
import type { QuickActionId } from '@/lib/models/actions';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from './app-store';

export const useMonitorStore = defineStore('monitor', {
  state: () => ({
    snapshot: null as MonitorSnapshot | null,
    loading: false,
    timer: null as number | null,
    /** Monotonic request id — drop out-of-order replies from slow polls. */
    pollSeq: 0,
    appliedSeq: 0,
    holders: 0,
    paused: false,
  }),
  actions: {
    async refresh() {
      this.loading = true;
      try {
        await this.refreshQuiet();
      } catch (error) {
        useAppStore().reportError(error);
      } finally {
        this.loading = false;
      }
    },
    /** Quiet refresh for titlebar / post-optimize — do not flip loading chrome. */
    async refreshQuiet() {
      const seq = ++this.pollSeq;
      try {
        const next = await MonitorService.snapshot();
        // Ignore stale completions so a slow nvidia-era poll cannot pin RAM%.
        if (seq < this.appliedSeq) return;
        this.snapshot = next;
        this.appliedSeq = seq;
      } catch {
        // keep last snapshot
      }
    },
    /**
     * Reference-counted 1s snapshot poll (titlebar uptime/disk + Monitor page). Every acquire must
     * be paired with one release; the poll runs while any holder exists and the window is visible.
     */
    acquirePolling() {
      this.holders += 1;
      if (this.holders === 1 && !this.paused) void this.refresh();
      this.syncTimer();
    },
    releasePolling() {
      this.holders = Math.max(0, this.holders - 1);
      this.syncTimer();
    },
    /** Pause while the window is hidden in the tray; resumes with an immediate refresh. */
    setPaused(paused: boolean) {
      if (this.paused === paused) return;
      this.paused = paused;
      if (!paused && this.holders > 0) void this.refreshQuiet();
      this.syncTimer();
    },
    syncTimer() {
      const wanted = this.holders > 0 && !this.paused;
      if (wanted && this.timer == null) {
        // ~1s like WinMemoryCleaner tray — titlebar circle stays live.
        this.timer = window.setInterval(() => {
          void this.refreshQuiet();
        }, 1000);
      } else if (!wanted && this.timer != null) {
        window.clearInterval(this.timer);
        this.timer = null;
      }
    },
    async openAction(action: QuickActionId) {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.monitor, true);
      try {
        await QuickActionService.open(action);
      } catch (error) {
        app.reportError(error);
      } finally {
        app.setBusy(PAGE_IDS.monitor, false);
      }
    },
  },
});
