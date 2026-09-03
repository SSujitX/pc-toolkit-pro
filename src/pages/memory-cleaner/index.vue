<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Brain,
  Check,
  Clock3,
  Gauge,
  HardDrive,
  MemoryStick,
} from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import PtSoftSelect from '@/components/custom/pt-soft-select.vue';
import {
  formatIntervalLabel,
  MEMORY_AREA_IDS,
  type MemoryAreaId,
} from '@/lib/models/memory-cleaner';
import { formatBytes } from '@/lib/utils/format';
import { useMemoryCleanerStore } from '@/stores/memory-cleaner-store';

const { t } = useI18n();
const store = useMemoryCleanerStore();
const confirmOpen = ref(false);
const elapsedSec = ref(0);
let elapsedTimer: number | null = null;

const isBusy = computed(() => store.loading);
const progressPct = computed(() => {
  if (!store.progress?.total) return isBusy.value ? 28 : 0;
  return Math.round((store.progress.current / store.progress.total) * 100);
});
const sourceValue = computed(() => {
  const area = store.progress?.area;
  if (area) return t(`memoryCleaner.areas.${area}`);
  return store.progress?.message || t('memoryCleaner.preparing');
});
const statsRows = computed(() => [
  { label: t('memoryCleaner.elapsed'), value: `${elapsedSec.value} sec` },
]);

