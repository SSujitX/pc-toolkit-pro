<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import { useCareStore } from '@/stores/care-store';
import PtCareHome from './pt-care-home.vue';
import PtCareRun from './pt-care-run.vue';
import PtCareResults from './pt-care-results.vue';
import PtCareSummary from './pt-care-summary.vue';
import PtStartupManager from './pt-startup-manager.vue';

type CareTab = 'scan' | 'startup';

const { t } = useI18n();
const store = useCareStore();
const tab = ref<CareTab>('scan');
const stopOpen = ref(false);
const fixOpen = ref(false);

const fixCategoryCount = computed(() => new Set(store.selectedItems.map((i) => i.category)).size);

function selectTab(next: CareTab) {
  if (store.busy) return;
  tab.value = next;
}

async function confirmFix() {
  await store.fix(false);
}
</script>

<template>
  <PtPageShell :title="t('care.engine.title')" :subtitle="t('care.engine.subtitle')" content-mode="workspace">
    <template #actions>
      <div class="tabs" role="tablist">
        <button
          v-for="id in ['scan', 'startup'] as const"
          :key="id"
          type="button"
          role="tab"
          class="tab"
          :aria-selected="tab === id"
          :data-active="tab === id"
          :disabled="store.busy && tab !== id"
          @click="selectTab(id)"
        >
          {{ t(`care.engine.tabs.${id}`) }}
        </button>
      </div>
    </template>

    <div v-if="tab === 'startup'" class="care-body">
      <PtStartupManager />
    </div>
    <div v-else class="care-body">
      <PtCareHome v-if="store.view === 'home'" />
      <PtCareRun v-else-if="store.view === 'scanning' || store.view === 'fixing'" @stop="stopOpen = true" />
      <PtCareResults v-else-if="store.view === 'results'" @fix="fixOpen = true" />
      <PtCareSummary v-else-if="store.view === 'summary'" />
    </div>

    <PtConfirmDialog
      v-model:open="stopOpen"
      :title="store.view === 'fixing' ? t('care.engine.scanning.stopFixTitle') : t('care.engine.scanning.stopTitle')"
      :message="store.view === 'fixing' ? t('care.engine.scanning.stopFixMessage') : t('care.engine.scanning.stopMessage')"
      :confirm-text="t('care.engine.scanning.stop')"
      :cancel-text="t('common.cancel')"
      destructive
      @confirm="store.stop()"
    />
    <PtConfirmDialog
      v-model:open="fixOpen"
      :title="t('care.engine.confirm.title')"
      :message="t('care.engine.confirm.message', { count: store.selectedItems.length, categories: fixCategoryCount }, store.selectedItems.length)"
      :confirm-text="t('care.engine.confirm.confirm')"
      :cancel-text="t('common.cancel')"
      destructive
      @confirm="confirmFix"
    />
  </PtPageShell>
</template>

<style scoped>
.care-body {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
}
.care-body > * {
  flex: 1;
  min-height: 0;
}
.tabs {
  display: inline-flex;
  gap: 2px;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 12px;
  background: var(--muted);
  padding: 3px;
}
.tab {
  min-height: 32px;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--muted-foreground);
  padding: 0 16px;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: background-color 0.12s ease, color 0.12s ease, box-shadow 0.12s ease;
}
.tab:hover:not(:disabled) {
  color: var(--foreground);
}
.tab[data-active='true'] {
  background: var(--card);
  color: var(--foreground);
  box-shadow: 0 1px 3px color-mix(in oklab, var(--foreground) 12%, transparent);
}
.tab:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
