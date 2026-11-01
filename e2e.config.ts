import { web } from '@e2e-dev/web'
import type { E2EConfig } from 'e2e'
import { buildLeaseEnv } from './apps/gui-go/frontend/e2e/tester-army/build-lease.mjs'

export default {
  tests: [
    'apps/gui-go/frontend/e2e/tester-army/*relays.e2e.mjs',
    'apps/gui-go/frontend/e2e/tester-army/relay-overview.e2e.mjs',
  ],
  workers: 1,
  retries: 0,
  timeout: 120_000,
  assertionTimeout: 15_000,
  trace: 'on',
  reporters: ['list', 'junit', 'markdown'],
  output: 'target/tester-army',
  targets: [
    {
      name: 'relay-browser',
      engine: web({
        locale: 'zh-CN',
        reducedMotion: 'reduce',
        viewport: { width: 1100, height: 900 },
      }),
      app: {
        url: 'http://127.0.0.1:0',
        command: {
          executable: 'node',
          inheritEnv: [buildLeaseEnv],
          args: ['apps/gui-go/frontend/e2e/tester-army/server.mjs', '{port}'],
          log: 'target/tester-army-server.log',
        },
      },
    },
  ],
} satisfies E2EConfig
