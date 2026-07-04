import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit'
import {
  getMemberSyncPreferences,
  updateMemberSyncPreferences as updateMemberSyncPreferencesApi,
  type MemberSyncPreferences,
  type MemberSyncPreferencesPatch,
} from '@/api/daemon/member'
import {
  getLocalDeviceInfo,
  getPairedPeersWithStatus,
  type LocalDeviceInfo,
  type SpaceMember,
} from '@/api/daemon/members'

interface DevicesState {
  // 当前设备
  localDevice: LocalDeviceInfo | null
  localDeviceLoading: boolean
  localDeviceError: string | null

  // 空间成员（曾用名 pairedDevices）
  spaceMembers: SpaceMember[]
  spaceMembersLoading: boolean
  spaceMembersError: string | null

  // 每成员同步偏好（phase 4b PR-3：从 DeviceSyncSettings 切换到 MemberSyncPreferences）
  memberSyncPreferences: Record<string, MemberSyncPreferences>
  memberSyncPreferencesLoading: Record<string, boolean>
}

/** Field-wise equality so we can reuse a peer's object identity when a
 * `peers.changed` snapshot leaves it unchanged — that keeps memoized
 * `PeerCard`s from re-rendering when only a sibling peer flipped state. */
function sameMember(a: SpaceMember, b: SpaceMember): boolean {
  return (
    a.peerId === b.peerId &&
    a.deviceName === b.deviceName &&
    a.pairingState === b.pairingState &&
    a.lastSeenAtMs === b.lastSeenAtMs &&
    a.connected === b.connected &&
    a.channel === b.channel &&
    a.connectionAddress === b.connectionAddress
  )
}

/** Merge a PATCH onto current preferences for an optimistic UI update, so a
 * toggle reflects immediately instead of waiting for the daemon round-trip.
 * Omitted fields keep their current value (mirrors the server-side merge). */
function applyMemberSyncPreferencesPatch(
  current: MemberSyncPreferences,
  patch: MemberSyncPreferencesPatch
): MemberSyncPreferences {
  return {
    sendEnabled: patch.sendEnabled ?? current.sendEnabled,
    receiveEnabled: patch.receiveEnabled ?? current.receiveEnabled,
    sendContentTypes: patch.sendContentTypes
      ? { ...current.sendContentTypes, ...patch.sendContentTypes }
      : current.sendContentTypes,
    receiveContentTypes: patch.receiveContentTypes
      ? { ...current.receiveContentTypes, ...patch.receiveContentTypes }
      : current.receiveContentTypes,
  }
}

/** Field-wise equality so the authoritative server value can reuse the
 * optimistic object's identity when they match — that keeps the panel from
 * re-rendering a second time after each toggle. */
function sameMemberSyncPreferences(a: MemberSyncPreferences, b: MemberSyncPreferences): boolean {
  const sameContentTypes = (x: MemberSyncPreferences['sendContentTypes'], y: typeof x): boolean =>
    x.text === y.text &&
    x.image === y.image &&
    x.link === y.link &&
    x.file === y.file &&
    x.codeSnippet === y.codeSnippet &&
    x.richText === y.richText
  return (
    a.sendEnabled === b.sendEnabled &&
    a.receiveEnabled === b.receiveEnabled &&
    sameContentTypes(a.sendContentTypes, b.sendContentTypes) &&
    sameContentTypes(a.receiveContentTypes, b.receiveContentTypes)
  )
}

