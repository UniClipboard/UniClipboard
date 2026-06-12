const { getDefaultConfig } = require('expo/metro-config');
const path = require('path');

const config = getDefaultConfig(__dirname);

const WEB_STUB = path.resolve(__dirname, 'web-stubs/empty.js');

// web 平台需要 stub 掉的原生模块名
const WEB_STUBBED_MODULES = new Set([
  'native-util',
  'native-timer',
  'clipboard-overlay',
  'shizuku-clipboard',
  'sms-forwarder',
  'foreground-service',
  'shortcut',
  'signalr-client',
]);

const originalResolveRequest = config.resolver.resolveRequest;

config.resolver.resolveRequest = (context, moduleName, platform) => {
  if (platform === 'web' && WEB_STUBBED_MODULES.has(moduleName)) {
    return { type: 'sourceFile', filePath: WEB_STUB };
  }
  if (originalResolveRequest) {
    return originalResolveRequest(context, moduleName, platform);
  }
  return context.resolveRequest(context, moduleName, platform);
};

module.exports = config;
