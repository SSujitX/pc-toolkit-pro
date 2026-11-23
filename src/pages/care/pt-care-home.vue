<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { ScanSearch } from '@lucide/vue';
import PtSoftSelect from '@/components/custom/pt-soft-select.vue';
import { CARE_AFTER_ACTIONS, CARE_CATEGORY_IDS, type CareAfterAction } from '@/lib/models/care';
import { formatBytes } from '@/lib/utils/format';
import { useCareStore } from '@/stores/care-store';
import { CARE_CATEGORY_ICONS, categoryName } from './care-ui';

const { t, locale } = useI18n();
const store = useCareStore();

const afterOptions = computed<{ value: string; label: string }[]>(() =>
  CARE_AFTER_ACTIONS.map((value) => ({ value, label: t(`care.engine.afterActions.${value}`) }))
);

const lastWhen = computed(() => {
  const at = store.lastSummary?.atMs;
  if (!at) return '';
  try {
    return new Intl.DateTimeFormat(locale.value || undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(at));
  } catch {
    return new Date(at).toLocaleString();
  }
});

function onAfterAction(value: string | number) {
  store.setAfterAction(String(value) as CareAfterAction);
}
</script>

<template>
  <div class="care-home">
    <section class="hero">
      <button
        type="button"
        class="scan-orb"
        :disabled="!store.selectedCategories.length"
        @click="store.scan()"
      >
        <span class="orb-ring" aria-hidden="true" />
        <span class="orb-face">
          <ScanSearch :size="30" :stroke-width="1.75" />
          <span class="orb-label">{{ t('care.engine.home.scan') }}</span>
        </span>
      </button>
      <p class="hero-hint">
        {{ store.selectedCategories.length ? t('care.engine.home.scanHint') : t('care.engine.home.noneSelected') }}
      </p>
      <p class="hero-last">
        <template v-if="store.lastSummary">
          <span>{{ t('care.engine.home.lastScan', { when: lastWhen }) }}</span>
          <span class="dot">·</span>
          <span>
            {{
              t('care.engine.home.lastScanResult', {
                issues: store.lastSummary.issues,
                size: formatBytes(store.lastSummary.bytes),
              })
            }}
          </span>
          <button v-if="store.report" type="button" class="text-btn" @click="store.showResults()">
            {{ t('care.engine.home.viewResults') }}
          </button>
        </template>
        <span v-else>{{ t('care.engine.home.neverScanned') }}</span>
      </p>
    </section>

    <section class="panel">
      <header class="panel-head">
        <h2>{{ t('care.engine.home.categoriesTitle') }}</h2>
        <button type="button" class="text-btn" @click="store.setAllCategories(!store.allCategoriesSelected)">
          {{ store.allCategoriesSelected ? t('care.engine.home.selectNone') : t('care.engine.home.selectAll') }}
        </button>
      </header>
      <div class="cat-grid">
        <label
          v-for="id in CARE_CATEGORY_IDS"
          :key="id"
          class="cat-check"
          :data-on="store.selectedCategories.includes(id)"
        >
          <input
            type="checkbox"
            :checked="store.selectedCategories.includes(id)"
            @change="store.toggleCategory(id)"
          />
          <span class="cat-icon" aria-hidden="true">
            <component :is="CARE_CATEGORY_ICONS[id]" :size="15" :stroke-width="1.75" />
          </span>
          <span class="cat-name">{{ categoryName(id, t) }}</span>
        </label>
      </div>

      <footer class="auto-row">
        <label class="auto-check">
          <input type="checkbox" :checked="store.autoFix" @change="store.setAutoFix(!store.autoFix)" />
          <span>
            <strong>{{ t('care.engine.home.autoFix') }}</strong>
            <small>{{ t('care.engine.home.autoFixHint') }}</small>
          </span>
        </label>
        <div class="after" :data-disabled="!store.autoFix">
          <span class="after-label">{{ t('care.engine.home.afterAction') }}</span>
          <PtSoftSelect
            :model-value="store.afterAction"
            :options="afterOptions"
            :aria-label="t('care.engine.home.afterAction')"
            @update:model-value="onAfterAction"
          />
        </div>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.care-home {
  display: flex;
  flex-direction: column;
  gap: 18px;
  min-height: 0;
  overflow: auto;
  padding-bottom: 8px;
}
.hero {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 18px 0 4px;
}
.scan-orb {
  position: relative;
  display: grid;
  place-items: center;
  width: 176px;
  height: 176px;
  border: 0;
  border-radius: 50%;
  padding: 0;
  background: transparent;
  cursor: pointer;
  isolation: isolate;
}
.scan-orb:disabled {
  cursor: default;
  opacity: 0.55;
}
/* Continuously rotating conic ring around the button (same language as our spinners). */
.orb-ring {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  background: conic-gradient(
    from 0deg,
    var(--primary) 0deg,
    color-mix(in oklab, var(--primary) 45%, transparent) 110deg,
    transparent 200deg,
    color-mix(in oklab, var(--primary) 18%, transparent) 360deg
  );
  -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 6px), #000 calc(100% - 5.5px));
  mask: radial-gradient(farthest-side, transparent calc(100% - 6px), #000 calc(100% - 5.5px));
  animation: orb-spin 3.2s linear infinite;
  pointer-events: none;
}
.orb-face {
  position: absolute;
  inset: 14px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border-radius: 50%;
  background: var(--primary);
  color: var(--primary-foreground);
  box-shadow: 0 12px 32px -14px color-mix(in oklab, var(--primary) 70%, transparent);
  transition: background-color 0.15s ease, box-shadow 0.15s ease;
}
.scan-orb:hover:not(:disabled) .orb-face {
  background: color-mix(in oklab, var(--primary) 88%, black);
  box-shadow: 0 16px 40px -14px color-mix(in oklab, var(--primary) 85%, transparent);
}
.scan-orb:focus-visible .orb-face {
  outline: 3px solid color-mix(in oklab, var(--primary) 40%, transparent);
  outline-offset: 4px;
}
.orb-label {
  font-size: 1.25rem;
  font-weight: 700;
  letter-spacing: -0.01em;
}
.hero-hint {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
}
.hero-last {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 22px;
  margin: 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.dot {
  opacity: 0.6;
}

.panel {
  border: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  border-radius: 14px;
  background: var(--card);
  padding: 14px 16px;
}
.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.panel-head h2 {
  margin: 0;
  font-size: 0.9375rem;
  font-weight: 650;
}
.cat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 6px;
}
@media (min-width: 1200px) {
  .cat-grid {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
}
.cat-check {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 40px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 10px;
  font-size: 0.8125rem;
  cursor: pointer;
  transition: background-color 0.12s ease, border-color 0.12s ease;
}
.cat-check:hover {
  background: var(--muted);
}
.cat-check[data-on='true'] {
  border-color: color-mix(in oklab, var(--primary) 22%, transparent);
  background: color-mix(in oklab, var(--primary) 6%, transparent);
}
.cat-check input,
.auto-check input {
  accent-color: var(--primary);
  width: 15px;
  height: 15px;
  flex: none;
}
.cat-icon {
  display: grid;
  width: 26px;
  height: 26px;
  flex: none;
  place-items: center;
  border-radius: 8px;
  background: color-mix(in oklab, var(--primary) 10%, transparent);
  color: var(--primary);
}
.cat-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.auto-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid color-mix(in oklab, var(--border) 60%, transparent);
}
.auto-check {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  max-width: 560px;
  font-size: 0.8125rem;
  cursor: pointer;
}
.auto-check input {
  margin-top: 2px;
}
.auto-check strong {
  display: block;
  font-weight: 600;
}
.auto-check small {
  display: block;
  margin-top: 2px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.after {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 280px;
  transition: opacity 0.15s ease;
}
.after[data-disabled='true'] {
  opacity: 0.5;
  pointer-events: none;
}
.after-label {
  flex: none;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 600;
}
.text-btn {
  border: 0;
  background: transparent;
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 600;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 6px;
}
.text-btn:hover {
  background: color-mix(in oklab, var(--primary) 10%, transparent);
}

@keyframes orb-spin {
  from {
    rotate: 0deg;
  }
  to {
    rotate: 360deg;
  }
}
@media (prefers-reduced-motion: reduce) {
  .orb-ring {
    animation-duration: 12s;
  }
}
</style>
