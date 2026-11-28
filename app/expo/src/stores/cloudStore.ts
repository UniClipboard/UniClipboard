import { AppState } from 'react-native';
import { create } from 'zustand';

import { isNativeEngineAvailable, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { CloudStatusSnapshot, EngineEvent } from '@/engine/types';
import { authorizeCloudLogin, clearLegacyCloudSession } from '@/lib/cloudAuth';

const SIGNED_OUT_STATUS: CloudStatusSnapshot = {
  signed_in: false,
  state: 'signed_out',
};

interface CloudStoreState {
  loaded: boolean;
  busy: boolean;
  status: CloudStatusSnapshot | null;
  hydrate: () => Promise<void>;
  refreshStatus: () => Promise<CloudStatusSnapshot>;
  signIn: (options?: { autoEnableRelay?: boolean }) => Promise<CloudStatusSnapshot>;
  ensureRelay: () => Promise<CloudStatusSnapshot>;
  signOut: () => Promise<void>;
}

let listenersAttached = false;

export const useCloudStore = create<CloudStoreState>((set, get) => ({
  loaded: false,
  busy: false,
  status: null,

  hydrate: async () => {
    await clearLegacyCloudSession();
    if (!listenersAttached && isNativeEngineAvailable()) {
      listenersAttached = true;
      unicEngineNative.addEventListener((eventJSON) => {
        try {
          const ev = JSON.parse(eventJSON) as EngineEvent;
          if (ev.kind === 'settings') {
            void get().refreshStatus();
          }
        } catch {
          // ignore malformed event payload
        }
      });
      unicEngineNative.addResyncListener(() => {
        void get().refreshStatus();
      });
      AppState.addEventListener('change', (nextState) => {
        if (nextState === 'active') {
          void get().refreshStatus();
        }
      });
    }
    await get().refreshStatus();
  },

  refreshStatus: async () => {
    if (!isNativeEngineAvailable()) {
      set({ status: SIGNED_OUT_STATUS, loaded: true });
      return SIGNED_OUT_STATUS;
    }
    try {
      const status = await unicEngineNative.getCloudStatus();
      set({ status, loaded: true });
      return status;
    } catch {
      const current = get().status ?? SIGNED_OUT_STATUS;
      set({ status: current, loaded: true });
      return current;
    }
  },

  signIn: async (options) => {
    if (!isNativeEngineAvailable()) {
      throw new Error('UniClipboard Engine is unavailable.');
    }
    if (get().busy) {
      throw new Error('Cloud sign-in is already in progress.');
    }
    set({ busy: true });
    try {
      const engineState = useEngineStore.getState();
      const deviceName = engineState.snapshot?.settings.value.device_name || undefined;
      const grant = await authorizeCloudLogin({ deviceName });
      const relayUrl = engineState.snapshot?.settings.value.sync.relay?.url?.trim() ?? '';
      const autoEnableRelay = options?.autoEnableRelay !== false && !relayUrl;

      const status = await unicEngineNative.finishCloudLogin({
        cloud_url: grant.cloudUrl,
        code: grant.code,
        code_verifier: grant.codeVerifier,
        redirect_uri: grant.redirectUri,
        device_name: grant.deviceName,
        auto_enable_relay: autoEnableRelay,
      });
      set({ status, loaded: true });
      await useEngineStore.getState().refreshSnapshot();
      return status;
    } finally {
      set({ busy: false });
    }
  },

  ensureRelay: async () => {
    if (!isNativeEngineAvailable()) {
      throw new Error('UniClipboard Engine is unavailable.');
    }
    if (get().busy) {
      return get().status ?? SIGNED_OUT_STATUS;
    }
    set({ busy: true });
    try {
      const status = await unicEngineNative.ensureCloudRelay();
      set({ status, loaded: true });
      await useEngineStore.getState().refreshSnapshot();
      return status;
    } catch (err) {
      await get().refreshStatus();
      await useEngineStore.getState().refreshSnapshot();
      throw err;
    } finally {
      set({ busy: false });
    }
  },

  signOut: async () => {
    if (get().busy) return;
    set({ busy: true });
    try {
      await clearLegacyCloudSession();
      if (!isNativeEngineAvailable()) {
        set({ status: SIGNED_OUT_STATUS, loaded: true });
        return;
      }
      const status = await unicEngineNative.logoutCloud();
      set({ status, loaded: true });
      await useEngineStore.getState().refreshSnapshot();
    } finally {
      set({ busy: false });
    }
  },
}));
