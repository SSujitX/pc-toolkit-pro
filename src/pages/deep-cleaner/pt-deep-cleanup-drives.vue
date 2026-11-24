<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { Check, ChevronDown, HardDrive } from '@lucide/vue';
import { formatBytes } from '@/lib/utils/format';
import type { DeepCleanupDrive } from '@/lib/models/deep-cleaner';

const props = defineProps<{
  drives: DeepCleanupDrive[];
  disabled?: boolean;
}>();

const emit = defineEmits<{
  toggle: [letter: string, selected: boolean];
}>();

const { t } = useI18n();
const open = ref(false);
const root = ref<HTMLElement | null>(null);

const selectedLetters = computed(() =>
  props.drives.filter((d) => d.selected).map((d) => `${d.letter}:`)
);
const buttonLabel = computed(() =>
  selectedLetters.value.length
    ? t('deepCleaner.drives.button', { drives: selectedLetters.value.join(', ') })
    : t('deepCleaner.drives.buttonEmpty')
);

function driveName(drive: DeepCleanupDrive): string {
  const name = drive.label || t('deepCleaner.drives.localDisk');
  return `${name} (${drive.letter}:)`;
}

function onToggle(drive: DeepCleanupDrive) {
  if (drive.system || props.disabled) return;
  emit('toggle', drive.letter, !drive.selected);
}

function onDocumentPointer(event: PointerEvent) {
  if (open.value && root.value && !root.value.contains(event.target as Node)) open.value = false;
}

function onKey(event: KeyboardEvent) {
  if (event.key === 'Escape' && open.value) {
    open.value = false;
    event.stopPropagation();
  }
}

onMounted(() => document.addEventListener('pointerdown', onDocumentPointer, true));
onBeforeUnmount(() => document.removeEventListener('pointerdown', onDocumentPointer, true));
</script>

<template>
  <div ref="root" class="drives" @keydown="onKey">
    <button
      type="button"
      class="pt-btn drives-btn"
      :disabled="disabled || !drives.length"
      :aria-expanded="open"
      aria-haspopup="true"
      :title="t('deepCleaner.drives.hint')"
      @click="open = !open"
    >
      <HardDrive :size="15" :stroke-width="1.9" aria-hidden="true" />
      <span>{{ buttonLabel }}</span>
      <ChevronDown :size="14" :stroke-width="2" aria-hidden="true" class="chev" :class="{ up: open }" />
    </button>

    <div v-if="open" class="menu" role="group" :aria-label="t('deepCleaner.drives.title')">
      <p class="menu-title">{{ t('deepCleaner.drives.title') }}</p>
      <ul>
        <li v-for="drive in drives" :key="drive.letter">
          <button
            type="button"
            class="drive-row"
            role="menuitemcheckbox"
            :aria-checked="drive.selected"
            :class="{ locked: drive.system, on: drive.selected }"
            :disabled="drive.system || disabled"
            @click="onToggle(drive)"
          >
            <span class="box" aria-hidden="true">
              <Check v-if="drive.selected" :size="12" :stroke-width="3" />
            </span>
            <HardDrive :size="15" :stroke-width="1.8" aria-hidden="true" class="drive-icon" />
            <span class="drive-copy">
              <strong>{{ driveName(drive) }}</strong>
              <small>{{
                t('deepCleaner.drives.space', {
                  free: formatBytes(drive.freeBytes),
                  total: formatBytes(drive.totalBytes),
                })
              }}</small>
            </span>
            <span v-if="drive.system" class="always">{{ t('deepCleaner.drives.always') }}</span>
          </button>
        </li>
      </ul>
      <p class="menu-hint">{{ t('deepCleaner.drives.hint') }}</p>
    </div>
  </div>
</template>

<style scoped>
.drives {
  position: relative;
}
.drives-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.chev {
  color: var(--muted-foreground);
  transition: transform 160ms ease;
}
.chev.up {
  transform: rotate(180deg);
}
.menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 40;
  width: 300px;
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--card);
  box-shadow: 0 12px 32px -8px rgb(0 0 0 / 22%);
  animation: menu-in 140ms ease-out;
}
@keyframes menu-in {
  from {
    opacity: 0;
  }
}
.menu-title {
  margin: 4px 8px 6px;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  font-weight: 650;
}
.menu ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.drive-row {
  display: flex;
  width: 100%;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  padding: 6px 8px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--foreground);
  text-align: left;
  cursor: pointer;
}
.drive-row:hover:not(:disabled) {
  background: color-mix(in oklab, var(--primary) 10%, transparent);
}
.drive-row:disabled {
  cursor: default;
}
.drive-row.locked {
  opacity: 0.85;
}
.box {
  display: grid;
  width: 16px;
  height: 16px;
  flex: none;
  place-items: center;
  border: 1.5px solid var(--border);
  border-radius: 4px;
  color: var(--primary-foreground);
}
.drive-row.on .box {
  border-color: var(--primary);
  background: var(--primary);
}
.drive-icon {
  flex: none;
  color: var(--muted-foreground);
}
.drive-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
}
.drive-copy strong {
  overflow: hidden;
  font-size: 0.8125rem;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.drive-copy small {
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.always {
  flex: none;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
}
.menu-hint {
  margin: 6px 8px 4px;
  padding-top: 8px;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  line-height: 1.4;
}
</style>
