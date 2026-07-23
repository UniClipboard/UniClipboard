import { common } from '@kit.AbilityKit';
import { FileHandleRegistry, FileMetadata } from './FileHandleRegistry';
import { SecureAssetStorage } from './SecureAssetStorage';

export interface ClipboardRepresentation {
  kind: string;
  format: string;
  mimeType?: string;
  bytes?: Uint8Array;
  handle?: string;
  displayName?: string;
  sizeBytes?: string;
}

export interface ClipboardSnapshot {
  observedAtMs: number;
  representations: ClipboardRepresentation[];
}

export interface EngineHost {
  privateDataDirectory: string;
  cacheDirectory: string;
  temporaryDirectory: string;
  secureStorageGet: (key: string) => HostResult<Uint8Array | null>;
  secureStorageSet: (key: string, value: Uint8Array) => HostResult<void>;
  secureStorageDelete: (key: string) => HostResult<void>;
  fileMetadata: (handle: string) => HostResult<FileMetadata>;
  fileReadChunk: (handle: string, offset: string, maxBytes: number) => HostResult<Uint8Array>;
  fileWriteChunk: (handle: string, offset: string, bytes: Uint8Array) => HostResult<void>;
  fileFinishWrite: (handle: string) => HostResult<void>;
  clipboardRead: () => HostResult<ClipboardSnapshot>;
  clipboardWrite: (snapshot: ClipboardSnapshot) => HostResult<void>;
}

export interface HostResult<T> {
  ok: boolean;
  value?: T;
  errorCategory?: string;
}

export function createEngineHost(
  context: common.UIAbilityContext,
  storage: SecureAssetStorage,
  files: FileHandleRegistry
): EngineHost {
  return {
    privateDataDirectory: context.filesDir,
    cacheDirectory: context.cacheDir,
    temporaryDirectory: context.tempDir,
    secureStorageGet: (key: string): HostResult<Uint8Array | null> =>
      hostResult((): Uint8Array | null => storage.get(key)),
    secureStorageSet: (key: string, value: Uint8Array): HostResult<void> =>
      hostResult((): void => storage.set(key, value)),
    secureStorageDelete: (key: string): HostResult<void> =>
      hostResult((): void => storage.delete(key)),
    fileMetadata: (handle: string): HostResult<FileMetadata> =>
      hostResult((): FileMetadata => files.fileMetadata(handle)),
    fileReadChunk: (handle: string, offset: string, maxBytes: number): HostResult<Uint8Array> =>
      hostResult((): Uint8Array => files.fileReadChunk(handle, offset, maxBytes)),
    fileWriteChunk: (handle: string, offset: string, bytes: Uint8Array): HostResult<void> =>
      hostResult((): void => files.fileWriteChunk(handle, offset, bytes)),
    fileFinishWrite: (handle: string): HostResult<void> =>
      hostResult((): void => files.fileFinishWrite(handle)),
    clipboardRead: (): HostResult<ClipboardSnapshot> => hostResult((): ClipboardSnapshot => ({
        observedAtMs: Date.now(),
        representations: []
      })),
    clipboardWrite: (_snapshot: ClipboardSnapshot): HostResult<void> => ({
      ok: false,
      errorCategory: 'unavailable'
    })
  };
}

function hostResult<T>(operation: () => T): HostResult<T> {
  try {
    return { ok: true, value: operation() };
  } catch (error) {
    const message = String(error);
    if (message.includes('PERMISSION_DENIED')) {
      return { ok: false, errorCategory: 'permission_denied' };
    }
    if (message.includes('INVALID_HANDLE')) {
      return { ok: false, errorCategory: 'invalid_handle' };
    }
    if (message.includes('UNAVAILABLE')) {
      return { ok: false, errorCategory: 'unavailable' };
    }
    return { ok: false, errorCategory: 'io' };
  }
}
