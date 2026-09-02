<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import { APP_NAME, APP_VERSION } from '@/lib/models/application-shell';
import { useAppStore } from '@/stores/app-store';

const { t } = useI18n();
const app = useAppStore();
</script>

<template>
  <PtPageShell :title="t('settings.title')" :subtitle="t('settings.subtitle')">
    <section class="card">
      <h3>{{ t('settings.theme') }}</h3>
      <div class="row">
        <button
          type="button"
          class="btn"
          :class="{ active: app.theme === 'light' }"
          @click="app.setTheme('light')"
        >
          {{ t('settings.light') }}
        </button>
        <button
          type="button"
          class="btn"
          :class="{ active: app.theme === 'dark' }"
          @click="app.setTheme('dark')"
        >
          {{ t('settings.dark') }}
        </button>
      </div>
    </section>
    <section class="card">
      <h3>{{ t('settings.about') }}</h3>
      <p>{{ APP_NAME }}</p>
      <p>{{ t('settings.version', { version: APP_VERSION }) }}</p>
      <p class="muted">{{ t('app.tagline') }}</p>
    </section>
  </PtPageShell>
</template>

<style scoped>
.card {
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card);
  padding: 14px 16px;
  margin-bottom: 10px;
}
h3 {
  margin: 0 0 10px;
  font-size: 0.8125rem;
}
.row {
  display: flex;
  gap: 8px;
}
.btn {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: transparent;
  color: var(--foreground);
  padding: 8px 12px;
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
}
.btn.active {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--primary-foreground);
}
.muted {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
p {
  margin: 0 0 4px;
  font-size: 0.8125rem;
}
</style>
