import { fileIo } from '@kit.CoreFileKit';
import { util } from '@kit.ArkTS';

export interface FileMetadata {
  displayName: string;
  sizeBytes: string;
  mimeType?: string;
}

interface FileEntry {
  uri: string;
  writable: boolean;
}

export class FileHandleRegistry {
  private readonly entries: Map<string, FileEntry> = new Map<string, FileEntry>();

  registerInput(uri: string): string {
    return this.register(uri, false);
  }

  registerOutput(uri: string): string {
    return this.register(uri, true);
  }

  remove(handle: string): void {
    this.entries.delete(handle);
  }

  clear(): void {
    this.entries.clear();
  }

  fileMetadata(handle: string): FileMetadata {
    const entry = this.requireEntry(handle);
    const file = this.open(entry.uri, fileIo.OpenMode.READ_ONLY);
    try {
      const stat = this.stat(file.fd);
      return {
        displayName: file.name,
        sizeBytes: stat.size.toString()
      };
    } finally {
      this.close(file);
    }
  }

  fileReadChunk(handle: string, offset: string, maxBytes: number): Uint8Array {
    const entry = this.requireEntry(handle);
    const file = this.open(entry.uri, fileIo.OpenMode.READ_ONLY);
    try {
      const buffer = new ArrayBuffer(maxBytes);
      const count = this.read(file.fd, buffer, this.offset(offset), maxBytes);
      return new Uint8Array(buffer).slice(0, count);
    } finally {
      this.close(file);
    }
  }

  fileWriteChunk(handle: string, offset: string, bytes: Uint8Array): void {
    const entry = this.requireEntry(handle);
    if (!entry.writable) {
      throw new Error('OHOS_FILE_PERMISSION_DENIED');
    }
    const file = this.open(entry.uri, fileIo.OpenMode.READ_WRITE);
    const copy = new Uint8Array(bytes);
    try {
      this.write(file.fd, copy, this.offset(offset));
      this.sync(file.fd);
    } finally {
      copy.fill(0);
      this.close(file);
    }
  }

  fileFinishWrite(handle: string): void {
    const entry = this.requireEntry(handle);
    if (!entry.writable) {
      throw new Error('OHOS_FILE_PERMISSION_DENIED');
    }
    const file = this.open(entry.uri, fileIo.OpenMode.READ_ONLY);
    this.close(file);
  }

  private register(uri: string, writable: boolean): string {
    const handle = util.generateRandomUUID();
    this.entries.set(handle, { uri, writable });
    return handle;
  }

  private requireEntry(handle: string): FileEntry {
    const entry = this.entries.get(handle);
    if (entry === undefined) {
      throw new Error('OHOS_FILE_INVALID_HANDLE');
    }
    return entry;
  }

  private offset(value: string): number {
    const parsed = Number(value);
    if (!Number.isSafeInteger(parsed) || parsed < 0) {
      throw new Error('OHOS_FILE_INVALID_OFFSET');
    }
    return parsed;
  }

  private open(uri: string, mode: number): fileIo.File {
    try {
      return fileIo.openSync(uri, mode);
    } catch (_error) {
      throw new Error('OHOS_FILE_IO');
    }
  }

  private stat(fd: number): fileIo.Stat {
    try {
      return fileIo.statSync(fd);
    } catch (_error) {
      throw new Error('OHOS_FILE_IO');
    }
  }

  private read(fd: number, buffer: ArrayBuffer, offset: number, length: number): number {
    try {
      return fileIo.readSync(fd, buffer, { offset, length });
    } catch (_error) {
      throw new Error('OHOS_FILE_IO');
    }
  }

  private write(fd: number, bytes: Uint8Array, offset: number): void {
    try {
      fileIo.writeSync(fd, bytes.buffer, { offset, length: bytes.byteLength });
    } catch (_error) {
      throw new Error('OHOS_FILE_IO');
    }
  }

  private sync(fd: number): void {
    try {
      fileIo.fsyncSync(fd);
    } catch (_error) {
      throw new Error('OHOS_FILE_IO');
    }
  }

  private close(file: fileIo.File): void {
    try {
      fileIo.closeSync(file);
    } catch (_error) {
      throw new Error('OHOS_FILE_IO');
    }
  }
}
