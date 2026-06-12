/**
 * SignalR Client Module
 *
 * 统一的 SignalR 客户端接口，对外不体现平台差异：
 * - Android: 使用 native Java SignalR 客户端（前后台均可靠运行）
 * - 其他平台: 使用 JS @microsoft/signalr 库
 */

import { Platform } from 'react-native';
import { requireNativeModule, type EventSubscription } from 'expo-modules-core';
import * as SignalR from '@microsoft/signalr';

// ========== Types (self-contained, no cross-module imports) ==========

export interface ServerConfig {
  type: 'syncclipboard' | 'webdav' | 's3';
  url: string;
  username?: string;
  password?: string;
}

interface ProfileDto {
  type: string;
  hash?: string;
  text: string;
  hasData: boolean;
  dataName?: string;
  size?: number;
}

// ========== Shared Types ==========

export interface ProfileChangedEvent {
  type: string;
  hash: string;
  text: string;
  hasData: boolean;
  dataName?: string;
  size: number;
}

export interface HistoryChangedEvent {
  hash: string;
  text: string;
  type: string;
  hasData: boolean;
  size: number;
  starred: boolean;
  pinned: boolean;
  version: number;
  isDeleted: boolean;
  createTime?: string;
  lastModified?: string;
  lastAccessed?: string;
}

export type ConnectionState = 'CONNECTED' | 'DISCONNECTED' | 'CONNECTING' | 'RECONNECTING';

export interface RemoteClipboardChangedCallback {
  (event: ProfileChangedEvent): void;
}

export interface RemoteHistoryChangedCallback {
  (event: HistoryChangedEvent): void;
}

// ========== Native Module Types ==========

type NativeStateChangedEvent = { state: ConnectionState };

type NativeEvents = {
  onProfileChanged: (event: ProfileChangedEvent) => void;
  onHistoryChanged: (event: HistoryChangedEvent) => void;
  onStateChanged: (event: NativeStateChangedEvent) => void;
};

interface NativeSignalRModule {
  connect(url: string, username: string, password: string): void;
  disconnect(): void;
  isConnected(): boolean;
  getState(): string;
  addListener<K extends keyof NativeEvents>(
    eventName: K,
    listener: NativeEvents[K]
  ): EventSubscription;
}

// ========== Unified SignalR Client ==========

class NativeSignalRClient {
  private nativeModule: NativeSignalRModule;
  private profileSub: EventSubscription | null = null;
  private historySub: EventSubscription | null = null;
  private stateSub: EventSubscription | null = null;
  private profileCallbacks = new Set<RemoteClipboardChangedCallback>();
  private historyCallbacks = new Set<RemoteHistoryChangedCallback>();

  constructor(nativeModule: NativeSignalRModule) {
    this.nativeModule = nativeModule;
  }

  async connect(serverConfig: ServerConfig): Promise<void> {
    if (serverConfig.type !== 'syncclipboard') {
      throw new Error('SignalR is only supported for SyncClipboard server type');
    }

    // Register native event listeners
    this.profileSub?.remove();
    this.profileSub = this.nativeModule.addListener('onProfileChanged', (event) => {
      this.profileCallbacks.forEach((cb) => {
        try {
          cb(event);
        } catch (e) {
          console.error('[NativeSignalRClient] Error in profile callback:', e);
        }
      });
    });

    this.historySub?.remove();
    this.historySub = this.nativeModule.addListener('onHistoryChanged', (event) => {
      this.historyCallbacks.forEach((cb) => {
        try {
          cb(event);
        } catch (e) {
          console.error('[NativeSignalRClient] Error in history callback:', e);
        }
      });
    });

    this.nativeModule.connect(
      serverConfig.url,
      serverConfig.username || '',
      serverConfig.password || ''
    );
  }

  async disconnect(): Promise<void> {
    this.profileSub?.remove();
    this.profileSub = null;
    this.historySub?.remove();
    this.historySub = null;
    this.stateSub?.remove();
    this.stateSub = null;
    this.nativeModule.disconnect();
  }

  isConnected(): boolean {
    return this.nativeModule.isConnected();
  }

  getConnectionState(): ConnectionState {
    return this.nativeModule.getState() as ConnectionState;
  }

  onRemoteClipboardChanged(callback: RemoteClipboardChangedCallback): void {
    this.profileCallbacks.add(callback);
  }

  offRemoteClipboardChanged(callback: RemoteClipboardChangedCallback): void {
    this.profileCallbacks.delete(callback);
  }

  onRemoteHistoryChanged(callback: RemoteHistoryChangedCallback): void {
    this.historyCallbacks.add(callback);
  }

  offRemoteHistoryChanged(callback: RemoteHistoryChangedCallback): void {
    this.historyCallbacks.delete(callback);
  }

  clearCallbacks(): void {
    this.profileCallbacks.clear();
    this.historyCallbacks.clear();
  }
}

