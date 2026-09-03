<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Check,
  ChevronRight,
  ExternalLink,
  FolderOpen,
  History,
  Info,
  Languages,
  Palette,
  RefreshCw,
  X,
} from '@lucide/vue';
import { openUrl } from '@tauri-apps/plugin-opener';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtSoftSelect from '@/components/custom/pt-soft-select.vue';
import { APP_NAME, APP_VERSION, PAGE_IDS } from '@/lib/models/application-shell';
import {
  APP_UPDATE_FAILURE_STAGE_IDS,
  APP_UPDATE_STATUS_IDS,
} from '@/lib/models/app-update';
import { SettingsApi } from '@/lib/services/api-services';
import { formatBytes } from '@/lib/utils/format';
import { useAppStore, type ThemeMode } from '@/stores/app-store';
import { useAppUpdateStore } from '@/stores/app-update-store';
import appLogo from '@/assets/brand/logo.png';

const GITHUB_URL = 'https://github.com/SSujitX/pc-toolkit-pro';
const LICENSE_URL = 'https://github.com/SSujitX/pc-toolkit-pro/blob/master/LICENSE';
const DEVELOPER_URL = 'https://github.com/SSujitX';
const COPYRIGHT_YEAR = 2025;

const { t } = useI18n();
const app = useAppStore();
const updates = useAppUpdateStore();
const aboutOpen = ref(false);

const themeOptions = computed(() => [
  { value: 'system', label: t('settings.themeSystem') },
  { value: 'light', label: t('settings.light') },
  { value: 'dark', label: t('settings.dark') },
]);

const languageOptions = computed(() => [
  { value: 'en', label: t('settings.languageEnglish') },
]);

const displayVersion = computed(
  () => updates.currentVersion || APP_VERSION,
);

const checking = computed(() => updates.status === APP_UPDATE_STATUS_IDS.checking);
const upToDate = computed(() => updates.status === APP_UPDATE_STATUS_IDS.upToDate);
const available = computed(() => updates.status === APP_UPDATE_STATUS_IDS.available);
const downloading = computed(() => updates.status === APP_UPDATE_STATUS_IDS.downloading);
const downloaded = computed(() => updates.status === APP_UPDATE_STATUS_IDS.downloaded);
const installing = computed(() => updates.status === APP_UPDATE_STATUS_IDS.installing);
const restartRequired = computed(
  () => updates.status === APP_UPDATE_STATUS_IDS.restartRequired,
);
const restarting = computed(() => updates.status === APP_UPDATE_STATUS_IDS.restarting);
const checkFailed = computed(() => updates.status === APP_UPDATE_STATUS_IDS.error);
const closeLocked = computed(
  () => checking.value || installing.value || restarting.value,
);
const updateFocused = computed(
  () =>
    available.value ||
    downloading.value ||
    downloaded.value ||
    installing.value ||
    restartRequired.value ||
    restarting.value,
);

const downloadPercent = computed(() => {
  const total = updates.totalBytes;
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, (updates.downloadedBytes / total) * 100));
});

const statusTitle = computed(() => {
  if (checking.value) return t('settings.updateChecking');
  if (upToDate.value) return t('settings.upToDate');
  if (available.value) return t('settings.updateAvailable');
  if (downloading.value) return t('settings.updateDownloading');
  if (downloaded.value) return t('settings.updateDownloaded');
  if (installing.value) return t('settings.updateInstalling');
  if (restartRequired.value) return t('settings.updateRestartRequired');
  if (restarting.value) return t('settings.updateRestarting');
  if (checkFailed.value) return t('settings.updateCheckFailed');
  return t('settings.softwareUpdate');
});

