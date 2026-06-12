/**
 * File Action Utilities
 * 文件操作公共函数 - 打开、分享文件
 */

import { NativeModules, Platform } from 'react-native';
import * as MediaLibrary from 'expo-media-library';
import { nativeCopyFile } from 'native-util';

const APP_PACKAGE = 'app.uniclipboard.android';

/**
 * 根据文件 URI / 文件名推断 MIME 类型（模块私有）
 */
function getMimeTypeFromUri(fileUri: string): string {
  const name = fileUri.split('?')[0].toLowerCase();
  if (name.endsWith('.apk')) return 'application/vnd.android.package-archive';
  if (name.endsWith('.pdf')) return 'application/pdf';
  if (name.endsWith('.mp4') || name.endsWith('.mkv') || name.endsWith('.avi')) return 'video/*';
  if (name.endsWith('.mp3') || name.endsWith('.flac') || name.endsWith('.aac')) return 'audio/*';
  if (
    name.endsWith('.jpg') ||
    name.endsWith('.jpeg') ||
    name.endsWith('.png') ||
    name.endsWith('.gif') ||
    name.endsWith('.webp') ||
    name.endsWith('.bmp') ||
    name.endsWith('.prm') // expo image format
  )
    return 'image/*';
  return '*/*';
}

/**
 * 通过系统 ACTION_VIEW Intent 打开文件
 * - APK 安装失败时自动跳转"安装未知来源"设置页
 * - Android 7+ 要求使用 content:// URI
 */
export async function openFile(fileUri: string): Promise<void> {
  const FileSystem = await import('expo-file-system/legacy');
  const IntentLauncher = await import('expo-intent-launcher');
  const mimeType = getMimeTypeFromUri(fileUri);

  const contentUri = await FileSystem.getContentUriAsync(fileUri);
  try {
    await IntentLauncher.startActivityAsync('android.intent.action.VIEW', {
      data: contentUri,
      type: mimeType,
      flags: 1, // FLAG_GRANT_READ_URI_PERMISSION
    });
  } catch (error) {
    // APK 安装失败时引导开启"安装未知来源"权限
    if (mimeType === 'application/vnd.android.package-archive') {
      try {
        await IntentLauncher.startActivityAsync('android.settings.MANAGE_UNKNOWN_APP_SOURCES', {
          data: `package:${APP_PACKAGE}`,
        });
      } catch {}
    }
    throw error;
  }
}

/**
 * 将文件储存到用户选择的目录（Android SAF）
 * 会弹出系统文件夹选择器，将文件复制到所选位置。
 */
export async function saveFile(fileUri: string, fileName?: string): Promise<void> {
  const FileSystem = await import('expo-file-system/legacy');
  const { StorageAccessFramework } = FileSystem;

  const permissions = await StorageAccessFramework.requestDirectoryPermissionsAsync();
  if (!permissions.granted) {
    throw new Error('Storage permission denied');
  }

  const name = fileName || fileUri.split('/').pop() || 'file';
  const mimeType = getMimeTypeFromUri(fileUri);

  const destUri = await StorageAccessFramework.createFileAsync(
    permissions.directoryUri,
    name,
    mimeType === '*/*' ? 'application/octet-stream' : mimeType
  );

  // 运行时检查，避免模块顶层静态求值时 NativeModules 尚未注入的问题
  const hashModule = Platform.OS === 'android' ? (NativeModules.NativeUtilModule ?? null) : null;
  console.log(
    '[saveFile] NativeModules.NativeUtilModule:',
    hashModule,
    'keys:',
    hashModule ? Object.keys(hashModule) : 'N/A'
  );

  if (hashModule?.copyFile) {
    // 原生流式拷贝：FileChannel.transferTo，不把文件读入 JS/Java 堆
    await nativeCopyFile(fileUri, destUri);
  } else {
    console.warn('[saveFile] falling back to base64, hashModule:', hashModule);
    // 降级：base64 读写（非 Android 或原生模块未加载时）
    const content = await FileSystem.readAsStringAsync(fileUri, {
      encoding: FileSystem.EncodingType.Base64,
    });
    await FileSystem.writeAsStringAsync(destUri, content, {
      encoding: FileSystem.EncodingType.Base64,
    });
  }
}

/**
 * 通过系统分享对话框分享文件
 */
export async function shareFile(fileUri: string, fileName?: string): Promise<void> {
  const Sharing = await import('expo-sharing');
  const mimeType = getMimeTypeFromUri(fileUri);
  await Sharing.shareAsync(fileUri, {
    mimeType,
    dialogTitle: fileName || '分享文件',
    UTI: mimeType,
  });
}

/**
 * 保存图片到相册
 * 仅支持图片类型文件
 */
export async function saveToGallery(fileUri: string): Promise<void> {
  const mimeType = getMimeTypeFromUri(fileUri);
  const isImage = mimeType.startsWith('image/');

  if (!isImage) {
    throw new Error('仅支持保存图片到相册');
  }

  const { status } = await MediaLibrary.requestPermissionsAsync();
  if (status !== 'granted') {
    throw new Error('Media library permission denied');
  }

  await MediaLibrary.createAssetAsync(fileUri);
}
