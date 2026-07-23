import { UIAbility } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { engineRuntime } from '../host/EngineRuntime';

const DOMAIN = 0x5543;

export default class EntryAbility extends UIAbility {
  onForeground(): void {
    engineRuntime.onForeground();
  }

  onBackground(): void {
    engineRuntime.onBackground();
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    try {
      windowStage.loadContent('pages/Index', (error) => {
        if (error.code !== 0) {
          hilog.error(DOMAIN, 'EngineProbe', 'Failed to load the main page: %{public}d', error.code);
        }
      });
    } catch (_error) {
      hilog.error(DOMAIN, 'EngineProbe', 'Failed to request the main page');
    }
  }
}
