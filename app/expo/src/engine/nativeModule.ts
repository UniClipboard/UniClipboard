import { type NativeModule, requireOptionalNativeModule } from "expo";
import {
  type ActionInfo,
  type ActionResult,
  type BackgroundSyncConfigPatch,
  type BackgroundSyncStatus,
  type Capabilities,
  type ClipboardChangedEvent,
  type CustomSourceSelectionResult,
  type DeviceBinding,
  EngineError,
  type EngineCandidate,
  type EngineDevice,
  type EngineErrorPayload,
  type EngineOperation,
  type EngineSettings,
  type FileCacheState,
  type HistoryFolder,
  type PartDescriptor,
  type EngineStateSnapshot,
  type HistoryItem,
  type HistoryPage,
  type HistorySearchCountsResult,
  type HistorySearchPage,
  type ImagePreviewResult,
  type KeyboardExtensionStatus,
  type MigrationProgress,
  type MigrationRunResult,
  type PairingCode,
  type PairingCodeResponse,
  type PickAndCommitFileResult,
  type ReceivedContent,
  type SensitiveTicket,
  type SettingsState,
  type SetupFlowState,
  type SourceDetectionResult,
  type SubmissionResult,
  type TextPreview,
} from "./types";

type NativeEvents = {
  onIncomingShare: () => void;
  onEngineEvent: (payload: { event: string }) => void;
  onEngineResync: (payload: { reason: string }) => void;
  onMigrationProgress: (payload: MigrationProgress) => void;
  onClipboardChanged: (payload: ClipboardChangedEvent) => void;
  onBackgroundSyncStateChanged: (payload: { reason: string }) => void;
  onUpdateDownloadProgress: (payload: {
    progress: number;
    bytesReceived: number;
    totalBytes: number;
  }) => void;
};

export type TelemetryState = { configured_enabled: boolean; effective_enabled: boolean };

declare class NativeUnicEngineModule extends NativeModule<NativeEvents> {
  openSetupFlow(): Promise<string>;
  getSetupState(): Promise<string>;
  detectSetupSource(dir: string): Promise<string>;
  selectSetupCustomSource(): Promise<string>;
  beginLaterMigration(): Promise<void>;
  runSetupMigration(passphrase: string | null): Promise<string>;
  cancelSetupMigration(): Promise<void>;
  markSetupComplete(): Promise<void>;

  initializeEngine(deviceName: string | null): Promise<string>;
  shutdownEngine(): Promise<void>;
  getState(): Promise<string>;
  getTelemetryState(): Promise<string>;
  setTelemetryEnabled(enabled: boolean): Promise<string>;
  exportRecentDiagnosticLogs(): Promise<string>;
  getCapabilities(): Promise<string>;
  updateSettings(reqJSON: string): Promise<string>;
  getFileCacheState(): Promise<string>;
  setAutomaticCapture(enabled: boolean): Promise<void>;
  setAutoApplyReceived?(enabled: boolean): Promise<void>;
  clearHistoryPreviews(): Promise<void>;
  reconcileHistoryPreviews(): Promise<void>;
  historyFileMetadata(historyID: string): Promise<string>;
  listHistoryFolder(historyID: string, limit: number): Promise<string>;
  getBackgroundSyncEnabled(): Promise<boolean>;
  setBackgroundSyncEnabled(enabled: boolean): Promise<boolean>;
  getBackgroundSyncStatus?(): Promise<string>;
  updateBackgroundSyncConfig?(patchJSON: string): Promise<string>;
  requestIgnoreBatteryOptimizations?(): Promise<boolean>;
  requestOverlayPermission?(): Promise<boolean>;
  requestShizukuPermission?(): Promise<boolean>;
  openShizukuAppOrGuide?(): Promise<boolean>;
  resolveBackgroundClipboardRestriction?(): Promise<boolean>;
  listIncomingShares(): Promise<string>;
  saveIncomingShare(id: string): Promise<string>;
  getKeyboardExtensionStatus?(): Promise<string>;
  setKeyboardPreferences?(soundEnabled: boolean, hapticEnabled: boolean): Promise<string>;
  syncKeyboardSnapshot?(): Promise<string>;
  copyPlainText(text: string): Promise<void>;
  copyPairingTicket(text: string, expiresAtMs: number): Promise<void>;
  pickAndCommitImage(camera: boolean): Promise<string>;
  scanQRCode?(cancelLabel: string, hint: string): Promise<string | null>;
  exportHistoryMedia(historyID: string, action: string): Promise<boolean>;
  shareHistoryMedia(idsJSON: string, text: string | null): Promise<void>;

