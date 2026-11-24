import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { afterAll, beforeAll, test } from '@e2e-dev/mobile';
import { expect } from 'e2e';

const BUNDLE_ID = 'app.uniclipboard.shell';
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');
const TMP_BASE = fs.mkdtempSync(path.join('/private/tmp', 'unic-mobile-e2e-'));
const FIXTURE_BIN = path.join(TMP_BASE, 'v1fixturegen');

function buildLaunchArguments(platform, paths) {
  if (platform === 'android') {
    return [
      '--es',
      'UNIC_MOBILE_DATA_DIR',
      paths.targetDir,
      '--es',
      'UNIC_MOBILE_CACHE_DIR',
      paths.cacheDir,
      '--es',
      'UNIC_MOBILE_V1_DIR',
      paths.sourceDir,
    ];
  }
  return [
    '-UNIC_MOBILE_DATA_DIR',
    paths.targetDir,
    '-UNIC_MOBILE_CACHE_DIR',
    paths.cacheDir,
    '-UNIC_MOBILE_V1_DIR',
    paths.sourceDir,
  ];
}

function runAdb(deviceId, args, options = {}) {
  return execFileSync('adb', ['-s', deviceId, ...args], {
    encoding: 'utf8',
    ...options,
  });
}

function prepareScenario(device, name, variant) {
  const { platform, deviceId } = device.unstable_raw;
  const runId = `${name}-${Date.now()}`;
  const hostRoot = path.join(TMP_BASE, runId);
  const hostSourceDir = path.join(hostRoot, 'source');
  const hostTargetDir = path.join(hostRoot, 'target');
  const hostCacheDir = path.join(hostRoot, 'cache');
  fs.mkdirSync(hostTargetDir, { recursive: true, mode: 0o700 });
  fs.mkdirSync(hostCacheDir, { recursive: true, mode: 0o700 });

  const raw = execFileSync(
    FIXTURE_BIN,
    ['-mode', 'generate', '-variant', variant, '-out', hostSourceDir],
    {
      cwd: REPO_ROOT,
      encoding: 'utf8',
      env: { ...process.env, TMPDIR: '/private/tmp' },
    },
  );
  const fixture = JSON.parse(raw);

  if (platform === 'android') {
    const tmpRemote = `/data/local/tmp/unic-e2e-${runId}`;
    const sandboxRel = `files/e2e/${runId}`;
    const sandboxBase = `/data/user/0/${BUNDLE_ID}/${sandboxRel}`;

    runAdb(deviceId, ['shell', 'rm', '-rf', tmpRemote]);
    runAdb(deviceId, ['shell', 'mkdir', '-p', tmpRemote]);
    runAdb(deviceId, ['push', `${fixture.sourceDir}/.`, tmpRemote]);
    runAdb(deviceId, ['shell', 'chmod', '-R', 'a+rX', tmpRemote]);
    runAdb(deviceId, ['shell', 'run-as', BUNDLE_ID, 'rm', '-rf', sandboxRel]);
    runAdb(deviceId, [
      'shell',
      'run-as',
      BUNDLE_ID,
      'mkdir',
      '-p',
      `${sandboxRel}/source`,
      `${sandboxRel}/target`,
      `${sandboxRel}/cache`,
    ]);
    if (fs.readdirSync(fixture.sourceDir).length > 0) {
      runAdb(deviceId, [
        'shell',
        'run-as',
        BUNDLE_ID,
        'cp',
        '-r',
        `${tmpRemote}/.`,
        `${sandboxRel}/source/`,
      ]);
    }
    runAdb(deviceId, ['shell', 'rm', '-rf', tmpRemote]);

    const paths = {
      sourceDir: `${sandboxBase}/source`,
      targetDir: `${sandboxBase}/target`,
      cacheDir: `${sandboxBase}/cache`,
    };
    return {
      platform,
      deviceId,
      runId,
      sandboxRel,
      hostRoot,
      fixture,
      paths,
      launchArguments: buildLaunchArguments(platform, paths),
    };
  }

  const paths = {
    sourceDir: fixture.sourceDir,
    targetDir: hostTargetDir,
    cacheDir: hostCacheDir,
  };
  return {
    platform,
    deviceId,
    runId,
    hostRoot,
    fixture,
    paths,
    launchArguments: buildLaunchArguments(platform, paths),
  };
}

