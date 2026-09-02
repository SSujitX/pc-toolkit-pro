<script setup lang="ts">
import { onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Battery,
  CircuitBoard,
  Cpu,
  HardDrive,
  Monitor,
  RefreshCw,
  Server,
  Zap,
} from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import { formatBytes } from '@/lib/utils/format';
import { useSystemInfoStore } from '@/stores/system-info-store';

const { t } = useI18n();
const store = useSystemInfoStore();
onMounted(() => {
  void store.load();
});
</script>

<template>
  <PtPageShell :title="t('information.title')" :subtitle="t('information.subtitle')">
    <template #actions>
      <button type="button" class="pt-btn" :disabled="store.loading" @click="store.load()">
        <RefreshCw :size="16" />
        {{ t('common.refresh') }}
      </button>
      <button type="button" class="pt-btn primary" :disabled="!store.info" @click="store.copy()">
        {{ store.copied ? t('common.copied') : t('common.copy') }}
      </button>
    </template>

    <div v-if="store.loading && !store.info" class="state">{{ t('common.loading') }}</div>

    <div v-else-if="store.info" class="sections">
      <section class="card">
        <div class="card-top">
          <Server :size="18" />
          <span>{{ t('information.system') }}</span>
        </div>
        <p class="lead">{{ store.info.hostname }} · {{ store.info.username }}</p>
        <p>{{ store.info.osEdition }} {{ store.info.osVersion }} (Build {{ store.info.osBuild }})</p>
        <p>{{ t('information.uptime') }}: {{ store.info.uptime }}</p>
      </section>

      <section class="card">
        <div class="card-top">
          <Cpu :size="18" />
          <span>{{ t('information.processor') }}</span>
        </div>
        <p class="lead">{{ store.info.cpuName }}</p>
        <p>
          {{ store.info.cpuCores }}C / {{ store.info.cpuThreads }}T ·
          {{ store.info.cpuUsage.toFixed(1) }}%
        </p>
      </section>

      <section class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('information.memory') }}</span>
        </div>
        <p class="lead">
          {{ formatBytes(store.info.memoryUsed) }} / {{ formatBytes(store.info.memoryTotal) }}
        </p>
        <p>{{ store.info.memoryPercent.toFixed(1) }}%</p>
      </section>

      <section class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('information.disk') }}</span>
        </div>
        <p class="lead">
          {{ formatBytes(store.info.diskUsed) }} / {{ formatBytes(store.info.diskTotal) }}
        </p>
        <p>{{ store.info.diskPercent.toFixed(1) }}%</p>
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
        <p class="lead">{{ store.info.motherboard }}</p>
        <p>{{ t('information.bios') }}: {{ store.info.bios }}</p>
      </section>

      <section class="card wide">
        <div class="card-top">
          <Zap :size="18" />
          <span>{{ t('information.power') }}</span>
        </div>
        <p>
          <span class="label">{{ t('information.powerPlan') }}</span>
          {{ store.info.powerPlan }}
        </p>
        <p>
          <span class="label">{{ t('information.acStatus') }}</span>
          {{ store.info.acLineStatus }}
        </p>
        <p>
          <span class="label">{{ t('information.powerSupply') }}</span>
        </p>
        <template v-if="store.info.powerSupplies.length">
          <p v-for="(row, i) in store.info.powerSupplies" :key="`psu-${i}`">{{ row }}</p>
        </template>
        <p v-else class="muted">{{ t('information.psuUnavailable') }}</p>
        <p>
          <span class="label">{{ t('information.battery') }}</span>
        </p>
        <template v-if="store.info.batteries.length">
          <p v-for="(row, i) in store.info.batteries" :key="`bat-${i}`" class="with-icon">
            <Battery :size="14" />
            {{ row }}
          </p>
        </template>
        <p v-else class="muted">{{ t('information.noneReported') }}</p>
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
        <p v-for="(s, i) in store.info.storageDevices" :key="i">{{ s }}</p>
        <p v-if="!store.info.storageDevices.length" class="muted">—</p>
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
.label {
  display: inline-block;
  min-width: 7.5rem;
  color: var(--muted-foreground);
  font-weight: 600;
}
.muted {
  color: var(--muted-foreground);
}
.with-icon {
  display: flex;
  align-items: center;
  gap: 6px;
}
.pt-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
  color: var(--foreground);
  padding: 8px 12px;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.pt-btn.primary {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--primary-foreground);
}
.pt-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.state {
  color: var(--muted-foreground);
  font-size: 0.875rem;
  padding: 28px 8px;
}
.state.empty {
  max-width: 36rem;
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
