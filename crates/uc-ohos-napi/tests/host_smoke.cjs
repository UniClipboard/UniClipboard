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
  const files = new Map([
    [
      'input-file-1',
      {
        displayName: 'private-name.txt',
        mimeType: 'text/plain',
        bytes: Buffer.from('private HarmonyOS binding file'),
        finished: false,
      },
    ],
    [
      'output-file-1',
      {
        displayName: 'private-export.txt',
        mimeType: 'text/plain',
        bytes: Buffer.alloc(0),
        finished: false,
      },
    ],
  ]);
  const privateClipboardText = Buffer.from('private HarmonyOS clipboard text');
  let clipboard = {
    observedAtMs: 1_700_000_000_000,
    representations: [
      {
        kind: 'inline',
        format: 'text/plain',
        mimeType: 'text/plain',
        bytes: privateClipboardText,
      },
    ],
  };
  const clipboardWrites = [];
  const host = {
    privateDataDirectory: path.join(root, 'data'),
    cacheDirectory: path.join(root, 'cache'),
    temporaryDirectory: path.join(root, 'temporary'),
    secureStorageGet(key) {
      const value = values.get(key);
      return success(value === undefined ? null : new Uint8Array(value));
    },
    secureStorageSet(key, value) {
      values.set(key, Buffer.from(value));
      return success();
    },
    secureStorageDelete(key) {
      values.delete(key);
      return success();
    },
    fileMetadata(handle) {
      const file = requireFile(files, handle);
      return success({
        displayName: file.displayName,
        sizeBytes: String(file.bytes.length),
        mimeType: file.mimeType,
      });
    },
    fileReadChunk(handle, offset, maxBytes) {
      const file = requireFile(files, handle);
      const start = Number(offset);
      return success(new Uint8Array(file.bytes.subarray(start, start + maxBytes)));
    },
    fileWriteChunk(handle, offset, bytes) {
      const file = requireFile(files, handle);
      const start = Number(offset);
      const end = start + bytes.length;
      if (file.bytes.length < end) {
        const expanded = Buffer.alloc(end);
        file.bytes.copy(expanded);
        file.bytes = expanded;
      }
      Buffer.from(bytes).copy(file.bytes, start);
      return success();
    },
    fileFinishWrite(handle) {
      requireFile(files, handle).finished = true;
      return success();
    },
    clipboardRead() {
      return success({
        ...clipboard,
        representations: clipboard.representations.map((representation) => ({
          ...representation,
          bytes: representation.bytes === undefined
            ? undefined
            : new Uint8Array(representation.bytes),
        })),
      });
    },
    clipboardWrite(snapshot) {
      clipboardWrites.push(snapshot);
      clipboard = snapshot;
      return success();
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

    const privateImage = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
    const imageReport = await engine.sendImage(privateImage, 'image/png', []);
    assert.ok(imageReport.entryId);
    assert.ok(imageReport.atMs > 0);
    assert.equal(imageReport.totalAccepted, 0);
    assert.equal(imageReport.totalPending, 0);

    const fileReport = await engine.sendFiles(['input-file-1'], []);
    assert.ok(fileReport.entryId);
    assert.ok(fileReport.atMs > 0);
    assert.equal(fileReport.totalAccepted, 0);
    assert.equal(fileReport.totalPending, 0);

    const clipboardEntryId = await engine.captureCurrentClipboard();
    assert.ok(clipboardEntryId);
    const restoreOutcome = await engine.restoreClipboard(clipboardEntryId, 'standard');
    assert.equal(restoreOutcome, 'restored');
    assert.equal(clipboardWrites.length, 1);
    assert.equal(clipboardWrites[0].representations.length, 1);
    assert.deepEqual(
      Buffer.from(clipboardWrites[0].representations[0].bytes),
      privateClipboardText
    );
    const plainTextOutcome = await engine.restoreClipboard(clipboardEntryId, 'plain_text');
    assert.equal(plainTextOutcome, 'restored');
    assert.equal(clipboardWrites.length, 2);
    const filePathsOutcome = await engine.restoreClipboard(clipboardEntryId, 'file_paths');
    assert.equal(filePathsOutcome, 'not_applicable');
    assert.equal(clipboardWrites.length, 2);

    await engine.exportEntry(clipboardEntryId, 'output-file-1');
    assert.deepEqual(requireFile(files, 'output-file-1').bytes, privateClipboardText);
    assert.equal(requireFile(files, 'output-file-1').finished, true);

    await engine.suspend();
    await waitForState(engine, 'suspended');
    await engine.resume();
    await waitForState(engine, 'running');
    await engine.shutdown(5_000);

    const restarted = await addon.startEngine(
      { appVersion: '1.2.3', profileId: 'ohos-host-smoke' },
      addon.prepareHost(host)
    );
    await assert.rejects(
      restarted.createSpace('ohos-host-smoke', 'correct horse battery staple'),
      /UC_ENGINE:1205:conflict:false/
    );
    const recovery = await restarted.recoverSession(true);
    assert.equal(recovery.unlocked, true);
    assert.equal(recovery.resumed, true);

    const restartText = 'private HarmonyOS restart export';
    const restartReport = await restarted.sendText(restartText, []);
    const restartOutput = requireFile(files, 'output-file-1');
    restartOutput.bytes = Buffer.alloc(0);
    restartOutput.finished = false;
    await restarted.exportEntry(restartReport.entryId, 'output-file-1');
    assert.deepEqual(restartOutput.bytes, Buffer.from(restartText));
    assert.equal(restartOutput.finished, true);
    await restarted.shutdown(5_000);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

function success(value) {
  return { ok: true, value };
}

function requireFile(files, handle) {
  const file = files.get(handle);
  assert.ok(file, `unknown file handle: ${handle}`);
  return file;
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
