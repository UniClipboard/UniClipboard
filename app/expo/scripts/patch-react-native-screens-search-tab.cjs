'use strict';

// react-native-screens builds the iOS search tab from the legacy UITabBarItem search system
// item. The iOS 27 SDK no longer shows that item as the detached search circle next to the tab
// bar; only a real UISearchTab gets it. The patch moves the tab bar controller to the UITab API
// (on iOS 26+, only when one of the tabs is a search tab) and creates a UISearchTab for it; on
// iOS 27 the search tab is also made the prominent tab, which is what detaches it from the bar.
// Upstream: https://github.com/software-mansion/react-native-screens/issues/4671
// Remove this patch once react-native-screens ships UISearchTab support.

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const SUPPORTED_VERSION = '4.26.2';
const PATCH_FILE = path.resolve(__dirname, '../patches/react-native-screens-search-tab.patch');

function runPatch(packageDir, args) {
  return spawnSync('patch', ['-p1', '--silent', '-i', PATCH_FILE, ...args], {
    cwd: packageDir,
    encoding: 'utf8',
  });
}

/** Applies the patch to the package sources in `packageDir`; returns 'applied' or 'already-applied'. */
function applySearchTabPatch(packageDir) {
  // Already applied: the patch reverses cleanly.
  if (runPatch(packageDir, ['--reverse', '--dry-run', '--force']).status === 0) return 'already-applied';

  const check = runPatch(packageDir, ['--forward', '--dry-run']);
  if (check.status !== 0) {
    throw new Error(`Unsupported react-native-screens sources: UISearchTab patch needs review\n${check.stdout}${check.stderr}`);
  }
  const applied = runPatch(packageDir, ['--forward']);
  if (applied.status !== 0) {
    throw new Error(`Failed to apply react-native-screens UISearchTab patch\n${applied.stdout}${applied.stderr}`);
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
      `react-native-screens ${version} is installed, but the UISearchTab patch targets ${SUPPORTED_VERSION}: ` +
        'check whether upstream fixed the search tab, then update or remove this patch'
    );
  }
  applySearchTabPatch(packageDir);
}

module.exports = { PATCH_FILE, SUPPORTED_VERSION, applySearchTabPatch };
