<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  FolderOpen,
  HardDrive,
  MemoryStick,
  Paintbrush,
  Recycle,
  Trash2,
} from '@lucide/vue';
import type { Component } from 'vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import PtResultWorkspace from '@/components/custom/pt-result-workspace.vue';
import { formatBytes } from '@/lib/utils/format';
import type { CleanerCategory } from '@/lib/models/actions';
import { useDeepCleanerStore } from '@/stores/deep-cleaner-store';

const { t } = useI18n();
const store = useDeepCleanerStore();
const confirmOpen = ref(false);
const elapsedSec = ref(0);
const activeId = ref<CleanerCategory | null>(null);
let elapsedTimer: number | null = null;

const categoryIcons: Record<CleanerCategory, Component> = {
  tempFiles: Trash2,
  recycleBin: Recycle,
  diskCleanup: HardDrive,
  freeMemory: MemoryStick,
};

const isBusy = computed(() => store.loading);
const hasResults = computed(() => store.items.length > 0);
const selectedCount = computed(() => store.items.filter((i) => i.selected).length);
const selectedBytes = computed(() =>
  store.items.filter((i) => i.selected).reduce((n, i) => n + i.estimatedBytes, 0)
);
const foundCount = computed(() => store.items.reduce((n, i) => n + (i.itemCount || 1), 0));
const foundBytes = computed(() => store.items.reduce((n, i) => n + i.estimatedBytes, 0));
const allSelected = computed(
  () => store.items.length > 0 && store.items.every((i) => i.selected)
);
const activeItem = computed(
  () => store.items.find((i) => i.id === activeId.value) ?? store.items[0] ?? null
);

const progressPct = computed(() => {
  if (!store.progress?.total) return isBusy.value ? 18 : 0;
  return Math.round((store.progress.current / store.progress.total) * 100);
});

const isCleaning = computed(() => store.progress?.phase === 'executing');
const statusLabel = computed(() =>
  isCleaning.value ? t('deepCleaner.cleaning') : t('deepCleaner.analyzing')
);
const statusTitle = computed(() =>
  isCleaning.value
    ? store.progress?.message || t('deepCleaner.cleaningTitle')
    : t('deepCleaner.analyzingTitle')
);
const sourceValue = computed(
  () =>
    store.progress?.message ||
    (isCleaning.value ? t('deepCleaner.selected') : t('deepCleaner.preparing'))
);

const stats = computed(() => {
  if (isCleaning.value) {
    return [
      { label: t('deepCleaner.selected'), value: String(selectedCount.value) },
      { label: t('deepCleaner.estimated'), value: formatBytes(selectedBytes.value) },
      { label: t('deepCleaner.elapsed'), value: `${elapsedSec.value} sec` },
    ];
  }
  return [
    {
      label: t('deepCleaner.filesChecked'),
      value: store.progress ? String(store.progress.current) : '0',
    },
    {
      label: t('deepCleaner.dataScanned'),
      value: formatBytes(foundBytes.value || selectedBytes.value),
    },
    { label: t('deepCleaner.elapsed'), value: `${elapsedSec.value} sec` },
  ];
});

watch(
  () => store.items,
  (items) => {
    if (!items.length) {
      activeId.value = null;
      return;
    }
    if (!activeId.value || !items.some((i) => i.id === activeId.value)) {
      activeId.value = items[0]?.id ?? null;
    }
  },
  { immediate: true }
);

watch(isBusy, (busy) => {
  if (busy) {
    elapsedSec.value = 0;
    if (elapsedTimer != null) window.clearInterval(elapsedTimer);
    elapsedTimer = window.setInterval(() => {
      elapsedSec.value += 1;
    }, 1000);
  } else if (elapsedTimer != null) {
    window.clearInterval(elapsedTimer);
    elapsedTimer = null;
  }
});

onUnmounted(() => {
  if (elapsedTimer != null) window.clearInterval(elapsedTimer);
});

