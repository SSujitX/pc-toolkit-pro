export type HistoryOutcome =
  | 'completed'
  | 'completedWithWarnings'
  | 'cancelled'
  | 'failed';

export interface HistoryRecord {
  id: string;
  category: string;
  titleKey: string;
  summary: string;
  startedAtMs: number;
  finishedAtMs: number;
  outcome: HistoryOutcome;
  plannedBytes?: number | null;
  resultBytes?: number | null;
  selectedItemCount: number;
  affectedItemCount: number;
  failedItemCount: number;
  detailLines: string[];
  domain: string;
  action: string;
  success: boolean;
  detail?: string | null;
  createdAt: string;
}

/** History retention (`history-settings.json`); Core clamps `maxRecords` to 1–1000. */
export interface HistorySettings {
  maxRecords: number;
}

export const HISTORY_LIMIT_MIN = 1;
export const HISTORY_LIMIT_MAX = 1000;
export const HISTORY_LIMIT_PRESETS = [10, 20, 50, 100, 200, 500] as const;
