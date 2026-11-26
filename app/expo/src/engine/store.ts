import AsyncStorage from "@react-native-async-storage/async-storage";
import { AppState } from "react-native";
import { create } from "zustand";
import i18n from "@/i18n";
import type { ClipboardItem } from "@/types/clipboard";
import type { DisplayKind } from "@/utils/displayKind";
import {
  DAY_MS,
  startOfDay,
  type HistoryDateFilter,
  type HistorySourceFilter,
} from "@/utils/historyFilters";
import { buildDeviceNameMap, mapEngineHistoryItem } from "./mapper";
import { isNativeEngineAvailable, parseEngineError, unicEngineNative } from "./nativeModule";
import type {
  ActionInfo,
  Capabilities,
  ClipboardChangedEvent,
  EngineCandidate,
  EngineDevice,
  EngineError,
  EngineEvent,
  EngineSettings,
  FileCacheState,
  PartDescriptor,
  EngineStateSnapshot,
  HistoryItem,
  ImagePreviewResult,
  MigrationProgress,
  MigrationRunResult,
  PairingCode,
  ReceivedContent,
  SensitiveTicket,
  SetupFlowState,
  SetupScreenViewState,
  SourceDetectionResult,
  SubmissionResult,
  TextPreview,
} from "./types";

export type EngineMode = "loading" | "live" | "unavailable";
export type BootStage = "loading" | "setup" | "main";
export type SetupEntryMode = "first-install" | "later";

const HISTORY_PAGE_SIZE = 50;

export interface EngineHistoryQueryFilters {
  text?: string;
  tags?: string[];
  kinds?: DisplayKind[];
  dateFilter?: HistoryDateFilter;
  sourceFilter?: HistorySourceFilter;
}

interface EngineStoreState {
  mode: EngineMode;
  bootStage: BootStage;
  setupEntryMode: SetupEntryMode;
  snapshot: EngineStateSnapshot | null;
  capabilities: Capabilities | null;

  // Setup & v1 migration
  setupState: SetupFlowState | null;
  sourceDetection: SourceDetectionResult | null;
  setupViewState: SetupScreenViewState;
  migrationProgress: MigrationProgress;
  migrationReport: MigrationRunResult | null;
  migrationErrorMessage: string;
  lastAttemptImported: number;
  cumulativeImported: number;

  // Live Engine collections
  records: ClipboardItem[];
  rawHistory: HistoryItem[];
  availableTags: string[];
  actions: ActionInfo[];
  devices: EngineDevice[];
  candidates: EngineCandidate[];
  received: ReceivedContent[];
  autoApplyReceived: boolean;
  automaticCapture: boolean;
  fileCache: FileCacheState | null;
  pendingClipboardHint: ClipboardChangedEvent | null;
  pairingCode: PairingCode | null;
  busy: boolean;
  lastError: EngineError | null;

  // Pagination & query filter state
  nextCursor: string | null;
  hasMore: boolean;
  loadingMore: boolean;
  historyRevision: string | null;
  queryText: string;
  queryTags: string[];
  queryKinds: DisplayKind[];
  queryDateFilter: HistoryDateFilter;
  querySourceFilter: HistorySourceFilter;

  // Lifecycle & Setup actions
  bootstrap: () => Promise<void>;
  redetectSource: (customDir?: string) => Promise<void>;
  pickCustomSetupSource: () => Promise<void>;
  setSetupViewState: (state: SetupScreenViewState) => void;
  startMigration: (passphrase?: string) => Promise<void>;
  cancelMigration: () => Promise<void>;
  completeSetupAndEnterMain: () => Promise<void>;
  beginLaterMigration: () => Promise<void>;

  // Engine UI actions
  setQueryFilters: (filters: EngineHistoryQueryFilters) => Promise<void>;
  refreshHistory: () => Promise<void>;
  loadMoreItems: () => Promise<void>;
  refreshReceived: () => Promise<void>;
  refreshDevicesAndCandidates: () => Promise<void>;
  refreshAll: () => Promise<void>;
  clearError: () => void;
  setAutoApplyReceived: (enabled: boolean) => Promise<void>;
  setAutomaticCapture: (enabled: boolean) => Promise<void>;
  updateSettings: (
    mutator: (settings: EngineSettings) => EngineSettings,
    options?: { relayAccessKey?: string },
  ) => Promise<void>;
  refreshSnapshot: () => Promise<void>;
  refreshFileCache: () => Promise<void>;
  clearPreviews: () => Promise<void>;
  dismissClipboardHint: () => void;