function requestClean() {
  confirmOpen.value = true;
}

function iconFor(id: CleanerCategory) {
  return categoryIcons[id] ?? Paintbrush;
}
</script>

<template>
  <PtPageShell
    :title="t('deepCleaner.title')"
    :subtitle="t('deepCleaner.subtitle')"
    content-mode="workspace"
  >
    <template #actions>
      <button
        v-if="hasResults && !isBusy"
        type="button"
        class="pt-btn"
        @click="store.scan()"
      >
        {{ t('deepCleaner.scanAgain') }}
      </button>
    </template>

    <PtOperationWorkspace
      v-if="isBusy"
      :status="statusLabel"
      :title="statusTitle"
      :source-label="t('deepCleaner.scanning')"
      :source-value="sourceValue"
      :source-icon="FolderOpen"
      :progress="progressPct"
      :stats="stats"
      :hint="t('deepCleaner.scanHint')"
      :icon="Paintbrush"
      :cancel-label="t('common.cancel')"
      @cancel="store.cancel()"
    />

    <div v-else-if="!hasResults" class="hero">
      <div class="hero-icon" aria-hidden="true">
        <Paintbrush :size="42" :stroke-width="1.6" />
      </div>
      <h2>{{ t('deepCleaner.findTitle') }}</h2>
      <p>{{ t('deepCleaner.findBody') }}</p>
      <button type="button" class="start-scan" @click="store.scan()">
        <Paintbrush :size="18" :stroke-width="2" />
        <span>{{ t('deepCleaner.startScan') }}</span>
      </button>
    </div>

    <PtResultWorkspace v-else>
      <div class="results">
        <header class="summary">
          <div class="summary-text">
            <strong>{{ t('deepCleaner.summaryCount', { count: foundCount }) }}</strong>
            <span>{{ t('deepCleaner.summarySpace') }}</span>
            <em>{{ formatBytes(foundBytes) }}</em>
          </div>
          <button
            type="button"
            class="pt-btn pt-btn-ghost"
            @click="store.selectAll(!allSelected)"
          >
            {{ allSelected ? t('deepCleaner.deselectAll') : t('deepCleaner.selectAll') }}
          </button>
        </header>

        <div class="browser">
          <nav class="categories" aria-label="Deep cleanup categories">
            <button
              v-for="item in store.items"
              :key="item.id"
              type="button"
              class="category-row"
              :class="{ active: activeItem?.id === item.id }"
              @click="activeId = item.id"
            >
              <span class="category-icon">
                <component :is="iconFor(item.id)" :size="18" :stroke-width="1.9" />
              </span>
              <span class="category-main">
                <strong>{{ t(item.titleKey) }}</strong>
                <small>{{ t('deepCleaner.categoryItems', { count: item.itemCount || 1 }) }}</small>
              </span>
              <strong class="category-size">{{ formatBytes(item.estimatedBytes) }}</strong>
            </button>
          </nav>

          <section v-if="activeItem" class="details">
            <header class="detail-header">
              <label class="select-row">
                <input
                  type="checkbox"
                  :checked="activeItem.selected"
                  @change="store.setSelected(activeItem.id, !activeItem.selected)"
                />
                <span>
                  <strong>{{ t(activeItem.titleKey) }}</strong>
                  <small>{{ t(activeItem.detailKey) }}</small>
                </span>
              </label>
              <div class="detail-meta">
                <span
                  class="risk"
                  :data-risk="activeItem.riskKey === 'deepCleaner.riskMedium' ? 'medium' : 'low'"
                >
                  {{ t(activeItem.riskKey) }}
                </span>
                <strong>{{ formatBytes(activeItem.estimatedBytes) }}</strong>
              </div>
            </header>

            <div class="detail-list">
              <label class="detail-row" :data-selected="activeItem.selected">
                <input
                  type="checkbox"
                  :checked="activeItem.selected"
                  @change="store.setSelected(activeItem.id, !activeItem.selected)"
                />
                <span class="detail-icon">
                  <component :is="iconFor(activeItem.id)" :size="18" :stroke-width="1.9" />
                </span>
                <span class="detail-main">
                  <strong>{{ t(activeItem.titleKey) }}</strong>
                  <small>{{ t(activeItem.detailKey) }}</small>
                </span>
                <strong>{{ formatBytes(activeItem.estimatedBytes) }}</strong>
              </label>
            </div>
          </section>
        </div>

        <footer class="action-bar">
          <div class="action-summary">
            <span>{{ t('deepCleaner.selected') }} {{ selectedCount }}</span>
            <div>
              <small>{{ t('deepCleaner.estimatedRelease') }}</small>
              <strong>{{ formatBytes(selectedBytes) }}</strong>
            </div>
          </div>
          <button
            type="button"
            class="clean-btn"
            :disabled="!selectedCount"
            @click="requestClean"
          >
            {{ t('deepCleaner.clean') }}
          </button>
        </footer>

        <div v-if="store.result" class="result-card">
          {{
            t('deepCleaner.result', {
              bytes: formatBytes(store.result.freedBytes),
              files: store.result.filesRemoved,
            })
          }}
          <ul>
            <li v-for="(line, idx) in store.log" :key="idx">{{ line }}</li>
          </ul>
        </div>
      </div>
    </PtResultWorkspace>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="t('deepCleaner.clean')"
      :message="t('deepCleaner.scanHint')"
      destructive
      @confirm="store.execute()"
    />
  </PtPageShell>
