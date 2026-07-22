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

    const invitation = await engine.issueInvitation();
    assert.ok(invitation.invitationCode);
    assert.ok(invitation.expiresAtMs > 0);
    assert.match(invitation.availability, /^(cross_network|same_local_network)$/);

    await assert.rejects(
      engine.joinSpace(invitation.invitationCode, '  ', 'correct horse battery staple'),
      /UC_ENGINE:\d+:invalid_input:false/
    );

    const report = await engine.sendText('private HarmonyOS binding text', []);
    assert.ok(report.entryId);
    assert.ok(report.atMs > 0);
    assert.equal(report.totalAccepted, 0);
    assert.equal(report.totalDuplicate, 0);
    assert.equal(report.totalOffline, 0);
    assert.equal(report.totalErrored, 0);
    assert.equal(report.totalPending, 0);

    await engine.suspend();
    await waitForState(engine, 'suspended');
    await engine.resume();
    await waitForState(engine, 'running');
    await engine.shutdown(5_000);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

async function waitForState(engine, expected) {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const event = await engine.nextEvent(100);
    if (event?.kind === 'state_changed' && event.state === expected) {
      return;
    }
  }
  assert.fail(`binding did not deliver state: ${expected}`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
