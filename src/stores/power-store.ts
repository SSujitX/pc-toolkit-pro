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
    scheduledSeconds: 0,
    timer: null as number | null,
  }),
  getters: {
    hasActiveSchedule(state): boolean {
      return state.deadline != null && state.countdownSeconds > 0;
    },
  },
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
      this.busy = true;
      try {
        await PowerService.schedule(seconds);
        this.scheduledSeconds = seconds;
        this.deadline = Date.now() + seconds * 1000;
        this.startCountdown();
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        this.clearCountdown();
        app.reportError(error);
      } finally {
        this.busy = false;
        app.setBusy(PAGE_IDS.power, false);
      }
    },
    async cancelSchedule() {
      const app = useAppStore();
      this.busy = true;
      try {
        await PowerService.cancelSchedule();
        void useHistoryStore().load({ reportError: false });
      } catch (error) {
        app.reportError(error);
      } finally {
        this.clearCountdown();
        this.busy = false;
      }
    },
    /** Resume ticking after leaving/returning to the Power page. */
    ensureCountdown() {
      if (!this.deadline) return;
      if (this.deadline <= Date.now()) {
        this.clearCountdown();
        return;
      }
      if (this.timer == null) this.startCountdown();
    },
    startCountdown() {
      this.clearCountdown(false);
      const tick = () => {
        if (!this.deadline) return;
        this.countdownSeconds = Math.max(0, Math.ceil((this.deadline - Date.now()) / 1000));
        if (this.countdownSeconds <= 0) this.clearCountdown();
      };
      tick();
      this.timer = window.setInterval(tick, 250);
    },
    clearCountdown(resetDeadline = true) {
      if (this.timer != null) clearInterval(this.timer);
      this.timer = null;
      if (resetDeadline) {
        this.deadline = null;
        this.countdownSeconds = 0;
        this.scheduledSeconds = 0;
      }
    },
  },
});
