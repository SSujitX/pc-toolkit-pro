<script setup lang="ts">
import { ApplicationWindowService } from '@/lib/services/application-window-service';

async function minimize() {
  await ApplicationWindowService.minimize();
}
async function toggleMax() {
  await ApplicationWindowService.toggleMaximize();
}
async function close() {
  await ApplicationWindowService.closeOrHide();
}
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <div class="controls">
      <button type="button" class="ctrl" aria-label="Minimize" @click="minimize">─</button>
      <button type="button" class="ctrl" aria-label="Maximize" @click="toggleMax">□</button>
      <button type="button" class="ctrl close" aria-label="Close" @click="close">✕</button>
    </div>
  </div>
</template>

<style scoped>
.titlebar {
  position: absolute;
  top: 0;
  right: 0;
  z-index: 40;
  height: 36px;
  width: var(--window-controls-width);
  display: flex;
  justify-content: flex-end;
}
.controls {
  display: flex;
  height: 100%;
}
.ctrl {
  width: 46px;
  border: 0;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
  font-size: 12px;
}
.ctrl:hover {
  background: var(--muted);
  color: var(--foreground);
}
.ctrl.close:hover {
  background: var(--destructive);
  color: white;
}
</style>
