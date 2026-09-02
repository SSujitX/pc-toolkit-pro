<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { Paintbrush } from '@lucide/vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import { formatBytes } from '@/lib/utils/format';
import { useCleanerStore } from '@/stores/cleaner-store';

const { t } = useI18n();
const store = useCleanerStore();
const confirmOpen = ref(false);

const progressPct = computed(() => {
  if (!store.progress?.total) return store.loading ? 12 : 0;
  return Math.round((store.progress.current / store.progress.total) * 100);
});

const stats = computed(() => [
  { label: 'Items', value: String(store.items.filter((i) => i.selected).length) },
  {
    label: 'Estimated',
    value: formatBytes(store.items.filter((i) => i.selected).reduce((n, i) => n + i.estimatedBytes, 0)),
  },
  {
    label: 'Phase',
    value: store.progress ? `${store.progress.current}/${store.progress.total}` : '—',
  },
]);

function requestClean() {
  confirmOpen.value = true;
}
</script>

<template>
  <PtPageShell :title="t('cleaner.title')" :subtitle="t('cleaner.subtitle')" content-mode="workspace">
    <template #actions>
      <button type="button" class="pt-btn" :disabled="store.loading" @click="store.scan()">
        {{ t('common.scan') }}
      </button>
      <button
        type="button"
        class="pt-btn pt-btn-primary"
        :disabled="store.loading || !store.items.some((i) => i.selected)"
        @click="requestClean"
      >
        {{ t('common.clean') }}
      </button>
    </template>

    <PtOperationWorkspace
      v-if="store.loading"
      :status="store.progress ? 'Cleaning' : 'Analyzing'"
      :title="store.progress?.message || 'Scanning cleanable content...'"
      source-label="Current source"
      source-value="User Temp · System Temp · Prefetch · Recycle Bin"
      :progress="progressPct"
      :stats="stats"
      :hint="t('cleaner.scanHint')"
      :icon="Paintbrush"
      :cancel-label="t('common.cancel')"
      @cancel="store.cancel()"
    />

    <template v-else>
      <p v-if="!store.isAdmin" class="warn">{{ t('common.adminRequired') }}</p>

      <div v-if="!store.items.length" class="empty-card">
        <Paintbrush :size="28" :stroke-width="1.8" />
        <h3>{{ t('cleaner.empty') }}</h3>
        <p>{{ t('cleaner.scanHint') }}</p>
        <button type="button" class="pt-btn pt-btn-primary" @click="store.scan()">
          {{ t('common.scan') }}
        </button>
      </div>

      <div v-else class="list-card">
        <label v-for="item in store.items" :key="item.id" class="row">
          <input type="checkbox" :checked="item.selected" @change="store.toggle(item.id)" />
          <div class="row-body">
            <strong>{{ t(item.titleKey) }}</strong>
            <span>{{ formatBytes(item.estimatedBytes) }}</span>
          </div>
        </label>
      </div>

      <div v-if="store.result" class="result-card">
        {{
          t('cleaner.result', {
            bytes: formatBytes(store.result.freedBytes),
            files: store.result.filesRemoved,
          })
        }}
        <ul>
          <li v-for="(line, idx) in store.log" :key="idx">{{ line }}</li>
        </ul>
      </div>
    </template>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="t('common.clean')"
      :message="t('cleaner.scanHint')"
      destructive
      @confirm="store.execute()"
    />
  </PtPageShell>
</template>

<style scoped>
.warn {
  color: var(--warning);
  font-size: 0.8125rem;
}
.empty-card,
.list-card,
.result-card {
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 18px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.empty-card {
  display: grid;
  place-items: center;
  gap: 8px;
  min-height: 320px;
  padding: 28px;
  color: var(--muted-foreground);
  text-align: center;
}
.empty-card h3 {
  margin: 8px 0 0;
  color: var(--foreground);
  font-size: 1.125rem;
}
.empty-card p {
  margin: 0 0 8px;
  max-width: 360px;
  font-size: 0.8125rem;
}
.list-card {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px;
  overflow: auto;
  min-height: 0;
  flex: 1;
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: var(--layout-result-row-height);
  padding: 0 12px;
  border-radius: 12px;
  cursor: pointer;
}
.row:hover {
  background: var(--surface-soft);
}
.row-body {
  display: flex;
  flex: 1;
  justify-content: space-between;
  gap: 12px;
  font-size: 0.875rem;
}
.result-card {
  padding: 14px 16px;
  font-size: 0.8125rem;
  color: var(--muted-foreground);
}
.result-card ul {
  margin: 8px 0 0;
  padding-left: 18px;
}
</style>
