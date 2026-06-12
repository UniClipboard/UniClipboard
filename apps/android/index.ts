import { registerRootComponent } from 'expo';
import { Platform, AppRegistry } from 'react-native';

import App from './App';

// registerRootComponent calls AppRegistry.registerComponent('main', () => App);
// It also ensures that whether you load the app in Expo Go or in a native build,
// the environment is set up appropriately
registerRootComponent(App);

// 仅在 Android 上注册额外的 Activity/HeadlessTask 入口。
// 这些子树会拉入 Logger / native-util / clipboard-overlay 等原生依赖,
// 在 web 平台上即使模块被 stub,顶层副作用(如 new Directory(Paths.document, 'logs'))
// 也会抛错导致白屏,所以必须守卫掉。
if (Platform.OS !== 'web') {
  const QuickActionApp = require('./src/QuickActionApp').default;
  const ServiceRestartApp = require('./src/ServiceRestartApp').default;

  // Separate entry point for the transparent QuickActionActivity
  AppRegistry.registerComponent('quickAction', () => QuickActionApp);

  // Separate entry point for the ServiceRestartActivity (service restarted by system)
  AppRegistry.registerComponent('serviceRestart', () => ServiceRestartApp);

  // Headless JS task for SMS verification code upload (runs without UI)
  AppRegistry.registerHeadlessTask(
    'SmsUploadTask',
    () => require('./src/tasks/SmsUploadTask').default
  );
}