const statusBody = computed(() => {
  const version = updates.update?.version ?? displayVersion.value;
  if (checking.value) return t('settings.updateCheckingBody');
  if (upToDate.value) {
    return t('settings.upToDateBody', { name: APP_NAME, version: displayVersion.value });
  }
  if (available.value) return t('settings.updateAvailableBody', { version });
  if (downloading.value) {
    if (updates.totalBytes) {
      return t('settings.updateDownloadProgress', {
        downloaded: formatBytes(updates.downloadedBytes),
        total: formatBytes(updates.totalBytes),
      });
    }
    return t('settings.updateDownloading');
  }
  if (downloaded.value) return t('settings.updateDownloadedBody', { version });
  if (installing.value) return t('settings.updateInstallingBody');
  if (restartRequired.value) return t('settings.updateRestartRequiredBody', { version });
  if (restarting.value) return t('settings.updateRestarting');
  if (checkFailed.value) {
    return updates.checkError || t('settings.updateCheckFailed');
  }
  return t('settings.softwareUpdateBody');
});

const actionErrorTitle = computed(() => {
  if (updates.failureStage === APP_UPDATE_FAILURE_STAGE_IDS.install) {
    return t('settings.installFailed');
  }
  if (updates.failureStage === APP_UPDATE_FAILURE_STAGE_IDS.restart) {
    return t('settings.restartFailed');
  }
  return t('settings.downloadFailed');
});

const primaryLabel = computed(() => {
  if (checking.value) return t('settings.updateChecking');
  if (available.value) {
    return updates.failureStage ? t('settings.updateRetry') : t('settings.downloadUpdate');
  }
  if (downloaded.value) return t('settings.installAndRestart');
  if (installing.value) return t('settings.updateInstalling');
  if (restartRequired.value) return t('settings.restartNow');
  if (restarting.value) return t('settings.updateRestarting');
  if (checkFailed.value) return t('settings.updateRetry');
  return t('settings.checkUpdates');
});

function onThemeChange(value: string | number) {
  app.setTheme(String(value) as ThemeMode);
}

async function openAbout() {
  aboutOpen.value = true;
  await updates.initialize();
}

function closeAbout() {
  if (closeLocked.value) return;
  aboutOpen.value = false;
  updates.resetIdle();
}

function openHistory() {
  app.navigate(PAGE_IDS.history);
}

async function openDataFolder() {
  try {
    await SettingsApi.openAppDataFolder();
  } catch (error) {
    app.reportError(error);
  }
}

async function openExternal(url: string) {
  try {
    await openUrl(url);
  } catch (error) {
    app.reportError(error);
  }
}

async function onPrimaryAction() {
  if (available.value) {
    await updates.download();
    return;
  }
  if (downloaded.value) {
    await updates.installDownloaded();
    return;
  }
  if (restartRequired.value) {
    await updates.restartApplication();
    return;
  }
  await updates.check(true);
}

function onEsc(event: KeyboardEvent) {
  if (event.key === 'Escape' && aboutOpen.value) closeAbout();
}

onMounted(() => {
  document.addEventListener('keydown', onEsc);
  void updates.initialize();
});
onUnmounted(() => document.removeEventListener('keydown', onEsc));
</script>

