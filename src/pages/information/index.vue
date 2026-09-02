<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Battery,
  CircuitBoard,
  Cpu,
  HardDrive,
  Info,
  Monitor,
  RefreshCw,
  Server,
  Zap,
} from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import { formatBytes } from '@/lib/utils/format';
import { useSystemInfoStore } from '@/stores/system-info-store';

const { t } = useI18n();
const store = useSystemInfoStore();
const elapsedSec = ref(0);
let elapsedTimer: number | null = null;

const isBusy = computed(() => store.loading);
const progressPct = computed(() => {
  if (!store.progress?.total) return isBusy.value ? 18 : 0;
  return Math.round((store.progress.current / store.progress.total) * 100);
});

const phaseLabel = computed(() => {
  switch (store.progress?.phase) {
    case 'metrics':
      return t('information.phaseMetrics');
    case 'hardware':
      return t('information.phaseHardware');
    case 'gpu':
      return t('information.phaseGpu');
    case 'assemble':
      return t('information.phaseAssemble');
    default:
      return t('information.phaseMetrics');
  }
});

const stats = computed(() => [
  {
    label: t('information.elapsed'),
    value: `${elapsedSec.value} sec`,
  },
  {
    label: t('information.loadingSource'),
    value: store.progress
      ? `${store.progress.current} / ${store.progress.total}`
      : '—',
  },
]);

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

onMounted(() => {
  void store.load();
});

onUnmounted(() => {
  if (elapsedTimer != null) window.clearInterval(elapsedTimer);
});
</script>

<template>
  <PtPageShell
    :title="t('information.title')"
    :subtitle="t('information.subtitle')"
    content-mode="workspace"
  >
    <template #actions>
      <button type="button" class="pt-btn" :disabled="store.loading" @click="store.load()">
        <RefreshCw :size="16" />
        {{ t('common.refresh') }}
      </button>
      <button
        type="button"
        class="pt-btn pt-btn-primary"
        :disabled="!store.info || store.loading"
        @click="store.copy()"
      >
        {{ store.copied ? t('common.copied') : t('common.copy') }}
      </button>
    </template>

    <PtOperationWorkspace
      v-if="isBusy"
      :status="t('information.loadingStatus')"
      :title="t('information.loadingTitle')"
      :source-label="t('information.loadingSource')"
      :source-value="phaseLabel"
      :source-icon="Info"
      :progress="progressPct"
      :stats="stats"
      :hint="t('information.loadingHint')"
      :icon="Info"
      :cancellable="false"
    />

    <div v-else-if="store.info" class="sections">
      <section class="card">
        <div class="card-top">
          <Cpu :size="18" />
          <span>{{ t('information.processor') }}</span>
        </div>
        <p class="lead">{{ store.info.cpuName }}</p>
        <p>
          {{ store.info.cpuCores }} cores, {{ store.info.cpuThreads }} threads ·
          {{ store.info.cpuUsage.toFixed(1) }}%
        </p>
        <p>{{ store.info.cpuFrequency }}</p>
        <p>{{ store.info.cpuCache }}</p>
        <p>Socket: {{ store.info.cpuSocket }}</p>
      </section>

      <section class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('information.disk') }}</span>
        </div>
        <p class="lead">{{ store.info.diskDevice }}</p>
        <p>{{ store.info.diskType }}</p>
        <p>
          {{ formatBytes(store.info.diskUsed) }} / {{ formatBytes(store.info.diskTotal) }} ({{
            store.info.diskPercent.toFixed(1)
          }}%)
        </p>
        <p>Free: {{ formatBytes(store.info.diskFree) }}</p>
      </section>

      <section class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('information.memory') }}</span>
        </div>
        <p class="lead">
          {{ formatBytes(store.info.memoryUsed) }} / {{ formatBytes(store.info.memoryTotal) }}
        </p>
        <p>Available: {{ formatBytes(store.info.memoryAvailable) }}</p>
        <p>RAM: {{ store.info.ramName }}</p>
        <p>{{ store.info.ramType }} · {{ store.info.ramSpeed }} · {{ store.info.ramSlotsUsed }}</p>
      </section>

      <section class="card">
        <div class="card-top">
          <span class="gpu-dot" />
          <span>{{ t('information.gpu') }}</span>
        </div>
        <p class="lead">{{ store.info.gpuName }}</p>
        <p v-if="store.info.gpuUsage != null">
          {{ store.info.gpuUsage.toFixed(0) }}% ·
          {{ formatBytes(store.info.gpuMemoryUsed ?? 0) }} /
          {{ formatBytes(store.info.gpuMemoryTotal ?? 0) }}
          <template v-if="store.info.gpuTemperature != null">
            · {{ store.info.gpuTemperature }}°C
          </template>
        </p>
      </section>

      <section class="card">
        <div class="card-top">
          <CircuitBoard :size="18" />
          <span>{{ t('information.motherboard') }}</span>
        </div>
        <p class="lead">{{ store.info.motherboardProduct }}</p>
        <p>{{ store.info.motherboardManufacturer }} · {{ store.info.motherboardVersion }}</p>
        <p>Chipset: {{ store.info.chipset }}</p>
        <p>BIOS: {{ store.info.biosVersion }} ({{ store.info.biosManufacturer }})</p>
        <p>BIOS date: {{ store.info.biosDate }}</p>
        <p>Model: {{ store.info.systemModel }}</p>
        <p>
          Memory slots: {{ store.info.memorySlotsTotal }} · Max {{ store.info.maxMemoryCapacity }}
        </p>
      </section>

      <section class="card">
        <div class="card-top">
          <Zap :size="18" />
          <span>{{ t('information.powerSupply') }}</span>
        </div>
        <p class="lead">{{ store.info.powerSupplyName }}</p>
        <p v-if="store.info.batteries.length" class="with-icon">
          <Battery :size="14" />
          {{ store.info.batteries.join(' · ') }}
        </p>
      </section>

      <section class="card">
        <div class="card-top">
          <Monitor :size="18" />
          <span>{{ t('information.monitors') }}</span>
        </div>
        <p v-for="(m, i) in store.info.monitors" :key="i">{{ m }}</p>
        <p v-if="!store.info.monitors.length" class="muted">—</p>
      </section>

      <section class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('information.storage') }}</span>
        </div>
        <p v-for="(s, i) in store.info.storageDevices" :key="i">
          Storage {{ i + 1 }}: {{ s }}
        </p>
        <p v-if="!store.info.storageDevices.length" class="muted">—</p>
      </section>

      <section class="card wide">
        <div class="card-top">
          <Server :size="18" />
          <span>{{ t('information.system') }}</span>
        </div>
        <p class="lead">{{ store.info.hostname }} · {{ store.info.username }}</p>
        <p>{{ store.info.osEdition }} {{ store.info.osVersion }} (Build {{ store.info.osBuild }})</p>
        <p>{{ store.info.osExperience }}</p>
        <p>{{ t('information.uptime') }}: {{ store.info.uptime }}</p>
      </section>
    </div>

    <div v-else class="state empty">
      <h3>{{ store.error ? t('information.loadFailed') : t('information.unavailableTitle') }}</h3>
      <p>{{ t('information.unavailableBody') }}</p>
      <p v-if="store.error" class="error-detail">{{ store.error }}</p>
    </div>
  </PtPageShell>
