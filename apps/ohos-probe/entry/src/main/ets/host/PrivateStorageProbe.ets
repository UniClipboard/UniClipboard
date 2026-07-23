import { fileIo } from '@kit.CoreFileKit';

export class PrivateStorageProbe {
  assertNoPlaintext(roots: string[], forbidden: string[]): void {
    roots.forEach((root: string): void => {
      this.list(root).forEach((relative: string): void => {
        const path = relative.startsWith('/') ? root + relative : `${root}/${relative}`;
        this.assertClean(path, forbidden);
        if (this.isFile(path)) {
          this.assertClean(this.readText(path), forbidden);
        }
      });
    });
  }

  private assertClean(value: string, forbidden: string[]): void {
    if (forbidden.some((needle: string): boolean => value.includes(needle))) {
      throw new Error('OHOS_PRIVATE_STORAGE_PLAINTEXT');
    }
  }

  private list(root: string): string[] {
    try {
      return fileIo.listFileSync(root, { recursion: true });
    } catch (_error) {
      throw new Error('OHOS_PRIVATE_STORAGE_SCAN_FAILED');
    }
  }

  private isFile(path: string): boolean {
    try {
      return fileIo.statSync(path).isFile();
    } catch (_error) {
      throw new Error('OHOS_PRIVATE_STORAGE_SCAN_FAILED');
    }
  }

  private readText(path: string): string {
    try {
      return fileIo.readTextSync(path);
    } catch (_error) {
      throw new Error('OHOS_PRIVATE_STORAGE_SCAN_FAILED');
    }
  }
}
