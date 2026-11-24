import { create } from "zustand";
import type { ClipboardItem } from "@/types/clipboard";
import { buildDeviceNameMap, mapEngineHistoryItem } from "./mapper";
import { isNativeEngineAvailable, parseEngineError, unicEngineNative } from "./nativeModule";
import type {
  ActionInfo,
  Capabilities,
  EngineCandidate,
  EngineDevice,
  EngineError,
  EngineStateSnapshot,
  HistoryItem,
  MigrationProgress,
  MigrationRunResult,
  PairingCode,
  SensitiveTicket,
  SetupFlowState,
  SetupScreenViewState,
  SourceDetectionResult,
} from "./types";

export type EngineMode = "loading" | "live" | "unavailable";
export type BootStage = "loading" | "setup" | "main";
export type SetupEntryMode = "first-install" | "later";

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
  pairingCode: PairingCode | null;
  exportedTicket: SensitiveTicket | null;
  busy: boolean;
  lastError: EngineError | null;

  // Query filter state (for refreshing on events)
  queryText: string;
  queryTags: string[];
  queryKind: string | null;

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
  setQueryFilters: (text: string, tags: string[], kind: string | null) => Promise<void>;
  refreshHistory: () => Promise<void>;
  refreshDevicesAndCandidates: () => Promise<void>;
  refreshAll: () => Promise<void>;
  clearError: () => void;

  commitText: (text: string, deviceIds?: string[]) => Promise<void>;
  pickAndCommitFile: (deviceIds?: string[]) => Promise<{ cancelled: boolean }>;
  readSystemClipboardAndCommit: () => Promise<{ skipped?: boolean; reason?: string }>;
  applyToClipboard: (historyID: string) => Promise<void>;
  toggleStar: (historyID: string) => Promise<void>;
  togglePin: (historyID: string) => Promise<void>;
  deleteHistoryItems: (ids: readonly string[]) => Promise<void>;
  runAction: (key: string, historyID: string) => Promise<void>;

  createPairingCode: () => Promise<PairingCode>;
  cancelPairingCode: () => Promise<void>;
  exportPairingTicket: () => Promise<SensitiveTicket>;
  joinByCode: (code: string) => Promise<void>;
  joinByTicket: (ticket: string) => Promise<void>;
  approveDevice: (device: EngineDevice, verifiedFingerprint: string) => Promise<void>;
  renameDevice: (id: string, expectedRevision: number, name: string) => Promise<void>;
  setBindingSync: (bindingId: string, expectedRevision: number, syncEnabled: boolean) => Promise<void>;
  unpairDevice: (id: string, expectedRevision: number) => Promise<void>;
  pairCandidate: (candidate: EngineCandidate) => Promise<void>;
  ignoreCandidate: (deviceId: string, ignored: boolean) => Promise<void>;
}

const textPreviewCache = new Map<string, { text: string; html?: string; truncated: boolean }>();
const imagePreviewCache = new Map<string, { mime?: string; name?: string; uri?: string }>();

let unsubscribeFns: Array<() => void> = [];
let coalesceTimer: ReturnType<typeof setTimeout> | null = null;

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

      out[idx] = mapEngineHistoryItem(
        item,
        deviceNames,
        preview,
        imagePreview,
        searchTexts?.get(item.id),
      );
    }
  };

  await Promise.all(Array.from({ length: Math.min(4, Math.max(1, items.length)) }, () => worker()));
  return out;
}

