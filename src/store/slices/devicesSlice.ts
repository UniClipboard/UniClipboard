import { createAsyncThunk, createSlice } from '@reduxjs/toolkit'
import {
  getLocalDeviceInfo,
  getPairedPeersWithStatus,
  getDeviceSyncSettings,
  updateDeviceSyncSettings as updateDeviceSyncSettingsApi,
  type LocalDeviceInfo,
  type PairedPeer,
  type SyncSettings,
} from '@/api/p2p'

interface DevicesState {
  // 当前设备
  localDevice: LocalDeviceInfo | null
  localDeviceLoading: boolean
  localDeviceError: string | null

  // 已配对的设备
  pairedDevices: PairedPeer[]
  pairedDevicesLoading: boolean
  pairedDevicesError: string | null

  // 每设备同步设置
  deviceSyncSettings: Record<string, SyncSettings>
  deviceSyncSettingsLoading: Record<string, boolean>
}

const initialState: DevicesState = {
  localDevice: null,
  localDeviceLoading: false,
  localDeviceError: null,
  pairedDevices: [],
  pairedDevicesLoading: false,
  pairedDevicesError: null,
  deviceSyncSettings: {},
  deviceSyncSettingsLoading: {},
}

// 异步 Thunk Actions
export const fetchLocalDeviceInfo = createAsyncThunk(
  'devices/fetchLocalInfo',
  async (_, { rejectWithValue }) => {
    try {
      return await getLocalDeviceInfo()
    } catch {
      return rejectWithValue('获取当前设备信息失败')
    }
  }
)

export const fetchPairedDevices = createAsyncThunk(
  'devices/fetchPaired',
  async (_, { rejectWithValue }) => {
    try {
      return await getPairedPeersWithStatus()
    } catch {
      return rejectWithValue('获取已配对设备失败')
    }
  }
)

export const fetchDeviceSyncSettings = createAsyncThunk(
  'devices/fetchSyncSettings',
  async (peerId: string, { rejectWithValue }) => {
    try {
      const settings = await getDeviceSyncSettings(peerId)
      return { peerId, settings }
    } catch {
      return rejectWithValue('Failed to fetch device sync settings')
    }
  }
)

export const updateDeviceSyncSettings = createAsyncThunk(
  'devices/updateSyncSettings',
  async (
    { peerId, settings }: { peerId: string; settings: SyncSettings | null },
    { rejectWithValue }
  ) => {
    try {
      await updateDeviceSyncSettingsApi(peerId, settings)
      return { peerId, settings }
    } catch {
      return rejectWithValue('Failed to update device sync settings')
    }
  }
)

const devicesSlice = createSlice({
  name: 'devices',
  initialState,
  reducers: {
    clearLocalDeviceError: state => {
      state.localDeviceError = null
    },
    clearPairedDevicesError: state => {
      state.pairedDevicesError = null
    },
    updatePeerPresenceStatus: (
      state,
      action: { payload: { peerId: string; connected: boolean; deviceName?: string | null } }
    ) => {
      const peer = state.pairedDevices.find(d => d.peerId === action.payload.peerId)
      if (peer) {
        peer.connected = action.payload.connected
        if (action.payload.deviceName) {
          peer.deviceName = action.payload.deviceName
        }
      }
    },
    updatePeerDeviceName: (state, action: { payload: { peerId: string; deviceName: string } }) => {
      const peer = state.pairedDevices.find(d => d.peerId === action.payload.peerId)
      if (peer) {
        peer.deviceName = action.payload.deviceName
      }
    },
  },
  extraReducers: builder => {
    // Local device info
    builder
      .addCase(fetchLocalDeviceInfo.pending, state => {
        state.localDeviceLoading = true
        state.localDeviceError = null
      })
      .addCase(fetchLocalDeviceInfo.fulfilled, (state, action) => {
        state.localDeviceLoading = false
        state.localDevice = action.payload
      })
      .addCase(fetchLocalDeviceInfo.rejected, (state, action) => {
        state.localDeviceLoading = false
        state.localDeviceError = action.payload as string
      })

    // Paired devices
    builder
      .addCase(fetchPairedDevices.pending, state => {
        // Only show loading state when there are no cached devices.
        // When devices already exist (e.g., navigating back to the page),
        // we fetch in the background without triggering skeleton/loading UI.
        if (state.pairedDevices.length === 0) {
          state.pairedDevicesLoading = true
        }
        state.pairedDevicesError = null
      })
      .addCase(fetchPairedDevices.fulfilled, (state, action) => {
        state.pairedDevicesLoading = false
        state.pairedDevices = action.payload
      })
      .addCase(fetchPairedDevices.rejected, (state, action) => {
        state.pairedDevicesLoading = false
        state.pairedDevicesError = action.payload as string
      })

    // Device sync settings
    builder
      .addCase(fetchDeviceSyncSettings.pending, (state, action) => {
        state.deviceSyncSettingsLoading[action.meta.arg] = true
      })
      .addCase(fetchDeviceSyncSettings.fulfilled, (state, action) => {
        const { peerId, settings } = action.payload
        state.deviceSyncSettings[peerId] = settings
        state.deviceSyncSettingsLoading[peerId] = false
      })
      .addCase(fetchDeviceSyncSettings.rejected, (state, action) => {
        state.deviceSyncSettingsLoading[action.meta.arg] = false
      })

    builder
      .addCase(updateDeviceSyncSettings.pending, (state, action) => {
        state.deviceSyncSettingsLoading[action.meta.arg.peerId] = true
      })
      .addCase(updateDeviceSyncSettings.fulfilled, (state, action) => {
        const { peerId, settings } = action.payload
        if (settings) {
          state.deviceSyncSettings[peerId] = settings
        }
        state.deviceSyncSettingsLoading[peerId] = false
      })
      .addCase(updateDeviceSyncSettings.rejected, (state, action) => {
        state.deviceSyncSettingsLoading[action.meta.arg.peerId] = false
      })
  },
})

export const {
  clearLocalDeviceError,
  clearPairedDevicesError,
  updatePeerPresenceStatus,
  updatePeerDeviceName,
} = devicesSlice.actions
export default devicesSlice.reducer
