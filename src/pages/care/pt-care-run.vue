<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Ban, Check, CircleSlash, Info, Minus, SkipForward, Square, TriangleAlert } from '@lucide/vue';
import type { CareCategoryProgress } from '@/lib/models/care';
import { useCareStore } from '@/stores/care-store';
import PtCareRing from './pt-care-ring.vue';
import { CARE_CATEGORY_ICONS, categoryName, formatElapsed, middleEllipsis } from './care-ui';

const emit = defineEmits<{ stop: [] }>();
const { t } = useI18n();
const store = useCareStore();

const fixing = computed(() => store.view === 'fixing');
const rows = computed<CareCategoryProgress[]>(() => store.progress?.categories ?? []);
const percent = computed(() => store.progress?.percent ?? 0);
const focusCategory = computed(() => store.progress?.currentCategory ?? store.runningCategory);

const title = computed(() => {
  const cat = focusCategory.value;
  if (!cat) {
    if (store.progress?.phase === 'complete') return t('care.engine.scanning.finishing');
    return fixing.value ? t('care.engine.scanning.fixingPreparing') : t('care.engine.scanning.preparing');
  }
  const key = fixing.value ? 'care.engine.scanning.fixingTitle' : 'care.engine.scanning.title';
  return t(key, { category: categoryName(cat, t) });
});

const currentText = computed(() => middleEllipsis(store.progress?.currentText ?? '', 84));
const skipDisabled = computed(
  () => !store.runningCategory || store.skipRequested.includes(store.runningCategory) || store.stopping
);

type Mark = 'spinner' | 'count' | 'clean' | 'attention' | 'na' | 'skipped' | 'failed' | 'queued';

function markOf(row: CareCategoryProgress): Mark {
  if (row.status === 'queued') return 'queued';
  if (row.status === 'running') return 'spinner';
  switch (row.state) {
    case 'issues':
      return 'count';
    case 'clean':
      return 'clean';
    case 'attention':
      return 'attention';
    case 'notApplicable':
      return 'na';
    case 'skipped':
      return 'skipped';
    case 'failed':
      return 'failed';
    default:
      return 'clean';
  }
}
</script>

