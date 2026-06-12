/**
 * SMS Upload Headless Task
 * 无头 JS 任务 — 在后台（无 UI）提取短信验证码并上传到服务器。
 * 由 SmsHeadlessTaskService (Native) 启动，不依赖 React Native 主界面线程。
 */

import AsyncStorage from '@react-native-async-storage/async-storage';
import { Platform } from 'react-native';
import { STORAGE_KEYS } from '../types/storage';
import type { AppConfig } from '../types/storage';
import type { ServerConfig, ProfileDto } from '../types/api';
import { SyncClipboardClient } from '../services/SyncClipboardClient';
import { WebDAVClient } from '../services/WebDAVClient';
import { S3Client } from '../services/S3Client';
import { AuthService } from '../services/AuthService';
import type { ISyncClipboardAPI } from '../services/APIClient';
import { sha256 } from 'js-sha256';

// 重试配置
const MAX_RETRIES = 3;
const RETRY_DELAYS = [3_000, 10_000, 30_000];

interface SmsTaskData {
  from: string;
  body: string;
}

/**
 * 从短信正文中提取验证码（调用 Native 正则）
 */
export function extractVerificationCode(body: string): string | null {
  try {
    const { extractVerificationCode: nativeExtract } = require('sms-forwarder');
    return nativeExtract(body);
  } catch (e) {
    console.error('[SmsUploadTask] extractVerificationCode native call failed:', e);
    return null;
  }
}

/**
 * 从 AsyncStorage 加载应用配置
 */
async function loadConfig(): Promise<AppConfig | null> {
  try {
    const json = await AsyncStorage.getItem(STORAGE_KEYS.CONFIG);
    if (!json) return null;
    return JSON.parse(json) as AppConfig;
  } catch (e) {
    console.error('[SmsUploadTask] Failed to load config:', e);
    return null;
  }
}

/**
 * 根据服务器配置创建 API 客户端
 */
function createAPIClient(server: ServerConfig): ISyncClipboardAPI {
  const { type, url, username, password } = server;

  if (type === 'syncclipboard') {
    const authService = username && password ? new AuthService(username, password) : undefined;
    return new SyncClipboardClient({ baseURL: url, authService });
  }

  if (type === 's3') {
    return new S3Client({
      serviceURL: url || undefined,
      region: server.region,
      bucketName: server.bucketName!,
      objectPrefix: server.objectPrefix,
      forcePathStyle: server.forcePathStyle,
      accessKeyId: username!,
      secretAccessKey: password!,
    });
  }

  // 非 SyncClipboard/S3 服务器，使用 WebDAV 客户端
  return new WebDAVClient({ baseURL: url, username: username!, password: password! });
}

/**
 * 计算文本 SHA256（大写十六进制，与主应用一致）
 */
function calculateHash(text: string): string {
  const hasher = sha256.create();
  hasher.update(text);
  return hasher.hex().toUpperCase();
}

/**
 * 更新 Headless 服务专属的短信验证码上传通知
 */
async function updateNotification(text: string): Promise<void> {
  try {
    const { updateSmsUploadNotification } = require('sms-forwarder');
    updateSmsUploadNotification(text);
  } catch {
    // sms-forwarder 模块不可用，忽略
  }
}

/**
 * 上传成功后启动原生侧 60 秒倒计时通知
 */
function startCountdown(code: string): void {
  try {
    const { startSmsUploadCountdown } = require('sms-forwarder');
    startSmsUploadCountdown(code);
  } catch {
    // sms-forwarder 模块不可用，忽略
  }
}

/**
 * 带重试的上传
 */
async function uploadWithRetry(
  client: ISyncClipboardAPI,
  code: string,
  profileHash: string
): Promise<boolean> {
  const profile: ProfileDto = {
    type: 'Text',
    text: code,
    hash: profileHash,
    hasData: false,
  };

  for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
    try {
      await client.putClipboard(profile);
      console.log(`[SmsUploadTask] Verification code uploaded: ${code}`);
      startCountdown(code);
      return true;
    } catch (error) {
      if (attempt < MAX_RETRIES) {
        const delay = RETRY_DELAYS[attempt] ?? RETRY_DELAYS[RETRY_DELAYS.length - 1];
        console.warn(
          `[SmsUploadTask] Upload failed (attempt ${attempt + 1}/${MAX_RETRIES + 1}): ${error}, retrying in ${delay}ms`
        );
        await updateNotification(
          `验证码上传重试中: ${code}\n第${attempt + 1}次失败，${Math.round(delay / 1000)}秒后重试…`
        );
        await new Promise((resolve) => setTimeout(resolve, delay));
      } else {
        console.error(`[SmsUploadTask] Upload failed after ${MAX_RETRIES + 1} attempts:`, error);
        await updateNotification(`验证码上传失败: ${code}\n已重试${MAX_RETRIES}次`);
        return false;
      }
    }
  }
  return false;
}

/**
 * Headless JS 任务入口
 */
export default async function SmsUploadTask(taskData?: SmsTaskData): Promise<void> {
  if (Platform.OS !== 'android') return;
  if (!taskData?.body) {
    console.warn('[SmsUploadTask] No SMS body in task data');
    return;
  }

  const { from, body } = taskData;
  console.log(`[SmsUploadTask] Headless task started, SMS from ${from}`);

  // 1. 提取验证码
  const code = extractVerificationCode(body);
  if (!code) {
    console.log('[SmsUploadTask] No verification code found in SMS');
    return;
  }
  console.log(`[SmsUploadTask] Verification code extracted: ${code}`);

  // 2. 复制到剪贴板（headless 模式下可能失败，不阻塞上传）
  try {
    const Clipboard = require('expo-clipboard');
    await Clipboard.setStringAsync(code);
    console.log(`[SmsUploadTask] Copied to clipboard: ${code}`);
  } catch (e) {
    console.warn('[SmsUploadTask] Failed to copy to clipboard (headless mode):', e);
  }

  // 3. 加载配置
  const config = await loadConfig();
  if (!config) {
    console.error('[SmsUploadTask] No config found, cannot upload');
    return;
  }

  if (!config.enableSmsForwarding) {
    console.log('[SmsUploadTask] SMS forwarding disabled in config');
    return;
  }

  // 4. 获取当前激活的服务器
  const server = config.servers?.[config.activeServerIndex];
  if (!server?.url) {
    console.error('[SmsUploadTask] No active server configured');
    return;
  }

  // 5. 创建 API 客户端并上传
  try {
    const client = createAPIClient(server);
    const profileHash = calculateHash(code);
    await updateNotification(`正在上传验证码：${code}`);
    await uploadWithRetry(client, code, profileHash);
  } catch (error) {
    console.error('[SmsUploadTask] Unexpected error during upload:', error);
  }

  console.log('[SmsUploadTask] Headless task completed');
}