function verifyScenario(scenario, { expectedCount, deleteSource }) {
  const { platform, deviceId, runId, sandboxRel, hostRoot, fixture, paths } = scenario;

  let sourceDirForVerify = fixture.sourceDir;
  let sourceDBForVerify = fixture.dbPath ?? '';
  let targetDirForVerify = paths.targetDir;

  if (platform === 'android') {
    const pulledDir = path.join(hostRoot, 'pulled');
    fs.mkdirSync(pulledDir, { recursive: true, mode: 0o700 });
    const tarStream = execFileSync(
      'adb',
      ['-s', deviceId, 'exec-out', 'run-as', BUNDLE_ID, 'tar', '-cf', '-', '-C', sandboxRel, 'target', 'source'],
      { maxBuffer: 64 * 1024 * 1024 },
    );
    execFileSync('tar', ['-xf', '-', '-C', pulledDir], { input: tarStream });
    sourceDirForVerify = path.join(pulledDir, 'source');
    sourceDBForVerify = fixture.dbPath
      ? path.join(sourceDirForVerify, path.relative(fixture.sourceDir, fixture.dbPath))
      : '';
    targetDirForVerify = path.join(pulledDir, 'target');
  }

  const args = [
    '-mode',
    'verify',
    '-target',
    targetDirForVerify,
    '-expected-count',
    String(expectedCount),
  ];
  if (sourceDirForVerify) {
    args.push('-source', sourceDirForVerify);
  }
  if (sourceDBForVerify && fixture.sourceDbSha256) {
    args.push('-source-db', sourceDBForVerify, '-source-sha', fixture.sourceDbSha256);
  }
  if (deleteSource) {
    args.push('-delete-source');
  }

  const out = execFileSync(FIXTURE_BIN, args, {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    env: { ...process.env, TMPDIR: '/private/tmp' },
  });
  const verified = JSON.parse(out);
  expect(verified.ok).toBe(true);
  expect(verified.verifiedCount).toBe(expectedCount);

  if (platform === 'android') {
    runAdb(deviceId, ['shell', 'run-as', BUNDLE_ID, 'rm', '-rf', `files/e2e/${runId}`]);
  }
}

async function submitPassphrase(device, screen, passphrase) {
  const input = screen.getByTestId('setup-passphrase-input');
  await expect(input).toBeVisible();
  await input.clear();
  await input.fill(passphrase);

  const submitBtn = screen.getByTestId('setup-submit-passphrase');
  if (!(await submitBtn.isVisible())) {
    await device.dismissKeyboard().catch(() => {});
  }
  await expect(submitBtn).toBeVisible();
  await submitBtn.tap();
}

async function assertLiveHistoryVisible(screen, { expectMigratedItems, expectEmptyState = false }) {
  await expect(screen.getByText(/^(剪贴板|Clipboard)$/).first()).toBeVisible({ timeout: 30_000 });
  await expect(screen.getByTestId('shell-banner')).toBeHidden();
  if (expectMigratedItems) {
    await expect(screen.getByText(/fixture text|recovered/).first()).toBeVisible({
      timeout: 30_000,
    });
  }
  if (expectEmptyState) {
    await expect(screen.getByText(/^(空空如也|Nothing here yet)$/).first()).toBeVisible({
      timeout: 15_000,
    });
  }
}

