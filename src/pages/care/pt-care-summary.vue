<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Check, CircleSlash, TriangleAlert, Undo2 } from '@lucide/vue';
import type { CareCategoryId, CareFixResult, CareItem } from '@/lib/models/care';
import { formatBytes } from '@/lib/utils/format';
import { useCareStore } from '@/stores/care-store';
import PtCareRing from './pt-care-ring.vue';
import { categoryName, formatElapsed, itemTitle, reasonText } from './care-ui';

const { t, te } = useI18n();
const store = useCareStore();
const report = computed(() => store.fixReport);

const tiles = computed(() => {
  const r = report.value;
  if (!r) return [];
  return [
    { key: 'fixed', value: r.fixed.toLocaleString(), tone: 'success' },
    { key: 'partial', value: r.partial.toLocaleString(), tone: 'warning' },
    { key: 'failed', value: r.failed.toLocaleString(), tone: 'danger' },
    { key: 'skipped', value: r.skipped.toLocaleString(), tone: 'default' },
    { key: 'freed', value: formatBytes(r.freedBytes), tone: 'primary' },
  ];
});

/** Items that did not get fully fixed, grouped by category in display order. */
const problems = computed(() => {
  const r = report.value;
  if (!r) return [] as { category: CareCategoryId; rows: { result: CareFixResult; title: string }[] }[];
  const items = new Map<string, CareItem>();
  for (const scan of store.scans) for (const item of scan.items) items.set(`${item.category}:${item.id}`, item);
  const groups = new Map<CareCategoryId, { result: CareFixResult; title: string }[]>();
  for (const result of r.results) {
    if (result.status === 'fixed') continue;
    const item = items.get(`${result.category}:${result.itemId}`);
    const rows = groups.get(result.category) ?? [];
    rows.push({ result, title: item ? itemTitle(item, t, te) : result.itemId });
    groups.set(result.category, rows);
  }
  return [...groups].map(([category, rows]) => ({ category, rows }));
});
</script>

