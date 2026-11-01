import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import { web } from '@e2e-dev/web'

const results = []
for (const nativeRuntime of ['WKWebView', 'WebKitGTK']) {
  assert.throws(
    () => web({ browser: 'webkit', connect: { cdpEndpoint: () => 'http://127.0.0.1:1' } }),
    /CDP attach is chromium-only/
  )
  results.push({
    nativeRuntime,
    suppliedDesktopEngine: false,
    webkitCdpAttachment: 'INVALID_CONFIG',
    testerArmyAcceptance: false,
    requiredAdapter: 'native WebKit inspector/WebDriver engine; not supplied by @e2e-dev/web',
  })
}
await mkdir('target/tester-army-native', { recursive: true })
await writeFile('target/tester-army-native/protocol.json', JSON.stringify(results, null, 2) + '\n')