  readHistoryFullText: (historyID: string) => Promise<string>;
  saveIncomingShare: (id: string) => Promise<ClipboardItem | undefined>;
  commitText: (text: string, deviceIds?: string[]) => Promise<void>;
  pickAndCommitFile: (deviceIds?: string[]) => Promise<{ cancelled: boolean }>;
  resendHistoryItem: (historyID: string, deviceIds: string[]) => Promise<SubmissionResult>;
  readSystemClipboardAndCommit: () => Promise<{ skipped?: boolean; reason?: string }>;
  applyToClipboard: (historyID: string) => Promise<void>;
  applyReceivedToClipboard: (receiptID: string) => Promise<void>;
  dismissReceived: (receiptID: string) => Promise<void>;
  toggleStar: (historyID: string) => Promise<void>;
  togglePin: (historyID: string) => Promise<void>;
  setHistoryTag: (historyID: string, tag: string, present: boolean) => Promise<void>;
  deleteHistoryItems: (ids: readonly string[]) => Promise<void>;
  runAction: (key: string, historyID: string) => Promise<void>;

  createPairingCode: () => Promise<PairingCode>;
  cancelPairingCode: () => Promise<void>;
  exportPairingTicket: () => Promise<SensitiveTicket>;
  joinByCode: (code: string) => Promise<string>;
  joinByTicket: (ticket: string) => Promise<string>;
  approveDevice: (device: EngineDevice, verifiedFingerprint: string) => Promise<void>;
  renameDevice: (id: string, expectedRevision: number, name: string) => Promise<void>;
  setBindingSync: (bindingId: string, expectedRevision: number, syncEnabled: boolean) => Promise<void>;
  unpairDevice: (id: string, expectedRevision: number) => Promise<void>;
  pairCandidate: (candidate: EngineCandidate) => Promise<string>;
  ignoreCandidate: (deviceId: string, ignored: boolean) => Promise<void>;
}

class PreviewCache<T> {
  private readonly entries = new Map<string, T>();

  constructor(private readonly capacity: number) {}

  get(key: string) {
    const value = this.entries.get(key);
    if (value !== undefined) this.set(key, value);
    return value;
  }

  set(key: string, value: T) {
    this.entries.delete(key);
    this.entries.set(key, value);
    if (this.entries.size > this.capacity) {
      const oldest = this.entries.keys().next();
      if (!oldest.done) this.entries.delete(oldest.value);
    }
  }

  clear() {
    this.entries.clear();
  }
}

const textPreviewCache = new PreviewCache<TextPreview>(200);
const imagePreviewCache = new PreviewCache<ImagePreviewResult>(60);
const fileMetadataCache = new PreviewCache<PartDescriptor[]>(200);
const autoAppliedReceiptIds = new Set<string>();
const autoApplyKey = "unic.auto-apply-received";
const captureKey = "unic.automatic-capture";
const pendingEvents = new Set<string>();
let settingsQueue = Promise.resolve();

let unsubscribeFns: Array<() => void> = [];
let coalesceTimer: ReturnType<typeof setTimeout> | null = null;
let historyRequestSeq = 0;

function clearNativeListeners() {
  if (coalesceTimer !== null) {
    clearTimeout(coalesceTimer);
    coalesceTimer = null;
  }
  for (const fn of unsubscribeFns) {
    try {
      fn();
    } catch {
      // ignore teardown errors
    }
  }
  unsubscribeFns = [];
  pendingEvents.clear();
  historyRequestSeq++;
  textPreviewCache.clear();
  imagePreviewCache.clear();
  fileMetadataCache.clear();
  autoAppliedReceiptIds.clear();
}

function detectionToWelcomeView(det: SourceDetectionResult): SetupScreenViewState {
  switch (det.status) {
    case "found":
      return "w-detected";
    case "invalid":
      return "w-invalid";
    default:
      return "w-clean";
  }
}

function buildDateRangeMs(dateFilter: HistoryDateFilter): {
  created_from_ms?: number;
  created_before_ms?: number;
} {
  const now = Date.now();
  switch (dateFilter) {
    case "today": {
      const from = startOfDay(now);
      return { created_from_ms: from, created_before_ms: from + DAY_MS };
    }
    case "yesterday": {
      const todayStart = startOfDay(now);
      return { created_from_ms: startOfDay(now - DAY_MS), created_before_ms: todayStart };
    }
    case "pastWeek":
      return { created_from_ms: startOfDay(now - 6 * DAY_MS) };
    case "all":
    default:
      return {};
  }
}

function mapDisplayKindsToSearchTypes(kinds: readonly DisplayKind[]): string[] {
  const out = new Set<string>();
  for (const k of kinds) {
    switch (k) {
      case "text":
      case "url":
        out.add("text");
        out.add("richtext");
        break;
      case "image":
        out.add("image");
        break;
      case "file":
      case "group":
        out.add("files");
        out.add("directory");
        out.add("directory_reference");
        break;
    }
  }
  return Array.from(out);
}

function mapDisplayKindsToHistoryKinds(kinds: readonly DisplayKind[]): string[] {
  const out = new Set<string>();
  for (const k of kinds) {
    switch (k) {
      case "text":
      case "url":
        out.add("text");
        break;
      case "image":
        out.add("image");
        break;
      case "file":
      case "group":
        out.add("files");
        out.add("directory");
        out.add("directory_reference");
        break;
    }
  }
  return Array.from(out);
}

