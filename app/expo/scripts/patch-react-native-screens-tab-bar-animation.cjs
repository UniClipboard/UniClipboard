'use strict';

// react-native-screens hides the native tab bar (`tabBarStyle: { display: 'none' }`) with
// `setTabBarHidden:animated:NO`, so the bar vanishes in one frame, and UIKit's animated variant only
// fades it in place on iOS 26. The patch slides the bar down out of the screen before hiding it and
// up from below the screen after showing it (iOS 18+). The settings and devices tabs hide it while
// one of their pushed sub-pages is shown.
// Remove this patch once react-native-screens lets the tab bar visibility change animate.

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const SUPPORTED_VERSION = '4.26.2';
const PATCH_FILE = path.resolve(__dirname, '../patches/react-native-screens-tab-bar-animation.patch');

function runPatch(packageDir, args) {
  return spawnSync('patch', ['-p1', '--silent', '-i', PATCH_FILE, ...args], {
    cwd: packageDir,
    encoding: 'utf8',
  });
}

/** Applies the patch to the package sources in `packageDir`; returns 'applied' or 'already-applied'. */
function applyTabBarAnimationPatch(packageDir) {
  // Already applied: the patch reverses cleanly.
  if (runPatch(packageDir, ['--reverse', '--dry-run', '--force']).status === 0) return 'already-applied';

  const check = runPatch(packageDir, ['--forward', '--dry-run']);
  if (check.status !== 0) {
    throw new Error(`Unsupported react-native-screens sources: tab bar animation patch needs review\n${check.stdout}${check.stderr}`);
  }
  const applied = runPatch(packageDir, ['--forward']);
  if (applied.status !== 0) {
    throw new Error(`Failed to apply react-native-screens tab bar animation patch\n${applied.stdout}${applied.stderr}`);
  }
  return 'applied';
}

if (require.main === module) {
  const packageDir = path.resolve(__dirname, '../node_modules/react-native-screens');
  const packageJsonPath = path.join(packageDir, 'package.json');
  if (!fs.existsSync(packageJsonPath)) process.exit(0);

  const { version } = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
  if (version !== SUPPORTED_VERSION) {
    throw new Error(
      `react-native-screens ${version} is installed, but the tab bar animation patch targets ${SUPPORTED_VERSION}: ` +
        'check whether upstream animates tab bar visibility, then update or remove this patch'
    );
  }
  applyTabBarAnimationPatch(packageDir);
}

module.exports = { PATCH_FILE, SUPPORTED_VERSION, applyTabBarAnimationPatch };
