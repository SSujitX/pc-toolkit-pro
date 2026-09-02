<script setup lang="ts">
withDefaults(
  defineProps<{
    title: string;
    subtitle?: string;
    contentMode?: 'document' | 'workspace';
  }>(),
  { subtitle: undefined, contentMode: 'document' }
);
</script>

<template>
  <section class="pt-page-shell" :data-mode="contentMode">
    <header class="pt-page-header" data-tauri-drag-region>
      <div class="pt-page-heading" data-tauri-drag-region>
        <h1 data-tauri-drag-region>{{ title }}</h1>
        <p v-if="subtitle" data-tauri-drag-region>{{ subtitle }}</p>
      </div>
      <div class="pt-page-actions" data-tauri-drag-region="false">
        <slot name="actions" />
      </div>
    </header>
    <div class="pt-page-content" :class="`pt-page-content--${contentMode}`">
      <slot />
    </div>
    <footer v-if="$slots.footer" class="pt-page-footer">
      <slot name="footer" />
    </footer>
  </section>
</template>

<style scoped>
.pt-page-shell {
  display: flex;
  width: 100%;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  /* Leave room under the full-width titlebar drag strip */
  padding: var(--layout-titlebar-height) var(--layout-page-padding-inline) 0;
}
.pt-page-header {
  display: grid;
  min-height: var(--layout-page-header-height);
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: start;
  gap: 16px;
  flex: none;
  padding-top: 4px;
  cursor: default;
}
.pt-page-heading h1 {
  margin: 0;
  font-size: 28px;
  font-weight: 700;
  letter-spacing: -0.035em;
  line-height: 1.15;
  user-select: none;
}
.pt-page-heading p {
  margin: 4px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  user-select: none;
}
.pt-page-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  justify-content: flex-end;
  justify-self: end;
  padding-top: 4px;
}
.pt-page-actions:empty {
  display: none;
}

@media (max-width: 960px) {
  .pt-page-header {
    grid-template-columns: minmax(0, 1fr);
  }
  .pt-page-actions {
    width: 100%;
    justify-content: flex-end;
  }
}
.pt-page-content {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 14px;
  overflow: auto;
  padding-bottom: 20px;
}
.pt-page-content--workspace {
  overflow: hidden;
}
.pt-page-footer {
  display: flex;
  flex: none;
  justify-content: flex-end;
  gap: 10px;
  min-height: var(--layout-action-bar-height);
  align-items: center;
  padding-bottom: 14px;
}
</style>
