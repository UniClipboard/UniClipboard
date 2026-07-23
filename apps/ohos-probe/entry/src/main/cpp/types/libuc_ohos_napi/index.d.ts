export interface OhFileMetadata {
  displayName: string
  sizeBytes: string
  mimeType?: string
}

export interface OhClipboardRepresentation {
  kind: string
  format: string
  mimeType?: string
  bytes?: Uint8Array
  handle?: string
  displayName?: string
  sizeBytes?: string
}

export interface OhClipboardSnapshot {
  observedAtMs: number
  representations: OhClipboardRepresentation[]
}

export interface OhHost {
  privateDataDirectory: string
  cacheDirectory: string
  temporaryDirectory: string
  secureStorageGet(key: string): OhHostResult<Uint8Array | null>
  secureStorageSet(key: string, value: Uint8Array): OhHostResult<void>
  secureStorageDelete(key: string): OhHostResult<void>
  fileMetadata(handle: string): OhHostResult<OhFileMetadata>
  fileReadChunk(handle: string, offset: string, maxBytes: number): OhHostResult<Uint8Array>
  fileWriteChunk(handle: string, offset: string, bytes: Uint8Array): OhHostResult<void>
  fileFinishWrite(handle: string): OhHostResult<void>
  clipboardRead(): OhHostResult<OhClipboardSnapshot>
  clipboardWrite(snapshot: OhClipboardSnapshot): OhHostResult<void>
}

export interface OhHostResult<T> {
  ok: boolean
  value?: T
  errorCategory?: string
}

export type PreparedHost = object

export interface OhSendReport {
  entryId: string
  atMs: number
  totalAccepted: number
  totalDuplicate: number
  totalOffline: number
  totalErrored: number
  totalPending: number
}

export interface OhSessionRecovery {
  unlocked: boolean
  resumed: boolean
}

export interface OhSpaceCreated {
  spaceId: string
  selfDeviceId: string
  identityFingerprint: string
}

export interface OhEngine {
  createSpace(deviceName: string | null, passphrase: string): Promise<OhSpaceCreated>
  recoverSession(allowSecureStorageUnlock: boolean): Promise<OhSessionRecovery>
  sendText(text: string, targetDevices: string[]): Promise<OhSendReport>
  exportEntry(entryId: string, destinationHandle: string): Promise<void>
  shutdown(deadlineMs: number): Promise<void>
}

declare const engine: {
  coreVersion(): string
  prepareHost(host: OhHost): PreparedHost
  startEngine(
    config: { appVersion: string; profileId: string },
    preparedHost: PreparedHost
  ): Promise<OhEngine>
}

export default engine
