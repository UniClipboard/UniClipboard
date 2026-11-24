import { mobile } from '@e2e-dev/mobile';
import type { E2EConfig, Target } from 'e2e';

const platform = (process.env.E2E_PLATFORM ?? 'ios').toLowerCase();
const BUNDLE_ID = 'app.uniclipboard.shell';

const iosTarget: Target = {
  name: 'ios',
  engine: mobile({
    platform: 'ios',
    device: process.env.E2E_IOS_DEVICE ?? 'A1B5F28E-5DB7-41BE-A80A-DFEB6608EDA2',
  }),
  app: {
    bundleId: BUNDLE_ID,
    appPath:
      process.env.E2E_IOS_APP ??
      '/private/tmp/unic-ios-derived/Build/Products/Release-iphonesimulator/UniClipboardUIShell.app',
  },
};

const androidTarget: Target = {
  name: 'android',
  engine: mobile({
    platform: 'android',
    device: process.env.E2E_ANDROID_DEVICE || undefined,
  }),
  app: {
    bundleId: BUNDLE_ID,
    appPath:
      process.env.E2E_ANDROID_APP ??
      '../../app/expo/android/app/build/outputs/apk/debug/app-debug.apk',
    command: {
      executable: 'npx',
      args: ['expo', 'start', '--port', '8081'],
      cwd: '../../app/expo',
    },
    readyUrl: 'http://127.0.0.1:8081/status',
  },
};

const targets: Target[] =
  platform === 'both'
    ? [iosTarget, androidTarget]
    : platform === 'android'
      ? [androidTarget]
      : [iosTarget];

export default {
  projectId: 'uniclipboard-mobile-e2e',
  tests: ['./v1-migration.e2e.mjs'],
  targets,
  workers: 1,
  retries: 0,
  timeout: 180_000,
  launchTimeout: 90_000,
  actionTimeout: 20_000,
  assertionTimeout: 30_000,
  trace: 'on',
  reporters: ['list', 'junit', 'markdown'],
  output: 'results',
} satisfies E2EConfig;
