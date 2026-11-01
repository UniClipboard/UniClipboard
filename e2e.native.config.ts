import { nativeEngine } from './apps/gui-go/e2e/tester-army-native/engine.mjs'

if (process.platform !== 'win32') throw new Error('WebView2 capability probe requires Windows')
export default {
  tests: ['apps/gui-go/e2e/tester-army-native/webview2.e2e.mjs'],
  output: 'target/tester-army-native/framework',
  workers: 1,
  retries: 0,
  timeout: 30_000,
  trace: 'on',
  reporters: ['list', 'junit'],
  targets: [
    { name: 'native-webview2', engine: nativeEngine, app: { url: process.env.UC_NATIVE_URL } },
  ],
}
