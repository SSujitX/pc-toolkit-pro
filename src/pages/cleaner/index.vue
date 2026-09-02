<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import { formatBytes } from '@/lib/utils/format';
import { useCleanerStore } from '@/stores/cleaner-store';

const { t } = useI18n();
const store = useCleanerStore();
const confirmOpen = ref(false);

function requestClean() {
  confirmOpen.value = true;
}
</script>

<template>
  <PtPageShell :title="t('cleaner.title')" :subtitle="t('cleaner.subtitle')" content-mode="workspace">
    <template #actions>
      <button type="button" class="btn" :disabled="store.loading" @click="store.scan()">
        {{ t('common.scan') }}
      </button>
      <button
        type="button"
        class="btn primary"
        :disabled="store.loading || !store.items.some((i) => i.selected)"
        @click="requestClean"
      >
        {{ t('common.clean') }}
      </button>
    </template>

    <p class="hint">{{ t('cleaner.scanHint') }}</p>
    <p v-if="!store.isAdmin" class="warn">{{ t('common.adminRequired') }}</p>

    <div v-if="store.progress" class="progress">
      {{ t('common.busy') }} {{ store.progress.current }}/{{ store.progress.total }} —
      {{ store.progress.message }}
      <button type="button" class="btn" @click="store.cancel()">{{ t('common.cancel') }}</button>
    </div>

    <div v-if="!store.items.length" class="empty">{{ t('cleaner.empty') }}</div>
    <div v-else class="list">
      <label v-for="item in store.items" :key="item.id" class="row">
        <input type="checkbox" :checked="item.selected" @change="store.toggle(item.id)" />
        <div class="row-body">
          <strong>{{ t(item.titleKey) }}</strong>
          <span>{{ formatBytes(item.estimatedBytes) }}</span>
        </div>
      </label>
    </div>

    <div v-if="store.result" class="result">
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
.hint,
.warn,
.empty,
.result {
  font-size: 0.75rem;
  color: var(--muted-foreground);
}
.warn {
  color: var(--warning);
}
.list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: auto;
  min-height: 0;
  flex: 1;
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: var(--layout-result-row-height);
  padding: 0 10px;
  border-radius: 8px;
  cursor: pointer;
}
.row:hover {
  background: var(--muted);
}
.row-body {
  display: flex;
  flex: 1;
  justify-content: space-between;
  gap: 12px;
  font-size: 0.8125rem;
}
.progress {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--accent);
  color: var(--accent-foreground);
  font-size: 0.75rem;
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
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
