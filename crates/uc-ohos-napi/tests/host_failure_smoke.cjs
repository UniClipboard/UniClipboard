const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

async function main() {
  const addonPath = process.env.UC_OHOS_NAPI_NODE;
  assert.ok(addonPath, 'UC_OHOS_NAPI_NODE must point to the built N-API module');

  const addon = require(addonPath);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-ohos-napi-failure-'));
  const host = {
    privateDataDirectory: path.join(root, 'data'),
    cacheDirectory: path.join(root, 'cache'),
    temporaryDirectory: path.join(root, 'temporary'),
    secureStorageGet() {
      return unavailable();
    },
    secureStorageSet() {
      return unavailable();
    },
    secureStorageDelete() {
      return unavailable();
    },
    fileMetadata() {
      return unavailable();
    },
    fileReadChunk() {
      return unavailable();
    },
    fileWriteChunk() {
      return unavailable();
    },
    fileFinishWrite() {
      return unavailable();
    },
    clipboardRead() {
      return unavailable();
    },
    clipboardWrite() {
      return unavailable();
    },
  };

  try {
    const preparedHost = addon.prepareHost(host);
    await assert.rejects(
      addon.startEngine(
        { appVersion: '1.2.3', profileId: 'ohos-host-failure-smoke' },
        preparedHost
      ),
      /UC_ENGINE:\d+:unavailable:true/
    );
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

function unavailable() {
  return { ok: false, errorCategory: 'unavailable' };
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
