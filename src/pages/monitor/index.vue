<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
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
      <button type="button" class="btn" :disabled="store.loading" @click="store.refresh()">
        {{ t('common.refresh') }}
      </button>
    </template>

    <div class="metrics">
      <div class="card">
        <div class="label">{{ t('monitor.cpu') }}</div>
        <div class="value">{{ (snap?.cpu ?? 0).toFixed(1) }}%</div>
        <div class="bar"><span :style="{ width: `${snap?.cpu ?? 0}%` }" /></div>
      </div>
      <div class="card">
        <div class="label">{{ t('monitor.memory') }}</div>
        <div class="value">
          {{ formatBytes(snap?.memoryUsed ?? 0) }} / {{ formatBytes(snap?.memoryTotal ?? 0) }}
        </div>
        <div class="bar"><span :style="{ width: `${snap?.memoryPercent ?? 0}%` }" /></div>
      </div>
      <div class="card">
        <div class="label">{{ t('monitor.disk') }}</div>
        <div class="value">
          {{ formatBytes(snap?.diskUsed ?? 0) }} / {{ formatBytes(snap?.diskTotal ?? 0) }}
        </div>
        <div class="bar"><span :style="{ width: `${snap?.diskPercent ?? 0}%` }" /></div>
      </div>
      <div class="card">
        <div class="label">{{ t('monitor.gpu') }}</div>
        <div class="value">
          <template v-if="snap?.gpuAvailable">{{ (snap?.gpuUtilization ?? 0).toFixed(0) }}%</template>
          <template v-else>{{ t('monitor.notAvailable') }}</template>
        </div>
        <div v-if="snap?.gpuAvailable" class="bar">
          <span :style="{ width: `${snap?.gpuUtilization ?? 0}%` }" />
        </div>
      </div>
    </div>

    <div class="meta">
      <div><strong>{{ t('monitor.uptime') }}:</strong> {{ formatUptime(snap?.uptimeSeconds ?? 0) }}</div>
      <div><strong>{{ t('monitor.os') }}:</strong> {{ snap?.osLabel ?? '—' }}</div>
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
  gap: 10px;
}
.card {
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
  padding: 12px 14px;
}
.label {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.value {
  margin-top: 6px;
  font-size: 0.9375rem;
  font-weight: 600;
}
.bar {
  margin-top: 10px;
  height: 6px;
  border-radius: 999px;
  background: var(--muted);
  overflow: hidden;
}
.bar span {
  display: block;
  height: 100%;
  background: var(--primary);
}
.meta {
  display: flex;
  gap: 18px;
  flex-wrap: wrap;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.section {
  margin: 8px 0 0;
  font-size: 0.9375rem;
}
.actions {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
}
.action,
.btn {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  color: var(--foreground);
  padding: 10px 12px;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.action:hover,
.btn:hover {
  background: var(--muted);
}
@media (max-width: 900px) {
  .metrics,
  .actions {
    grid-template-columns: 1fr;
  }
}
</style>