<template>
  <PtPageShell :title="t('settings.title')" :subtitle="t('settings.subtitle')">
    <div class="settings">
      <section class="group">
        <h2 class="group-label">{{ t('settings.groups.general') }}</h2>
        <div class="card">
          <div class="setting-row">
            <span class="row-icon" aria-hidden="true">
              <Languages :size="18" :stroke-width="1.9" />
            </span>
            <div class="row-copy">
              <strong>{{ t('settings.language') }}</strong>
              <p>{{ t('settings.languageBody') }}</p>
            </div>
            <div class="row-control">
              <PtSoftSelect
                model-value="en"
                :options="languageOptions"
                :aria-label="t('settings.language')"
              />
            </div>
          </div>

          <div class="setting-row">
            <span class="row-icon" aria-hidden="true">
              <Palette :size="18" :stroke-width="1.9" />
            </span>
            <div class="row-copy">
              <strong>{{ t('settings.theme') }}</strong>
              <p>{{ t('settings.themeBody') }}</p>
            </div>
            <div class="row-control">
              <PtSoftSelect
                :model-value="app.theme"
                :options="themeOptions"
                :aria-label="t('settings.theme')"
                @update:model-value="onThemeChange"
              />
            </div>
          </div>
        </div>
      </section>

      <section class="group">
        <h2 class="group-label">{{ t('settings.groups.support') }}</h2>
        <div class="card">
          <button type="button" class="setting-row action" @click="openDataFolder">
            <span class="row-icon" aria-hidden="true">
              <FolderOpen :size="18" :stroke-width="1.9" />
            </span>
            <div class="row-copy">
              <strong>{{ t('settings.dataFolder') }}</strong>
              <p>{{ t('settings.dataFolderBody') }}</p>
            </div>
            <span class="row-link">
              {{ t('settings.openFolder') }}
              <ExternalLink :size="14" :stroke-width="2" />
            </span>
          </button>

          <button type="button" class="setting-row action" @click="openHistory">
            <span class="row-icon" aria-hidden="true">
              <History :size="18" :stroke-width="1.9" />
            </span>
            <div class="row-copy">
              <strong>{{ t('settings.activityHistory') }}</strong>
              <p>{{ t('settings.activityHistoryBody') }}</p>
            </div>
            <span class="row-link">
              {{ t('settings.openHistory') }}
              <ChevronRight :size="16" :stroke-width="2" />
            </span>
          </button>
        </div>
      </section>

      <section class="group">
        <h2 class="group-label">{{ t('settings.groups.about') }}</h2>
        <div class="card">
          <button type="button" class="setting-row action" @click="openAbout">
            <img :src="appLogo" :alt="APP_NAME" class="about-mark" width="36" height="36" />
            <div class="row-copy">
              <strong>{{ t('settings.aboutTitle') }}</strong>
              <p>{{ t('settings.aboutBody') }}</p>
            </div>
            <span class="row-link muted">
              {{ t('settings.versionShort', { version: displayVersion }) }}
              <ChevronRight :size="16" :stroke-width="2" />
            </span>
          </button>
        </div>
      </section>
    </div>

    <div v-if="aboutOpen" class="about-root" role="dialog" aria-modal="true">
      <div class="about-overlay" @click="closeAbout" />
      <div class="about-panel">
        <button
          type="button"
          class="about-close"
          aria-label="Close"
          :disabled="closeLocked"
          @click="closeAbout"
        >
          <X :size="16" :stroke-width="2" />
        </button>

        <template v-if="!updateFocused">
          <img :src="appLogo" :alt="APP_NAME" class="about-logo" width="72" height="72" />
          <h2>{{ t('settings.aboutTitle') }}</h2>
          <p class="about-version">{{ displayVersion }}</p>
          <p class="about-desc">{{ t('settings.aboutDialogBody') }}</p>

          <div class="about-links">
            <button type="button" class="link" @click="openExternal(GITHUB_URL)">
              {{ t('settings.github') }}
              <ExternalLink :size="13" :stroke-width="2" />
            </button>
            <button type="button" class="link" @click="openExternal(LICENSE_URL)">
              {{ t('settings.license') }}
              <ExternalLink :size="13" :stroke-width="2" />
            </button>
          </div>
        </template>

        <template v-else>
          <img :src="appLogo" :alt="APP_NAME" class="about-logo compact" width="52" height="52" />
          <h2>{{ t('settings.updateAvailable') }} {{ updates.update?.version }}</h2>
          <p class="about-version">
            {{ t('settings.versionShort', { version: displayVersion }) }}
          </p>
          <div v-if="updates.update?.notes" class="release-notes">
            <strong>{{ t('settings.releaseNotes') }}</strong>
            <p>{{ updates.update.notes }}</p>
          </div>
          <p v-else class="about-desc">{{ t('settings.noReleaseNotes') }}</p>
        </template>

        <div class="status-card" aria-live="polite">
          <span class="status-icon" aria-hidden="true">
            <RefreshCw
              v-if="checking || downloading || installing || restarting"
              class="spin"
              :size="18"
              :stroke-width="1.9"
            />
            <Check v-else-if="upToDate || downloaded || restartRequired" :size="18" :stroke-width="1.9" />
            <Info v-else :size="18" :stroke-width="1.9" />
          </span>
          <div>
            <strong>{{ statusTitle }}</strong>
            <p>{{ statusBody }}</p>
          </div>
        </div>

        <div v-if="downloading" class="download-track-wrap">
          <div
            class="download-track"
            role="progressbar"
            :aria-valuenow="downloadPercent === null ? undefined : Math.round(downloadPercent)"
            aria-valuemin="0"
            aria-valuemax="100"
          >
            <span
              :class="{ indeterminate: downloadPercent === null }"
              :style="downloadPercent === null ? undefined : { width: `${downloadPercent}%` }"
            />
          </div>
        </div>

        <div v-if="updates.actionError" class="action-error" role="alert">
          <strong>{{ actionErrorTitle }}</strong>
          <p>{{ updates.actionError }}</p>
        </div>

        <div class="about-actions">
          <button
            v-if="!downloading && !installing && !restarting"
            type="button"
            class="pt-btn"
            :disabled="closeLocked"
            @click="closeAbout"
          >
            {{
              available || downloaded || restartRequired
                ? available
                  ? t('settings.notNow')
                  : t('settings.later')
                : t('common.close')
            }}
          </button>
          <button
            v-if="downloading"
            type="button"
            class="pt-btn"
            @click="closeAbout"
          >
            {{ t('settings.continueInBackground') }}
          </button>
          <button
            v-if="!downloading"
            type="button"
            class="pt-btn pt-btn-primary"
            :disabled="checking || installing || restarting"
            @click="onPrimaryAction"
          >
            <RefreshCw
              v-if="checking || installing || restarting"
              class="spin"
              :size="15"
              :stroke-width="2"
            />
            {{ primaryLabel }}
          </button>
        </div>

        <footer class="about-credit">
          <span>{{ t('settings.developedBy') }} SSujitX</span>
          <span class="sep" aria-hidden="true">|</span>
          <button type="button" class="credit-link" @click="openExternal(DEVELOPER_URL)">
            github.com/SSujitX
          </button>
          <span class="sep" aria-hidden="true">|</span>
          <span>© {{ COPYRIGHT_YEAR }}</span>
        </footer>
      </div>
    </div>
  </PtPageShell>
