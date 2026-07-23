import engine from 'libuc_ohos_napi.so';
import type { OhEngine } from 'libuc_ohos_napi.so';
import { common } from '@kit.AbilityKit';
import { util } from '@kit.ArkTS';
import { picker } from '@kit.CoreFileKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { createEngineHost } from '../host/EngineHost';
import { FileHandleRegistry } from '../host/FileHandleRegistry';
import { PrivateStorageProbe } from '../host/PrivateStorageProbe';
import { SecureAssetStorage } from '../host/SecureAssetStorage';

const DOMAIN = 0x5543;

@Entry
@Component
struct Index {
  @State private version: string = 'Loading';
  @State private status: string = 'Checking unified core';
  @State private details: string = '';
  @State private exporting: boolean = false;
  private readonly storage = new SecureAssetStorage();
  private readonly files = new FileHandleRegistry();
  private readonly privateStorage = new PrivateStorageProbe();
  private activeEngine?: OhEngine;

  aboutToAppear(): void {
    this.start().catch((error: Object) => this.fail(error));
  }

  private async start(): Promise<void> {
    try {
      this.version = engine.coreVersion();
      this.storage.verifyAccess();
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      const preparedHost = engine.prepareHost(createEngineHost(context, this.storage, this.files));
      this.activeEngine = await engine.startEngine(
        { appVersion: '1.0.0', profileId: 'ohos-system-host-probe' },
        preparedHost
      );
      const recovery = await this.activeEngine.recoverSession(true);
      if (recovery.unlocked && !recovery.resumed) {
        throw new Error('OHOS_SESSION_NOT_RESUMED');
      }
      if (!recovery.unlocked) {
        await this.activeEngine.createSpace('HarmonyOS Probe', 'system-host-acceptance');
      }
      this.status = 'Ready';
    } catch (error) {
      this.fail(error as Object);
    }
  }

  private fail(error: Object): void {
    this.version = 'Unavailable';
    this.status = 'Native core failed to load';
    this.details = String(error);
    hilog.error(DOMAIN, 'EngineProbe', 'Native core load failed: %{public}s', JSON.stringify(error));
  }

  private async exportSample(): Promise<void> {
    const active = this.activeEngine;
    if (active === undefined || this.exporting) {
      return;
    }
    this.exporting = true;
    this.details = '';
    let handle: string | undefined;
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      const documentPicker = new picker.DocumentViewPicker(context);
      const exportId = Date.now();
      const exportText = `UniClipboard HarmonyOS export check ${exportId}`;
      const uris = await documentPicker.save({
        newFileNames: [`uniclipboard-export-check-${exportId}.txt`],
        fileSuffixChoices: ['Text|.txt']
      });
      if (uris.length === 0) {
        return;
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
    } catch (error) {
      this.details = `Export failed: ${String(error)}`;
      hilog.error(DOMAIN, 'EngineProbe', 'Export failed: %{public}s', JSON.stringify(error));
    } finally {
      if (handle !== undefined) {
        this.files.remove(handle);
      }
      this.exporting = false;
    }
  }

  build() {
    Column({ space: 16 }) {
      Text('UniClipboard')
        .fontSize(28)
        .fontWeight(FontWeight.Bold)
      Text('Unified core')
        .fontSize(18)
        .fontColor('#5B6570')
      Text(this.version)
        .fontSize(24)
        .fontWeight(FontWeight.Medium)
      Text(this.status)
        .fontSize(16)
        .fontColor(this.status === 'Ready' ? '#1F7A5A' : '#B3261E')
      if (this.status === 'Ready') {
        Button(this.exporting ? 'Exporting' : 'Export sample')
          .enabled(!this.exporting)
          .onClick(() => this.exportSample())
      }
      if (this.details.length > 0) {
        Text(this.details)
          .fontSize(12)
          .fontColor('#5B6570')
          .textAlign(TextAlign.Center)
      }
    }
    .width('100%')
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .padding(32)
    .backgroundColor('#F7F8F9')
  }
}
