import { common } from '@kit.AbilityKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { engineRuntime, EngineRuntimeSnapshot } from '../host/EngineRuntime';

const DOMAIN = 0x5543;

@Entry
@Component
struct Index {
  @State private version: string = 'Loading';
  @State private status: string = 'Checking unified core';
  @State private details: string = '';
  @State private ready: boolean = false;
  @State private exporting: boolean = false;

  aboutToAppear(): void {
    this.start().catch((error: Object) => this.fail(error));
  }

  private async start(): Promise<void> {
    const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
    this.applySnapshot(await engineRuntime.start(context));
  }

  private applySnapshot(snapshot: EngineRuntimeSnapshot): void {
    this.version = snapshot.version;
    this.status = snapshot.status;
    this.details = snapshot.details;
    this.ready = snapshot.ready;
  }

  private fail(error: Object): void {
    this.applySnapshot(engineRuntime.snapshot());
    hilog.error(DOMAIN, 'EngineProbe', 'Native core load failed: %{public}s', JSON.stringify(error));
  }

  private async exportSample(): Promise<void> {
    if (!this.ready || this.exporting) {
      return;
    }
    this.exporting = true;
    this.details = '';
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      this.applySnapshot(await engineRuntime.exportSample(context));
    } catch (_error) {
      this.applySnapshot(engineRuntime.snapshot());
    } finally {
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
        .fontColor(this.ready ? '#1F7A5A' : '#B3261E')
      if (this.ready) {
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
