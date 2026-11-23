import { defineStore } from 'pinia';
import { MemoryCleanerService, UninstallerService } from '@/lib/services/api-services';
import {
  programFromRemoved,
  leftoverGroup,
  matchesFilter,
  matchesQuery,
  sortPrograms,
  type InstalledProgram,
  type LeftoverArea,
  type LeftoverCleanResult,
  type LeftoverGroup,
  type LeftoverItem,
  type LeftoverItemStatus,
  type LeftoverScan,
  type ListingStage,
  type ProgramFilter,
  type ProgramSort,
  type RemovedProgram,
  type UninstallOptions,
  type UninstallReport,
  type UninstallerProgress,
} from '@/lib/models/uninstaller';
import { PAGE_IDS } from '@/lib/models/application-shell';
import { isCancelledError } from '@/lib/utils/errors';
import { useAppStore } from './app-store';
import { useHistoryStore } from './history-store';

export type UninstallerView = 'list' | 'leftovers' | 'summary';

export type UninstallerOperation = 'idle' | 'listing' | 'uninstalling' | 'scanning' | 'cleaning';

/** One finished item in the live cleaning log shown by the progress dialog. */
export interface CleanLogEntry {
  key: string;
  location: string;
  status: LeftoverItemStatus;
}

/** Live state of the program listing shown on the loading card. */
export interface ListingProgress {
  stage: ListingStage;
  current: number;
  total: number;
  found: number;
  /** Program being read right now. */
  name: string;
  startedAt: number;
}

const ICON_BATCH = 24;
const LISTING_FEED_LIMIT = 6;
const CLEAN_LOG_LIMIT = 200;

/** In-flight quiet inventory refresh (module scope: a promise is not UI state). */
let refreshInFlight: Promise<void> | null = null;

function emptyCleanResult(): LeftoverCleanResult {
  return {
    removedCount: 0,
    freedBytes: 0,
    pendingRebootCount: 0,
    skippedNeedAdminCount: 0,
    failedCount: 0,
    registryBackupSaved: false,
    items: [],
  };
}

