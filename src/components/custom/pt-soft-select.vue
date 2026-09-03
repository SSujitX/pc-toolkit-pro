<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from 'vue';
import { ChevronDown } from '@lucide/vue';

export type SoftSelectOption = {
  value: string | number;
  label: string;
};

const props = defineProps<{
  modelValue: string | number;
  options: SoftSelectOption[];
  ariaLabel?: string;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: string | number];
}>();

const open = ref(false);
const placement = ref<'bottom' | 'top'>('bottom');
const root = ref<HTMLElement | null>(null);
const list = ref<HTMLElement | null>(null);

const selectedLabel = computed(() => {
  const match = props.options.find((o) => o.value === props.modelValue);
  return match?.label ?? String(props.modelValue);
});

function overflowClipRect(el: HTMLElement): DOMRect {
  let node = el.parentElement;
  while (node) {
    const overflowY = getComputedStyle(node).overflowY;
    if (overflowY === 'auto' || overflowY === 'scroll' || overflowY === 'hidden') {
      return node.getBoundingClientRect();
    }
    node = node.parentElement;
  }
  return new DOMRect(0, 0, window.innerWidth, window.innerHeight);
}

function updatePlacement() {
  const el = root.value;
  if (!el) return;
  const trigger = el.getBoundingClientRect();
  const clip = overflowClipRect(el);
  const spaceBelow = clip.bottom - trigger.bottom;
  const spaceAbove = trigger.top - clip.top;
  const optionCount = Math.max(props.options.length, 1);
  const needed = Math.min(240, optionCount * 36 + 12) + 6;
  placement.value = spaceBelow < needed && spaceAbove > spaceBelow ? 'top' : 'bottom';
}

function toggle() {
  if (!open.value) {
    updatePlacement();
    open.value = true;
    void nextTick(() => {
      const active = list.value?.querySelector<HTMLElement>('[data-active="true"]');
      active?.scrollIntoView({ block: 'nearest' });
    });
    return;
  }
  open.value = false;
}

function pick(value: string | number) {
  emit('update:modelValue', value);
  open.value = false;
}

function onDocPointer(event: PointerEvent) {
  if (!open.value || !root.value) return;
  if (event.target instanceof Node && !root.value.contains(event.target)) {
    open.value = false;
  }
}

function onKey(event: KeyboardEvent) {
  if (!open.value) return;
  if (event.key === 'Escape') {
    open.value = false;
  }
}

onMounted(() => {
  document.addEventListener('pointerdown', onDocPointer);
  document.addEventListener('keydown', onKey);
});

onUnmounted(() => {
  document.removeEventListener('pointerdown', onDocPointer);
  document.removeEventListener('keydown', onKey);
});
</script>

<template>
  <div
    ref="root"
    class="pt-soft-select"
    :data-open="open ? 'true' : 'false'"
    :data-placement="placement"
  >
    <button
      type="button"
      class="pt-soft-select-trigger"
      :aria-label="ariaLabel"
      :aria-expanded="open"
      aria-haspopup="listbox"
      @click="toggle"
    >
      <span class="pt-soft-select-value">{{ selectedLabel }}</span>
      <ChevronDown :size="16" :stroke-width="2" class="pt-soft-select-caret" />
    </button>

    <div
      v-if="open"
      ref="list"
      class="pt-soft-select-panel"
      role="listbox"
      :aria-label="ariaLabel"
    >
      <button
        v-for="option in options"
        :key="String(option.value)"
        type="button"
        class="pt-soft-select-option"
        role="option"
        :aria-selected="option.value === modelValue"
        :data-active="option.value === modelValue ? 'true' : 'false'"
        @click="pick(option.value)"
      >
        {{ option.label }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.pt-soft-select {
  position: relative;
  width: 100%;
}
.pt-soft-select-trigger {
  display: flex;
  width: 100%;
  height: 40px;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  border: 1px solid color-mix(in oklab, var(--border) 80%, transparent);
  border-radius: 12px;
  padding: 0 12px;
  background: var(--card);
  color: var(--foreground);
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition: border-color 0.12s ease, background-color 0.12s ease;
}
.pt-soft-select-trigger:hover,
.pt-soft-select[data-open='true'] .pt-soft-select-trigger {
  border-color: color-mix(in oklab, var(--primary) 40%, var(--border));
}
.pt-soft-select-value {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-align: left;
}
.pt-soft-select-caret {
  flex: none;
  color: var(--muted-foreground);
  transition: transform 0.12s ease;
}
.pt-soft-select[data-open='true'] .pt-soft-select-caret {
  transform: rotate(180deg);
}
.pt-soft-select-panel {
  position: absolute;
  z-index: 40;
  left: 0;
  right: 0;
  top: calc(100% + 6px);
  max-height: 240px;
  overflow: auto;
  padding: 6px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 12px;
  background: var(--card);
  box-shadow:
    0 12px 28px -16px color-mix(in oklab, var(--foreground) 35%, transparent),
    0 1px 0 color-mix(in oklab, var(--foreground) 4%, transparent);
}
.pt-soft-select[data-placement='top'] .pt-soft-select-panel {
  top: auto;
  bottom: calc(100% + 6px);
}
.pt-soft-select-option {
  display: flex;
  width: 100%;
  min-height: 36px;
  align-items: center;
  border: 0;
  border-radius: 10px;
  padding: 0 10px;
  background: transparent;
  color: var(--foreground);
  font-size: 0.8125rem;
  font-weight: 550;
  text-align: left;
  cursor: pointer;
}
.pt-soft-select-option:hover {
  background: color-mix(in oklab, var(--muted) 75%, transparent);
}
.pt-soft-select-option[data-active='true'] {
  background: color-mix(in oklab, var(--primary) 14%, transparent);
  color: var(--primary);
  font-weight: 650;
}
.pt-soft-select-option[data-active='true']:hover {
  background: color-mix(in oklab, var(--primary) 18%, transparent);
}
</style>
