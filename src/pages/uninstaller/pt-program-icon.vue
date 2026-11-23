<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { programHue, programInitials } from '@/lib/models/uninstaller';

const props = withDefaults(
  defineProps<{
    name: string;
    src?: string | null;
    /** Store logos are designed for a coloured tile behind them (many are white glyphs). */
    plated?: boolean;
    size?: number;
  }>(),
  { src: null, plated: false, size: 32 }
);

const failed = ref(false);
watch(
  () => props.src,
  () => {
    failed.value = false;
  }
);

const showImage = computed(() => !!props.src && !failed.value);
const style = computed(() => ({
  '--hue': programHue(props.name),
  '--size': `${props.size}px`,
}));
</script>

<template>
  <span class="icon" :class="{ plated: plated && showImage, initials: !showImage }" :style="style" aria-hidden="true">
    <img v-if="showImage" :src="src ?? undefined" alt="" draggable="false" @error="failed = true" />
    <template v-else>{{ programInitials(name) }}</template>
  </span>
</template>

<style scoped>
.icon {
  display: grid;
  width: var(--size);
  height: var(--size);
  flex: none;
  place-items: center;
  overflow: hidden;
  border-radius: calc(var(--size) * 0.28);
}
.icon img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  image-rendering: auto;
}
.icon.plated {
  padding: calc(var(--size) * 0.12);
  background: color-mix(in oklab, var(--foreground) 62%, var(--card));
}
.icon.initials {
  background: oklch(0.9 0.05 var(--hue));
  color: oklch(0.38 0.09 var(--hue));
  font-size: calc(var(--size) * 0.34);
  font-weight: 750;
}
:global([data-theme='dark']) .icon.initials {
  background: oklch(0.34 0.06 var(--hue));
  color: oklch(0.88 0.07 var(--hue));
}
</style>
