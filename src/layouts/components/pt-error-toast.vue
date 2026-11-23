<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { CircleAlert, X } from '@lucide/vue';
import { useAppStore } from '@/stores/app-store';

const AUTO_DISMISS_MS = 6000;

const { t } = useI18n();
const app = useAppStore();
let timer: number | null = null;

function clearTimer() {
  if (timer != null) window.clearTimeout(timer);
  timer = null;
}

// Restart the countdown for every new report (a repeat of the same code gets a new id).
watch(
  () => app.lastError?.id,
  (id) => {
    clearTimer();
    if (id == null) return;
    timer = window.setTimeout(() => app.dismissError(id), AUTO_DISMISS_MS);
  },
  { immediate: true }
);
onBeforeUnmount(clearTimer);

function close() {
  clearTimer();
  app.dismissError(app.lastError?.id);
}
</script>

<template>
  <!-- The live region stays mounted so screen readers announce each new message. -->
  <div class="toast-region" role="alert" aria-live="assertive" aria-atomic="true">
    <Transition name="toast">
      <div v-if="app.lastError" :key="app.lastError.id" class="toast">
        <CircleAlert class="toast-icon" :size="16" :stroke-width="2" aria-hidden="true" />
        <div class="toast-copy">
          <strong>{{ t('errors.title') }}</strong>
          <span>{{ t(`errors.codes.${app.lastError.code}`) }}</span>
        </div>
        <button type="button" class="toast-close" :aria-label="t('common.close')" @click="close">
          <X :size="14" :stroke-width="2" />
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.toast-region {
  position: fixed;
  right: 16px;
  bottom: 16px;
  z-index: 90;
  pointer-events: none;
}
.toast {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: min(380px, calc(100vw - 32px));
  padding: 12px 12px 12px 14px;
  border: 1px solid color-mix(in oklab, var(--destructive) 35%, var(--border));
  border-radius: 12px;
  background: color-mix(in oklab, var(--destructive) 6%, var(--card));
  color: var(--foreground);
  box-shadow: var(--shadow-card), 0 12px 28px -12px color-mix(in oklab, var(--foreground) 30%, transparent);
  pointer-events: auto;
}
.toast-icon {
  flex: none;
  margin-top: 1px;
  color: var(--destructive);
}
.toast-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 2px;
}
.toast-copy strong {
  font-size: 0.8125rem;
  font-weight: 650;
}
.toast-copy span {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.toast-close {
  display: grid;
  width: 24px;
  height: 24px;
  flex: none;
  place-items: center;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}
.toast-close:hover {
  background: var(--muted);
  color: var(--foreground);
}
.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 180ms ease,
    transform 180ms cubic-bezier(0.22, 1, 0.36, 1);
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
@media (prefers-reduced-motion: reduce) {
  .toast-enter-active,
  .toast-leave-active {
    transition: opacity 120ms linear;
  }
  .toast-enter-from,
  .toast-leave-to {
    transform: none;
  }
}
</style>
