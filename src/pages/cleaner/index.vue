<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  HardDrive,
  Paintbrush,
  Recycle,
  Sparkles,
} from '@lucide/vue';
import type { Component } from 'vue';
import PtPageShell from '@/components/custom/pt-page-shell.vue';
import PtConfirmDialog from '@/components/custom/pt-confirm-dialog.vue';
import PtOperationWorkspace from '@/components/custom/pt-operation-workspace.vue';
import { formatBytes } from '@/lib/utils/format';
import type { CleanerPreset } from '@/lib/models/actions';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { useAppStore } from '@/stores/app-store';
import { useCleanerStore } from '@/stores/cleaner-store';

const { t } = useI18n();
const app = useAppStore();
const store = useCleanerStore();
const confirmOpen = ref(false);
const pendingPreset = ref<CleanerPreset | null>(null);
const elapsedSec = ref(0);
let elapsedTimer: number | null = null;

const presets: {
  id: CleanerPreset;
  icon: Component;
  tone: 'primary' | 'info' | 'neutral' | 'warn';
}[] = [
      { id: 'tempPrefetchRecycle', icon: Paintbrush, tone: 'primary' },
  { id: 'diskCleanup', icon: HardDrive, tone: 'info' },
  { id: 'recycleOnly', icon: Recycle, tone: 'neutral' },
];