<template>
  <div class="care-run">
    <aside class="rail" :aria-label="t('care.engine.home.categoriesTitle')">
      <ul>
        <li v-for="row in rows" :key="row.category" class="rail-row" :data-status="row.status">
          <span class="rail-icon" aria-hidden="true">
            <component :is="CARE_CATEGORY_ICONS[row.category]" :size="15" :stroke-width="1.75" />
          </span>
          <span class="rail-name">{{ categoryName(row.category, t) }}</span>
          <span class="rail-mark" :data-mark="markOf(row)">
            <template v-if="markOf(row) === 'spinner'">
              <span class="mini-spin" aria-hidden="true" />
              <span v-if="row.count > 0" class="live-count">{{ row.count.toLocaleString() }}</span>
            </template>
            <span v-else-if="markOf(row) === 'count'" class="count-badge">{{ row.count.toLocaleString() }}</span>
            <Check v-else-if="markOf(row) === 'clean'" :size="15" :stroke-width="2.25" :aria-label="t('care.engine.states.clean')" />
            <Info v-else-if="markOf(row) === 'attention'" :size="15" :stroke-width="2" :aria-label="t('care.engine.states.attention')" />
            <Minus v-else-if="markOf(row) === 'na'" :size="15" :stroke-width="2" :aria-label="t('care.engine.states.notApplicable')" />
            <CircleSlash v-else-if="markOf(row) === 'skipped'" :size="14" :stroke-width="2" :aria-label="t('care.engine.states.skipped')" />
            <TriangleAlert v-else-if="markOf(row) === 'failed'" :size="14" :stroke-width="2" :aria-label="t('care.engine.states.failed')" />
            <span v-else class="queued-dot" :title="t('care.engine.scanning.queued')" />
          </span>
        </li>
      </ul>
    </aside>

    <section class="stage">
      <PtCareRing :percent="percent" :size="208" :thickness="12" spinning>
        <div class="ring-copy">
          <strong>{{ Math.floor(percent) }}<small>%</small></strong>
          <span>{{ t('care.engine.scanning.elapsed', { time: formatElapsed(store.progress?.elapsedMs ?? 0) }) }}</span>
        </div>
      </PtCareRing>

      <h2 class="stage-title">{{ title }}</h2>

      <div class="current">
        <span class="current-label">{{ t('care.engine.scanning.current') }}</span>
        <code :title="store.progress?.currentText ?? ''">{{ currentText || t('care.engine.scanning.currentIdle') }}</code>
      </div>

      <div class="controls">
        <button type="button" class="pt-btn pt-btn-ghost" :disabled="skipDisabled" @click="store.skip()">
          <SkipForward :size="16" :stroke-width="2" />
          {{ t('care.engine.scanning.skip') }}
        </button>
        <button type="button" class="pt-btn" :disabled="store.stopping" @click="emit('stop')">
          <Ban v-if="store.stopping" :size="16" :stroke-width="2" />
          <Square v-else :size="14" :stroke-width="2.25" />
          {{ store.stopping ? t('care.engine.scanning.stopping') : t('care.engine.scanning.stop') }}
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.care-run {
  display: grid;
  grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
  gap: 16px;
  height: 100%;
  min-height: 0;
  animation: care-fade 200ms ease-out;
}
.rail {
  min-height: 0;
  overflow: auto;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  padding: 6px;
}
.rail ul {
  margin: 0;
  padding: 0;
  list-style: none;
}
.rail-row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 40px;
  padding: 0 10px;
  border-radius: 10px;
  font-size: 0.8125rem;
  color: var(--muted-foreground);
  transition: background-color 0.2s ease, color 0.2s ease;
}
.rail-row[data-status='running'] {
  background: color-mix(in oklab, var(--primary) 8%, transparent);
  color: var(--foreground);
  font-weight: 600;
}
.rail-row[data-status='done'] {
  color: var(--foreground);
}
.rail-icon {
  display: grid;
  width: 24px;
  height: 24px;
  flex: none;
  place-items: center;
  border-radius: 7px;
  background: color-mix(in oklab, var(--primary) 10%, transparent);
  color: var(--primary);
}
.rail-name {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* Fixed-width mark column so state changes never shift the row. */
.rail-mark {
  display: inline-flex;
  min-width: 56px;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
  font-variant-numeric: tabular-nums;
}
.rail-mark[data-mark='clean'] {
  color: var(--success);
}
.rail-mark[data-mark='attention'] {
  color: var(--warning);
}
.rail-mark[data-mark='failed'] {
  color: var(--destructive);
}
.rail-mark[data-mark='na'],
.rail-mark[data-mark='skipped'] {
  color: var(--muted-foreground);
}
.count-badge {
  display: inline-flex;
  align-items: center;
  height: 20px;
  padding: 0 7px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--primary) 16%, transparent);
  color: var(--primary);
  font-size: 0.6875rem;
  font-weight: 700;
  animation: care-pop-in 180ms ease-out;
}
.live-count {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.mini-spin {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: conic-gradient(from 0deg, var(--primary) 0deg, transparent 260deg);
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px));
  mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px));
  animation: care-spin 0.75s linear infinite;
}
.queued-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: color-mix(in oklab, var(--muted-foreground) 45%, transparent);
}

.stage {
  display: flex;
  min-height: 0;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  padding: 24px;
  contain: layout style;
}
.ring-copy {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}
.ring-copy strong {
  font-size: 2.5rem;
  font-weight: 750;
  letter-spacing: -0.04em;
  line-height: 1;
  font-variant-numeric: tabular-nums;
}
.ring-copy small {
  margin-left: 2px;
  font-size: 1rem;
  font-weight: 650;
  color: var(--muted-foreground);
}
.ring-copy span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
}
.stage-title {
  margin: 6px 0 0;
  min-height: 1.5rem;
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  text-align: center;
}
.current {
  display: flex;
  width: min(640px, 100%);
  flex-direction: column;
  gap: 3px;
  border-radius: 12px;
  background: var(--surface-soft, var(--muted));
  padding: 10px 14px;
  overflow: hidden;
}
.current-label {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 600;
}
.current code {
  display: block;
  height: 1.25em;
  overflow: hidden;
  color: var(--foreground);
  font-family: ui-monospace, 'Cascadia Code', Consolas, monospace;
  font-size: 0.75rem;
  line-height: 1.25;
  white-space: nowrap;
}
.controls {
  display: flex;
  gap: 10px;
  margin-top: 4px;
}

@keyframes care-spin {
  from {
    rotate: 0deg;
  }
  to {
    rotate: 360deg;
  }
}
@keyframes care-pop-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
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
@media (max-width: 860px) {
  .care-run {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
  }
  .rail {
    max-height: 180px;
  }
}
@media (prefers-reduced-motion: reduce) {
  .care-run,
  .count-badge {
    animation: none;
  }
  .mini-spin {
    animation-duration: 2.5s;
  }
}
</style>