const physical = computed(() => store.stats);
const freePhysPct = computed(() => {
  const s = physical.value;
  if (!s || s.physicalTotal <= 0) return 0;
  return (s.physicalAvail / s.physicalTotal) * 100;
});
const selectedCount = computed(() => store.selectedAreas.length);
const thresholdLabel = computed(() => {
  const pct = store.settings.autoFreeBelowPercent;
  if (pct === 0) return t('memoryCleaner.autoOff');
  return t('memoryCleaner.autoBelow', { percent: pct });
});
const intervalOptions = computed(() =>
  store.intervalSteps.map((step) => ({
    value: step,
    label: step === 0 ? t('memoryCleaner.autoOff') : formatIntervalLabel(step),
  }))
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

onMounted(async () => {
  if (!store.settingsLoaded) await store.loadSettings();
  if (!store.elevationLoaded) await store.loadElevation();
  store.startStatsPolling();
});

onUnmounted(() => {
  store.stopStatsPolling();
  if (elapsedTimer != null) window.clearInterval(elapsedTimer);
});

function toggleArea(id: MemoryAreaId) {
  store.setArea(id, !store.settings.areas[id]);
}

function selectRecommended() {
  const areas: Record<string, boolean> = {};
  for (const id of MEMORY_AREA_IDS) {
    areas[id] = id !== 'standbyListLowPriority';
  }
  store.setAreas(areas);
}

function clearAreas() {
  const areas: Record<string, boolean> = {};
  for (const id of MEMORY_AREA_IDS) {
    areas[id] = false;
  }
  store.setAreas(areas);
}

function areaDetail(status: string, detail?: string | null) {
  if (detail === 'skippedNeedAdmin') return t('memoryCleaner.outcome.skippedNeedAdmin');
  if (detail === 'skippedUnsupportedOs') return t('memoryCleaner.outcome.skippedUnsupportedOs');
  if (detail === 'cancelled') return t('memoryCleaner.outcome.cancelled');
  if (status === 'ok') return t('memoryCleaner.outcome.ok');
  if (status === 'failed') return t('memoryCleaner.outcome.failed');
  return t('memoryCleaner.outcome.skipped');
}
</script>

<template>
  <PtPageShell
    :title="t('memoryCleaner.title')"
    :subtitle="t('memoryCleaner.subtitle')"
    content-mode="workspace"
  >
    <template #actions>
      <button
        type="button"
        class="pt-btn pt-btn-primary"
        :disabled="isBusy || selectedCount === 0"
        @click="confirmOpen = true"
      >
        {{ t('memoryCleaner.run') }}
      </button>
    </template>

    <PtOperationWorkspace
      v-if="isBusy"
      :status="t('memoryCleaner.cleaning')"
      :title="t('memoryCleaner.cleaningTitle')"
      :source-label="t('memoryCleaner.title')"
      :source-value="sourceValue"
      :source-icon="Brain"
      :progress="progressPct"
      :stats="statsRows"
      :hint="t('memoryCleaner.cancelHint')"
      :icon="Brain"
      :cancel-label="t('common.cancel')"
      @cancel="store.cancel()"
    />

    <div v-else class="workspace">
      <div class="meters">
        <section class="meter-card">
          <header>
            <span class="meter-icon" aria-hidden="true">
              <MemoryStick :size="16" :stroke-width="1.75" />
            </span>
            <div>
              <strong>{{ t('memoryCleaner.physical') }}</strong>
              <span v-if="physical">{{ formatBytes(physical.physicalTotal) }}</span>
            </div>
            <em class="meter-pct">{{ (physical?.physicalLoadPercent ?? 0).toFixed(0) }}%</em>
          </header>
          <div class="bar" aria-hidden="true">
            <span class="used" :style="{ width: `${physical?.physicalLoadPercent ?? 0}%` }" />
            <span class="free" :style="{ width: `${freePhysPct}%` }" />
          </div>
          <div class="meter-meta">
            <span>
              {{ t('memoryCleaner.used') }}
              <strong>{{ formatBytes(physical?.physicalUsed ?? 0) }}</strong>
            </span>
            <span>
              {{ t('memoryCleaner.free') }}
              <strong>{{ formatBytes(physical?.physicalAvail ?? 0) }}</strong>
            </span>
          </div>
        </section>

        <section class="meter-card">
          <header>
            <span class="meter-icon" aria-hidden="true">
              <HardDrive :size="16" :stroke-width="1.75" />
            </span>
            <div>
              <strong>{{ t('memoryCleaner.virtual') }}</strong>
              <span v-if="physical">{{ formatBytes(physical.virtualTotal) }}</span>
            </div>
            <em class="meter-pct">{{ (physical?.virtualLoadPercent ?? 0).toFixed(0) }}%</em>
          </header>
          <div class="bar" aria-hidden="true">
            <span class="used" :style="{ width: `${physical?.virtualLoadPercent ?? 0}%` }" />
            <span
              class="free"
              :style="{
                width: `${
                  physical && physical.virtualTotal
                    ? (physical.virtualAvail / physical.virtualTotal) * 100
                    : 0
                }%`,
              }"
            />
          </div>
          <div class="meter-meta">
            <span>
              {{ t('memoryCleaner.used') }}
              <strong>{{ formatBytes(physical?.virtualUsed ?? 0) }}</strong>
            </span>
            <span>
              {{ t('memoryCleaner.free') }}
              <strong>{{ formatBytes(physical?.virtualAvail ?? 0) }}</strong>
            </span>
          </div>
        </section>
      </div>

      <section class="panel">
        <header class="panel-head">
          <div class="panel-copy">
            <h2>{{ t('memoryCleaner.areasTitle') }}</h2>
            <p>{{ t('memoryCleaner.areasBody') }}</p>
          </div>
          <div class="panel-actions">
            <span class="count-pill">
              {{ t('memoryCleaner.selectedCount', { count: selectedCount }) }}
            </span>
            <button type="button" class="text-btn" @click="selectRecommended">
              {{ t('memoryCleaner.selectRecommended') }}
            </button>
            <button type="button" class="text-btn" @click="clearAreas">
              {{ t('memoryCleaner.clearAreas') }}
            </button>
          </div>
        </header>

        <div class="area-grid" role="group" :aria-label="t('memoryCleaner.areasTitle')">
          <button
            v-for="id in MEMORY_AREA_IDS"
            :key="id"
            type="button"
            class="area-tile"
            :class="{ on: !!store.settings.areas[id] }"
            :aria-pressed="!!store.settings.areas[id]"
            @click="toggleArea(id)"
          >
            <span class="check" aria-hidden="true">
              <Check v-if="store.settings.areas[id]" :size="14" :stroke-width="2.4" />
            </span>
            <span class="area-label">{{ t(`memoryCleaner.areas.${id}`) }}</span>
          </button>
        </div>
      </section>

      <section class="panel auto-panel">
        <header class="panel-head">
          <div class="panel-copy with-icon">
            <span class="section-icon" aria-hidden="true">
              <Clock3 :size="18" :stroke-width="1.9" />
            </span>
            <div>
              <h2>{{ t('memoryCleaner.autoTitle') }}</h2>
              <p>{{ t('memoryCleaner.autoBody') }}</p>
            </div>
          </div>
        </header>

        <div class="auto-grid">
          <article class="control-card">
            <div class="control-top">
              <span class="control-icon" aria-hidden="true">
                <Clock3 :size="16" :stroke-width="1.9" />
              </span>
              <div>
                <strong>{{ t('memoryCleaner.autoInterval') }}</strong>
                <p>{{ t('memoryCleaner.autoIntervalHint') }}</p>
              </div>
            </div>
            <PtSoftSelect
              :model-value="store.settings.autoIntervalMinutes"
              :options="intervalOptions"
              :aria-label="t('memoryCleaner.autoInterval')"
              @update:model-value="store.setAutoIntervalMinutes(Number($event))"
            />
          </article>

          <article class="control-card">
            <div class="control-top">
              <span class="control-icon" aria-hidden="true">
                <Gauge :size="16" :stroke-width="1.9" />
              </span>
              <div>
                <strong>{{ t('memoryCleaner.autoThreshold') }}</strong>
                <p>{{ t('memoryCleaner.autoThresholdHint') }}</p>
              </div>
              <span class="value-chip">{{ thresholdLabel }}</span>
            </div>
            <div class="threshold">
              <input
                type="range"
                min="0"
                max="100"
                step="1"
                :value="store.settings.autoFreeBelowPercent"
                @input="
                  store.setAutoFreeBelowPercent(
                    Number(($event.target as HTMLInputElement).value)
                  )
                "
              />
              <div class="threshold-scale" aria-hidden="true">
                <span>0%</span>
                <span>50%</span>
                <span>100%</span>
              </div>
            </div>
          </article>
        </div>
      </section>

      <section v-if="store.result" class="panel result">
        <header class="panel-head">
          <div class="panel-copy">
            <h2>{{ t('memoryCleaner.done') }}</h2>
            <p>
              {{
                t('memoryCleaner.result', {
                  bytes: formatBytes(store.result.freedBytes),
                })
              }}
            </p>
          </div>
        </header>
        <ul class="outcome-list">
          <li
            v-for="(outcome, idx) in store.result.areas"
            :key="idx"
            class="outcome-row"
            :data-status="outcome.status"
          >
            <span>{{ t(`memoryCleaner.areas.${outcome.id}`) }}</span>
            <em>{{ areaDetail(outcome.status, outcome.detail) }}</em>
          </li>
        </ul>
      </section>
    </div>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="t('memoryCleaner.title')"
      :message="t('memoryCleaner.confirm')"
      :confirm-text="t('memoryCleaner.run')"
      @confirm="store.requestOptimize('manual')"
    />
  </PtPageShell>
