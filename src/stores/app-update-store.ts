import { defineStore } from 'pinia';

import {
  APP_UPDATE_FAILURE_STAGE_IDS,
  APP_UPDATE_STATUS_IDS,
  type AppUpdateFailureStage,
  type AppUpdateInfo,
  type AppUpdateStatus,
} from '@/lib/models/app-update';
import { AppUpdateService } from '@/lib/services/app-update-service';

function normalizeError(error: unknown): string {
  if (error instanceof Error) return error.message.trim();
  return String(error ?? '').trim();
}

interface AppUpdateState {
  status: AppUpdateStatus;
  currentVersion: string;
  update: AppUpdateInfo | null;
  checkError: string;
  downloadedBytes: number;
  totalBytes: number | null;
  actionError: string;
  failureStage: AppUpdateFailureStage | null;
}

export const useAppUpdateStore = defineStore('app-update', {
  state: (): AppUpdateState => ({
    status: APP_UPDATE_STATUS_IDS.idle,
    currentVersion: '',
    update: null,
    checkError: '',
    downloadedBytes: 0,
    totalBytes: null,
    actionError: '',
    failureStage: null,
  }),
  getters: {
    busy: (state) =>
      state.status === APP_UPDATE_STATUS_IDS.checking ||
      state.status === APP_UPDATE_STATUS_IDS.downloading ||
      state.status === APP_UPDATE_STATUS_IDS.installing ||
      state.status === APP_UPDATE_STATUS_IDS.restarting,
  },
  actions: {
    async initialize() {
      if (this.currentVersion) return;
      try {
        this.currentVersion = await AppUpdateService.currentVersion();
      } catch {
        // Version falls back to package constant in the About UI.
      }
    },
    async check(manual = true) {
      if (
        this.status === APP_UPDATE_STATUS_IDS.downloaded ||
        this.status === APP_UPDATE_STATUS_IDS.restartRequired
      ) {
        return;
      }
      if (this.busy) return;

      this.status = APP_UPDATE_STATUS_IDS.checking;
      this.checkError = '';
      this.actionError = '';
      this.failureStage = null;

      try {
        await this.initialize();
        const update = await AppUpdateService.check();
        if (!update) {
          this.update = null;
          this.status = APP_UPDATE_STATUS_IDS.upToDate;
          return;
        }

        this.currentVersion = update.currentVersion;
        this.update = update;
        this.status = APP_UPDATE_STATUS_IDS.available;
      } catch (error) {
        this.status = manual ? APP_UPDATE_STATUS_IDS.error : APP_UPDATE_STATUS_IDS.idle;
        this.checkError = manual ? normalizeError(error) : '';
      }
    },
    async download() {
      if (!this.update || this.status !== APP_UPDATE_STATUS_IDS.available) return;

      this.status = APP_UPDATE_STATUS_IDS.downloading;
      this.downloadedBytes = 0;
      this.totalBytes = null;
      this.actionError = '';
      this.failureStage = null;

      try {
        await AppUpdateService.download((progress) => {
          this.downloadedBytes = progress.downloadedBytes;
          this.totalBytes = progress.totalBytes;
        });
        this.status = APP_UPDATE_STATUS_IDS.downloaded;
      } catch (error) {
        this.status = APP_UPDATE_STATUS_IDS.available;
        this.actionError = normalizeError(error);
        this.failureStage = APP_UPDATE_FAILURE_STAGE_IDS.download;
      }
    },
    async installDownloaded() {
      if (!this.update || this.status !== APP_UPDATE_STATUS_IDS.downloaded) return;

      this.status = APP_UPDATE_STATUS_IDS.installing;
      this.actionError = '';
      this.failureStage = null;

      try {
        await AppUpdateService.installDownloaded();
      } catch (error) {
        this.status = APP_UPDATE_STATUS_IDS.downloaded;
        this.actionError = normalizeError(error);
        this.failureStage = APP_UPDATE_FAILURE_STAGE_IDS.install;
        return;
      }

      this.status = APP_UPDATE_STATUS_IDS.restartRequired;
      await this.restartApplication();
    },
    async restartApplication() {
      if (!this.update || this.status !== APP_UPDATE_STATUS_IDS.restartRequired) return;

      this.status = APP_UPDATE_STATUS_IDS.restarting;
      this.actionError = '';
      this.failureStage = null;

      try {
        await AppUpdateService.restartApplication();
      } catch (error) {
        this.status = APP_UPDATE_STATUS_IDS.restartRequired;
        this.actionError = normalizeError(error);
        this.failureStage = APP_UPDATE_FAILURE_STAGE_IDS.restart;
      }
    },
    resetIdle() {
      if (this.busy) return;
      if (
        this.status === APP_UPDATE_STATUS_IDS.upToDate ||
        this.status === APP_UPDATE_STATUS_IDS.error
      ) {
        this.status = APP_UPDATE_STATUS_IDS.idle;
        this.checkError = '';
      }
      this.actionError = '';
      this.failureStage = null;
    },
  },
});
