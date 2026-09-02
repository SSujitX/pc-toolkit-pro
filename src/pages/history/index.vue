<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Brain,
  CheckCircle2,
  ChevronRight,
  History,
  Layers,
  Paintbrush,
  Power,
  Trash2,
  X,
  XCircle,
} from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import PtResultWorkspace from '@/components/custom/pt-result-workspace.vue';
import type { HistoryRecord } from '@/lib/models/history';
import { formatBytes } from '@/lib/utils/format';
import { useHistoryStore } from '@/stores/history-store';

const { t, locale } = useI18n();
const store = useHistoryStore();
const clearOpen = ref(false);
const selected = ref<HistoryRecord | null>(null);

const hasRecords = computed(() => store.records.length > 0);

onMounted(() => {
  void store.load();
});

function categoryIcon(category: string) {
  if (category === 'deepCleaner') return Layers;
  if (category === 'memoryCleaner') return Brain;
  if (category === 'power') return Power;
  if (category === 'cleaner') return Paintbrush;
  return History;
}

function formatWhen(ms: number) {
  try {
    return new Intl.DateTimeFormat(locale.value || undefined, {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(ms));
  } catch {
    return new Date(ms).toLocaleString();
  }
}

function durationSec(record: HistoryRecord) {
  const ms = Math.max(0, record.finishedAtMs - record.startedAtMs);
  return Math.max(1, Math.round(ms / 1000));
}

function plannedValue(record: HistoryRecord) {
  if (record.plannedBytes != null) return formatBytes(record.plannedBytes);
  if (record.selectedItemCount > 0) return String(record.selectedItemCount);
  return t('history.notCounted');
}

function resultValue(record: HistoryRecord) {
  if (record.resultBytes != null) return formatBytes(record.resultBytes);
  if (record.affectedItemCount > 0) return String(record.affectedItemCount);
  return t('history.notCounted');
}

function openDetails(record: HistoryRecord) {
  selected.value = record;
}

function closeDetails() {
  selected.value = null;
}

async function confirmClear() {
  await store.clear();
}
</script>

<template>
  <PtPageShell
    :title="t('history.title')"
    :subtitle="t('history.subtitle')"
    content-mode="workspace"
  >
    <template #actions>
      <button
        v-if="hasRecords"
        type="button"
        class="pt-btn"
        :disabled="store.loading"
        @click="clearOpen = true"
      >
        <Trash2 :size="16" :stroke-width="2" />
        {{ t('history.clear') }}
      </button>
    </template>

    <PtResultWorkspace>
      <div v-if="!hasRecords && !store.loading" class="empty">
        <div class="empty-icon" aria-hidden="true">
          <History :size="28" :stroke-width="1.7" />
        </div>
        <strong>{{ t('history.emptyTitle') }}</strong>
        <p>{{ t('history.emptyBody') }}</p>
      </div>

      <div v-else class="table-wrap">
        <div class="table-head" aria-hidden="true">
          <span />
          <span>{{ t('history.activity') }}</span>
          <span class="num">{{ t('history.planned') }}</span>
          <span class="num">{{ t('history.result') }}</span>
          <span />
        </div>

        <div class="table-body">
          <button
            v-for="record in store.records"
            :key="record.id"
            type="button"
            class="row"
            @click="openDetails(record)"
          >
              <span class="row-icon" :data-category="record.category">
              <component :is="categoryIcon(record.category)" :size="18" :stroke-width="1.9" />
            </span>
            <span class="row-main">
              <span class="row-title">
                <strong>{{ t(record.titleKey) }}</strong>
                <em v-if="record.failedItemCount > 0" class="warn">{{
                  t('history.statusWarnings')
                }}</em>
                <em v-else-if="record.outcome === 'cancelled'" class="muted-pill">{{
                  t('history.statusCancelled')
                }}</em>
              </span>
              <small>
                {{ formatWhen(record.startedAtMs) }} · {{ record.summary }}
              </small>
            </span>
            <strong class="num">{{ plannedValue(record) }}</strong>
            <strong class="num">{{ resultValue(record) }}</strong>
            <span class="chevron" aria-hidden="true">
              <ChevronRight :size="16" :stroke-width="2" />
            </span>
          </button>
        </div>
      </div>
    </PtResultWorkspace>

    <div v-if="selected" class="detail-root" role="dialog" aria-modal="true">
      <div class="detail-overlay" @click="closeDetails" />
      <div class="detail-panel">
        <header class="detail-header">
          <div>
            <h2>{{ t('history.detailTitle') }}</h2>
            <p>{{ t('history.detailBody') }}</p>
          </div>
          <button type="button" class="icon-close" :aria-label="t('common.cancel')" @click="closeDetails">
            <X :size="18" />
          </button>
        </header>

        <div class="metrics">
          <div class="metric">
            <small>{{ t('history.selectedItems') }}</small>
            <strong>{{ selected.selectedItemCount }}</strong>
          </div>
          <div class="metric">
            <small>{{ t('history.changedItems') }}</small>
            <strong>{{ selected.affectedItemCount }}</strong>
          </div>
          <div class="metric">
            <small>{{ t('history.failedItems') }}</small>
            <strong>{{ selected.failedItemCount }}</strong>
          </div>
          <div v-if="selected.resultBytes != null" class="metric">
            <small>{{ t('history.released') }}</small>
            <strong>{{ formatBytes(selected.resultBytes) }}</strong>
          </div>
        </div>

        <div class="meta">
          <span>{{ formatWhen(selected.startedAtMs) }}</span>
          <span>{{ t('history.duration', { seconds: durationSec(selected) }) }}</span>
        </div>

        <section class="detail-list">
          <header>
            <strong>{{ t(selected.titleKey) }}</strong>
            <span class="outcome" :data-outcome="selected.outcome">
              <CheckCircle2 v-if="selected.success" :size="14" />
              <XCircle v-else :size="14" />
              {{ t(`history.outcomes.${selected.outcome}`) }}
            </span>
          </header>
          <ul v-if="selected.detailLines.length">
            <li v-for="(line, idx) in selected.detailLines" :key="idx">{{ line }}</li>
          </ul>
          <p v-else class="muted">{{ selected.summary }}</p>
        </section>

        <footer class="detail-footer">
          <button type="button" class="pt-btn" @click="closeDetails">
            {{ t('common.close') }}
          </button>
        </footer>
      </div>
    </div>

    <PtConfirmDialog
      v-model:open="clearOpen"
      :title="t('history.clearTitle')"
      :message="t('history.clearBody')"
      :confirm-text="t('history.clear')"
      destructive
      @confirm="confirmClear"
    />
  </PtPageShell>
</template>

<style scoped>
.empty {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 28px;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  text-align: center;
  box-shadow: var(--shadow-card);
}
.empty-icon {
  display: grid;
  width: 64px;
  height: 64px;
  place-items: center;
  margin-bottom: 4px;
  border-radius: 16px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.empty strong {
  font-size: 1rem;
}
.empty p {
  margin: 0;
  max-width: 36ch;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}

.table-wrap {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  overflow: hidden;
  box-shadow: var(--shadow-card);
}
.table-head,
.row {
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr) 96px 96px 18px;
  align-items: center;
  gap: 10px;
}
.table-head {
  flex: none;
  min-height: 36px;
  padding: 0 14px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 650;
  letter-spacing: 0.02em;
}
.table-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 6px;
}
.row {
  width: 100%;
  min-height: 56px;
  margin: 0;
  padding: 8px 10px;
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}
.row:hover {
  background: var(--surface-soft);
  border-color: color-mix(in oklab, var(--border) 55%, transparent);
}
.row-icon {
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  border-radius: 9px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.row-icon[data-category='power'],
.row-icon[data-category='memoryCleaner'] {
  background: color-mix(in oklab, var(--warning) 16%, transparent);
  color: var(--warning);
}
.row-main {
  min-width: 0;
}
.row-title {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.row-title strong {
  font-size: 0.8125rem;
  font-weight: 650;
}
.row-main small {
  display: block;
  margin-top: 2px;
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.warn,
.muted-pill {
  display: inline-flex;
  align-items: center;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: 0.625rem;
  font-style: normal;
  font-weight: 650;
}
.warn {
  background: color-mix(in oklab, var(--warning) 16%, transparent);
  color: var(--warning);
}
.muted-pill {
  background: var(--surface-soft);
  color: var(--muted-foreground);
}
.num {
  text-align: right;
  font-size: 0.8125rem;
  font-variant-numeric: tabular-nums;
}
.chevron {
  display: grid;
  place-items: center;
  color: var(--muted-foreground);
}

.detail-root {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: grid;
  place-items: center;
  padding: 24px;
}
.detail-overlay {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 28%);
}
.detail-panel {
  position: relative;
  z-index: 1;
  display: flex;
  width: min(640px, 100%);
  max-height: min(780px, 92vh);
  flex-direction: column;
  overflow: hidden;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 16px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.detail-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 18px 18px 10px;
}
.detail-header h2 {
  margin: 0;
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.03em;
}
.detail-header p {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.icon-close {
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
.icon-close:hover {
  background: var(--surface-soft);
  color: var(--foreground);
}
.metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
  padding: 0 18px 12px;
}
.metric {
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--surface-soft);
}
.metric small {
  display: block;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 650;
}
.metric strong {
  display: block;
  margin-top: 4px;
  font-size: 1rem;
  font-variant-numeric: tabular-nums;
}
.meta {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 0 18px 12px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.detail-list {
  min-height: 0;
  flex: 1;
  margin: 0 18px;
  padding: 12px;
  overflow: auto;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 12px;
}
.detail-list header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 10px;
}
.outcome {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 650;
}
.outcome[data-outcome='cancelled'],
.outcome[data-outcome='failed'] {
  color: var(--warning);
}
.detail-list ul {
  margin: 0;
  padding-left: 18px;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.detail-list .muted {
  margin: 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.detail-footer {
  display: flex;
  justify-content: flex-end;
  padding: 14px 18px 18px;
}

@media (max-width: 720px) {
  .table-head,
  .row {
    grid-template-columns: 30px minmax(0, 1fr) 18px;
  }
  .table-head .num,
  .row .num {
    display: none;
  }
  .metrics {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