export const useUninstallerStore = defineStore('uninstaller', {
  state: () => ({
    programs: [] as InstalledProgram[],
    /** Program id → PNG data URL (`null` = no icon, fall back to initials). */
    icons: {} as Record<string, string | null>,
    isAdmin: false,
    storeUnavailable: false,
    /** Programs removed by another uninstaller whose leftovers can still be scanned. */
    removedElsewhere: [] as RemovedProgram[],
    loaded: false,
    operation: 'idle' as UninstallerOperation,
    progress: null as UninstallerProgress | null,
    listing: null as ListingProgress | null,
    /** Most recently read programs (newest first) for the loading card feed. */
    listingFeed: [] as Array<{ id: string; name: string }>,
    /** Programs in the running batch (snapshot, so names/icons survive list refreshes). */
    queue: [] as InstalledProgram[],
    /** 1-based position in a batch uninstall. */
    queueIndex: 0,
    restorePointRequested: false,
    /** Scanner areas finished for the program currently being scanned. */
    areasDone: [] as LeftoverArea[],
    cleanLog: [] as CleanLogEntry[],
    /** Items to clean across the whole batch (all programs). */
    cleanTotal: 0,
    /** Items finished in earlier programs of the batch; added to each program's `current`. */
    cleanOffset: 0,
    /** Items processed so far in the batch (never capped by the log limit). */
    cleanProcessed: 0,
    /** Items that did not end as removed / not found (never capped by the log limit). */
    cleanIssues: 0,
    /** Monotonic key source for log rows: unique across the cap and across scans. */
    cleanSeq: 0,
    /** Set by cancel(); stops multi-program loops before the next program starts. */
    cancelRequested: false,
    /** A quiet background inventory refresh is running (new operations wait for it). */
    refreshing: false,
    query: '',
    filter: 'all' as ProgramFilter,
    sort: 'name' as ProgramSort,
    showRuntimes: false,
    selectedIds: [] as string[],
    view: 'list' as UninstallerView,
    reports: [] as UninstallReport[],
    scans: [] as LeftoverScan[],
    cleanResult: null as LeftoverCleanResult | null,
    /** True when the leftovers came from a forced removal (uninstaller not run). */
    forced: false,
  }),
  getters: {
    busy(state): boolean {
      return state.operation !== 'idle';
    },
    currentProgram(state): InstalledProgram | null {
      return state.queue[Math.max(0, state.queueIndex - 1)] ?? null;
    },
    visiblePrograms(state): InstalledProgram[] {
      const filtered = state.programs.filter(
        (p) =>
          (state.showRuntimes || !p.isRuntime) &&
          matchesFilter(p, state.filter) &&
          matchesQuery(p, state.query)
      );
      return sortPrograms(filtered, state.sort);
    },
    /** O(1) membership for row rendering (the list can hold hundreds of programs). */
    selectedIdSet(state): Set<string> {
      return new Set(state.selectedIds);
    },
    selectedPrograms(state): InstalledProgram[] {
      const ids = new Set(state.selectedIds);
      return state.programs.filter((p) => ids.has(p.id));
    },
    selectedBytes(): number {
      return this.selectedPrograms.reduce((n: number, p: InstalledProgram) => n + p.sizeBytes, 0);
    },
    totalBytes(state): number {
      return state.programs
        .filter((p) => state.showRuntimes || !p.isRuntime)
        .reduce((n, p) => n + p.sizeBytes, 0);
    },
    filterCounts(state): Record<ProgramFilter, number> {
      const base = state.programs.filter((p) => state.showRuntimes || !p.isRuntime);
      return {
        all: base.length,
        desktop: base.filter((p) => matchesFilter(p, 'desktop')).length,
        store: base.filter((p) => matchesFilter(p, 'store')).length,
        recent: base.filter((p) => matchesFilter(p, 'recent')).length,
        large: base.filter((p) => matchesFilter(p, 'large')).length,
      };
    },
    leftoverItems(state): LeftoverItem[] {
      return state.scans.flatMap((s) => s.items);
    },
    selectedLeftovers(): LeftoverItem[] {
      return this.leftoverItems.filter((i: LeftoverItem) => i.selected);
    },
    selectedLeftoverBytes(): number {
      return this.selectedLeftovers.reduce((n: number, i: LeftoverItem) => n + i.bytes, 0);
    },
    selectedNeedsAdmin(state): boolean {
      return !state.isAdmin && this.selectedLeftovers.some((i: LeftoverItem) => i.requiresAdmin);
    },
  },
  actions: {
    setBusy(operation: UninstallerOperation) {
      this.operation = operation;
      useAppStore().setBusy(PAGE_IDS.uninstaller, operation !== 'idle');
      if (operation === 'idle') this.progress = null;
    },
    /** Wait for a quiet background refresh so a new operation does not hit `operationBusy`. */
    async waitForRefresh() {
      while (refreshInFlight) await refreshInFlight;
    },
    async load(options: { quiet?: boolean } = {}): Promise<void> {
      if (this.busy) return;
      if (options.quiet) {
        if (refreshInFlight) return refreshInFlight;
        this.refreshing = true;
        const run = this.loadInventory(true).finally(() => {
          refreshInFlight = null;
          this.refreshing = false;
        });
        refreshInFlight = run;
        return run;
      }
      await this.waitForRefresh();
      if (this.busy) return;
      await this.loadInventory(false);
    },
    async loadInventory(quiet: boolean) {
      const options = { quiet };
      if (!options.quiet) this.setBusy('listing');
      const startedAt = Date.now();
      this.listing = { stage: 'registry', current: 0, total: 0, found: 0, name: '', startedAt };
      this.listingFeed = [];
      try {
        const inventory = await UninstallerService.listProgramsWithProgress((p) => {
          if (options.quiet) return;
          const stage = (p.area ?? 'registry') as ListingStage;
          this.listing = { stage, current: p.current, total: p.total, found: p.itemsFound, name: p.programName, startedAt };
          if (stage === 'desktop' && p.itemId && p.programName) {
            const id = p.itemId;
            // Seed the shared icon map so the list does not fetch these icons again.
            if (!(id in this.icons)) this.icons = { ...this.icons, [id]: p.icon };
            if (!this.listingFeed.some((f) => f.id === id)) {
              this.listingFeed = [{ id, name: p.programName }, ...this.listingFeed].slice(0, LISTING_FEED_LIMIT);
            }
          }
        });
        this.programs = inventory.programs;
        this.isAdmin = inventory.isAdmin;
        this.storeUnavailable = inventory.storeUnavailable;
        this.removedElsewhere = inventory.removedElsewhere;
        this.loaded = true;
        const known = new Set(this.programs.map((p) => p.id));
        this.selectedIds = this.selectedIds.filter((id) => known.has(id));
      } catch (error) {
        if (!isCancelledError(error)) useAppStore().reportError(error);
      } finally {
        if (!options.quiet) this.setBusy('idle');
      }
      void this.loadIcons();
    },
    /** Fetch icons in small batches so rows fill in progressively without blocking the list. */
    async loadIcons() {
      const missing = this.programs.map((p) => p.id).filter((id) => !(id in this.icons));
      for (let i = 0; i < missing.length; i += ICON_BATCH) {
        try {
          const batch = await UninstallerService.getIcons(missing.slice(i, i + ICON_BATCH));
          const next = { ...this.icons };
          for (const icon of batch) next[icon.id] = icon.dataUrl;
          this.icons = next;
        } catch {
          // Icons are decorative; initials remain as the fallback.
          return;
        }
      }
    },
    async ensureLoaded() {
      if (!this.loaded) await this.load();
    },
    toggleSelected(id: string) {
      this.selectedIds = this.selectedIdSet.has(id)
        ? this.selectedIds.filter((x) => x !== id)
        : [...this.selectedIds, id];
    },
    setVisibleSelected(selected: boolean) {
      const visible = this.visiblePrograms.map((p) => p.id);
      if (selected) {
        this.selectedIds = [...new Set([...this.selectedIds, ...visible])];
      } else {
        const drop = new Set(visible);
        this.selectedIds = this.selectedIds.filter((id) => !drop.has(id));
      }
    },
    clearSelection() {
      this.selectedIds = [];
    },
    onProgress(p: UninstallerProgress) {
      const previous = this.progress;
      this.progress = p;
      if (p.phase === 'scanning') {
        this.operation = 'scanning';
        // An area is done once the scanner moves to a different one.
        if (previous?.phase === 'scanning' && previous.area && previous.area !== p.area) {
          // Scanning events only carry leftover areas.
          const area = previous.area as LeftoverArea;
          if (!this.areasDone.includes(area)) this.areasDone = [...this.areasDone, area];
        }
      } else if (p.phase === 'uninstalling' || p.phase === 'restorePoint') {
        this.operation = 'uninstalling';
        this.areasDone = [];
      } else if (p.phase === 'cleaning') {
        // `current` / `total` are per program; the batch total is fixed in cleanSelected().
        this.cleanProcessed = Math.max(this.cleanProcessed, this.cleanOffset + p.current);
        if (p.itemStatus && p.itemId) {
          if (p.itemStatus !== 'removed' && p.itemStatus !== 'notFound') this.cleanIssues += 1;
          this.cleanSeq += 1;
          const entry: CleanLogEntry = {
            key: `${this.cleanSeq}:${p.itemId}`,
            location: p.currentPath ?? '',
            status: p.itemStatus,
          };
          this.cleanLog = [entry, ...this.cleanLog].slice(0, CLEAN_LOG_LIMIT);
        }
      }
    },
    async uninstall(ids: string[], options: UninstallOptions) {
      if (this.busy || !ids.length) return;
      await this.waitForRefresh();
      if (this.busy) return;
      this.cancelRequested = false;
      const byId = new Map(this.programs.map((p) => [p.id, p] as const));
      this.queue = ids.map((id) => byId.get(id)).filter((p): p is InstalledProgram => !!p);
      this.reports = [];
      this.scans = [];
      this.cleanResult = null;
      this.forced = false;
      this.restorePointRequested = options.createRestorePoint;
      this.setBusy('uninstalling');
      try {
        for (const [index, program] of this.queue.entries()) {
          this.queueIndex = index + 1;
          this.areasDone = [];
          this.progress = null;
          this.operation = 'uninstalling';
          const report = await UninstallerService.uninstallWithProgress(program.id, options, (p) =>
            this.onProgress(p)
          );
          this.reports.push(report);
          if (report.leftovers && report.leftovers.items.length) {
            this.scans.push(report.leftovers);
          }
          // Stop the batch when the user stopped waiting on an uninstaller.
          if (report.status === 'cancelled' || this.cancelRequested) break;
          // Only one restore point per batch.
          options = { ...options, createRestorePoint: false };
        }
      } catch (error) {
        if (!isCancelledError(error)) useAppStore().reportError(error);
      } finally {
        this.setBusy('idle');
        this.queueIndex = 0;
        this.selectedIds = [];
        this.view = this.scans.length ? 'leftovers' : 'summary';
        void useHistoryStore().load({ reportError: false });
        void this.load({ quiet: true });
      }
    },
    /** Skip the program's uninstaller and scan for everything it owns (broken uninstallers). */
    async forceRemove(id: string) {
      if (this.busy) return;
      await this.waitForRefresh();
      if (this.busy) return;
      this.cancelRequested = false;
      const removed = this.removedElsewhere.find((p) => p.id === id);
      const program = this.programs.find((p) => p.id === id) ?? (removed ? programFromRemoved(removed) : null);
      this.queue = program ? [program] : [];
      this.queueIndex = 1;
      this.areasDone = [];
      this.restorePointRequested = false;
      this.reports = [];
      this.scans = [];
      this.cleanResult = null;
      this.forced = true;
      this.setBusy('scanning');
      try {
        const scan = await UninstallerService.scanLeftoversWithProgress(id, (p) => this.onProgress(p));
        this.scans = scan.items.length ? [scan] : [];
        this.view = scan.items.length ? 'leftovers' : 'summary';
      } catch (error) {
        if (!isCancelledError(error)) useAppStore().reportError(error);
      } finally {
        this.setBusy('idle');
        this.queueIndex = 0;
      }
    },
    // Selection mutates `selected` in place: copying every scan/item array on each checkbox
    // invalidated all leftover-derived computeds and re-rendered the whole review list.
    setLeftoverSelectedIn(scanId: string, itemId: string, selected: boolean) {
      const item = this.scans.find((s) => s.scanId === scanId)?.items.find((i) => i.id === itemId);
      if (item) item.selected = selected;
    },
    setGroupSelected(group: LeftoverGroup | 'all', selected: boolean) {
      for (const scan of this.scans) {
        for (const item of scan.items) {
          if ((group === 'all' || leftoverGroup(item.kind) === group) && item.selected !== selected) {
            item.selected = selected;
          }
        }
      }
    },
    async cleanSelected() {
      if (this.busy) return;
      await this.waitForRefresh();
      if (this.busy) return;
      const work = this.scans
        .map((scan) => ({ scan, ids: scan.items.filter((i) => i.selected).map((i) => i.id) }))
        .filter((w) => w.ids.length);
      if (!work.length) return;
      this.cancelRequested = false;
      this.cleanLog = [];
      this.cleanTotal = work.reduce((n, w) => n + w.ids.length, 0);
      this.cleanOffset = 0;
      this.cleanProcessed = 0;
      this.cleanIssues = 0;
      this.setBusy('cleaning');
      const total = emptyCleanResult();
      /** Scans whose clean ran to completion (not cancelled, no error). */
      const finished: LeftoverScan[] = [];
      try {
        for (const { scan, ids } of work) {
          // Rust resets its cancel flag at the start of every clean call, so the loop itself
          // must stop once the user pressed Cancel.
          if (this.cancelRequested) break;
          const result = await UninstallerService.cleanLeftoversWithProgress(scan.scanId, ids, (p) =>
            this.onProgress(p)
          );
          total.removedCount += result.removedCount;
          total.freedBytes += result.freedBytes;
          total.pendingRebootCount += result.pendingRebootCount;
          total.skippedNeedAdminCount += result.skippedNeedAdminCount;
          total.failedCount += result.failedCount;
          total.registryBackupSaved ||= result.registryBackupSaved;
          total.items.push(...result.items);
          this.cleanOffset += ids.length;
          this.cleanProcessed = Math.max(this.cleanProcessed, this.cleanOffset);
          // Core returns Ok with fewer outcomes when it stopped early on cancel.
          const stopped =
            this.cancelRequested || result.cancelled === true || result.items.length < ids.length;
          if (stopped) break;
          finished.push(scan);
        }
      } catch (error) {
        if (!isCancelledError(error)) useAppStore().reportError(error);
      } finally {
        // Fully handled programs never reappear in the "removed by another uninstaller" list;
        // cancelled or failed ones stay so their leftovers can be scanned again.
        this.dismissScanned(finished);
        this.cleanResult = total;
        this.setBusy('idle');
        this.view = 'summary';
        void useHistoryStore().load({ reportError: false });
        void this.load({ quiet: true });
      }
    },
    skipLeftovers() {
      this.dismissScanned(this.scans);
      this.view = 'summary';
    },
    dismissScanned(scans: LeftoverScan[]) {
      const removedIds = new Set(this.removedElsewhere.map((p) => p.id));
      for (const scan of scans) {
        if (this.forced || removedIds.has(scan.programId)) void this.dismissRemoved(scan.programId);
      }
    },
    async dismissRemoved(id: string) {
      this.removedElsewhere = this.removedElsewhere.filter((p) => p.id !== id);
      try {
        await UninstallerService.dismissRemoved(id);
      } catch {
        // Best effort: the entry simply shows again on the next listing.
      }
    },
    finish() {
      this.view = 'list';
      this.reports = [];
      this.scans = [];
      this.cleanResult = null;
      this.cleanLog = [];
      this.forced = false;
      this.queue = [];
    },
    async focusUninstaller() {
      try {
        await UninstallerService.focusUninstaller();
      } catch {
        // Nothing to focus; the dialog keeps showing the waiting state.
      }
    },
    async cancel() {
      this.cancelRequested = true;
      try {
        await UninstallerService.cancel();
      } catch (error) {
        useAppStore().reportError(error);
      }
    },
    async restartAsAdministrator() {
      try {
        await MemoryCleanerService.restartAsAdministrator();
      } catch (error) {
        if (!isCancelledError(error)) useAppStore().reportError(error);
      }
    },
  },
});
