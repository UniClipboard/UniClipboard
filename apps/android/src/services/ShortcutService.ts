import { Platform } from 'react-native';
import {
  requestPinDownloadShortcut,
  requestPinUploadShortcut,
  isShortcutModuleAvailable,
} from 'shortcut';

export const ShortcutService = {
  addDownloadShortcut(): Promise<boolean> {
    if (Platform.OS !== 'android') {
      return Promise.reject(new Error('Home-screen shortcuts are only supported on Android'));
    }
    if (!isShortcutModuleAvailable) {
      return Promise.reject(new Error('ShortcutModule is not available'));
    }
    return requestPinDownloadShortcut().catch((error) => {
      console.error('ShortcutModule addDownloadShortcut error:', error);
      throw error;
    });
  },

  addUploadShortcut(): Promise<boolean> {
    if (Platform.OS !== 'android') {
      return Promise.reject(new Error('Home-screen shortcuts are only supported on Android'));
    }
    if (!isShortcutModuleAvailable) {
      return Promise.reject(new Error('ShortcutModule is not available'));
    }
    return requestPinUploadShortcut().catch((error) => {
      console.error('ShortcutModule addUploadShortcut error:', error);
      throw error;
    });
  },
};