const initialState: DevicesState = {
  localDevice: null,
  localDeviceLoading: false,
  localDeviceError: null,
  spaceMembers: [],
  spaceMembersLoading: false,
  spaceMembersError: null,
  memberSyncPreferences: {},
  memberSyncPreferencesLoading: {},
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

export const fetchSpaceMembers = createAsyncThunk(
  'devices/fetchSpaceMembers',
  async (_, { rejectWithValue }) => {
    try {
      return await getPairedPeersWithStatus()
    } catch {
      return rejectWithValue('获取空间成员失败')
    }
  }
)

export const fetchMemberSyncPreferences = createAsyncThunk(
  'devices/fetchMemberSyncPreferences',
  async (deviceId: string, { rejectWithValue }) => {
    try {
      const preferences = await getMemberSyncPreferences(deviceId)
      return { deviceId, preferences }
    } catch {
      return rejectWithValue('Failed to fetch member sync preferences')
    }
  }
)

export const updateMemberSyncPreferences = createAsyncThunk(
  'devices/updateMemberSyncPreferences',
  async (
    { deviceId, patch }: { deviceId: string; patch: MemberSyncPreferencesPatch },
    { rejectWithValue }
  ) => {
    try {
      const preferences = await updateMemberSyncPreferencesApi(deviceId, patch)
      return { deviceId, preferences }
    } catch {
      return rejectWithValue('Failed to update member sync preferences')
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
    clearSpaceMembersError: state => {
      state.spaceMembersError = null
    },
    /**
     * Replace the member list from a `peers.changed` WS snapshot, skipping the
     * `GET /paired-devices` round-trip the event used to trigger. The snapshot
     * is the full, authoritative member set (same source as the HTTP endpoint),
     * so we replace wholesale — but reuse the previous object for any peer whose
     * fields are unchanged so memoized cards don't needlessly re-render.
     */
    setSpaceMembers: (state, action: PayloadAction<SpaceMember[]>) => {
      const prevById = new Map(state.spaceMembers.map(m => [m.peerId, m]))
      state.spaceMembers = action.payload.map(next => {
        const prev = prevById.get(next.peerId)
        return prev && sameMember(prev, next) ? prev : next
      })
      state.spaceMembersLoading = false
      state.spaceMembersError = null
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

    // Space members (曾用名 paired devices)
    builder
      .addCase(fetchSpaceMembers.pending, state => {
        // Only show loading state when there are no cached members.
        // When members already exist (e.g., navigating back to the page),
        // we fetch in the background without triggering skeleton/loading UI.
        if (state.spaceMembers.length === 0) {
          state.spaceMembersLoading = true
        }
        state.spaceMembersError = null
      })
      .addCase(fetchSpaceMembers.fulfilled, (state, action) => {
        state.spaceMembersLoading = false
        state.spaceMembers = action.payload
      })
      .addCase(fetchSpaceMembers.rejected, (state, action) => {
        state.spaceMembersLoading = false
        state.spaceMembersError = action.payload as string
      })

    // Member sync preferences
    builder
      .addCase(fetchMemberSyncPreferences.pending, (state, action) => {
        state.memberSyncPreferencesLoading[action.meta.arg] = true
      })
      .addCase(fetchMemberSyncPreferences.fulfilled, (state, action) => {
        const { deviceId, preferences } = action.payload
        state.memberSyncPreferences[deviceId] = preferences
        state.memberSyncPreferencesLoading[deviceId] = false
      })
      .addCase(fetchMemberSyncPreferences.rejected, (state, action) => {
        state.memberSyncPreferencesLoading[action.meta.arg] = false
      })

    builder
      .addCase(updateMemberSyncPreferences.pending, (state, action) => {
        // Optimistic update: apply the patch immediately so the toggle
        // responds without waiting for the loopback round-trip. Deliberately
        // does NOT flip `memberSyncPreferencesLoading` — that flag drives the
        // panel's disabled/opacity state, and toggling it on every mutation
        // made the whole section flicker + disabled mid-toggle. On failure the
        // caller re-fetches the authoritative value to reconcile.
        const { deviceId, patch } = action.meta.arg
        const current = state.memberSyncPreferences[deviceId]
        if (current) {
          state.memberSyncPreferences[deviceId] = applyMemberSyncPreferencesPatch(current, patch)
        }
      })
      .addCase(updateMemberSyncPreferences.fulfilled, (state, action) => {
        const { deviceId, preferences } = action.payload
        const current = state.memberSyncPreferences[deviceId]
        // Reuse the optimistic object's identity when the server agrees, so the
        // selector returns a stable reference and the panel does not re-render
        // a second time.
        if (!current || !sameMemberSyncPreferences(current, preferences)) {
          state.memberSyncPreferences[deviceId] = preferences
        }
      })
      .addCase(updateMemberSyncPreferences.rejected, () => {
        // Keep the optimistic value; the caller re-fetches to reconcile.
      })
  },
})

export const { clearLocalDeviceError, clearSpaceMembersError, setSpaceMembers } =
  devicesSlice.actions
export default devicesSlice.reducer
