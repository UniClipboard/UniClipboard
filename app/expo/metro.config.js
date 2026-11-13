const { getDefaultConfig } = require('expo/metro-config');
const path = require('path');
const config = getDefaultConfig(__dirname);
const shims = {
  '@expo/ui/jetpack-compose': 'expo-ui-jetpack-compose.tsx',
  '@expo/ui/jetpack-compose/modifiers': 'expo-ui-jetpack-compose-modifiers.ts',
};
config.resolver.resolveRequest = (context, name, platform) => {
  if (platform === 'ios' && shims[name]) return { type: 'sourceFile', filePath: path.resolve(__dirname, 'ios-shims', shims[name]) };
  return context.resolveRequest(context, name, platform);
};
module.exports = config;