function buildHistoryQueryPlan(state: EngineStoreState): {
  useSearch: boolean;
  searchQuery?: Record<string, unknown>;
  listFilter?: Record<string, unknown>;
} {
  const trimmed = state.queryText.trim();
  const dateRange = buildDateRangeMs(state.queryDateFilter);

  if (trimmed.length > 0) {
    const query: Record<string, unknown> = {
      text_terms: trimmed.split(/\s+/).filter(Boolean),
      ...dateRange,
    };
    if (state.queryTags.length > 0) {
      query.tags = state.queryTags;
    }
    const types = mapDisplayKindsToSearchTypes(state.queryKinds);
    if (types.length > 0) {
      query.types = types;
    }
    if (state.querySourceFilter === "local") {
      query.sources = [{ kind: "local" }, { kind: "migration" }];
    } else if (state.querySourceFilter === "remote") {
      const deviceSources = state.devices.map((d) => ({ kind: "device", device_id: d.id }));
      query.sources = [...deviceSources, { kind: "external" }];
    }
    return { useSearch: true, searchQuery: query };
  }

  const filter: Record<string, unknown> = { ...dateRange };
  if (state.queryTags.length > 0) {
    filter.tags = state.queryTags;
  }
  const kinds = mapDisplayKindsToHistoryKinds(state.queryKinds);
  if (kinds.length > 0) {
    filter.kinds = kinds;
  }
  if (state.querySourceFilter === "local") {
    filter.origin_kinds = ["local_system", "explicit", "action", "migration"];
  } else if (state.querySourceFilter === "remote") {
    filter.origin_kinds = ["remote_device", "external"];
  }
  return { useSearch: false, listFilter: filter };
}

async function hydrateHistoryItems(
  items: HistoryItem[],
  devices: EngineDevice[],
  searchTexts?: ReadonlyMap<string, string>,
): Promise<ClipboardItem[]> {
  const deviceNames = buildDeviceNameMap(devices);
  const out: ClipboardItem[] = new Array(items.length);

  // Fetch bounded previews with modest concurrency (4 workers).
  let cursor = 0;
  const worker = async () => {
    while (cursor < items.length) {
      const idx = cursor++;
      const item = items[idx];
      const cacheKey = `${item.id}@${item.revision}`;
      let preview = textPreviewCache.get(cacheKey);
      let imagePreview = imagePreviewCache.get(cacheKey);
      let files = fileMetadataCache.get(item.id);

      if (!preview && (item.kind === "text" || item.kind === "html" || item.kind === "mixed")) {
        try {
          preview = await unicEngineNative.readHistoryTextPreview(item.id, 512);
          textPreviewCache.set(cacheKey, preview);
        } catch {
          // keep fallback metadata
        }
      }
      if (!imagePreview && item.kind === "image" && item.parts.some((p) => p.available)) {
        try {
          imagePreview = await unicEngineNative.exportHistoryImagePreview(item.id);
          imagePreviewCache.set(cacheKey, imagePreview);
        } catch {
          // keep fallback metadata
        }
      }

      if (!files && item.parts.some((part) => part.kind === "file")) {
        try {
          files = (await unicEngineNative.historyFileMetadata(item.id)) ?? [];
          fileMetadataCache.set(item.id, files);
        } catch {
          // The content can expire between listing and retention.
        }
      }

      out[idx] = mapEngineHistoryItem(
        item,
        deviceNames,
        preview,
        imagePreview,
        searchTexts?.get(item.id),
        files,
      );
    }
  };

  await Promise.all(Array.from({ length: Math.min(4, Math.max(1, items.length)) }, () => worker()));
  return out;
}

