import { defineStore } from 'pinia';
import { PowerService } from '@/lib/services/api-services';
import type { PowerAction } from '@/lib/models/actions';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

export const usePowerStore = defineStore('power', {
  state: () => ({
    busy: false,
    deadline: null as number | null,
    countdownSeconds: 0,
    timer: null as number | null,
  }),
  actions: {
    async execute(action: PowerAction) {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.power, true);
      this.busy = true;
      try {
        await PowerService.execute(action);
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        app.reportError(error);
      } finally {
        this.busy = false;
        app.setBusy(PAGE_IDS.power, false);
      }
    },
    async schedule(seconds: number) {
      const app = useAppStore();
      app.setBusy(PAGE_IDS.power, true);
      try {
        await PowerService.schedule(seconds);
        this.deadline = Date.now() + seconds * 1000;
        this.startCountdown();
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        app.reportError(error);
      } finally {
        app.setBusy(PAGE_IDS.power, false);
      }
    },
    async cancelSchedule() {
      try {
        await PowerService.cancelSchedule();
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        useAppStore().reportError(error);
      }
      this.clearCountdown();
    },
    startCountdown() {
      this.clearCountdown(false);
      const tick = () => {
        if (!this.deadline) return;
        this.countdownSeconds = Math.max(0, Math.ceil((this.deadline - Date.now()) / 1000));
        if (this.countdownSeconds <= 0) this.clearCountdown();
      };
      tick();
      this.timer = window.setInterval(tick, 1000);
    },
    clearCountdown(resetDeadline = true) {
      if (this.timer != null) clearInterval(this.timer);
      this.timer = null;
      if (resetDeadline) {
        this.deadline = null;
        this.countdownSeconds = 0;
      }
    },
  },
});