beforeAll(async ({ device }) => {
  execFileSync('go', ['build', '-o', FIXTURE_BIN, './e2e/mobile/cmd/v1fixturegen'], {
    cwd: REPO_ROOT,
    env: { ...process.env, TMPDIR: '/private/tmp' },
  });
  try {
    await device.installApp(undefined, { reinstall: true });
  } catch {
    await device.installApp();
  }
  if (device.unstable_raw.platform === 'android') {
    runAdb(device.unstable_raw.deviceId, ['reverse', 'tcp:8081', 'tcp:8081']);
  }
});

afterAll(async ({ device }) => {
  await device.closeApp().catch(() => {});
  fs.rmSync(TMP_BASE, { recursive: true, force: true });
});

test('complete v1 migration: passphrase error recovery, verification, live history, restart bypass, and byte-level integrity', async ({
  device,
  screen,
}) => {
  const scenario = prepareScenario(device, 'complete', 'complete');

  await device.openApp(BUNDLE_ID, {
    relaunch: true,
    launchArguments: scenario.launchArguments,
  });

  await expect(screen.getByText('已检测到旧版数据')).toBeVisible({ timeout: 45_000 });
  await expect(screen.getByText('发现可迁移的旧版剪贴板数据')).toBeVisible();

  await screen.getByTestId('setup-start-migration').tap();

  await expect(screen.getByText('输入旧版加密口令以解锁')).toBeVisible();

  await submitPassphrase(device, screen, 'wrong-passphrase');
  await expect(screen.getByText('旧口令不正确或无法解锁凭据，请重新输入')).toBeVisible();

  await submitPassphrase(device, screen, scenario.fixture.passphrase);

  await expect(screen.getByText('已完整校验')).toBeVisible({ timeout: 45_000 });
  await expect(screen.getByText('迁移完成')).toBeVisible();

  await screen.getByTestId('setup-enter-history').tap();
  await assertLiveHistoryVisible(screen, { expectMigratedItems: true });

  await device.openApp(BUNDLE_ID, {
    relaunch: true,
    launchArguments: scenario.launchArguments,
  });
  await assertLiveHistoryVisible(screen, { expectMigratedItems: true });
  await expect(screen.getByText('已检测到旧版数据')).toBeHidden();

  await device.closeApp();
  verifyScenario(scenario, { expectedCount: 5, deleteSource: true });
});

test('partial v1 migration: reports partial honestly without blocking entry to live history', async ({
  device,
  screen,
}) => {
  const scenario = prepareScenario(device, 'partial', 'partial');

  await device.openApp(BUNDLE_ID, {
    relaunch: true,
    launchArguments: scenario.launchArguments,
  });

  await expect(screen.getByText('已检测到旧版数据')).toBeVisible({ timeout: 45_000 });
  await screen.getByTestId('setup-start-migration').tap();

  await expect(screen.getByText('输入旧版加密口令以解锁')).toBeVisible();
  await submitPassphrase(device, screen, scenario.fixture.passphrase);

  await expect(screen.getByText('部分记录未能导入')).toBeVisible({ timeout: 45_000 });
  await expect(screen.getByText('迁移完成')).toBeHidden();

  await screen.getByTestId('setup-enter-after-partial').tap();
  await assertLiveHistoryVisible(screen, { expectMigratedItems: true });

  await device.closeApp();
  verifyScenario(scenario, { expectedCount: 4, deleteSource: false });
});

test('clean fresh install: enters live history with zero blocking', async ({
  device,
  screen,
}) => {
  const scenario = prepareScenario(device, 'clean', 'empty');

  await device.openApp(BUNDLE_ID, {
    relaunch: true,
    launchArguments: scenario.launchArguments,
  });

  await expect(screen.getByText('全新空间已就绪')).toBeVisible({ timeout: 45_000 });
  await screen.getByTestId('setup-start-clean').tap();

  await assertLiveHistoryVisible(screen, { expectMigratedItems: false, expectEmptyState: true });

  await device.closeApp();
  verifyScenario(scenario, { expectedCount: 0, deleteSource: false });
});
