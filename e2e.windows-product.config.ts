import { nativeEngine } from './apps/gui-go/e2e/tester-army-native/engine.mjs'

if (process.platform !== 'win32')
  throw new Error('Windows product acceptance requires native Windows')
export default {
  tests: ['apps/gui-go/e2e/tester-army-native/windows-product.e2e.mjs'],
  output: 'target/tester-army-windows-product/framework',
  workers: 1,
  retries: 0,
  timeout: 240_000,
  trace: 'off',
  reporters: ['list', 'junit'],
  targets: [
    { name: 'product-webview2', engine: nativeEngine, app: { url: process.env.UC_NATIVE_URL } },
  ],
}