</template>

<style scoped>
.hero {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 24px;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 20px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  text-align: center;
}
.hero-icon {
  display: grid;
  width: 88px;
  height: 88px;
  place-items: center;
  margin-bottom: 8px;
  border: 2px solid color-mix(in oklab, var(--primary) 55%, transparent);
  border-radius: 999px;
  color: var(--primary);
}
.hero h2 {
  margin: 0;
  color: var(--foreground);
  font-size: 1.5rem;
  font-weight: 700;
  letter-spacing: -0.03em;
}
.hero p {
  margin: 0;
  max-width: 420px;
  color: var(--muted-foreground);
  font-size: 0.875rem;
  line-height: 1.45;
}
.start-scan {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  min-width: 200px;
  min-height: 48px;
  margin-top: 14px;
  padding: 0 28px;
  border: 0;
  border-radius: 14px;
  background: var(--primary);
  color: var(--primary-foreground);
  font-size: 0.9375rem;
  font-weight: 700;
  cursor: pointer;
  box-shadow: 0 10px 24px -14px color-mix(in oklab, var(--primary) 80%, transparent);
}
.start-scan:hover:not(:disabled) {
  filter: none;
  background: color-mix(in oklab, var(--primary) 86%, black);
  color: var(--primary-foreground);
}

.results {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  overflow: hidden;
}
.summary {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 48px;
  padding: 8px 16px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
}
.summary .pt-btn {
  flex: none;
  margin-inline-start: auto;
}
.summary-text {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
}
.summary-text strong {
  color: var(--foreground);
  font-size: 0.9375rem;
}
.summary-text span {
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.summary-text em {
  color: var(--primary);
  font-style: normal;
  font-size: 1.25rem;
  font-weight: 700;
  letter-spacing: -0.03em;
}

.browser {
  display: grid;
  min-height: 0;
  flex: 1;
  grid-template-columns: clamp(250px, 31%, 324px) minmax(0, 1fr);
  overflow: hidden;
}
.categories {
  display: flex;
  min-height: 0;
  flex-direction: column;
  gap: 2px;
  padding: 8px 6px;
  overflow: auto;
  border-right: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
}
.category-row {
  position: relative;
  display: grid;
  width: 100%;
  min-width: 0;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  align-items: center;
  gap: 9px;
  padding: 8px 10px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}
.category-row:hover {
  background: var(--surface-soft);
}
.category-row.active {
  background: color-mix(in oklab, var(--primary) 12%, transparent);
}
.category-row.active::before {
  position: absolute;
  top: 10px;
  bottom: 10px;
  left: 0;
  width: 2px;
  border-radius: 999px;
  background: var(--primary);
  content: '';
}
.category-icon {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border-radius: 8px;
  background: var(--surface-soft);
  color: var(--muted-foreground);
}
.category-row.active .category-icon {
  color: var(--primary);
}
.category-main {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}
.category-main strong {
  overflow: hidden;
  color: var(--foreground);
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.category-main small {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.category-size {
  color: var(--foreground);
  font-size: 0.8125rem;
  font-weight: 600;
}

.details {
  display: flex;
  min-height: 0;
  flex-direction: column;
  overflow: hidden;
}
.detail-header {
  display: flex;
  flex: none;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
}
.select-row {
  display: flex;
  min-width: 0;
  align-items: flex-start;
  gap: 10px;
  cursor: pointer;
}
.select-row strong {
  display: block;
  color: var(--foreground);
  font-size: 0.875rem;
}
.select-row small {
  display: block;
  margin-top: 2px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.detail-meta {
  display: flex;
  flex: none;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
}
.detail-meta strong {
  font-size: 0.9375rem;
}
.risk {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--primary) 14%, transparent);
  color: var(--primary);
  font-size: 0.6875rem;
  font-weight: 600;
}
.risk[data-risk='medium'] {
  background: color-mix(in oklab, var(--warning) 18%, transparent);
  color: var(--warning);
}

.detail-list {
  flex: 1;
  min-height: 0;
  padding: 8px;
  overflow: auto;
}
.detail-row {
  display: grid;
  grid-template-columns: 20px 28px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  min-height: var(--layout-result-row-height);
  padding: 8px 12px;
  border-radius: 12px;
  cursor: pointer;
}
.detail-row:hover,
.detail-row[data-selected='true'] {
  background: var(--surface-soft);
}
.detail-icon {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.detail-main {
  min-width: 0;
}
.detail-main strong {
  display: block;
  overflow: hidden;
  font-size: 0.8125rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.detail-main small {
  display: block;
  margin-top: 2px;
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.action-bar {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: var(--layout-action-bar-height);
  padding: 12px 16px;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  background: var(--surface-soft);
}
.action-summary {
  display: flex;
  align-items: center;
  gap: 20px;
}
.action-summary > span {
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.action-summary small {
  display: block;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.action-summary strong {
  display: block;
  margin-top: 2px;
  font-size: 1rem;
}
.clean-btn {
  min-width: 120px;
  min-height: 40px;
  padding: 0 22px;
  border: 0;
  border-radius: 12px;
  background: color-mix(in oklab, var(--warning) 88%, oklch(0.55 0.18 45));
  color: #fff;
  font-size: 0.875rem;
  font-weight: 700;
  cursor: pointer;
  box-shadow: 0 10px 22px -14px color-mix(in oklab, var(--warning) 80%, transparent);
}
.clean-btn:hover:not(:disabled) {
  filter: none;
  background: color-mix(in oklab, var(--warning) 78%, oklch(0.45 0.16 45));
  color: #fff;
}
.clean-btn:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.result-card {
  margin: 0;
  padding: 12px 16px;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.result-card ul {
  margin: 8px 0 0;
  padding-left: 18px;
}

.pt-btn-ghost {
  background: transparent;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
}

@media (max-width: 900px) {
  .summary {
    flex-wrap: wrap;
  }
  .summary .pt-btn {
    width: 100%;
  }
  .browser {
    grid-template-columns: minmax(0, 1fr);
  }
  .categories {
    max-height: 180px;
    border-right: 0;
    border-bottom: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  }
  .action-bar {
    flex-wrap: wrap;
  }
  .clean-btn {
    width: 100%;
  }
}
</style>
