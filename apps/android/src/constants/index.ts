/**
 * App Constants
 */

import * as Application from 'expo-application';

export const APP_NAME = 'UniClip';
export const APP_VERSION = Application.nativeApplicationVersion ?? '1.0.0';

// API Endpoints
export const API_ENDPOINTS = {
  SYNC: '/api/sync',
  PROFILE: '/api/profile',
  HISTORY: '/api/history',
  UPLOAD: '/api/upload',
  DOWNLOAD: '/api/download',
};

// Storage Keys
export const STORAGE_KEYS = {
  SETTINGS: '@settings',
  SERVER_CONFIG: '@server_config',
  CLIPBOARD_HISTORY: '@clipboard_history',
  LAST_SYNC_TIME: '@last_sync_time',
};

// Default Settings
export const DEFAULT_SETTINGS = {
  autoSync: true,
  syncInterval: 5000, // 5 seconds
  maxHistorySize: 100,
  theme: 'auto' as const,
};

// Supported Image MIME Types
export const IMAGE_MIME_TYPES = [
  'image/png',
  'image/jpeg',
  'image/jpg',
  'image/gif',
  'image/bmp',
  'image/webp',
];
