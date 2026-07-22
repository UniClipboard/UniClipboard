const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

async function main() {
  const addonPath = process.env.UC_OHOS_NAPI_NODE;
  assert.ok(addonPath, 'UC_OHOS_NAPI_NODE must point to the built N-API module');

  const addon = require(addonPath);
  assert.equal(addon.coreVersion(), 'core-v0.19.1');
  assert.equal(typeof addon.prepareHost, 'function');
  assert.equal(typeof addon.startEngine, 'function');

  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-ohos-napi-'));
  const values = new Map();
  const host = {
    privateDataDirectory: path.join(root, 'data'),
    cacheDirectory: path.join(root, 'cache'),
    temporaryDirectory: path.join(root, 'temporary'),
    secureStorageGet(key) {
      return values.get(key) ?? null;
    },
    secureStorageSet(key, value) {
      values.set(key, Buffer.from(value));
    },
    secureStorageDelete(key) {
      values.delete(key);
    },
  };

  try {
    const preparedHost = addon.prepareHost(host);
    const engine = await addon.startEngine(
      { appVersion: '1.2.3', profileId: 'ohos-host-smoke' },
      preparedHost
    );
    const created = await engine.createSpace(
      'ohos-host-smoke',
      'correct horse battery staple'
    );
    assert.ok(created.spaceId);
    assert.ok(created.selfDeviceId);
    assert.ok(created.identityFingerprint);
    assert.ok(values.size > 0, 'space secrets must be persisted through the host callback');
    await engine.shutdown(5_000);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
