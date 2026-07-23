import { asset } from '@kit.AssetStoreKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { util } from '@kit.ArkTS';

const ASSET_NOT_FOUND = 24000002;
const ASSET_ALREADY_EXISTS = 24000003;
const ALIAS_PREFIX = 'app.uniclipboard.engine.';

export class SecureAssetStorage {
  private readonly encoder = new util.TextEncoder();

  get(key: string): Uint8Array | null {
    try {
      const results = asset.querySync(this.query(key, true));
      if (results.length === 0) {
        return null;
      }
      const value = results[0].get(asset.Tag.SECRET);
      return value instanceof Uint8Array ? value : null;
    } catch (error) {
      if (this.errorCode(error) === ASSET_NOT_FOUND) {
        return null;
      }
      throw new Error('OHOS_SECURE_STORAGE_UNAVAILABLE');
    }
  }

  set(key: string, value: Uint8Array): void {
    const secret = new Uint8Array(value);
    try {
      try {
        asset.updateSync(this.query(key, false), new Map([
          [asset.Tag.SECRET, secret]
        ]));
      } catch (error) {
        if (this.errorCode(error) !== ASSET_NOT_FOUND) {
          throw new Error('OHOS_SECURE_STORAGE_UNAVAILABLE');
        }
        try {
          const attributes: asset.AssetMap = new Map<asset.Tag, asset.Value>();
          attributes.set(asset.Tag.ALIAS, this.alias(key));
          attributes.set(asset.Tag.SECRET, secret);
          attributes.set(asset.Tag.ACCESSIBILITY, asset.Accessibility.DEVICE_FIRST_UNLOCKED);
          asset.addSync(attributes);
        } catch (addError) {
          if (this.errorCode(addError) !== ASSET_ALREADY_EXISTS) {
            throw new Error('OHOS_SECURE_STORAGE_UNAVAILABLE');
          }
          try {
            asset.updateSync(this.query(key, false), new Map([
              [asset.Tag.SECRET, secret]
            ]));
          } catch (_updateError) {
            throw new Error('OHOS_SECURE_STORAGE_UNAVAILABLE');
          }
        }
      }
    } finally {
      secret.fill(0);
    }
  }

  delete(key: string): void {
    try {
      asset.removeSync(this.query(key, false));
    } catch (error) {
      if (this.errorCode(error) !== ASSET_NOT_FOUND) {
        throw new Error('OHOS_SECURE_STORAGE_UNAVAILABLE');
      }
    }
  }

  verifyAccess(): void {
    const key = `probe-${util.generateRandomUUID()}`;
    const expected = new Uint8Array([0x55, 0x43, 0x01]);
    try {
      this.set(key, expected);
      const actual = this.get(key);
      if (actual === null || actual.length !== expected.length ||
        actual.some((value: number, index: number) => value !== expected[index])) {
        throw new Error('OHOS_SECURE_STORAGE_VERIFY_FAILED');
      }
    } finally {
      this.delete(key);
    }
  }

  private query(key: string, includeSecret: boolean): asset.AssetMap {
    const query: asset.AssetMap = new Map([
      [asset.Tag.ALIAS, this.alias(key)]
    ]);
    if (includeSecret) {
      query.set(asset.Tag.RETURN_TYPE, asset.ReturnType.ALL);
      query.set(asset.Tag.RETURN_LIMIT, 1);
    }
    return query;
  }

  private alias(key: string): Uint8Array {
    return this.encoder.encodeInto(ALIAS_PREFIX + key);
  }

  private errorCode(error: Object): number {
    return (error as BusinessError).code;
  }
}