</template>

<style scoped>
.workspace {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 14px;
  overflow: auto;
  padding-bottom: 18px;
}
.meters {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.meter-card,
.panel {
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.meter-card {
  padding: 14px 16px;
}
.meter-card header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
  color: var(--muted-foreground);
}
.meter-icon,
.section-icon,
.control-icon {
  display: grid;
  width: 34px;
  height: 34px;
  flex: none;
  place-items: center;
  border-radius: 10px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.meter-card header > div {
  min-width: 0;
  flex: 1;
}
.meter-card header strong {
  display: block;
  color: var(--foreground);
  font-size: 0.8125rem;
}
.meter-card header span {
  font-size: 0.75rem;
}
.meter-pct {
  flex: none;
  font-style: normal;
  font-size: 1.125rem;
  font-weight: 700;
  letter-spacing: -0.03em;
  color: var(--foreground);
  font-variant-numeric: tabular-nums;
}
.bar {
  display: flex;
  height: 8px;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in oklab, var(--muted) 70%, transparent);
}
.bar .used {
  display: block;
  height: 100%;
  background: color-mix(in oklab, var(--primary) 78%, white 8%);
}
.bar .free {
  display: block;
  height: 100%;
  background: color-mix(in oklab, var(--success, oklch(0.72 0.12 145)) 55%, transparent);
}
.meter-meta {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin-top: 10px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.meter-meta strong {
  color: var(--foreground);
  font-weight: 600;
}
.panel {
  padding: 14px 16px 16px;
}
.panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}
.panel-copy h2 {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.panel-copy p {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.45;
}
.panel-copy.with-icon {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}
.panel-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
.count-pill {
  display: inline-flex;
  align-items: center;
  min-height: 28px;
  padding: 0 10px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 650;
}
.text-btn {
  border: 0;
  background: transparent;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  padding: 4px 6px;
  border-radius: 8px;
}
.text-btn:hover {
  color: var(--foreground);
  background: color-mix(in oklab, var(--muted) 70%, transparent);
}
.area-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}
.area-tile {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  padding: 0 12px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 12px;
  background: color-mix(in oklab, var(--background) 55%, var(--card));
  color: var(--foreground);
  text-align: left;
  cursor: pointer;
  transition:
    background-color 0.12s ease,
    border-color 0.12s ease,
    box-shadow 0.12s ease;
}
.area-tile:hover {
  background: color-mix(in oklab, var(--muted) 55%, var(--card));
  border-color: color-mix(in oklab, var(--border) 55%, transparent);
}
.area-tile.on {
  border-color: color-mix(in oklab, var(--primary) 45%, var(--border));
  background: color-mix(in oklab, var(--primary) 8%, var(--card));
  box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--primary) 18%, transparent);
}
.check {
  display: grid;
  width: 20px;
  height: 20px;
  flex: none;
  place-items: center;
  border-radius: 6px;
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  background: var(--card);
  color: transparent;
}
.area-tile.on .check {
  border-color: var(--primary);
  background: var(--primary);
  color: var(--primary-foreground);
}
.area-label {
  min-width: 0;
  font-size: 0.8125rem;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.auto-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.control-card {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: 132px;
  padding: 14px;
  overflow: visible;
  border: 1px solid color-mix(in oklab, var(--border) 78%, transparent);
  border-radius: 14px;
  background: color-mix(in oklab, var(--background) 50%, var(--card));
}
.control-top {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}
.control-top > div {
  min-width: 0;
  flex: 1;
}
.control-top strong {
  display: block;
  font-size: 0.8125rem;
  font-weight: 700;
}
.control-top p {
  margin: 3px 0 0;
  color: var(--muted-foreground);
  font-size: 0.71875rem;
  line-height: 1.4;
}
.value-chip {
  flex: none;
  max-width: 140px;
  padding: 5px 9px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--muted) 80%, transparent);
  color: var(--foreground);
  font-size: 0.6875rem;
  font-weight: 650;
  line-height: 1.25;
  text-align: center;
}
.threshold {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: auto;
}
.threshold input[type='range'] {
  width: 100%;
  height: 6px;
  appearance: none;
  border-radius: 999px;
  background: color-mix(in oklab, var(--muted) 85%, transparent);
  outline: none;
}
.threshold input[type='range']::-webkit-slider-thumb {
  appearance: none;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 2px solid var(--card);
  background: var(--primary);
  box-shadow: 0 2px 8px color-mix(in oklab, var(--primary) 35%, transparent);
  cursor: pointer;
}
.threshold input[type='range']::-moz-range-thumb {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 2px solid var(--card);
  background: var(--primary);
  cursor: pointer;
}
.threshold-scale {
  display: flex;
  justify-content: space-between;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.outcome-list {
  margin: 0;
  padding: 0;
  list-style: none;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
}
.outcome-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 40px;
  border-bottom: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
  font-size: 0.8125rem;
}
.outcome-row em {
  font-style: normal;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--muted-foreground);
}
.outcome-row[data-status='ok'] em {
  color: color-mix(in oklab, var(--success, oklch(0.62 0.14 145)) 80%, var(--foreground));
}
.outcome-row[data-status='failed'] em {
  color: var(--destructive);
}
.outcome-row[data-status='skipped'] em {
  color: var(--muted-foreground);
}

@media (max-width: 900px) {
  .meters,
  .auto-grid,
  .area-grid {
    grid-template-columns: 1fr;
  }
  .panel-head {
    flex-direction: column;
  }
  .panel-actions {
    justify-content: flex-start;
  }
  .value-chip {
    max-width: none;
  }
}
</style>
