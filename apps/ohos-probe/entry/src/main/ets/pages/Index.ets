import engine from 'libuc_ohos_napi.so';
import { hilog } from '@kit.PerformanceAnalysisKit';

const DOMAIN = 0x5543;

@Entry
@Component
struct Index {
  @State private version: string = 'Loading';
  @State private status: string = 'Checking unified core';
  @State private details: string = '';

  aboutToAppear(): void {
    try {
      this.version = engine.coreVersion();
      this.status = 'Ready';
    } catch (error) {
      this.version = 'Unavailable';
      this.status = 'Native core failed to load';
      this.details = String(error);
      hilog.error(DOMAIN, 'EngineProbe', 'Native core load failed: %{public}s', JSON.stringify(error));
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
