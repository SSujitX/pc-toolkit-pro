<script setup lang="ts">
import { computed } from 'vue';

/** Progress ring: determinate arc (smoothly tweened via a registered custom property) plus an
 *  optional continuously rotating activity sweep. Pure CSS — no SVG markup. */
const props = withDefaults(
  defineProps<{
    percent: number;
    size?: number;
    thickness?: number;
    spinning?: boolean;
  }>(),
  { size: 200, thickness: 10, spinning: false }
);

const style = computed(() => ({
  '--care-pct': Math.max(0, Math.min(100, props.percent)),
  '--ring-size': `${props.size}px`,
  '--ring-thickness': `${props.thickness}px`,
}));
</script>

<template>
  <div class="care-ring" :style="style" role="progressbar" :aria-valuenow="Math.round(percent)" aria-valuemin="0" aria-valuemax="100">
    <div class="care-ring-track" />
    <div class="care-ring-arc" />
    <div v-if="spinning" class="care-ring-sweep" />
    <div class="care-ring-center">
      <slot />
    </div>
  </div>
</template>

<style scoped>
@property --care-pct {
  syntax: '<number>';
  inherits: true;
  initial-value: 0;
}

.care-ring {
  position: relative;
  width: var(--ring-size);
  height: var(--ring-size);
  flex: none;
  border-radius: 50%;
  isolation: isolate;
  transition: --care-pct 420ms cubic-bezier(0.22, 1, 0.36, 1);
}
.care-ring-track,
.care-ring-arc,
.care-ring-sweep {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  -webkit-mask: radial-gradient(
    farthest-side,
    transparent calc(100% - var(--ring-thickness)),
    #000 calc(100% - var(--ring-thickness) + 0.5px)
  );
  mask: radial-gradient(
    farthest-side,
    transparent calc(100% - var(--ring-thickness)),
    #000 calc(100% - var(--ring-thickness) + 0.5px)
  );
  pointer-events: none;
}
.care-ring-track {
  background: color-mix(in oklab, var(--primary) 14%, var(--muted));
}
.care-ring-arc {
  background: conic-gradient(
    from 0deg,
    var(--primary) calc(var(--care-pct) * 1%),
    transparent calc(var(--care-pct) * 1%)
  );
}
/* Activity sweep on top of the arc: always rotating while work is in flight. */
.care-ring-sweep {
  background: conic-gradient(
    from 0deg,
    color-mix(in oklab, var(--primary-foreground) 55%, transparent) 0deg,
    transparent 70deg
  );
  mix-blend-mode: soft-light;
  animation: care-ring-spin 1.1s linear infinite;
}
.care-ring-center {
  position: absolute;
  inset: calc(var(--ring-thickness) + 8px);
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: color-mix(in oklab, var(--primary) 6%, var(--card));
  text-align: center;
}

@keyframes care-ring-spin {
  from {
    rotate: 0deg;
  }
  to {
    rotate: 360deg;
  }
}

@media (prefers-reduced-motion: reduce) {
  .care-ring {
    transition: none;
  }
  .care-ring-sweep {
    animation-duration: 3s;
  }
}
</style>