  listHistory(reqJSON: string): Promise<string>;
  getHistoryItem(historyID: string): Promise<string>;
  readHistoryTextPreview(historyID: string, maxBytes: number): Promise<string>;
  readHistoryFullText(historyID: string, maxBytes: number): Promise<string>;
  exportHistoryImagePreview(historyID: string): Promise<string>;
  updateHistoryItem(reqJSON: string): Promise<string>;
  deleteHistoryItem(historyID: string, expectedRevision: string): Promise<void>;
  listHistoryTags(): Promise<string>;
  searchHistory(reqJSON: string): Promise<string>;
  countHistorySearches(reqJSON: string): Promise<string>;
  commitText(text: string, deviceIdsJson: string | null): Promise<string>;
  pickAndCommitFile(deviceIdsJson: string | null): Promise<string>;
  resendHistoryItem(reqJSON: string): Promise<string>;
  listReceived(): Promise<string>;
  dismissReceived(receiptID: string): Promise<void>;
  applyReceivedToClipboard(receiptID: string): Promise<string>;
  applyHistoryToClipboard(historyID: string): Promise<string>;
  readSystemClipboardAndCommit(): Promise<string>;

  listActions(): Promise<string>;
  runAction(reqJSON: string): Promise<string>;

  createPairingCode(): Promise<string>;
  getPairingCode(): Promise<string>;
  cancelPairingCode(): Promise<void>;
  exportPairingTicket(): Promise<string>;
  joinByCode(reqJSON: string): Promise<string>;
  joinByTicket(reqJSON: string): Promise<string>;
  getOperation(opID: string): Promise<string>;
  listDevices(): Promise<string>;
  approveDevice(reqJSON: string): Promise<string>;
  renameDevice(reqJSON: string): Promise<string>;
  setBindingSync(reqJSON: string): Promise<string>;
  unpairDevice(deviceID: string, expectedRevision: number): Promise<void>;
  listCandidates(): Promise<string>;
  pairCandidate(reqJSON: string): Promise<string>;
  ignoreCandidate(deviceID: string, ignored: boolean): Promise<void>;

  checkAndroidUpdate?(channel: string): Promise<string>;
  downloadAndInstallAndroidUpdate?(
    url: string,
    filename: string,
    expectedSha256: string | null,
    versionKey: string,
  ): Promise<string>;
  cancelAndroidUpdateDownload?(): Promise<void>;
  cleanAndroidUpdateCache?(keepVersionKey: string | null): Promise<void>;
  hideAndroidSplashScreen?(): void;
}

const nativeMod = requireOptionalNativeModule<NativeUnicEngineModule>("UnicEngine");

export function isNativeEngineAvailable(): boolean {
  return nativeMod !== null;
}

export function parseEngineError(err: unknown): EngineError {
  if (err instanceof EngineError) return err;
  const raw = err instanceof Error ? err.message : String(err ?? "");
  const tagged = raw.match(/\[unic:([a-z0-9_]+)\]\s*(\{[\s\S]*\})/);
  if (tagged) {
    try {
      const payload = JSON.parse(tagged[2]) as Partial<EngineErrorPayload>;
      return new EngineError({
        code: (payload.code ?? tagged[1] ?? "internal") as EngineError["code"],
        message: payload.message ?? "operation failed",
        retryable: payload.retryable,
        retry_after_ms: payload.retry_after_ms,
      });
    } catch {
      return new EngineError({ code: tagged[1] as EngineError["code"], message: "operation failed" });
    }
  }
  const trimmed = raw.trim();
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const payload = JSON.parse(trimmed) as Partial<EngineErrorPayload>;
      if (payload.code) {
        return new EngineError({
          code: payload.code,
          message: payload.message ?? "operation failed",
          retryable: payload.retryable,
          retry_after_ms: payload.retry_after_ms,
        });
      }
    } catch {
      // ignore non-JSON message
    }
  }
  return new EngineError({ code: "internal", message: trimmed || "engine error" });
}

function requireMod(): NativeUnicEngineModule {
  if (!nativeMod) {
    throw new EngineError({
      code: "unavailable",
      message: "Native UnicEngine module is unavailable in this runtime",
    });
  }
  return nativeMod;
}