async function startLiveEngineAndSubscribe(
  set: (partial: Partial<EngineStoreState>) => void,
  get: () => EngineStoreState,
) {
  clearNativeListeners();

  const preferences = new Map(await AsyncStorage.multiGet([autoApplyKey, captureKey]));
  const automaticCapture = preferences.get(captureKey) !== "false";
  const autoApplyReceived = preferences.get(autoApplyKey) === "true";
  await unicEngineNative.setAutomaticCapture(automaticCapture);
  await unicEngineNative.setAutoApplyReceived(autoApplyReceived);
  const snapshot = await unicEngineNative.initializeEngine();

  const subEvt = unicEngineNative.addEventListener((eventJSON) => {
    let event: EngineEvent;
    try {
      event = JSON.parse(eventJSON) as EngineEvent;
    } catch {
      return;
    }
    if (event.instance_id !== snapshot.instance_id) return;
    pendingEvents.add(event.kind);
    if (coalesceTimer !== null) return;
    coalesceTimer = setTimeout(() => {
      coalesceTimer = null;
      const kinds = new Set(pendingEvents);
      pendingEvents.clear();
      const state = get();
      const received = kinds.has("received") || kinds.has("apply");
      const status = received || ["settings", "capabilities", "runtime", "lifecycle"].some(
        (kind) => kinds.has(kind),
      );
      if (kinds.has("devices") || kinds.has("pairing_code")) {
        void state.refreshDevicesAndCandidates();
      }
      if (kinds.has("history")) {
        void unicEngineNative.reconcileHistoryPreviews().catch(() => {});
        void state.refreshHistory();
        if (state.fileCache) void state.refreshFileCache();
      }
      if (status) {
        void state.refreshSnapshot().then(() => {
          if (received || kinds.has("capabilities")) void get().refreshReceived();
        });
      }
    }, 120);
  });
  const subResync = unicEngineNative.addResyncListener(() => {
    void get().refreshAll();
  });
  const subAppState = AppState.addEventListener("change", (state) => {
    if (state === "active") void get().refreshAll();
  });
  const subClipboard = unicEngineNative.addClipboardChangedListener((payload) => {
    if (get().automaticCapture) set({ pendingClipboardHint: payload });
  });
  unsubscribeFns.push(
    () => subEvt.remove(),
    () => subResync.remove(),
    () => subClipboard.remove(),
    () => subAppState.remove(),
  );

  set({
    mode: "live",
    bootStage: "main",
    snapshot,
    capabilities: snapshot.capabilities,
    automaticCapture,
    autoApplyReceived: preferences.get(autoApplyKey) === "true",
    devices: snapshot.devices ?? [],
    received: snapshot.received ?? [],
    lastError: null,
  });

  await get().refreshAll();
}

