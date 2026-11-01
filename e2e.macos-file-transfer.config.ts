import { macosProduct } from './apps/gui-go/e2e/tester-army-native/macos-engine.mjs'

export default {
  tests: ['apps/gui-go/e2e/tester-army-native/macos-file-transfer.e2e.mjs'],
  output: 'target/tester-army-macos-file-transfer',
  workers: 1,
  retries: 0,
  timeout: 180_000,
  launchTimeout: 240_000,
  actionTimeout: 20_000,
  assertionTimeout: 90_000,
  cleanupTimeout: 240_000,
  trace: 'on',
  reporters: ['list', 'junit', 'markdown'],
  targets: [
    {
      name: 'macos-native-file-transfer',
      engine: macosProduct({ scenario: 'file-transfer' }),
      app: { appPath: 'target/gui-go/UniClipboardGoE2E.app' },
    },
  ],
}
