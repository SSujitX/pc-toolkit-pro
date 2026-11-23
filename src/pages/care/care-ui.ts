import type { Component } from 'vue';
import {
  Download,
  EyeOff,
  HardDrive,
  Link2Off,
  PackageCheck,
  Rocket,
  ShieldCheck,
  ShieldHalf,
  Sparkles,
  Trash2,
  Wrench,
} from '@lucide/vue';
import type { CareCategoryId, CareItem } from '@/lib/models/care';

type Translate = (key: string, params?: Record<string, unknown>, plural?: number) => string;
type HasKey = (key: string) => boolean;

/** Page-local icon per Care category. */
export const CARE_CATEGORY_ICONS: Record<CareCategoryId, Component> = {
  privacy: EyeOff,
  junk: Trash2,
  shortcuts: Link2Off,
  registry: Wrench,
  startup: Rocket,
  security: ShieldHalf,
  protection: ShieldCheck,
  windowsUpdate: Download,
  software: PackageCheck,
  diskHealth: HardDrive,
  diskOptimize: Sparkles,
};

export function categoryName(id: CareCategoryId, t: Translate): string {
  return t(`care.categories.${id}`);
}

/** Item title: locale key (with params) when it exists, otherwise the literal title. */
export function itemTitle(item: CareItem, t: Translate, te: HasKey): string {
  if (item.titleKey && te(item.titleKey)) {
    const n = Number(item.params?.count);
    return Number.isFinite(n) ? t(item.titleKey, item.params, n) : t(item.titleKey, item.params);
  }
  return item.title ?? item.titleKey ?? item.id;
}

export function itemDetail(item: CareItem, t: Translate, te: HasKey): string {
  if (item.detailKey && te(item.detailKey)) {
    const n = Number(item.params?.count);
    return Number.isFinite(n) ? t(item.detailKey, item.params, n) : t(item.detailKey, item.params);
  }
  return item.detail ?? '';
}

/** Typed reason → locale text (unknown codes fall back to the code itself). */
export function reasonText(reason: string | null, t: Translate, te: HasKey): string {
  if (!reason) return '';
  const key = `care.engine.reasons.${reason}`;
  return te(key) ? t(key) : reason;
}

export function actionLabel(action: string, t: Translate, te: HasKey): string {
  const key = `care.engine.actions.${action}`;
  return te(key) ? t(key) : t('care.engine.actions.open');
}

/** "C:\Users\…\Cache\f_0001" — keeps both ends of long paths visible. */
export function middleEllipsis(text: string, max = 72): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep * 0.45);
  const tail = keep - head;
  return `${text.slice(0, head)}…${text.slice(text.length - tail)}`;
}

export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}