<template>
  <div v-if="report" class="care-summary">
    <section class="hero">
      <PtCareRing :percent="100" :size="132" :thickness="9">
        <Check v-if="!report.stopped && report.failed === 0" :size="40" :stroke-width="2.25" class="hero-check" />
        <TriangleAlert v-else :size="34" :stroke-width="2" class="hero-warn" />
      </PtCareRing>
      <div class="hero-copy">
        <h2>{{ report.stopped ? t('care.engine.summary.stoppedTitle') : t('care.engine.summary.title') }}</h2>
        <span>{{ t('care.engine.summary.elapsed', { time: formatElapsed(report.elapsedMs) }) }}</span>
        <p v-if="report.undoable > 0" class="hint">
          <Undo2 :size="14" :stroke-width="2" />
          {{ t('care.engine.summary.undoHint', { count: report.undoable }, report.undoable) }}
          <button v-if="!store.undoOutcome" type="button" class="link-btn" @click="store.undoLastFix()">
            {{ t('care.engine.summary.undoNow') }}
          </button>
          <span v-else>
            {{ t('care.engine.summary.undoDone', { restored: store.undoOutcome.restored, skipped: store.undoOutcome.skipped }) }}
          </span>
        </p>
        <p v-if="store.afterActionCountdown !== null" class="countdown">
          <TriangleAlert :size="14" :stroke-width="2" />
          {{ t(`care.engine.summary.afterAction.${report.afterAction}`, { seconds: store.afterActionCountdown }) }}
          <button type="button" class="pt-btn pt-btn-primary small" @click="store.cancelAfterAction()">
            {{ t('care.engine.summary.cancelAfterAction') }}
          </button>
        </p>
        <p v-if="report.afterActionError" class="hint hint-warn">
          <TriangleAlert :size="14" :stroke-width="2" />
          {{ t('care.engine.summary.afterActionFailed') }}
        </p>
      </div>
    </section>

    <div class="tiles">
      <div v-for="tile in tiles" :key="tile.key" class="tile" :data-tone="tile.tone">
        <span>{{ t(`care.engine.summary.${tile.key}`) }}</span>
        <strong>{{ tile.value }}</strong>
      </div>
    </div>

    <div v-if="problems.length" class="problems">
      <section v-for="group in problems" :key="group.category" class="problem-group">
        <h3>{{ categoryName(group.category, t) }}</h3>
        <ul>
          <li v-for="row in group.rows" :key="row.result.itemId" :data-status="row.result.status">
            <CircleSlash v-if="row.result.status === 'skipped'" :size="14" :stroke-width="2" />
            <TriangleAlert v-else :size="14" :stroke-width="2" />
            <span class="p-title">{{ row.title }}</span>
            <span class="p-reason">{{ reasonText(row.result.reason, t, te) || t(`care.engine.summary.${row.result.status}`) }}</span>
          </li>
        </ul>
      </section>
    </div>

    <footer class="action-bar">
      <button type="button" class="pt-btn pt-btn-ghost" @click="store.showResults()">
        {{ t('care.engine.summary.backToResults') }}
      </button>
      <button type="button" class="pt-btn pt-btn-primary" @click="store.backToHome()">
        {{ t('care.engine.summary.done') }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.care-summary {
  display: flex;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  gap: 14px;
  animation: care-fade 220ms ease-out;
}
.hero {
  display: flex;
  align-items: center;
  gap: 20px;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  padding: 18px 22px;
}
.hero-check {
  color: var(--success);
}
.hero-warn {
  color: var(--warning);
}
.hero-copy h2 {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.hero-copy > span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 8px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.hint-warn {
  color: var(--warning);
}
.link-btn {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 650;
  cursor: pointer;
}
.link-btn:hover {
  text-decoration: underline;
}
.countdown {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin: 10px 0 0;
  padding: 8px 10px;
  border: 1px solid color-mix(in oklab, var(--warning) 45%, var(--border));
  border-radius: 10px;
  background: color-mix(in oklab, var(--warning) 9%, var(--card));
  color: var(--foreground);
  font-size: 0.8125rem;
  font-variant-numeric: tabular-nums;
}
.countdown svg {
  color: var(--warning);
}
.pt-btn.small {
  min-height: 28px;
  padding: 0 10px;
  font-size: 0.75rem;
}
.tiles {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 10px;
}
.tile {
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 12px;
  background: var(--card);
  padding: 12px 14px;
}
.tile span {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 600;
}
.tile strong {
  font-size: 1.25rem;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.tile[data-tone='success'] strong {
  color: var(--success);
}
.tile[data-tone='warning'] strong {
  color: var(--warning);
}
.tile[data-tone='danger'] strong {
  color: var(--destructive);
}
.tile[data-tone='primary'] strong {
  color: var(--primary);
}
.problems {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
}
.problem-group {
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 12px;
  background: var(--card);
  padding: 10px 12px;
}
.problem-group h3 {
  margin: 0 0 6px;
  font-size: 0.8125rem;
  font-weight: 650;
}
.problem-group ul {
  margin: 0;
  padding: 0;
  list-style: none;
}
.problem-group li {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 32px;
  font-size: 0.8125rem;
  color: var(--muted-foreground);
}
.problem-group li[data-status='failed'],
.problem-group li[data-status='partial'] {
  color: var(--warning);
}
.p-title {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  color: var(--foreground);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.p-reason {
  flex: none;
  font-size: 0.75rem;
}
.action-bar {
  display: flex;
  flex: none;
  justify-content: flex-end;
  gap: 10px;
  margin-top: auto;
  min-height: var(--layout-action-bar-height, 56px);
  align-items: center;
}
@media (max-width: 860px) {
  .tiles {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
@keyframes care-fade {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
@media (prefers-reduced-motion: reduce) {
  .care-summary {
    animation: none;
  }
}
</style>
