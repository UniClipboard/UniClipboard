import { macosHistory } from './apps/gui-go/e2e/tester-army-native/macos-engine.mjs'

export default {
  tests: ['apps/gui-go/e2e/tester-army-native/macos-history.e2e.mjs'],
  output: 'target/tester-army-macos-history',
  workers: 1,
  retries: 0,
  timeout: 120_000,
  launchTimeout: 180_000,
  actionTimeout: 20_000,
  assertionTimeout: 20_000,
  cleanupTimeout: 240_000,
  trace: 'on',
  reporters: ['list', 'junit', 'markdown'],
  targets: [
    {
      name: 'macos-native-history',
      engine: macosHistory(),
      app: { appPath: 'target/gui-go/UniClipboardGoE2E.app' },
    },
  ],
}