export const useEngineStore = create<EngineStoreState>((set, get) => ({
  mode: isNativeEngineAvailable() ? "loading" : "unavailable",
  bootStage: isNativeEngineAvailable() ? "loading" : "main",
  setupEntryMode: "first-install",
  snapshot: null,
  capabilities: null,

  setupState: null,
  sourceDetection: null,
  setupViewState: "loading",
  migrationProgress: { stage: "import", processed: 0, total: 0 },
  migrationReport: null,
  migrationErrorMessage: "",
  lastAttemptImported: 0,
  cumulativeImported: 0,

  records: [],
  rawHistory: [],
  availableTags: [],
  actions: [],
  devices: [],
  candidates: [],
  received: [],
  autoApplyReceived: false,
  automaticCapture: true,
  fileCache: null,
  pendingClipboardHint: null,
  pairingCode: null,
  busy: false,
  lastError: null,

  nextCursor: null,
  hasMore: false,
  loadingMore: false,
  historyRevision: null,
  queryText: "",
  queryTags: [],
  queryKinds: [],
  queryDateFilter: "all",
  querySourceFilter: "all",

  bootstrap: async () => {
    if (!isNativeEngineAvailable()) {
      set({ mode: "unavailable", bootStage: "main" });
      return;
    }

    set({ mode: "live", bootStage: "loading", lastError: null });
    try {
      const setupState = await unicEngineNative.openSetupFlow();
      const det = await unicEngineNative.detectSetupSource("");
      if (setupState.complete) {
        set({ setupState, sourceDetection: det });
        try {
          await startLiveEngineAndSubscribe(set, get);
        } catch (engineErr) {
          set({
            bootStage: "main",
            lastError: parseEngineError(engineErr),
          });
        }
        return;
      }

      set({
        setupEntryMode: "first-install",
        bootStage: "setup",
        setupState,
        sourceDetection: det,
        setupViewState: detectionToWelcomeView(det),
      });
    } catch (err) {
      set({
        bootStage: "setup",
        setupViewState: "r-failed",
        lastError: parseEngineError(err),
        migrationErrorMessage: parseEngineError(err).message,
      });
    }
  },

  redetectSource: async (customDir = "") => {
    try {
      const det = await unicEngineNative.detectSetupSource(customDir);
      const setupState = await unicEngineNative.getSetupState();
      set({
        setupState,
        sourceDetection: det,
        setupViewState: detectionToWelcomeView(det),
        migrationErrorMessage: "",
      });
    } catch (err) {
      const e = parseEngineError(err);
      set({ lastError: e, migrationErrorMessage: e.message });
    }
  },

  pickCustomSetupSource: async () => {
    try {
      const picked = await unicEngineNative.selectSetupCustomSource();
      if (picked.cancelled) return;
      const setupState = await unicEngineNative.getSetupState();
      const det: SourceDetectionResult = {
        status: "found",
        source: picked.source ?? setupState.source,
      };
      set({
        setupState,
        sourceDetection: det,
        setupViewState: "w-detected",
        migrationErrorMessage: "",
      });
    } catch (err) {
      const e = parseEngineError(err);
      if (e.code === "invalid_argument") {
        set({ setupViewState: "w-invalid", migrationErrorMessage: e.message });
        return;
      }
      set({ lastError: e, setupViewState: "r-failed", migrationErrorMessage: e.message });
    }
  },

  setSetupViewState: (setupViewState) => set({ setupViewState }),

  startMigration: async (passphrase) => {
    const suppliedPassphrase = typeof passphrase === "string" && passphrase.length > 0;
    let active = true;
    const subProg = unicEngineNative.addMigrationProgressListener((progress) => {
      if (!active) return;
      set((prev) => {
        if (!prev.busy) return prev;
        if (
          prev.setupViewState !== "m-importing" &&
          prev.setupViewState !== "m-verifying" &&
          prev.setupViewState !== "m-stopping"
        ) {
          return prev;
        }
        return {
          migrationProgress: progress,
          setupViewState:
            prev.setupViewState === "m-stopping"
              ? "m-stopping"
              : progress.stage === "verify"
                ? "m-verifying"
                : "m-importing",
        };
      });
    });

    set({
      busy: true,
      migrationErrorMessage: "",
      migrationProgress: { stage: "import", processed: 0, total: 0 },
      setupViewState: "m-importing",
    });

    try {
      const res = await unicEngineNative.runSetupMigration(suppliedPassphrase ? passphrase : null);
      active = false;
      subProg.remove();
      const nextCumulative = get().cumulativeImported + (res.imported || 0);
      let nextView: SetupScreenViewState = "r-failed";
      if (res.status === "complete" || res.status === "done") nextView = "r-done";
      else if (res.status === "partial") nextView = "r-partial";
      else if (res.status === "cancelled") nextView = "r-cancelled";

      set({
        busy: false,
        migrationReport: res,
        lastAttemptImported: res.imported || 0,
        cumulativeImported: nextCumulative,
        setupViewState: nextView,
        migrationErrorMessage: res.error ? i18n.t(`errors:engine.${res.error.code}`, {
          defaultValue: i18n.t("settingsStorage:migration.failedBody"),
        }) : "",
      });
    } catch (err) {
      active = false;
      subProg.remove();
      const e = parseEngineError(err);
      let nextView: SetupScreenViewState = "r-failed";
      let msg = i18n.t(`errors:engine.${e.code}`, { defaultValue: i18n.t("settingsStorage:migration.failedBody") });
      if (e.code === "permission_denied") {
        nextView = suppliedPassphrase ? "m-auth-err" : "m-auth";
        msg = suppliedPassphrase ? i18n.t("settingsStorage:migration.unlockFailed") : "";
      } else if (e.code === "in_use") {
        nextView = "r-conflict";
      } else if (e.code === "cancelled") {
        nextView = "r-cancelled";
      } else if (e.code === "invalid_argument") {
        nextView = "w-invalid";
      }
      set({
        busy: false,
        setupViewState: nextView,
        migrationErrorMessage: msg,
        lastError: e.code === "permission_denied" ? null : e,
      });
    }
  },

  cancelMigration: async () => {
    set({ setupViewState: "m-stopping" });
    try {
      await unicEngineNative.cancelSetupMigration();
    } catch {
      // cancellation outcome is surfaced by the running Run call
    }
  },

  completeSetupAndEnterMain: async () => {
    set({ busy: true, lastError: null });
    try {
      await unicEngineNative.markSetupComplete();
      const setupState = await unicEngineNative.getSetupState();
      set({ setupState });
      await startLiveEngineAndSubscribe(set, get);
    } catch (err) {
      const e = parseEngineError(err);
      set({
        lastError: e,
        setupViewState: "r-failed",
        migrationErrorMessage: e.message,
      });
    } finally {
      set({ busy: false });
    }
  },

  beginLaterMigration: async () => {
    if (!isNativeEngineAvailable()) return;
    set({ busy: true, lastError: null });
    try {
      clearNativeListeners();
      await unicEngineNative.shutdownEngine();
      await unicEngineNative.beginLaterMigration();
      const setupState = await unicEngineNative.openSetupFlow();
      const det = await unicEngineNative.detectSetupSource("");

      set({
        busy: false,
        setupEntryMode: "later",
        bootStage: "setup",
        setupState,
        sourceDetection: det,
        migrationReport: null,
        migrationErrorMessage: "",
        migrationProgress: { stage: "import", processed: 0, total: 0 },
        setupViewState: det.status === "found" ? "w-detected" : "w-custom",
      });
    } catch (err) {
      const e = parseEngineError(err);
      set({ busy: false, lastError: e });
      // Try restarting live engine if entering later migration aborted cleanly.
      try {
        await startLiveEngineAndSubscribe(set, get);
      } catch {
        // leave error visible
      }
    }
  },

  setQueryFilters: async (filters) => {
    const cur = get();
    set({
      queryText: filters.text ?? cur.queryText,
      queryTags: filters.tags ?? cur.queryTags,
      queryKinds: filters.kinds ?? cur.queryKinds,
      queryDateFilter: filters.dateFilter ?? cur.queryDateFilter,
      querySourceFilter: filters.sourceFilter ?? cur.querySourceFilter,
      nextCursor: null,
      hasMore: false,
    });
    await get().refreshHistory();
  },

  refreshHistory: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    const reqSeq = ++historyRequestSeq;
    const state = get();
    const plan = buildHistoryQueryPlan(state);
    try {
      let items: HistoryItem[] = [];
      let nextCursor: string | null = null;
      let historyRevision: string | null = null;
      const searchTexts = new Map<string, string>();

      if (plan.useSearch && plan.searchQuery) {
        const page = await unicEngineNative.searchHistory({
          query: plan.searchQuery,
          limit: HISTORY_PAGE_SIZE,
        });
        items = (page.hits ?? []).map((hit) => {
          if (hit.preview?.text) searchTexts.set(hit.item.id, hit.preview.text);
          return hit.item;
        });
        nextCursor = page.next_cursor || null;
        historyRevision = page.history_revision || null;
      } else {
        const request = { limit: HISTORY_PAGE_SIZE, filter: plan.listFilter };
        const pinnedRequest = state.queryTags.length === 0
          ? unicEngineNative.listHistory({ ...request, filter: { ...request.filter, tags: ["pinned"] } })
          : null;
        const [page, pinned] = await Promise.all([unicEngineNative.listHistory(request), pinnedRequest]);
        const head = page.items ?? [];
        const pins = pinned?.history_revision === page.history_revision ? pinned?.items ?? [] : [];
        const pinnedIDs = new Set(pins.map((item) => item.id));
        items = [...pins, ...head.filter((item) => !pinnedIDs.has(item.id))];
        nextCursor = page.next_cursor || null;
        historyRevision = page.history_revision || null;
      }

      if (reqSeq !== historyRequestSeq) return;

      const records = await hydrateHistoryItems(items, get().devices, searchTexts);
      if (reqSeq !== historyRequestSeq) return;

      let tags: string[] = [];
      try {
        tags = await unicEngineNative.listHistoryTags();
      } catch {
        tags = get().availableTags;
      }
      if (reqSeq !== historyRequestSeq) return;

      set({
        rawHistory: items,
        records,
        availableTags: tags,
        nextCursor,
        hasMore: Boolean(nextCursor),
        loadingMore: false,
        historyRevision,
      });
    } catch (err) {
      if (reqSeq !== historyRequestSeq) return;
      set({ lastError: parseEngineError(err), loadingMore: false });
    }
  },

  loadMoreItems: async () => {
    const state = get();
    if (
      state.mode !== "live" ||
      state.bootStage !== "main" ||
      !state.hasMore ||
      !state.nextCursor ||
      state.loadingMore
    ) {
      return;
    }
    const reqSeq = historyRequestSeq;
    const plan = buildHistoryQueryPlan(state);
    set({ loadingMore: true });
    try {
      let pageItems: HistoryItem[] = [];
      let nextCursor: string | null = null;
      let pageRev: string | null = null;
      const searchTexts = new Map<string, string>();

      if (plan.useSearch && plan.searchQuery) {
        const page = await unicEngineNative.searchHistory({
          query: plan.searchQuery,
          cursor: state.nextCursor,
          limit: HISTORY_PAGE_SIZE,
        });
        pageItems = (page.hits ?? []).map((hit) => {
          if (hit.preview?.text) searchTexts.set(hit.item.id, hit.preview.text);
          return hit.item;
        });
        nextCursor = page.next_cursor || null;
        pageRev = page.history_revision || null;
      } else {
        const page = await unicEngineNative.listHistory({
          cursor: state.nextCursor,
          limit: HISTORY_PAGE_SIZE,
          filter: plan.listFilter,
        });
        pageItems = page.items ?? [];
        nextCursor = page.next_cursor || null;
        pageRev = page.history_revision || null;
      }

      if (reqSeq !== historyRequestSeq) return;

      // If history_revision moved and returned an empty page, reset and reload cleanly.
      if (pageRev && state.historyRevision && pageRev !== state.historyRevision && pageItems.length === 0) {
        set({ nextCursor: null, hasMore: false, loadingMore: false });
        await get().refreshHistory();
        return;
      }

      const existingIds = new Set(get().rawHistory.map((it) => it.id));
      const freshItems = pageItems.filter((it) => !existingIds.has(it.id));
      const freshRecords = await hydrateHistoryItems(freshItems, get().devices, searchTexts);

      if (reqSeq !== historyRequestSeq) return;

      set((prev) => ({
        rawHistory: [...prev.rawHistory, ...freshItems],
        records: [...prev.records, ...freshRecords],
        nextCursor,
        hasMore: Boolean(nextCursor),
        loadingMore: false,
        historyRevision: pageRev ?? prev.historyRevision,
      }));
    } catch (err) {
      if (reqSeq !== historyRequestSeq) return;
      const e = parseEngineError(err);
      if (e.code === "conflict" || e.code === "resource_expired" || e.code === "invalid_argument") {
        set({ nextCursor: null, hasMore: false, loadingMore: false });
        await get().refreshHistory();
        return;
      }
      set({ lastError: e, loadingMore: false });
    }
  },

  refreshReceived: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    try {
      const received = await unicEngineNative.listReceived();
      set({ received: received ?? [] });
      const activeIds = new Set((received ?? []).map((item) => item.receipt_id));
      for (const id of autoAppliedReceiptIds) {
        if (!activeIds.has(id)) autoAppliedReceiptIds.delete(id);
      }

      const { autoApplyReceived, capabilities } = get();
      if (
        autoApplyReceived &&
        capabilities?.effective?.apply === "available" &&
        received?.length > 0
      ) {
        const newest = received[0];
        if (
          newest.apply_state === "available" &&
          !autoAppliedReceiptIds.has(newest.receipt_id)
        ) {
          autoAppliedReceiptIds.add(newest.receipt_id);
          try {
            await unicEngineNative.applyReceivedToClipboard(newest.receipt_id);
            const updated = await unicEngineNative.listReceived();
            set({ received: updated });
          } catch {
            // Keep entry in received list so user can apply manually.
          }
        }
      }
    } catch (err) {
      set({ lastError: parseEngineError(err) });
    }
  },

  refreshDevicesAndCandidates: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    try {
      const [devices, candidates, pairingCode] = await Promise.all([
        unicEngineNative.listDevices(),
        unicEngineNative.listCandidates(),
        unicEngineNative.getPairingCode(),
      ]);
      set({ devices: devices ?? [], candidates: candidates ?? [], pairingCode });
    } catch (err) {
      set({ lastError: parseEngineError(err) });
    }
  },

  refreshSnapshot: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    try {
      const snapshot = await unicEngineNative.getState();
      set((state) => {
        if (state.snapshot?.instance_id !== snapshot.instance_id ||
            BigInt(state.snapshot.revision) > BigInt(snapshot.revision)) return state;
        if (BigInt(state.snapshot.settings.revision) > BigInt(snapshot.settings.revision)) return state;
        return { snapshot, capabilities: snapshot.capabilities };
      });
    } catch (err) {
      set({ lastError: parseEngineError(err) });
    }
  },

  refreshAll: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    await Promise.all([
      get().refreshSnapshot(),
      get().refreshDevicesAndCandidates(),
      unicEngineNative.listActions().then((actions) => set({ actions: actions ?? [] }))
        .catch((err) => set({ lastError: parseEngineError(err) })),
      unicEngineNative.reconcileHistoryPreviews().catch(() => {}),
    ]);
    await Promise.all([get().refreshHistory(), get().refreshReceived()]);
  },

  refreshFileCache: async () => {
    try {
      set({ fileCache: await unicEngineNative.getFileCacheState() });
    } catch (err) {
      set({ lastError: parseEngineError(err), fileCache: null });
    }
  },

  clearPreviews: async () => {
    ++historyRequestSeq;
    await unicEngineNative.clearHistoryPreviews();
    imagePreviewCache.clear();
    textPreviewCache.clear();
    // Recreated only when the history UI requests previews again.
    set((state) => ({
      loadingMore: false,
      records: state.records.map((item) => item.fileUri ? { ...item, fileUri: undefined } : item),
    }));
  },

  updateSettings: (mutator, options) => {
    const update = settingsQueue.then(async () => {
      const snapshot = get().snapshot;
      if (!snapshot) return;
      try {
        const req: { expected_revision: string; value: EngineSettings; relay_access_key?: string } = {
          expected_revision: snapshot.settings.revision,
          value: mutator(snapshot.settings.value),
        };
        if (options?.relayAccessKey !== undefined) {
          req.relay_access_key = options.relayAccessKey;
        }
        const settings = await unicEngineNative.updateSettings(req);
        set((state) => state.snapshot?.instance_id === snapshot.instance_id &&
          BigInt(state.snapshot.settings.revision) <= BigInt(settings.revision)
          ? { snapshot: { ...state.snapshot, settings } }
          : state);
        await get().refreshSnapshot();
      } catch (error) {
        await get().refreshSnapshot();
        throw error;
      }
    });
    settingsQueue = update.catch(() => {});
    return update;
  },

  clearError: () => set({ lastError: null }),

  setAutoApplyReceived: async (enabled) => {
    await AsyncStorage.setItem(autoApplyKey, String(enabled));
    await unicEngineNative.setAutoApplyReceived(enabled);
    set({ autoApplyReceived: enabled });
    if (enabled) await get().refreshReceived();
  },

  setAutomaticCapture: async (enabled) => {
    await AsyncStorage.setItem(captureKey, String(enabled));
    await unicEngineNative.setAutomaticCapture(enabled);
    set({ automaticCapture: enabled, pendingClipboardHint: null });
  },

  dismissClipboardHint: () => set({ pendingClipboardHint: null }),

  readHistoryFullText: async (historyID) => {
    const res = await unicEngineNative.readHistoryFullText(historyID, 2_097_152);
    return res.text ?? "";
  },

  saveIncomingShare: async (id) => {
    const { history_id: historyID } = await unicEngineNative.saveIncomingShare(id);
    if (!historyID) return;
    void get().refreshHistory();
    const item = await unicEngineNative.getHistoryItem(historyID);
    const [record] = await hydrateHistoryItems([item], get().devices);
    return record;
  },

  commitText: async (text, deviceIds) => {
    await unicEngineNative.commitText(text, deviceIds);
    await get().refreshHistory();
  },

  pickAndCommitFile: async (deviceIds) => {
    const res = await unicEngineNative.pickAndCommitFile(deviceIds);
    if (!res.cancelled) {
      await get().refreshHistory();
    }
    return { cancelled: res.cancelled };
  },

  resendHistoryItem: async (historyID, deviceIds) => {
    return unicEngineNative.resendHistoryItem({
      request_id: `mobile-resend-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      history_id: historyID,
      device_ids: deviceIds,
    });
  },

  readSystemClipboardAndCommit: async () => {
    const res = await unicEngineNative.readSystemClipboardAndCommit();
    set({ pendingClipboardHint: null });
    await get().refreshHistory();
    return res;
  },

  applyToClipboard: async (historyID) => {
    await unicEngineNative.applyHistoryToClipboard(historyID);
  },

  applyReceivedToClipboard: async (receiptID) => {
    await unicEngineNative.applyReceivedToClipboard(receiptID);
    await get().refreshReceived();
  },

  dismissReceived: async (receiptID) => {
    await unicEngineNative.dismissReceived(receiptID);
    await get().refreshReceived();
  },

  toggleStar: async (historyID) => {
    const item = get().rawHistory.find((it) => it.id === historyID);
    if (!item) return;
    const current = new Set(item.tags ?? []);
    if (current.has("starred")) current.delete("starred");
    else current.add("starred");
    await unicEngineNative.updateHistoryItem({
      id: item.id,
      expected_revision: item.revision,
      tags: Array.from(current),
    });
    await get().refreshHistory();
  },

  togglePin: async (historyID) => {
    const item = get().rawHistory.find((entry) => entry.id === historyID);
    if (item) await get().setHistoryTag(historyID, "pinned", !item.tags?.includes("pinned"));
  },

  setHistoryTag: async (historyID, tag, present) => {
    const item = get().rawHistory.find((entry) => entry.id === historyID);
    if (!item) return;
    const tags = new Set(item.tags ?? []);
    if (present) tags.add(tag);
    else tags.delete(tag);
    await unicEngineNative.updateHistoryItem({
      id: item.id,
      expected_revision: item.revision,
      tags: Array.from(tags),
    });
    await get().refreshHistory();
  },

  deleteHistoryItems: async (ids) => {
    const rawMap = new Map(get().rawHistory.map((it) => [it.id, it.revision]));
    for (const id of ids) {
      const rev = rawMap.get(id);
      if (rev) await unicEngineNative.deleteHistoryItem(id, rev);
    }
    await get().refreshHistory();
  },

  runAction: async (key, historyID) => {
    await unicEngineNative.runAction({
      request_id: `mobile-act-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      key,
      history_id: historyID,
    });
    await get().refreshHistory();
  },

  createPairingCode: async () => {
    const code = await unicEngineNative.createPairingCode();
    set({ pairingCode: code });
    return code;
  },

  cancelPairingCode: async () => {
    await unicEngineNative.cancelPairingCode();
    set({ pairingCode: null });
  },

  exportPairingTicket: () => unicEngineNative.exportPairingTicket(),

  joinByCode: (code) => unicEngineNative.joinByCode({
    request_id: `mobile-join-code-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    code: code.trim(),
  }),

  joinByTicket: (ticket) => unicEngineNative.joinByTicket({
    request_id: `mobile-join-ticket-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    ticket: ticket.trim(),
  }),

  approveDevice: async (device, verifiedFingerprint) => {
    await unicEngineNative.approveDevice({
      id: device.id,
      expected_revision: device.revision,
      expected_fingerprint: verifiedFingerprint,
      bindings: (device.bindings ?? []).map((b) => ({
        id: b.id,
        expected_revision: b.revision,
      })),
    });
    await get().refreshDevicesAndCandidates();
  },

  renameDevice: async (id, expectedRevision, name) => {
    await unicEngineNative.renameDevice({ id, expected_revision: expectedRevision, name });
    await get().refreshDevicesAndCandidates();
  },

  setBindingSync: async (bindingId, expectedRevision, syncEnabled) => {
    await unicEngineNative.setBindingSync({
      id: bindingId,
      expected_revision: expectedRevision,
      enabled: syncEnabled,
    });
    await get().refreshDevicesAndCandidates();
  },

  unpairDevice: async (id, expectedRevision) => {
    await unicEngineNative.unpairDevice(id, expectedRevision);
    await get().refreshDevicesAndCandidates();
  },

  pairCandidate: (candidate) => unicEngineNative.pairCandidate({
    request_id: `mobile-pair-candidate-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    device_id: candidate.device_id,
    backend_id: candidate.backend_id,
  }),

  ignoreCandidate: async (deviceId, ignored) => {
    await unicEngineNative.ignoreCandidate(deviceId, ignored);
    await get().refreshDevicesAndCandidates();
  },
}));
