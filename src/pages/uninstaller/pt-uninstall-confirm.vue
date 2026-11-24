<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { FolderOpen, ShieldAlert, X } from '@lucide/vue';
import { formatBytes } from '@/lib/utils/format';
import PtProgramIcon from '@/pages/uninstaller/pt-program-icon.vue';
import PtMiddleEllipsis from '@/components/custom/pt-middle-ellipsis.vue';
import { UninstallerService } from '@/lib/services/api-services';
import { trapDialogFocus } from '@/lib/services/dialog-focus-service';
import {
  type InstalledProgram,
  type LeftoverItem,
  type UninstallOptions,
} from '@/lib/models/uninstaller';

const open = defineModel<boolean>('open', { default: false });

const props = defineProps<{
  programs: InstalledProgram[];
  icons: Record<string, string | null>;
  isAdmin: boolean;
}>();

const emit = defineEmits<{
  confirm: [options: UninstallOptions];
  force: [programId: string];
  restartAdmin: [];
}>();

const { t } = useI18n();

const quiet = ref(true);
const scanLeftovers = ref(true);
const createRestorePoint = ref(false);
const panel = ref<HTMLElement | null>(null);
let releaseFocus: (() => void) | null = null;

watch(
  panel,
  (el) => {
    releaseFocus?.();
    releaseFocus = el ? trapDialogFocus(el, onCancel) : null;
  },
  { flush: 'post' }
);
onBeforeUnmount(() => releaseFocus?.());

/** Data folders of the single program being removed (app data and profile dot-folders). */
const related = ref<LeftoverItem[]>([]);
const relatedLoading = ref(false);
let relatedRequest = 0;

async function loadRelated(programId: string) {
  const request = ++relatedRequest;
  related.value = [];
  relatedLoading.value = true;
  try {
    const preview = await UninstallerService.previewRelatedData(programId);
    if (request === relatedRequest) related.value = preview.items;
  } catch {
    // Informational only: the leftover scan after the uninstall still finds these folders.
    if (request === relatedRequest) related.value = [];
  } finally {
    if (request === relatedRequest) relatedLoading.value = false;
  }
}

watch(open, (isOpen) => {
  if (!isOpen) {
    relatedRequest++;
    return;
  }
  // Only pre-tick quick mode when at least one selected installer supports it.
  quiet.value = props.programs.some((p) => p.quietSupported);
  scanLeftovers.value = true;
  createRestorePoint.value = false;
  const only = props.programs.length === 1 ? props.programs[0] : null;
  if (only) {
    void loadRelated(only.id);
  } else {
    related.value = [];
    relatedLoading.value = false;
  }
});

const relatedBytes = computed(() => related.value.reduce((n, i) => n + i.bytes, 0));

const title = computed(() =>
  props.programs.length === 1
    ? t('uninstaller.confirm.titleOne', { name: props.programs[0]?.name ?? '' })
    : t('uninstaller.confirm.titleMany', { count: props.programs.length }, props.programs.length)
);

const totalBytes = computed(() => props.programs.reduce((n, p) => n + p.sizeBytes, 0));
const anyQuiet = computed(() => props.programs.some((p) => p.quietSupported));
const single = computed(() => (props.programs.length === 1 ? props.programs[0] : null));
const canForce = computed(() => single.value?.source === 'desktop');
const needsAdminHint = computed(() => !props.isAdmin && props.programs.some((p) => p.machineWide));

function onCancel() {
  open.value = false;
}

function onConfirm() {
  open.value = false;
  emit('confirm', {
    quiet: quiet.value,
    scanLeftovers: scanLeftovers.value,
    createRestorePoint: createRestorePoint.value && props.isAdmin,
  });
}

function onForce() {
  if (!single.value) return;
  open.value = false;
  emit('force', single.value.id);
}
</script>

