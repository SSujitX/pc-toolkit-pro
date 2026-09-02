<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { HardDrive, Cpu, RefreshCw } from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import { QUICK_ACTIONS } from '@/lib/models/actions';
import { formatBytes, formatUptime } from '@/lib/utils/format';
import { useMonitorStore } from '@/stores/monitor-store';

const { t } = useI18n();
const store = useMonitorStore();
const snap = computed(() => store.snapshot);

onMounted(() => store.startPolling());
onUnmounted(() => store.stopPolling());
</script>

<template>
  <PtPageShell :title="t('monitor.title')" :subtitle="t('monitor.subtitle')">
    <template #actions>
      <button type="button" class="pt-btn" :disabled="store.loading" @click="store.refresh()">
        <RefreshCw :size="16" />
        {{ t('common.refresh') }}
      </button>
    </template>

    <div class="metrics">
      <div class="card">
        <div class="card-top">
          <Cpu :size="18" />
          <span>{{ t('monitor.cpu') }}</span>
        </div>
        <div class="value">{{ (snap?.cpu ?? 0).toFixed(1) }}%</div>
        <div class="bar"><span :style="{ width: `${snap?.cpu ?? 0}%` }" /></div>
      </div>
      <div class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('monitor.memory') }}</span>
        </div>
        <div class="value">
          {{ formatBytes(snap?.memoryUsed ?? 0) }} / {{ formatBytes(snap?.memoryTotal ?? 0) }}
        </div>
        <div class="bar"><span :style="{ width: `${snap?.memoryPercent ?? 0}%` }" /></div>
      </div>
      <div class="card">
        <div class="card-top">
          <HardDrive :size="18" />
          <span>{{ t('monitor.disk') }}</span>
        </div>
        <div class="value">
          {{ formatBytes(snap?.diskUsed ?? 0) }} / {{ formatBytes(snap?.diskTotal ?? 0) }}
        </div>
        <div class="bar"><span :style="{ width: `${snap?.diskPercent ?? 0}%` }" /></div>
      </div>
      <div class="card">
        <div class="card-top">
          <span class="gpu-dot" />
          <span>{{ t('monitor.gpu') }}</span>
        </div>
        <div class="value">
          <template v-if="snap?.gpuAvailable">{{ (snap?.gpuUtilization ?? 0).toFixed(0) }}%</template>
          <template v-else>{{ t('monitor.notAvailable') }}</template>
        </div>
        <div v-if="snap?.gpuAvailable" class="bar">
          <span :style="{ width: `${snap?.gpuUtilization ?? 0}%` }" />
        </div>
      </div>
    </div>

    <div class="meta-card">
      <div><strong>{{ t('monitor.uptime') }}</strong> {{ formatUptime(snap?.uptimeSeconds ?? 0) }}</div>
      <div><strong>{{ t('monitor.os') }}</strong> {{ snap?.osLabel ?? '—' }}</div>
    </div>

    <h2 class="section">{{ t('monitor.quickActions') }}</h2>
    <div class="actions">
      <button
        v-for="action in QUICK_ACTIONS"
        :key="action.id"
        type="button"
        class="action"
        @click="store.openAction(action.id)"
      >
        {{ t(action.labelKey) }}
      </button>
    </div>
  </PtPageShell>
</template>

<style scoped>
.metrics {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}
.card,
.meta-card {
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 16px;
  background: var(--card);
  box-shadow: var(--shadow-card);
  padding: 16px;
}
.card-top {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 650;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.gpu-dot {
  width: 10px;
  height: 10px;
  border-radius: 999px;
  background: var(--primary);
}
.value {
  margin-top: 10px;
  font-size: 1.05rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.bar {
  margin-top: 12px;
  height: 7px;
  border-radius: 999px;
  background: var(--muted);
  overflow: hidden;
}
.bar span {
  display: block;
  height: 100%;
  background: var(--primary);
}
.meta-card {
  display: flex;
  gap: 24px;
  flex-wrap: wrap;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.meta-card strong {
  color: var(--foreground);
  margin-right: 6px;
}
.section {
  margin: 4px 0 0;
  font-size: 1rem;
  font-weight: 700;
}
.actions {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.action {
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  border-radius: 12px;
  background: var(--card);
  color: var(--foreground);
  padding: 12px 12px;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  box-shadow: var(--shadow-card);
}
.action:hover {
  background: var(--surface-soft);
}
@media (max-width: 900px) {
  .metrics,
  .actions {
    grid-template-columns: 1fr;
  }
}
</style>