</template>

<style scoped>
.sections {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
  padding: 4px 2px 12px;
  overflow: auto;
  min-height: 0;
  height: 100%;
}
.card {
  border: 1px solid var(--border);
  border-radius: 14px;
  background: color-mix(in srgb, var(--card) 92%, white);
  box-shadow: 0 1px 0 color-mix(in srgb, var(--foreground) 4%, transparent);
  padding: 14px 16px;
}
.card.wide {
  grid-column: 1 / -1;
}
.card-top {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
  font-size: 0.75rem;
  font-weight: 600;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  color: var(--muted-foreground);
}
.gpu-dot {
  width: 10px;
  height: 10px;
  border-radius: 999px;
  background: #22c55e;
}
.lead {
  font-size: 0.9375rem;
  font-weight: 600;
  color: var(--foreground);
}
p {
  margin: 0 0 4px;
  font-size: 0.8125rem;
  color: var(--foreground);
}
.muted {
  color: var(--muted-foreground);
}
.with-icon {
  display: flex;
  align-items: center;
  gap: 6px;
}
.state {
  color: var(--muted-foreground);
  font-size: 0.875rem;
  padding: 28px 8px;
}
.state.empty {
  max-width: 36rem;
  margin: 24px auto;
}
.state.empty h3 {
  margin: 0 0 8px;
  font-size: 1rem;
  color: var(--foreground);
}
.state.empty p {
  margin: 0 0 8px;
  line-height: 1.5;
}
.error-detail {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.75rem;
  opacity: 0.85;
}
@media (max-width: 900px) {
  .sections {
    grid-template-columns: 1fr;
  }
  .card.wide {
    grid-column: auto;
  }
}
</style>