const isBusy = computed(() => store.loading);
const progressPct = computed(() => {
  if (!store.progress?.total) return isBusy.value ? 22 : 0;
  return Math.round((store.progress.current / store.progress.total) * 100);
});
const sourceValue = computed(
  () => store.progress?.message || t('cleaner.preparing')
);
const stats = computed(() => [
  {
    label: t('cleaner.elapsed'),
    value: `${elapsedSec.value} sec`,
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

onUnmounted(() => {
  if (elapsedTimer != null) window.clearInterval(elapsedTimer);
});

function requestRun(preset: CleanerPreset) {
  pendingPreset.value = preset;
  confirmOpen.value = true;
}

function confirmRun() {
  const preset = pendingPreset.value;
  pendingPreset.value = null;
  if (preset) void store.runPreset(preset);
}

function openDeepCleaner() {
  app.navigate(PAGE_IDS.deepCleaner);
}
</script>

<template>
  <PtPageShell
    :title="t('cleaner.title')"
    :subtitle="t('cleaner.subtitle')"
    content-mode="workspace"
  >
    <PtOperationWorkspace
      v-if="isBusy"
      :status="t('cleaner.cleaning')"
      :title="t('cleaner.cleaningTitle')"
      :source-label="t('cleaner.scanning')"
      :source-value="sourceValue"
      :source-icon="Paintbrush"
      :progress="progressPct"
      :stats="stats"
      :hint="t('cleaner.cancelHint')"
      :icon="Paintbrush"
      :cancel-label="t('common.cancel')"
      @cancel="store.cancel()"
    />

    <div v-else class="cleaner-workspace">
      <header class="intro">
        <div class="intro-main">
          <div class="intro-icon" aria-hidden="true">
            <Paintbrush :size="22" :stroke-width="1.8" />
          </div>
          <div class="intro-copy">
            <h2>{{ t('cleaner.sectionTitle') }}</h2>
            <p>{{ t('cleaner.sectionBody') }}</p>
          </div>
        </div>
        <button type="button" class="pt-btn intro-action" @click="openDeepCleaner">
          <Sparkles :size="16" :stroke-width="2" />
          {{ t('cleaner.openDeepCleaner') }}
        </button>
      </header>

      <div class="action-list">
        <article
          v-for="preset in presets"
          :key="preset.id"
          class="action-card"
          :data-tone="preset.tone"
        >
          <div class="action-icon" aria-hidden="true">
            <component :is="preset.icon" :size="20" :stroke-width="1.9" />
          </div>
          <div class="action-body">
            <strong>{{ t(`cleaner.${preset.id}.title`) }}</strong>
            <p>{{ t(`cleaner.${preset.id}.body`) }}</p>
          </div>
          <button
            type="button"
            class="pt-btn pt-btn-primary action-run"
            @click="requestRun(preset.id)"
          >
            {{ t('cleaner.run') }}
          </button>
        </article>
      </div>

      <div v-if="store.result" class="result-card">
        <strong>{{ t('cleaner.done') }}</strong>
        <p>
          {{
            t('cleaner.result', {
              bytes: formatBytes(store.result.freedBytes),
              files: store.result.filesRemoved,
            })
          }}
        </p>
        <ul v-if="store.log.length">
          <li v-for="(line, idx) in store.log" :key="idx">{{ line }}</li>
        </ul>
      </div>
    </div>

    <PtConfirmDialog
      v-model:open="confirmOpen"
      :title="pendingPreset ? t(`cleaner.${pendingPreset}.title`) : t('common.confirm')"
      :message="pendingPreset ? t(`cleaner.${pendingPreset}.confirm`) : ''"
      destructive
      :confirm-text="t('cleaner.run')"
      @confirm="confirmRun"
    />
  </PtPageShell>
</template>

<style scoped>
.cleaner-workspace {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 14px;
}
.intro {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 16px 18px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.intro-main {
  display: flex;
  min-width: 0;
  flex: 1;
  align-items: flex-start;
  gap: 14px;
}
.intro-icon {
  display: grid;
  width: 44px;
  height: 44px;
  flex: none;
  place-items: center;
  border-radius: 12px;
  background: color-mix(in oklab, var(--primary) 14%, transparent);
  color: var(--primary);
}
.intro-copy {
  min-width: 0;
}
.intro h2 {
  margin: 0;
  color: var(--foreground);
  font-size: 0.9375rem;
  font-weight: 650;
  letter-spacing: -0.02em;
}
.intro p {
  margin: 4px 0 0;
  max-width: 62ch;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.intro-action {
  flex: none;
  margin-inline-start: auto;
}
.action-list {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 8px;
  padding: 8px;
  overflow: auto;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.action-card {
  display: grid;
  grid-template-columns: 44px minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  min-height: 72px;
  padding: 12px 14px;
  border: 1px solid transparent;
  border-radius: 12px;
  background: transparent;
  transition:
    background-color 0.16s ease,
    border-color 0.16s ease;
}
.action-card:hover {
  background: var(--surface-soft);
  border-color: color-mix(in oklab, var(--border) 70%, transparent);
}
.action-icon {
  display: grid;
  width: 44px;
  height: 44px;
  place-items: center;
  border-radius: 12px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: var(--primary);
}
.action-card[data-tone='info'] .action-icon {
  background: color-mix(in oklab, var(--info, var(--primary)) 14%, transparent);
  color: var(--info, var(--primary));
}
.action-card[data-tone='neutral'] .action-icon {
  background: var(--surface-soft);
  color: var(--muted-foreground);
}
.action-card[data-tone='warn'] .action-icon {
  background: color-mix(in oklab, var(--warning) 16%, transparent);
  color: var(--warning);
}
.action-body {
  min-width: 0;
}
.action-body strong {
  display: block;
  color: var(--foreground);
  font-size: 0.875rem;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.action-body p {
  margin: 3px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.action-run {
  min-width: 88px;
  justify-self: end;
}
.result-card {
  flex: none;
  padding: 14px 16px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 14px;
  background: var(--card);
  box-shadow: var(--shadow-card);
}
.result-card strong {
  display: block;
  color: var(--foreground);
  font-size: 0.8125rem;
}
.result-card p {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.result-card ul {
  margin: 8px 0 0;
  padding-left: 18px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}

@media (max-width: 900px) {
  .intro {
    flex-wrap: wrap;
    align-items: stretch;
  }
  .intro-action {
    width: 100%;
    justify-content: center;
  }
  .action-card {
    grid-template-columns: 44px minmax(0, 1fr);
    grid-template-areas:
      'icon body'
      'run run';
  }
  .action-icon {
    grid-area: icon;
  }
  .action-body {
    grid-area: body;
  }
  .action-run {
    grid-area: run;
    width: 100%;
    min-width: 0;
  }
}
</style>