<template>
  <div v-if="open" class="confirm-root" role="dialog" aria-modal="true" aria-labelledby="pt-uninstall-confirm-title">
    <div class="confirm-overlay" @click="onCancel" />
    <div ref="panel" class="confirm-panel">
      <header class="confirm-head">
        <div>
          <h2 id="pt-uninstall-confirm-title">{{ title }}</h2>
          <p class="lead">{{ t('uninstaller.confirm.body') }}</p>
        </div>
        <button type="button" class="icon-btn" :aria-label="t('common.cancel')" @click="onCancel">
          <X :size="16" :stroke-width="2" />
        </button>
      </header>

      <div class="confirm-body">
        <ul class="program-list">
          <li v-for="program in programs" :key="program.id">
            <PtProgramIcon
              :name="program.name"
              :src="icons[program.id]"
              :plated="program.source === 'store'"
              :size="30"
            />
            <span class="program-copy">
              <strong>{{ program.name }}</strong>
              <small>{{ [program.publisher, program.version].filter(Boolean).join(' · ') }}</small>
            </span>
            <span class="program-size">{{
              program.sizeBytes ? formatBytes(program.sizeBytes) : t('uninstaller.unknownSize')
            }}</span>
          </li>
        </ul>
        <p v-if="programs.length > 1 && totalBytes" class="total">
          {{ t('uninstaller.selectedSize') }} <strong>{{ formatBytes(totalBytes) }}</strong>
        </p>

        <section v-if="relatedLoading || related.length" class="related" aria-live="polite">
          <header class="related-head">
            <strong>{{ t('uninstaller.confirm.relatedTitle') }}</strong>
            <span v-if="relatedLoading" class="related-spinner" aria-hidden="true" />
            <span v-else-if="relatedBytes" class="related-total">{{ formatBytes(relatedBytes) }}</span>
          </header>
          <p v-if="relatedLoading" class="related-note">{{ t('uninstaller.confirm.relatedLoading') }}</p>
          <template v-else>
            <ul class="related-list">
              <li v-for="item in related" :key="item.id">
                <FolderOpen :size="15" :stroke-width="1.8" aria-hidden="true" />
                <PtMiddleEllipsis class="related-path" :text="item.location" />
                <span
                  v-if="item.userData"
                  class="related-badge"
                  :title="t('uninstaller.leftovers.userDataHint')"
                >{{ t('uninstaller.leftovers.userData') }}</span>
                <span class="related-size">{{ item.bytes ? formatBytes(item.bytes) : '' }}</span>
              </li>
            </ul>
            <p class="related-note">{{ t('uninstaller.confirm.relatedHint') }}</p>
          </template>
        </section>

        <section v-if="needsAdminHint" class="admin-hint">
          <ShieldAlert :size="16" :stroke-width="1.9" aria-hidden="true" />
          <div>
            <p>{{ t('uninstaller.confirm.adminHint') }}</p>
            <button type="button" class="text-btn" @click="emit('restartAdmin')">
              {{ t('uninstaller.confirm.restartAdmin') }}
            </button>
          </div>
        </section>

        <section class="options">
          <label class="option" :class="{ disabled: !anyQuiet }">
            <input v-model="quiet" type="checkbox" :disabled="!anyQuiet" />
            <span>
              <strong>{{ t('uninstaller.confirm.quiet') }}</strong>
              <small>{{ t('uninstaller.confirm.quietHint') }}</small>
            </span>
          </label>
          <label class="option">
            <input v-model="scanLeftovers" type="checkbox" />
            <span>
              <strong>{{ t('uninstaller.confirm.scanLeftovers') }}</strong>
              <small>{{ t('uninstaller.confirm.scanLeftoversHint') }}</small>
            </span>
          </label>
          <label class="option" :class="{ disabled: !isAdmin }">
            <input v-model="createRestorePoint" type="checkbox" :disabled="!isAdmin" />
            <span>
              <strong>{{ t('uninstaller.confirm.restorePoint') }}</strong>
              <small>{{
                isAdmin
                  ? t('uninstaller.confirm.restorePointHint')
                  : t('uninstaller.confirm.restorePointNeedsAdmin')
              }}</small>
            </span>
          </label>
        </section>

        <section v-if="canForce" class="force">
          <ShieldAlert :size="16" :stroke-width="1.9" aria-hidden="true" />
          <div>
            <strong>{{ t('uninstaller.confirm.forceTitle') }}</strong>
            <p>{{ t('uninstaller.confirm.forceBody') }}</p>
            <button type="button" class="text-btn" @click="onForce">
              {{ t('uninstaller.confirm.force') }}
            </button>
          </div>
        </section>
      </div>

      <footer class="confirm-foot">
        <button type="button" class="pt-btn" data-autofocus @click="onCancel">{{ t('uninstaller.confirm.back') }}</button>
        <button type="button" class="pt-btn pt-btn-primary pt-btn-danger" @click="onConfirm">
          {{ t('uninstaller.confirm.start') }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.confirm-root {
  position: fixed;
  inset: 0;
  z-index: 80;
  display: grid;
  place-items: center;
  padding: 16px;
}
.confirm-overlay {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 32%);
}
.confirm-panel {
  position: relative;
  z-index: 1;
  display: flex;
  width: min(600px, calc(100vw - 32px));
  max-height: min(820px, calc(100vh - 32px));
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--card);
  box-shadow: 0 25px 50px -12px rgb(0 0 0 / 28%);
}
.confirm-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 16px 18px 10px;
}
.confirm-head h2 {
  margin: 0;
  font-size: 1.05rem;
  font-weight: 700;
}
.lead {
  margin: 6px 0 0;
  color: var(--muted-foreground);
  font-size: 0.8125rem;
  line-height: 1.45;
}
.icon-btn {
  display: grid;
  width: 32px;
  height: 32px;
  flex: none;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}
