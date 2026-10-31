import { browserEngine } from './apps/gui-go/frontend/e2e/tester-army/engine.mjs'
import {
  selected,
  selectedPlatform,
  selectedSuite,
} from './apps/gui-go/frontend/e2e/tester-army/suites.mjs'
import config from './e2e.config'

export default {
  ...config,
  timeout: 600_000,
  actionTimeout: 15_000,
  tests: [`apps/gui-go/frontend/e2e/tester-army/${selectedSuite}.e2e.mjs`],
  output: `target/tester-army-layout-report/${selectedPlatform}/${selectedSuite}`,
  targets: [
    {
      name: 'layout-browser',
      engine: browserEngine,
      app: {
        ...config.targets[0].app,
        command: {
          ...config.targets[0].app.command,
          log: `target/tester-army-layout-report/${selectedPlatform}/${selectedSuite}/server.log`,
          env: {
            ...(selectedPlatform === 'none' ? {} : { UC_UI_PLATFORM: selectedPlatform }),
            UC_TESTER_FIXTURE: selected.fixture,
          },
        },
      },
    },
  ],
}
