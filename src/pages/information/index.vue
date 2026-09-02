<script setup lang="ts">
import { onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
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
      <button type="button" class="btn" :disabled="store.loading" @click="store.load()">
        {{ t('common.refresh') }}
      </button>
      <button type="button" class="btn primary" :disabled="!store.info" @click="store.copy()">
        {{ store.copied ? t('common.copied') : t('common.copy') }}
      </button>
    </template>

    <div v-if="store.loading && !store.info" class="empty">{{ t('common.loading') }}</div>
    <div v-else-if="store.info" class="sections">
      <section>
        <h3>System</h3>
        <p>{{ store.info.hostname }} · {{ store.info.username }}</p>
        <p>{{ store.info.osEdition }} {{ store.info.osVersion }} (Build {{ store.info.osBuild }})</p>
        <p>Uptime: {{ store.info.uptime }}</p>
      </section>
      <section>
        <h3>Processor</h3>
        <p>{{ store.info.cpuName }}</p>
        <p>{{ store.info.cpuCores }}C / {{ store.info.cpuThreads }}T · {{ store.info.cpuUsage.toFixed(1) }}%</p>
      </section>
      <section>
        <h3>Memory</h3>
        <p>
          {{ formatBytes(store.info.memoryUsed) }} / {{ formatBytes(store.info.memoryTotal) }} ({{
            store.info.memoryPercent.toFixed(1)
          }}%)
        </p>
      </section>
      <section>
        <h3>Disk C:</h3>
        <p>
          {{ formatBytes(store.info.diskUsed) }} / {{ formatBytes(store.info.diskTotal) }} ({{
            store.info.diskPercent.toFixed(1)
          }}%)
        </p>
      </section>
      <section>
        <h3>GPU</h3>
        <p>{{ store.info.gpuName }}</p>
        <p v-if="store.info.gpuUsage != null">
          {{ store.info.gpuUsage.toFixed(0) }}% ·
          {{ formatBytes(store.info.gpuMemoryUsed ?? 0) }} /
          {{ formatBytes(store.info.gpuMemoryTotal ?? 0) }} · {{ store.info.gpuTemperature }}°C
        </p>
      </section>
      <section>
        <h3>Motherboard</h3>
        <p>{{ store.info.motherboard }}</p>
        <p>BIOS: {{ store.info.bios }}</p>
      </section>
      <section>
        <h3>Monitors</h3>
        <p v-for="(m, i) in store.info.monitors" :key="i">{{ m }}</p>
        <p v-if="!store.info.monitors.length">—</p>
      </section>
      <section>
        <h3>Storage</h3>
        <p v-for="(s, i) in store.info.storageDevices" :key="i">{{ s }}</p>
        <p v-if="!store.info.storageDevices.length">—</p>
      </section>
    </div>
  </PtPageShell>
</template>

<style scoped>
.sections {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
section {
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
  padding: 12px 14px;
}
h3 {
  margin: 0 0 8px;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--muted-foreground);
}
p {
  margin: 0 0 4px;
  font-size: 0.8125rem;
}
.btn {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card);
  color: var(--foreground);
  padding: 8px 12px;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.btn.primary {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--primary-foreground);
}
.empty {
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
</style>
