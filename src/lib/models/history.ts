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
