<script setup lang="ts">
import { onBeforeUnmount, ref, useId, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { trapDialogFocus } from '@/lib/services/dialog-focus-service';

const open = defineModel<boolean>('open', { default: false });
defineProps<{
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  destructive?: boolean;
}>();
/**
 * `cancel` = the explicit cancel button (some callers treat it as an action, e.g. "continue
 * without admin"); `dismiss` = overlay click / Escape, which never triggers an action.
 */
const emit = defineEmits<{ confirm: []; cancel: []; dismiss: [] }>();
const { t } = useI18n();
const titleId = useId();
const panel = ref<HTMLElement | null>(null);
let releaseFocus: (() => void) | null = null;

watch(
  panel,
  (el) => {
    releaseFocus?.();
    releaseFocus = el ? trapDialogFocus(el, onDismiss) : null;
  },
  { flush: 'post' }
);
onBeforeUnmount(() => releaseFocus?.());

function onConfirm() {
  open.value = false;
  emit('confirm');
}
function onCancel() {
  open.value = false;
  emit('cancel');
}
function onDismiss() {
  open.value = false;
  emit('dismiss');
}
</script>

<template>
  <div v-if="open" class="pt-dialog-root" role="dialog" aria-modal="true" :aria-labelledby="titleId">
    <div class="pt-dialog-overlay" @click="onDismiss" />
    <div ref="panel" class="pt-dialog-panel">
      <header class="pt-dialog-header">
        <h2 :id="titleId">{{ title }}</h2>
        <p>{{ message }}</p>
      </header>
      <footer class="pt-dialog-footer">
        <button type="button" class="pt-btn" @click="onCancel">{{ cancelText ?? t('common.cancel') }}</button>
        <button
          type="button"
          class="pt-btn pt-btn-primary"
          :class="{ 'pt-btn-danger': destructive }"
          @click="onConfirm"
        >
          {{ confirmText ?? t('common.confirm') }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.pt-dialog-root {
  position: fixed;
  inset: 0;
  z-index: 80;
  display: grid;
  place-items: center;
}
.pt-dialog-overlay {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 30%);
}
.pt-dialog-panel {
  position: relative;
  z-index: 1;
  width: min(520px, calc(100vw - 32px));
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--card);
  box-shadow: 0 25px 50px -12px rgb(0 0 0 / 25%);
}
.pt-dialog-header {
  padding: 16px 20px 10px;
}
.pt-dialog-header h2 {
  margin: 0;
  font-size: 1rem;
}
.pt-dialog-header p {
  margin: 6px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.pt-dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 10px 20px 16px;
}
/* Use global .pt-btn / .pt-btn-primary from main.css — local :hover was
   washing out primary text (muted background + light foreground). */
</style>