.icon-btn:hover {
  background: var(--muted);
  color: var(--foreground);
}
.confirm-body {
  min-height: 0;
  flex: 1;
  overflow: auto;
  padding: 0 18px 12px;
}
.program-list {
  list-style: none;
  margin: 0;
  padding: 4px;
  max-height: 200px;
  overflow: auto;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 12px;
  background: var(--surface-soft);
}
.program-list li {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  padding: 6px 8px;
  border-radius: 10px;
}
.program-copy {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
}
.program-copy strong {
  overflow: hidden;
  font-size: 0.8125rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.program-copy small {
  overflow: hidden;
  color: var(--muted-foreground);
  font-size: 0.6875rem;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.program-size {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 650;
}
.total {
  margin: 8px 2px 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
}
.total strong {
  color: var(--foreground);
}
.options {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 14px;
}
.option {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 10px;
  cursor: pointer;
}
.option:hover:not(.disabled) {
  background: color-mix(in oklab, var(--muted) 45%, transparent);
}
.option.disabled {
  cursor: default;
  opacity: 0.6;
}
.option input {
  margin-top: 3px;
  flex: none;
}
.option span {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.option strong {
  font-size: 0.8125rem;
  font-weight: 650;
}
.option small {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.related {
  margin-top: 12px;
  padding: 10px 12px;
  border: 1px solid color-mix(in oklab, var(--border) 75%, transparent);
  border-radius: 12px;
}
.related-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 0.8125rem;
}
.related-total {
  color: var(--muted-foreground);
  font-size: 0.75rem;
  font-weight: 650;
}
.related-spinner {
  width: 14px;
  height: 14px;
  border: 2px solid color-mix(in oklab, var(--primary) 25%, transparent);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: related-spin 0.8s linear infinite;
}
@keyframes related-spin {
  to {
    transform: rotate(360deg);
  }
}
.related-list {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
}
.related-list li {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  color: var(--muted-foreground);
}
.related-list li > svg {
  flex: none;
}
.related-path {
  min-width: 0;
  flex: 1;
  color: var(--foreground);
  font-size: 0.75rem;
}
.related-badge {
  flex: none;
  padding: 1px 7px;
  border-radius: 999px;
  background: color-mix(in oklab, var(--primary) 12%, transparent);
  color: color-mix(in oklab, var(--primary) 80%, var(--foreground));
  font-size: 0.625rem;
  font-weight: 700;
}
.related-size {
  flex: none;
  min-width: 56px;
  font-size: 0.75rem;
  font-weight: 650;
  text-align: right;
}
.related-note {
  margin: 6px 0 0;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.admin-hint {
  display: flex;
  gap: 10px;
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: color-mix(in oklab, var(--primary) 8%, var(--card));
}
.admin-hint svg {
  flex: none;
  margin-top: 2px;
  color: var(--primary);
}
.admin-hint p {
  margin: 0 0 6px;
  font-size: 0.75rem;
  line-height: 1.4;
}
.force {
  display: flex;
  gap: 10px;
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: color-mix(in oklab, var(--warning) 12%, var(--card));
  color: var(--foreground);
}
.force svg {
  flex: none;
  margin-top: 2px;
  color: var(--warning);
}
.force strong {
  font-size: 0.8125rem;
}
.force p {
  margin: 4px 0 6px;
  color: var(--muted-foreground);
  font-size: 0.75rem;
  line-height: 1.4;
}
.text-btn {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--primary);
  font-size: 0.75rem;
  font-weight: 650;
  cursor: pointer;
}
.text-btn:hover {
  text-decoration: underline;
}
.confirm-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 18px 16px;
  border-top: 1px solid color-mix(in oklab, var(--border) 70%, transparent);
}
</style>