class JSSignalRClient {
  private connection: SignalR.HubConnection | null = null;
  private serverConfig: ServerConfig | null = null;
  private profileCallbacks = new Set<RemoteClipboardChangedCallback>();
  private historyCallbacks = new Set<RemoteHistoryChangedCallback>();
  private isConnecting = false;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;

  async connect(serverConfig: ServerConfig): Promise<void> {
    if (serverConfig.type !== 'syncclipboard') {
      throw new Error('SignalR is only supported for SyncClipboard server type');
    }

    if (this.isConnecting) {
      return;
    }

    if (this.connection && this.serverConfig?.url === serverConfig.url) {
      if (this.connection.state === SignalR.HubConnectionState.Connected) {
        return;
      }
    }

    await this.disconnect();
    this.serverConfig = serverConfig;
    this.isConnecting = true;

    try {
      const hubUrl = serverConfig.url.replace(/\/$/, '') + '/SyncClipboardHub';
      const headers: Record<string, string> = {};
      if (serverConfig.username && serverConfig.password) {
        const credentials = `${serverConfig.username}:${serverConfig.password}`;
        headers.Authorization = `Basic ${btoa(credentials)}`;
      }

      this.connection = new SignalR.HubConnectionBuilder()
        .withUrl(hubUrl, { headers, skipNegotiation: false })
        .withAutomaticReconnect({
          nextRetryDelayInMilliseconds: (retryContext) => {
            if (retryContext.previousRetryCount >= this.maxReconnectAttempts) return null;
            return Math.min(2000 * Math.pow(2, retryContext.previousRetryCount), 32000);
          },
        })
        .configureLogging(SignalR.LogLevel.Information)
        .build();

      this.connection.on('RemoteProfileChanged', (profile: ProfileDto) => {
        const event: ProfileChangedEvent = {
          type: profile.type,
          hash: profile.hash || '',
          text: profile.text,
          hasData: profile.hasData,
          dataName: profile.dataName,
          size: profile.size || 0,
        };
        this.profileCallbacks.forEach((cb) => {
          try {
            cb(event);
          } catch (e) {
            console.error('[JSSignalRClient] Error in profile callback:', e);
          }
        });
      });

      this.connection.on('RemoteHistoryChanged', (record: HistoryChangedEvent) => {
        this.historyCallbacks.forEach((cb) => {
          try {
            cb(record);
          } catch (e) {
            console.error('[JSSignalRClient] Error in history callback:', e);
          }
        });
      });

      this.connection.onreconnecting(() => {
        this.reconnectAttempts++;
      });

      this.connection.onreconnected(() => {
        this.reconnectAttempts = 0;
      });

      await this.connection.start();
      this.reconnectAttempts = 0;
      this.isConnecting = false;
    } catch (error) {
      this.isConnecting = false;
      throw error;
    }
  }

  async disconnect(): Promise<void> {
    if (this.connection) {
      try {
        await this.connection.stop();
      } catch (e) {
        console.error('[JSSignalRClient] Error during disconnect:', e);
      }
      this.connection = null;
    }
    this.serverConfig = null;
    this.isConnecting = false;
  }

  isConnected(): boolean {
    return this.connection?.state === SignalR.HubConnectionState.Connected;
  }

  getConnectionState(): ConnectionState {
    if (!this.connection) return 'DISCONNECTED';
    switch (this.connection.state) {
      case SignalR.HubConnectionState.Connected:
        return 'CONNECTED';
      case SignalR.HubConnectionState.Connecting:
        return 'CONNECTING';
      case SignalR.HubConnectionState.Reconnecting:
        return 'RECONNECTING';
      default:
        return 'DISCONNECTED';
    }
  }

  onRemoteClipboardChanged(callback: RemoteClipboardChangedCallback): void {
    this.profileCallbacks.add(callback);
  }

  offRemoteClipboardChanged(callback: RemoteClipboardChangedCallback): void {
    this.profileCallbacks.delete(callback);
  }

  onRemoteHistoryChanged(callback: RemoteHistoryChangedCallback): void {
    this.historyCallbacks.add(callback);
  }

  offRemoteHistoryChanged(callback: RemoteHistoryChangedCallback): void {
    this.historyCallbacks.delete(callback);
  }

  clearCallbacks(): void {
    this.profileCallbacks.clear();
    this.historyCallbacks.clear();
  }
}

// ========== Public Interface ==========

export type SignalRClient = NativeSignalRClient | JSSignalRClient;

let signalRClientInstance: SignalRClient | null = null;

export function getSignalRClient(): SignalRClient {
  if (!signalRClientInstance) {
    if (Platform.OS === 'android') {
      const nativeModule: NativeSignalRModule = requireNativeModule('SignalRClientModule');
      signalRClientInstance = new NativeSignalRClient(nativeModule);
    } else {
      signalRClientInstance = new JSSignalRClient();
    }
  }
  return signalRClientInstance;
}

export function resetSignalRClient(): void {
  if (signalRClientInstance) {
    signalRClientInstance.disconnect();
    signalRClientInstance = null;
  }
}
