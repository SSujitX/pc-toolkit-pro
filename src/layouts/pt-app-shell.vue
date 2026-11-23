<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useAppStore } from '@/stores/app-store';
import { useMemoryCleanerStore } from '@/stores/memory-cleaner-store';
import { useMonitorStore } from '@/stores/monitor-store';
import { PAGE_IDS, type PageId } from '@/lib/models/application-shell';
import PtSidebar from './components/pt-sidebar.vue';
import PtWindowTitlebar from './components/pt-window-titlebar.vue';
import PtErrorToast from './components/pt-error-toast.vue';
import { watchWindowVisibility } from '@/lib/services/window-visibility-service';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import MonitorPage from '@/pages/monitor/index.vue';

const CleanerPage = defineAsyncComponent(() => import('@/pages/cleaner/index.vue'));
const DeepCleanerPage = defineAsyncComponent(() => import('@/pages/deep-cleaner/index.vue'));
const MemoryCleanerPage = defineAsyncComponent(() => import('@/pages/memory-cleaner/index.vue'));
const CarePage = defineAsyncComponent(() => import('@/pages/care/index.vue'));
const UninstallerPage = defineAsyncComponent(() => import('@/pages/uninstaller/index.vue'));
const PowerPage = defineAsyncComponent(() => import('@/pages/power/index.vue'));
const InformationPage = defineAsyncComponent(() => import('@/pages/information/index.vue'));
const HistoryPage = defineAsyncComponent(() => import('@/pages/history/index.vue'));
const SettingsPage = defineAsyncComponent(() => import('@/pages/settings/index.vue'));

const { t } = useI18n();
const app = useAppStore();
const monitor = useMonitorStore();
const memory = useMemoryCleanerStore();

const pageMap: Partial<Record<PageId, unknown>> = {
  [PAGE_IDS.monitor]: MonitorPage,
  [PAGE_IDS.cleaner]: CleanerPage,
  [PAGE_IDS.deepCleaner]: DeepCleanerPage,
  [PAGE_IDS.memoryCleaner]: MemoryCleanerPage,
  [PAGE_IDS.care]: CarePage,
  [PAGE_IDS.uninstaller]: UninstallerPage,
  [PAGE_IDS.power]: PowerPage,
  [PAGE_IDS.information]: InformationPage,
  [PAGE_IDS.history]: HistoryPage,
  [PAGE_IDS.settings]: SettingsPage,
};

const current = computed(() => pageMap[app.currentPage] ?? MonitorPage);

const elevationPromptOpen = computed({
  get: () => memory.elevationPromptOpen,
  set: (open: boolean) => memory.setElevationPromptOpen(open),
});

let stopVisibilityWatch: (() => void) | null = null;
let unmounted = false;

onMounted(() => {
  // The shell holds both 1s polls for the titlebar; pages acquire/release their own share.
  monitor.acquirePolling();
  // Live titlebar RAM% shares Memory Cleaner physical stats (Task Manager / WMC).
  memory.acquireStatsPolling();
  // Hidden in the tray / minimized: pause the 1s polls (the tray keeps its own slow tick).
  void watchWindowVisibility((visible) => {
    monitor.setPaused(!visible);
    memory.setStatsPollingPaused(!visible);
  }).then((stop) => {
    if (unmounted) stop();
    else stopVisibilityWatch = stop;
  });
  void memory.loadElevation();
  // Idle preload secondary pages
  window.setTimeout(() => {
    void import('@/pages/cleaner/index.vue');
    void import('@/pages/deep-cleaner/index.vue');
    void import('@/pages/memory-cleaner/index.vue');
    void import('@/pages/care/index.vue');
    void import('@/pages/uninstaller/index.vue');
    void import('@/pages/power/index.vue');
    void import('@/pages/information/index.vue');
    void import('@/pages/history/index.vue');
    void import('@/pages/settings/index.vue');
  }, 1500);
});

onBeforeUnmount(() => {
  unmounted = true;
  stopVisibilityWatch?.();
  monitor.releasePolling();
  memory.releaseStatsPolling();
});

// The layout slot follows the pinned state only; a hover peek overlays the workspace
// (see `.sidebar.peeking`) so the page never shifts.
watch(
  () => app.sidebarPinned,
  (pinned) => {
    document.documentElement.style.setProperty(
      '--sidebar-width',
      pinned
        ? 'var(--layout-sidebar-expanded-width)'
        : 'var(--layout-sidebar-collapsed-width)'
    );
  },
  { immediate: true }
);
</script>

<template>
  <div class="shell">
    <div class="sidebar-slot">
      <PtSidebar
        :current-page="app.currentPage"
        :busy-pages="app.busyPages"
        :pinned="app.sidebarPinned"
        :peeking="app.sidebarPeeking"
        @navigate="app.navigate"
        @toggle-pin="app.toggleSidebarPin"
        @peek="app.setSidebarPeek"
      />
    </div>
    <main class="workspace">
      <PtWindowTitlebar />
      <component :is="current" />
    </main>

    <PtConfirmDialog
      v-model:open="elevationPromptOpen"
      :title="t('memoryCleaner.elevateTitle')"
      :message="t('memoryCleaner.elevateBody')"
      :confirm-text="t('memoryCleaner.elevateConfirm')"
      :cancel-text="t('memoryCleaner.elevateContinue')"
      @confirm="memory.confirmElevationRestart()"
      @cancel="memory.declineElevationContinue()"
      @dismiss="memory.dismissElevationPrompt()"
    />

    <PtErrorToast />
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  width: 100%;
  height: 100%;
  background: var(--background);
  color: var(--foreground);
}
.sidebar-slot {
  position: relative;
  z-index: 30;
  width: var(--sidebar-width);
  min-width: var(--sidebar-width);
  flex: none;
  transition:
    width var(--sidebar-transition-duration) var(--sidebar-transition-easing),
    min-width var(--sidebar-transition-duration) var(--sidebar-transition-easing);
}
.workspace {
  position: relative;
  flex: 1;
  min-width: 0;
  background: var(--workspace);
  transition: flex-basis var(--sidebar-transition-duration) var(--sidebar-transition-easing);
}
</style>
