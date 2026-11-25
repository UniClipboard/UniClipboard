'use strict';

const fs = require('node:fs');

function patchAppleTargets(source) {
  if (source.includes('const previousConfigurationList = targetToUpdate.props.buildConfigurationList;')) return source;
  const start = source.indexOf('        // Remove existing build phases');
  const end = source.indexOf('        // Create new build phases', start);
  if (start < 0 || end < start) throw new Error('Unsupported apple-targets configuration update: patch needs review');
  const original = source.slice(start, end);
  if (!original.includes('targetToUpdate.props.buildConfigurationList.removeFromProject();')) {
    throw new Error('Unsupported apple-targets configuration removal: patch needs review');
  }
  const patched = original.replace('        // Remove existing build phases\n',
    '        // Remove existing build phases\n        const previousConfigurationList = targetToUpdate.props.buildConfigurationList;\n')
    .replaceAll('targetToUpdate.props.buildConfigurationList', 'previousConfigurationList')
    .replace('const previousConfigurationList = previousConfigurationList;',
      'const previousConfigurationList = targetToUpdate.props.buildConfigurationList;');
  return source.slice(0, start) + patched + source.slice(end);
}

if (require.main === module) {
  const file = require.resolve('@bacons/apple-targets/build/with-xcode-changes.js');
  const source = fs.readFileSync(file, 'utf8');
  const patched = patchAppleTargets(source);
  if (patched !== source) fs.writeFileSync(file, patched);
  console.log('[Apple targets] Existing extension configuration update patch applied');
}

module.exports = { patchAppleTargets };
