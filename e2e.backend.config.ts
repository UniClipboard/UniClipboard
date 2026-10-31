import { backendEngine } from './apps/gui-go/frontend/e2e/tester-army/backend-engine.mjs'
import {
  backendName,
  backendPlatform,
  backendReport,
} from './apps/gui-go/frontend/e2e/tester-army/backend-suites.mjs'
import config from './e2e.config'

if (process.platform !== 'darwin') throw new Error('Backend fixtures require macOS HOME isolation')
export default {
  ...config,
  timeout: 900_000,
  cleanupTimeout: 120_000,
  trace: 'off',
  tests: [`apps/gui-go/frontend/e2e/tester-army/${backendName}.e2e.mjs`],
  output: backendReport,
  targets: [
    {
      name: 'isolated-backend-browser',
      engine: backendEngine,
      app: {
        ...config.targets[0].app,
        command: {
          ...config.targets[0].app.command,
          log: `${backendReport}/server.log`,
          env: {
            ...(backendPlatform === 'none' ? {} : { UC_UI_PLATFORM: backendPlatform }),
            UC_TESTER_FIXTURE: 'history-full-app',
            UC_TESTER_BACKENDS: '1',
          },
        },
      },
    },
  ],
}
