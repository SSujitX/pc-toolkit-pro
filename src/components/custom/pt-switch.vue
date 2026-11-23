<script setup lang="ts">
/** Soft on/off switch (button with role="switch"). */
defineProps<{
  modelValue: boolean;
  disabled?: boolean;
  ariaLabel?: string;
}>();

const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>();
</script>

<template>
  <button
    type="button"
    role="switch"
    class="pt-switch"
    :class="{ on: modelValue }"
    :aria-checked="modelValue"
    :aria-label="ariaLabel"
    :disabled="disabled"
    @click="emit('update:modelValue', !modelValue)"
  >
    <span class="knob" />
  </button>
</template>

<style scoped>
.pt-switch {
  position: relative;
  width: 38px;
  height: 22px;
  flex: none;
  padding: 0;
  border: 1px solid color-mix(in oklab, var(--border) 85%, transparent);
  border-radius: 999px;
  background: var(--muted);
  cursor: pointer;
  transition:
    background-color 160ms ease,
    border-color 160ms ease;
}
.knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--card);
  box-shadow: 0 1px 2px rgb(0 0 0 / 18%);
  transition: left 160ms ease;
}
.pt-switch.on {
  border-color: var(--primary);
  background: var(--primary);
}
.pt-switch.on .knob {
  left: 18px;
}
.pt-switch:disabled {
  cursor: default;
  opacity: 0.45;
}
.pt-switch:focus-visible {
  outline: 2px solid color-mix(in oklab, var(--primary) 60%, transparent);
  outline-offset: 2px;
}
@media (prefers-reduced-motion: reduce) {
  .pt-switch,
  .knob {
    transition: none;
  }
}
</style>