async function callJSON<T>(fn: () => Promise<string>): Promise<T> {
  try {
    const raw = await fn();
    return JSON.parse(raw) as T;
  } catch (err) {
    throw parseEngineError(err);
  }
}

async function callString(fn: () => Promise<string>): Promise<string> {
  try {
    return await fn();
  } catch (err) {
    throw parseEngineError(err);
  }
}

async function callVoid(fn: () => Promise<void>): Promise<void> {
  try {
    await fn();
  } catch (err) {
    throw parseEngineError(err);
  }
}

export const unicEngineNative = {
  addIncomingShareListener: (cb: () => void) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onIncomingShare", cb);
  },
  getBackgroundSyncEnabled: () => requireMod().getBackgroundSyncEnabled(),
  setBackgroundSyncEnabled: (enabled: boolean) => requireMod().setBackgroundSyncEnabled(enabled)
    .catch((error) => { throw parseEngineError(error); }),
  getBackgroundSyncStatus: () => callJSON<BackgroundSyncStatus>(() => {
    const mod = requireMod();
    if (!mod.getBackgroundSyncStatus) {
      throw new EngineError({ code: "unsupported", message: "background sync status unavailable" });
    }
    return mod.getBackgroundSyncStatus();
  }),
  updateBackgroundSyncConfig: (patch: BackgroundSyncConfigPatch) => callJSON<BackgroundSyncStatus>(() => {
    const mod = requireMod();
    if (!mod.updateBackgroundSyncConfig) {
      throw new EngineError({ code: "unsupported", message: "background sync config unavailable" });
    }
    return mod.updateBackgroundSyncConfig(JSON.stringify(patch));
  }),
  requestIgnoreBatteryOptimizations: async (): Promise<boolean> => {
    const mod = requireMod();
    if (!mod.requestIgnoreBatteryOptimizations) return false;
    try {
      return await mod.requestIgnoreBatteryOptimizations();
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  requestOverlayPermission: async (): Promise<boolean> => {
    const mod = requireMod();
    if (!mod.requestOverlayPermission) return false;
    try {
      return await mod.requestOverlayPermission();
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  requestShizukuPermission: async (): Promise<boolean> => {
    const mod = requireMod();
    if (!mod.requestShizukuPermission) return false;
    try {
      return await mod.requestShizukuPermission();
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  openShizukuAppOrGuide: async (): Promise<boolean> => {
    const mod = requireMod();
    if (!mod.openShizukuAppOrGuide) return false;
    try {
      return await mod.openShizukuAppOrGuide();
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  resolveBackgroundClipboardRestriction: async (): Promise<boolean> => {
    const mod = requireMod();
    if (!mod.resolveBackgroundClipboardRestriction) return false;
    try {
      return await mod.resolveBackgroundClipboardRestriction();
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  addBackgroundSyncStateListener: (cb: (reason: string) => void) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onBackgroundSyncStateChanged", ({ reason }) => cb(reason));
  },
  listIncomingShares: () => callJSON<string[]>(() => requireMod().listIncomingShares()),
  saveIncomingShare: (id: string) => callJSON<SubmissionResult>(
    () => requireMod().saveIncomingShare(id),
  ),
  getKeyboardExtensionStatus: async (): Promise<KeyboardExtensionStatus> => {
    const mod = nativeMod;
    if (!mod?.getKeyboardExtensionStatus) {
      return {
        added: false,
        has_full_access_seen: false,
        sound_enabled: true,
        haptic_enabled: true,
      };
    }
    return callJSON<KeyboardExtensionStatus>(() => mod.getKeyboardExtensionStatus!());
  },
  setKeyboardPreferences: async (
    soundEnabled: boolean,
    hapticEnabled: boolean,
  ): Promise<KeyboardExtensionStatus> => {
    const mod = requireMod();
    if (!mod.setKeyboardPreferences) {
      throw new EngineError({ code: "unsupported", message: "keyboard preferences unavailable" });
    }
    return callJSON<KeyboardExtensionStatus>(() =>
      mod.setKeyboardPreferences!(soundEnabled, hapticEnabled),
    );
  },
  syncKeyboardSnapshot: async (): Promise<{ count: number }> => {
    const mod = nativeMod;
    if (!mod?.syncKeyboardSnapshot) return { count: 0 };
    return callJSON<{ count: number }>(() => mod.syncKeyboardSnapshot!());
  },
  addEventListener: (cb: (eventJSON: string) => void) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onEngineEvent", ({ event }) => cb(event));
  },
  addResyncListener: (cb: (reason: string) => void) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onEngineResync", ({ reason }) => cb(reason));
  },
  addMigrationProgressListener: (cb: (progress: MigrationProgress) => void) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onMigrationProgress", cb);
  },
  addClipboardChangedListener: (cb: (payload: ClipboardChangedEvent) => void) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onClipboardChanged", cb);
  },

  openSetupFlow: () => callJSON<SetupFlowState>(() => requireMod().openSetupFlow()),
  getSetupState: () => callJSON<SetupFlowState>(() => requireMod().getSetupState()),
  detectSetupSource: (dir = "") =>
    callJSON<SourceDetectionResult>(() => requireMod().detectSetupSource(dir)),
  selectSetupCustomSource: () =>
    callJSON<CustomSourceSelectionResult>(() => requireMod().selectSetupCustomSource()),
  beginLaterMigration: () => callVoid(() => requireMod().beginLaterMigration()),
  runSetupMigration: (passphrase?: string | null) =>
    callJSON<MigrationRunResult>(() =>
      requireMod().runSetupMigration(passphrase && passphrase.length > 0 ? passphrase : null),
    ),
  cancelSetupMigration: () => callVoid(() => requireMod().cancelSetupMigration()),
  markSetupComplete: () => callVoid(() => requireMod().markSetupComplete()),

  initializeEngine: (deviceName?: string) =>
    callJSON<EngineStateSnapshot>(() => requireMod().initializeEngine(deviceName ?? null)),
  shutdownEngine: () => callVoid(() => requireMod().shutdownEngine()),
  getState: () => callJSON<EngineStateSnapshot>(() => requireMod().getState()),
  getTelemetryState: () => callJSON<TelemetryState>(() => requireMod().getTelemetryState()),
  setTelemetryEnabled: (enabled: boolean) => callJSON<TelemetryState>(() => requireMod().setTelemetryEnabled(enabled)),
  exportRecentDiagnosticLogs: () => callString(() => requireMod().exportRecentDiagnosticLogs()),
  getCapabilities: () => callJSON<Capabilities>(() => requireMod().getCapabilities()),
  updateSettings: (req: { expected_revision: string; value: EngineSettings }) =>
    callJSON<SettingsState>(() => requireMod().updateSettings(JSON.stringify(req))),

  getFileCacheState: () => callJSON<FileCacheState>(() => requireMod().getFileCacheState()),
  setAutomaticCapture: (enabled: boolean) => callVoid(() => requireMod().setAutomaticCapture(enabled)),
  setAutoApplyReceived: (enabled: boolean) => callVoid(async () => {
    const mod = nativeMod;
    if (mod?.setAutoApplyReceived) {
      await mod.setAutoApplyReceived(enabled);
    }
  }),
  clearHistoryPreviews: () => callVoid(() => requireMod().clearHistoryPreviews()),
  reconcileHistoryPreviews: () => callVoid(() => requireMod().reconcileHistoryPreviews()),
  historyFileMetadata: (id: string) =>
    callJSON<PartDescriptor[] | null>(() => requireMod().historyFileMetadata(id)),
  listHistoryFolder: (id: string) =>
    callJSON<HistoryFolder>(() => requireMod().listHistoryFolder(id, 200)),
  copyPairingTicket: (text: string, expiresAtMs: number) => callVoid(() => requireMod().copyPairingTicket(text, expiresAtMs)),
  copyPlainText: (text: string) => callVoid(() => requireMod().copyPlainText(text)),
  pickAndCommitImage: (camera: boolean) =>
    callJSON<PickAndCommitFileResult>(() => requireMod().pickAndCommitImage(camera)),
  scanQRCode: async (cancelLabel: string, hint: string): Promise<string | null> => {
    const mod = requireMod();
    if (!mod.scanQRCode) {
      throw new EngineError({ code: "unsupported", message: "native QR scanner unavailable" });
    }
    try {
      return await mod.scanQRCode(cancelLabel, hint);
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  exportHistoryMedia: async (id: string, action: "photos" | "files") => {
    try {
      return await requireMod().exportHistoryMedia(id, action);
    } catch (err) {
      throw parseEngineError(err);
    }
  },
  shareHistoryMedia: (ids: string[], text?: string) =>
    callVoid(() => requireMod().shareHistoryMedia(JSON.stringify(ids), text ?? null)),

  listHistory: (req: { limit?: number; cursor?: string; filter?: Record<string, unknown> } = {}) =>
    callJSON<HistoryPage>(() => requireMod().listHistory(JSON.stringify(req))),
  getHistoryItem: (historyID: string) =>
    callJSON<HistoryItem>(() => requireMod().getHistoryItem(historyID)),
  readHistoryTextPreview: (historyID: string, maxBytes = 512) =>
    callJSON<TextPreview>(() => requireMod().readHistoryTextPreview(historyID, maxBytes)),
  readHistoryFullText: (historyID: string, maxBytes = 2_097_152) =>
    callJSON<TextPreview>(() => requireMod().readHistoryFullText(historyID, maxBytes)),
  exportHistoryImagePreview: async (historyID: string): Promise<ImagePreviewResult> => {
    const raw = await callJSON<ImagePreviewResult>(() =>
      requireMod().exportHistoryImagePreview(historyID),
    );
    return {
      ...raw,
      mime: raw.part?.mime ?? raw.mime,
      name: raw.part?.name ?? raw.name,
    };
  },
  updateHistoryItem: (req: { id: string; expected_revision: string; tags?: string[] }) =>
    callJSON<HistoryItem>(() => requireMod().updateHistoryItem(JSON.stringify(req))),
  deleteHistoryItem: (historyID: string, expectedRevision: string) =>
    callVoid(() => requireMod().deleteHistoryItem(historyID, expectedRevision)),
  listHistoryTags: () => callJSON<string[]>(() => requireMod().listHistoryTags()),
  searchHistory: (req: { query: Record<string, unknown>; cursor?: string; limit?: number }) =>
    callJSON<HistorySearchPage>(() => requireMod().searchHistory(JSON.stringify(req))),
  countHistorySearches: async (
    queries: Array<Record<string, unknown>>,
  ): Promise<HistorySearchCountsResult> => {
    const raw = await callJSON<{
      counts: Array<string | number>;
      history_revision: string;
      evaluated_at_ms: number;
    }>(() => requireMod().countHistorySearches(JSON.stringify({ queries })));
    return {
      counts: (raw.counts ?? []).map((c) => Number(c) || 0),
      history_revision: raw.history_revision,
      evaluated_at_ms: raw.evaluated_at_ms,
    };
  },
  commitText: (text: string, deviceIds?: string[]) =>
    callJSON<SubmissionResult>(() =>
      requireMod().commitText(text, deviceIds && deviceIds.length > 0 ? JSON.stringify(deviceIds) : null),
    ),
  pickAndCommitFile: (deviceIds?: string[]) =>
    callJSON<PickAndCommitFileResult>(() =>
      requireMod().pickAndCommitFile(
        deviceIds && deviceIds.length > 0 ? JSON.stringify(deviceIds) : null,
      ),
    ),
  resendHistoryItem: (req: {
    request_id: string;
    history_id: string;
    device_ids?: string[];
    backend_ids?: string[];
  }) => callJSON<SubmissionResult>(() => requireMod().resendHistoryItem(JSON.stringify(req))),
  listReceived: () => callJSON<ReceivedContent[]>(() => requireMod().listReceived()),
  dismissReceived: (receiptID: string) =>
    callVoid(() => requireMod().dismissReceived(receiptID)),
  applyReceivedToClipboard: (receiptID: string) =>
    callJSON<{ applied: boolean }>(() => requireMod().applyReceivedToClipboard(receiptID)),
  applyHistoryToClipboard: (historyID: string) =>
    callJSON<{ applied: boolean }>(() => requireMod().applyHistoryToClipboard(historyID)),
  readSystemClipboardAndCommit: () =>
    callJSON<{ skipped?: boolean; reason?: string; disposition?: string; operation_id?: string }>(() =>
      requireMod().readSystemClipboardAndCommit(),
    ),

  listActions: () => callJSON<ActionInfo[]>(() => requireMod().listActions()),
  runAction: (req: { request_id: string; key: string; history_id: string }) =>
    callJSON<ActionResult>(() => requireMod().runAction(JSON.stringify(req))),

  createPairingCode: () => callJSON<PairingCode>(() => requireMod().createPairingCode()),
  getPairingCode: async (): Promise<PairingCode | null> => {
    const raw = await callJSON<PairingCodeResponse>(() => requireMod().getPairingCode());
    if (!raw || !raw.exists || !raw.code) {
      return null;
    }
    return { code: raw.code, expires_at_ms: raw.expires_at_ms };
  },
  cancelPairingCode: () => callVoid(() => requireMod().cancelPairingCode()),
  exportPairingTicket: () => callJSON<SensitiveTicket>(() => requireMod().exportPairingTicket()),
  joinByCode: (req: { request_id: string; code: string; backend_id?: string }) =>
    callString(() => requireMod().joinByCode(JSON.stringify(req))),
  joinByTicket: (req: { request_id: string; ticket: string; backend_id?: string }) =>
    callString(() => requireMod().joinByTicket(JSON.stringify(req))),
  getOperation: (opID: string) => callJSON<EngineOperation>(() => requireMod().getOperation(opID)),
  listDevices: () => callJSON<EngineDevice[]>(() => requireMod().listDevices()),
  approveDevice: (req: {
    id: string;
    expected_revision: number;
    expected_fingerprint: string;
    bindings: Array<{ id: string; expected_revision: number }>;
  }) => callJSON<EngineDevice>(() => requireMod().approveDevice(JSON.stringify(req))),
  renameDevice: (req: { id: string; expected_revision: number; name: string }) =>
    callJSON<EngineDevice>(() => requireMod().renameDevice(JSON.stringify(req))),
  setBindingSync: (req: { id: string; expected_revision: number; enabled: boolean }) =>
    callJSON<DeviceBinding>(() => requireMod().setBindingSync(JSON.stringify(req))),
  unpairDevice: (deviceID: string, expectedRevision: number) =>
    callVoid(() => requireMod().unpairDevice(deviceID, expectedRevision)),
  listCandidates: () => callJSON<EngineCandidate[]>(() => requireMod().listCandidates()),
  pairCandidate: (req: { request_id: string; device_id: string; backend_id: string }) =>
    callString(() => requireMod().pairCandidate(JSON.stringify(req))),
  ignoreCandidate: (deviceID: string, ignored: boolean) =>
    callVoid(() => requireMod().ignoreCandidate(deviceID, ignored)),

  addUpdateDownloadProgressListener: (
    cb: (payload: { progress: number; bytesReceived: number; totalBytes: number }) => void,
  ) => {
    if (!nativeMod) return { remove: () => {} };
    return nativeMod.addListener("onUpdateDownloadProgress", cb);
  },
  checkAndroidUpdate: (channel: "alpha" | "stable") =>
    callJSON<{
      found: boolean;
      channel: "alpha" | "stable";
      manifest?: Record<string, unknown>;
      supported_abis: string[];
    }>(() => {
      const mod = requireMod();
      if (!mod.checkAndroidUpdate) {
        throw new EngineError({ code: "unsupported", message: "android update check unavailable" });
      }
      return mod.checkAndroidUpdate(channel);
    }),
  downloadAndInstallAndroidUpdate: (
    url: string,
    filename: string,
    expectedSha256: string | null,
    versionKey: string,
  ) =>
    callJSON<{
      status: "installer_launched" | "needs_install_permission";
      cached: boolean;
      path: string;
    }>(() => {
      const mod = requireMod();
      if (!mod.downloadAndInstallAndroidUpdate) {
        throw new EngineError({
          code: "unsupported",
          message: "android update download unavailable",
        });
      }
      return mod.downloadAndInstallAndroidUpdate(url, filename, expectedSha256, versionKey);
    }),
  cancelAndroidUpdateDownload: () =>
    callVoid(async () => {
      const mod = nativeMod;
      if (mod?.cancelAndroidUpdateDownload) {
        await mod.cancelAndroidUpdateDownload();
      }
    }),
  cleanAndroidUpdateCache: (keepVersionKey?: string | null) =>
    callVoid(async () => {
      const mod = nativeMod;
      if (mod?.cleanAndroidUpdateCache) {
        await mod.cleanAndroidUpdateCache(keepVersionKey ?? null);
      }
    }),
  hideAndroidSplashScreen: () => {
    try {
      nativeMod?.hideAndroidSplashScreen?.();
    } catch {
      // ignore if native splash already dismissed or unavailable
    }
  },
};
