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
    startPolling() {
      void this.refresh();
      if (this.timer != null) return;
      // ~1s like WinMemoryCleaner tray — titlebar circle stays live.
      this.timer = window.setInterval(() => {
        void this.refreshQuiet();
      }, 1000);
    },
    stopPolling() {
      if (this.timer != null) {
        clearInterval(this.timer);
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