</template>

<style scoped>
.settings {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 18px;
  max-width: 760px;
  overflow: auto;
  padding-bottom: 20px;
}
.group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.group-label {
  margin: 0;
  padding-inline: 4px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 650;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.card {
  overflow: visible;
  border: 1px solid color-mix(in oklab, var(--border) 78%, transparent);
  border-radius: 16px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.setting-row {
  display: flex;
  width: 100%;
  min-height: 72px;
  align-items: center;
  gap: 14px;
  border: 0;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  background: transparent;
  padding: 14px 16px;
  color: inherit;
  text-align: left;
}
.setting-row:last-child {
  border-bottom: 0;
}
.setting-row.action {
  cursor: pointer;
}
.setting-row.action:hover {
  background: color-mix(in oklab, var(--muted) 45%, transparent);
}
.row-icon {
  display: grid;
  width: 36px;
  height: 36px;
  flex: none;
  place-items: center;
  border-radius: 10px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.about-mark {
  width: 36px;
  height: 36px;
  flex: none;
  border-radius: 10px;
  object-fit: cover;
  box-shadow: 0 1px 0 color-mix(in oklab, var(--foreground) 6%, transparent);
}
.row-copy {
  min-width: 0;
  flex: 1;
}
.row-copy strong {
  display: block;
  font-size: 0.875rem;
  font-weight: 650;
}
.row-copy p {
  margin: 3px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.row-control {
  width: min(180px, 38vw);
  flex: none;
}
.row-link {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 6px;
  color: var(--primary);
  font-size: 0.8125rem;
  font-weight: 600;
  white-space: nowrap;
}
.row-link.muted {
  color: var(--muted-foreground);
}

.about-root {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: grid;
  place-items: center;
}
.about-overlay {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 28%);
}
.about-panel {
  position: relative;
  z-index: 1;
  width: min(440px, calc(100vw - 32px));
  padding: 28px 24px 16px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 24px;
  background: var(--card);
  box-shadow: 0 28px 60px -28px color-mix(in oklab, var(--foreground) 40%, transparent);
  text-align: center;
}
.about-close {
  position: absolute;
  top: 12px;
  right: 12px;
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}
.about-close:hover:not(:disabled) {
  background: color-mix(in oklab, var(--muted) 70%, transparent);
  color: var(--foreground);
}
.about-close:disabled {
  opacity: 0.4;
  cursor: default;
}
.about-logo {
  width: 72px;
  height: 72px;
  margin: 8px auto 0;
  border-radius: 18px;
  object-fit: cover;
  box-shadow: var(--shadow-card);
}
.about-logo.compact {
  width: 52px;
  height: 52px;
  border-radius: 14px;
}
.about-panel h2 {
  margin: 14px 0 0;
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.about-version {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.about-desc {
  margin: 12px auto 0;
  max-width: 34ch;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.about-links {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 14px;
  margin-top: 14px;
}
.link {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 0;
  background: transparent;
  color: var(--primary);
  font-size: 0.8125rem;
  font-weight: 650;
  cursor: pointer;
}
.link:hover {
  text-decoration: underline;
}
.release-notes {
  margin: 14px auto 0;
  max-width: 36ch;
  padding: 12px 14px;
  border-radius: 14px;
  background: color-mix(in oklab, var(--muted) 45%, transparent);
  text-align: left;
}
.release-notes strong {
  display: block;
  font-size: 0.75rem;
  font-weight: 650;
}
.release-notes p {
  margin: 6px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.45;
  white-space: pre-wrap;
}
.status-card {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  margin-top: 18px;
  padding: 14px;
  border-radius: 16px;
  background: color-mix(in oklab, var(--muted) 55%, transparent);
  text-align: left;
}
.status-icon {
  display: grid;
  width: 34px;
  height: 34px;
  flex: none;
  place-items: center;
  border-radius: 10px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.status-card strong {
  display: block;
  font-size: 0.8125rem;
}
.status-card p {
  margin: 3px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.download-track-wrap {
  margin-top: 12px;
}
.download-track {
  overflow: hidden;
  height: 6px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--muted) 80%, transparent);
}
.download-track > span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--primary);
  transition: width 120ms ease;
}
.download-track > span.indeterminate {
  width: 36%;
  animation: progress-slide 1.1s ease-in-out infinite;
}
.action-error {
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: 12px;
  background: color-mix(in oklab, var(--destructive, #c44) 12%, transparent);
  text-align: left;
}
.action-error strong {
  display: block;
  font-size: 0.75rem;
}
.action-error p {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.72rem;
  line-height: 1.4;
}
.about-actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  margin-top: 18px;
}
.about-actions .pt-btn {
  width: 100%;
}
.about-actions:has(> :only-child) {
  grid-template-columns: 1fr;
}
.about-credit {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 6px;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  line-height: 1.4;
}
.about-credit .sep {
  opacity: 0.55;
}
.credit-link {
  border: 0;
  background: transparent;
  color: color-mix(in oklab, var(--primary) 78%, #6b8cff);
  font: inherit;
  cursor: pointer;
}
.credit-link:hover {
  text-decoration: underline;
}
.spin {
  animation: spin 0.9s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
@keyframes progress-slide {
  0% {
    transform: translateX(-120%);
  }
  100% {
    transform: translateX(320%);
  }
}

@media (max-width: 720px) {
  .setting-row {
    flex-wrap: wrap;
  }
  .row-control {
    width: 100%;
  }
  .row-link {
    margin-left: auto;
  }
  .about-actions {
    grid-template-columns: 1fr;
  }
}
</style>