async function startLiveEngineAndSubscribe(set: (partial: Partial<EngineStoreState>) => void, get: () => EngineStoreState) {
  clearNativeListeners();

  const snapshot = await unicEngineNative.initializeEngine();
  let capabilities: Capabilities | null = null;
  try {
    capabilities = await unicEngineNative.getCapabilities();
  } catch {
    capabilities = snapshot.capabilities ?? null;
  }

  const subEvt = unicEngineNative.addEventListener(() => {
    if (coalesceTimer !== null) return;
    coalesceTimer = setTimeout(() => {
      coalesceTimer = null;
      void get().refreshAll();
    }, 120);
  });
  const subResync = unicEngineNative.addResyncListener(() => {
    void get().refreshAll();
  });
  unsubscribeFns.push(() => subEvt.remove(), () => subResync.remove());

  set({
    mode: "live",
    bootStage: "main",
    snapshot,
    capabilities,
    devices: snapshot.devices ?? [],
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
  pairingCode: null,
  exportedTicket: null,
  busy: false,
  lastError: null,

  queryText: "",
  queryTags: [],
  queryKind: null,

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
        migrationErrorMessage: res.error?.message ?? "",
      });
    } catch (err) {
      active = false;
      subProg.remove();
      const e = parseEngineError(err);
      let nextView: SetupScreenViewState = "r-failed";
      let msg = e.message;
      if (e.code === "permission_denied") {
        nextView = suppliedPassphrase ? "m-auth-err" : "m-auth";
        msg = suppliedPassphrase ? "旧口令不正确或无法解锁凭据，请重新输入" : "";
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

      const subProg = unicEngineNative.addMigrationProgressListener((progress) => {
        set((prev) => ({
          migrationProgress: progress,
          setupViewState:
            prev.setupViewState === "m-stopping"
              ? "m-stopping"
              : progress.stage === "verify"
                ? "m-verifying"
                : "m-importing",
        }));
      });
      unsubscribeFns.push(() => subProg.remove());

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

  setQueryFilters: async (queryText, queryTags, queryKind) => {
    set({ queryText, queryTags, queryKind });
    await get().refreshHistory();
  },

  refreshHistory: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    const { queryText, queryTags, queryKind, devices } = get();
    try {
      const trimmed = queryText.trim();
      const hasSearchFilters = trimmed.length > 0 || queryTags.length > 0 || queryKind !== null;

      let items: HistoryItem[] = [];
      const searchTexts = new Map<string, string>();
      if (hasSearchFilters) {
        const query: Record<string, unknown> = {};
        if (trimmed.length > 0) {
          query.text_terms = trimmed.split(/\s+/).filter(Boolean);
        }
        if (queryTags.length > 0) {
          query.tags = queryTags;
        }
        if (queryKind) {
          query.types = [queryKind];
        }
        const page = await unicEngineNative.searchHistory({ query, limit: 100 });
        items = page.hits.map((hit) => {
          if (hit.preview?.text) searchTexts.set(hit.item.id, hit.preview.text);
          return hit.item;
        });
      } else {
        const page = await unicEngineNative.listHistory({ limit: 100 });
        items = page.items ?? [];
      }

      const records = await hydrateHistoryItems(items, devices, searchTexts);
      let tags: string[] = [];
      try {
        tags = await unicEngineNative.listHistoryTags();
      } catch {
        tags = get().availableTags;
      }
      set({ rawHistory: items, records, availableTags: tags });
    } catch (err) {
      set({ lastError: parseEngineError(err) });
    }
  },

  refreshDevicesAndCandidates: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    try {
      const [snapshot, capabilities, devices, candidates, pairingCode, actions] = await Promise.all([
        unicEngineNative.getState(),
        unicEngineNative.getCapabilities(),
        unicEngineNative.listDevices(),
        unicEngineNative.listCandidates(),
        unicEngineNative.getPairingCode().catch(() => null),
        unicEngineNative.listActions().catch(() => []),
      ]);
      set({ snapshot, capabilities, devices, candidates, pairingCode, actions });
    } catch (err) {
      set({ lastError: parseEngineError(err) });
    }
  },

  refreshAll: async () => {
    if (get().mode !== "live" || get().bootStage !== "main") return;
    await get().refreshDevicesAndCandidates();
    await get().refreshHistory();
  },

  clearError: () => set({ lastError: null }),

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

  readSystemClipboardAndCommit: async () => {
    const res = await unicEngineNative.readSystemClipboardAndCommit();
    await get().refreshHistory();
    return res;
  },

  applyToClipboard: async (historyID) => {
    await unicEngineNative.applyHistoryToClipboard(historyID);
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
    const item = get().rawHistory.find((it) => it.id === historyID);
    if (!item) return;
    const current = new Set(item.tags ?? []);
    if (current.has("pinned")) current.delete("pinned");
    else current.add("pinned");
    await unicEngineNative.updateHistoryItem({
      id: item.id,
      expected_revision: item.revision,
      tags: Array.from(current),
    });
    await get().refreshHistory();
  },

  deleteHistoryItems: async (ids) => {
    const rawMap = new Map(get().rawHistory.map((it) => [it.id, it.revision]));
    for (const id of ids) {
      const rev = rawMap.get(id);
      if (rev) {
        await unicEngineNative.deleteHistoryItem(id, rev);
      }
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

  exportPairingTicket: async () => {
    const ticket = await unicEngineNative.exportPairingTicket();
    set({ exportedTicket: ticket });
    return ticket;
  },

  joinByCode: async (code) => {
    const opID = await unicEngineNative.joinByCode({
      request_id: `mobile-join-code-${Date.now()}`,
      code: code.trim(),
    });
    // Poll briefly so user sees any immediate join failure or pending peer right away.
    for (let i = 0; i < 15; i++) {
      const op = await unicEngineNative.getOperation(opID);
      if (op.phase === "failed") {
        throw parseEngineError(op.error ?? new Error("pairing failed"));
      }
      if (op.phase === "succeeded" || op.phase === "completed") break;
      await new Promise((r) => setTimeout(r, 200));
    }
    await get().refreshDevicesAndCandidates();
  },

  joinByTicket: async (ticket) => {
    const opID = await unicEngineNative.joinByTicket({
      request_id: `mobile-join-ticket-${Date.now()}`,
      ticket: ticket.trim(),
    });
    for (let i = 0; i < 15; i++) {
      const op = await unicEngineNative.getOperation(opID);
      if (op.phase === "failed") {
        throw parseEngineError(op.error ?? new Error("pairing failed"));
      }
      if (op.phase === "succeeded" || op.phase === "completed") break;
      await new Promise((r) => setTimeout(r, 200));
    }
    await get().refreshDevicesAndCandidates();
  },

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
      sync_enabled: syncEnabled,
    });
    await get().refreshDevicesAndCandidates();
  },

  unpairDevice: async (id, expectedRevision) => {
    await unicEngineNative.unpairDevice(id, expectedRevision);
    await get().refreshDevicesAndCandidates();
  },

  pairCandidate: async (candidate) => {
    await unicEngineNative.pairCandidate({
      device_id: candidate.device_id,
      backend_id: candidate.backend_id,
      name: candidate.name,
    });
    await get().refreshDevicesAndCandidates();
  },

  ignoreCandidate: async (deviceId, ignored) => {
    await unicEngineNative.ignoreCandidate(deviceId, ignored);
    await get().refreshDevicesAndCandidates();
  },
}));
