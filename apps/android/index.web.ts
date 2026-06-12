/**
 * Web 入口 —— 仅注册主 App,跳过 QuickActionApp/ServiceRestartApp/SmsUploadTask
 * (它们会拖入 Android-only 的 services/Logger 等,在 web 上无法加载)
 */

import { registerRootComponent } from 'expo';

import App from './App';

registerRootComponent(App);
