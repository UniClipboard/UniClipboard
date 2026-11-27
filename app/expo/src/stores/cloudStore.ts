import { create } from 'zustand';

import { useEngineStore } from '@/engine/store';
import {
  type CloudSession,
  loadCloudSession,
  provisionCloudRelayKey,
  signInToCloud,
  signOutFromCloud,
} from '@/lib/cloudAuth';

interface CloudStoreState {
  loaded: boolean;
  busy: boolean;
  session: CloudSession | null;
  hydrate: () => Promise<void>;
  signIn: (options?: { autoEnableRelay?: boolean }) => Promise<CloudSession>;
  provisionRelay: () => Promise<void>;
  signOut: () => Promise<void>;
}

export const useCloudStore = create<CloudStoreState>((set, get) => ({
  loaded: false,
  busy: false,
  session: null,

  hydrate: async () => {
    if (get().loaded) return;
    const session = await loadCloudSession();
    set({ session, loaded: true });
  },

  signIn: async (options) => {
    if (get().busy) {
      const existing = get().session;
      if (existing) return existing;
      throw new Error('Cloud sign-in is already in progress.');
    }
    set({ busy: true });
    try {
      const engineState = useEngineStore.getState();
      const deviceName = engineState.snapshot?.settings.value.device_name || undefined;
      let session = await signInToCloud({ deviceName });
      set({ session, loaded: true });

      const autoEnableRelay = options?.autoEnableRelay !== false;
      const relayUrl = engineState.snapshot?.settings.value.sync.relay?.url?.trim() ?? '';
      if (autoEnableRelay && !relayUrl) {
        const res = await provisionCloudRelayKey(session, deviceName);
        session = res.session;
        set({ session });
        await engineState.updateSettings(
          (old) => ({
            ...old,
            sync: {
              ...old.sync,
              relay: {
                ...(old.sync.relay ?? { enabled: false }),
                enabled: true,
                url:
                  res.credential.relayUrl === 'https://relay.uniclipboard.app'
                    ? ''
                    : res.credential.relayUrl,
              },
            },
          }),
          { relayAccessKey: res.credential.apiKey }
        );
      }
      return session;
    } finally {
      set({ busy: false });
    }
  },

  provisionRelay: async () => {
    const cur = get().session;
    if (!cur || get().busy) return;
    set({ busy: true });
    try {
      const engineState = useEngineStore.getState();
      const deviceName = engineState.snapshot?.settings.value.device_name || undefined;
      const res = await provisionCloudRelayKey(cur, deviceName);
      set({ session: res.session });
      await engineState.updateSettings(
        (old) => ({
          ...old,
          sync: {
            ...old.sync,
            relay: {
              ...(old.sync.relay ?? { enabled: false }),
              enabled: true,
              url:
                res.credential.relayUrl === 'https://relay.uniclipboard.app'
                  ? ''
                  : res.credential.relayUrl,
            },
          },
        }),
        { relayAccessKey: res.credential.apiKey }
      );
    } finally {
      set({ busy: false });
    }
  },

  signOut: async () => {
    if (get().busy) return;
    set({ busy: true });
    try {
      const cur = get().session;
      await signOutFromCloud(cur);
      set({ session: null, loaded: true });
      const engineState = useEngineStore.getState();
      const relay = engineState.snapshot?.settings.value.sync.relay;
      if (relay && !relay.url?.trim() && (relay.enabled || relay.has_access_key)) {
        await engineState.updateSettings(
          (old) => ({
            ...old,
            sync: {
              ...old.sync,
              relay: { ...(old.sync.relay ?? { enabled: false }), enabled: false },
            },
          }),
          { relayAccessKey: '' }
        );
      }
    } finally {
      set({ busy: false });
    }
  },
}));
