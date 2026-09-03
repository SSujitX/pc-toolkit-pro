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
  }),
  actions: {
    async refresh() {
      this.loading = true;
      try {
        this.snapshot = await MonitorService.snapshot();
      } catch (error) {
        useAppStore().reportError(error);
      } finally {
        this.loading = false;
      }
    },
    /** Quiet refresh for post-optimize / tray — do not flip loading chrome. */
    async refreshQuiet() {
      try {
        this.snapshot = await MonitorService.snapshot();
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
