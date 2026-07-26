import engine from 'libuc_ohos_napi.so';
import type { OhEngine } from 'libuc_ohos_napi.so';
import { common } from '@kit.AbilityKit';
import { util } from '@kit.ArkTS';
import { picker } from '@kit.CoreFileKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { createEngineHost } from './EngineHost';
import { FileHandleRegistry } from './FileHandleRegistry';
import { PrivateStorageProbe } from './PrivateStorageProbe';
import { SecureAssetStorage } from './SecureAssetStorage';

const DOMAIN = 0x5543;

export interface EngineRuntimeSnapshot {
  version: string;
  status: string;
  details: string;
  ready: boolean;
}

class EngineRuntime {
  private readonly storage = new SecureAssetStorage();
  private readonly files = new FileHandleRegistry();
  private readonly privateStorage = new PrivateStorageProbe();
  private activeEngine?: OhEngine;
  private identityDeviceId?: string;
  private version: string = 'Loading';
  private status: string = 'Checking unified core';
  private details: string = '';

  async start(context: common.UIAbilityContext): Promise<EngineRuntimeSnapshot> {
    if (this.activeEngine !== undefined) {
      return this.snapshot();
    }

    try {
      this.version = engine.coreVersion();
      this.storage.verifyAccess();
      const preparedHost = engine.prepareHost(createEngineHost(context, this.storage, this.files));
      const active = await engine.startEngine(
        { appVersion: '1.0.0', profileId: 'ohos-system-host-probe' },
        preparedHost
      );
      const recovery = await active.recoverSession(true);
      if (recovery.unlocked && !recovery.resumed) {
        throw new Error('OHOS_SESSION_NOT_RESUMED');
      }
      if (recovery.unlocked) {
        this.identityDeviceId = (await active.queryLocalDevice()).deviceId;
      } else {
        this.identityDeviceId = (await active.createSpace(
          'HarmonyOS Probe',
          'system-host-acceptance'
        )).selfDeviceId;
      }
      this.activeEngine = active;
      this.status = 'Ready';
      this.details = '';
      return this.snapshot();
    } catch (error) {
      this.fail('Native core failed to load', error as Object);
      throw new Error(String(error));
    }
  }

  onBackground(): void {
    this.transitionToBackground().catch((error: Object) => this.failLifecycle('suspend', error));
  }

  onForeground(): void {
    this.transitionToForeground().catch((error: Object) => this.failLifecycle('resume', error));
  }

  async exportSample(context: common.UIAbilityContext): Promise<EngineRuntimeSnapshot> {
    const active = this.activeEngine;
    if (active === undefined) {
      throw new Error('OHOS_ENGINE_NOT_READY');
    }

    let handle: string | undefined;
    try {
      const documentPicker = new picker.DocumentViewPicker(context);
      const exportId = Date.now();
      const exportText = `UniClipboard HarmonyOS export check ${exportId}`;
      const uris = await documentPicker.save({
        newFileNames: [`uniclipboard-export-check-${exportId}.txt`],
        fileSuffixChoices: ['Text|.txt']
      });
      if (uris.length === 0) {
        return this.snapshot();
      }
      handle = this.files.registerOutput(uris[0]);
      const report = await active.sendText(exportText, []);
      await active.exportEntry(report.entryId, handle);
      const expected = new util.TextEncoder().encodeInto(exportText);
      const actual = this.files.fileReadChunk(handle, '0', expected.byteLength + 1);
      if (actual.byteLength !== expected.byteLength ||
        actual.some((value: number, index: number) => value !== expected[index])) {
        throw new Error('OHOS_EXPORT_VERIFY_FAILED');
      }
      this.privateStorage.assertNoPlaintext(
        [context.filesDir, context.cacheDir, context.tempDir],
        [exportText, 'uniclipboard-export-check-']
      );
      this.details = 'Export complete';
      return this.snapshot();
    } catch (error) {
      this.details = `Export failed: ${String(error)}`;
      hilog.error(DOMAIN, 'EngineProbe', 'Export failed: %{public}s', JSON.stringify(error));
      throw new Error(String(error));
    } finally {
      if (handle !== undefined) {
        this.files.remove(handle);
      }
    }
  }

  snapshot(): EngineRuntimeSnapshot {
    return {
      version: this.version,
      status: this.status,
      details: this.details,
      ready: this.activeEngine !== undefined && this.status === 'Ready'
    };
  }

  private async transitionToBackground(): Promise<void> {
    const active = this.activeEngine;
    if (active === undefined) {
      return;
    }
    const state = await active.lifecycleState();
    if (state === 'running' || state === 'quiesced') {
      await active.suspend();
    }
  }

  private async transitionToForeground(): Promise<void> {
    const active = this.activeEngine;
    if (active === undefined || await active.lifecycleState() !== 'suspended') {
      return;
    }
    await active.resume();
    const recoveredIdentity = (await active.queryLocalDevice()).deviceId;
    if (recoveredIdentity !== this.identityDeviceId) {
      throw new Error('OHOS_IDENTITY_CHANGED');
    }
    this.status = 'Ready';
    this.details = '';
  }

  private failLifecycle(action: string, error: Object): void {
    this.fail(`Lifecycle transition failed: ${action}`, error);
  }

  private fail(status: string, error: Object): void {
    this.status = status;
    this.details = String(error);
    hilog.error(DOMAIN, 'EngineProbe', '%{public}s: %{public}s', status, JSON.stringify(error));
  }
}

export const engineRuntime = new EngineRuntime();
