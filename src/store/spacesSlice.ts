import { createAsyncThunk, createSlice } from '@reduxjs/toolkit'
import {
  createSpaceProfile,
  deleteSpaceProfile,
  joinSpaceProfile,
  listSpaces,
  setActiveSendSpace,
  type CreateSpaceProfileRequest,
  type JoinSpaceProfileRequest,
  type SpaceProfileSummary,
} from '@/api/daemon/spaces'

export interface SpacesState {
  items: SpaceProfileSummary[]
  listLoading: boolean
  listError: string | null
  mutationError: string | null
  activeSendPendingProfileId: string | null
  activeSendError: string | null
  activeSendRequestId: string | null
  previousActiveSendProfileId: string | null
}

const initialState: SpacesState = {
  items: [],
  listLoading: false,
  listError: null,
  mutationError: null,
  activeSendPendingProfileId: null,
  activeSendError: null,
  activeSendRequestId: null,
  previousActiveSendProfileId: null,
}

function upsertSpace(items: SpaceProfileSummary[], incoming: SpaceProfileSummary): void {
  const index = items.findIndex(space => space.profileId === incoming.profileId)
  if (index === -1) {
    items.push(incoming)
  } else {
    items[index] = incoming
  }
}

export const fetchSpaces = createAsyncThunk<SpaceProfileSummary[], void, { rejectValue: string }>(
  'spaces/fetch',
  async (_, { rejectWithValue }) => {
    try {
      return await listSpaces()
    } catch {
      return rejectWithValue('Failed to load spaces')
    }
  }
)

export const createSpace = createAsyncThunk<
  SpaceProfileSummary,
  CreateSpaceProfileRequest,
  { rejectValue: string }
>('spaces/create', async (request, { rejectWithValue }) => {
  try {
    return await createSpaceProfile(request)
  } catch {
    return rejectWithValue('Failed to create space')
  }
})

export const joinSpace = createAsyncThunk<
  SpaceProfileSummary,
  JoinSpaceProfileRequest,
  { rejectValue: string }
>('spaces/join', async (request, { rejectWithValue }) => {
  try {
    return await joinSpaceProfile(request)
  } catch {
    return rejectWithValue('Failed to join space')
  }
})

export const selectActiveSendSpace = createAsyncThunk<
  SpaceProfileSummary,
  string,
  { rejectValue: string }
>('spaces/selectActiveSend', async (profileId, { rejectWithValue }) => {
  try {
    return await setActiveSendSpace(profileId)
  } catch {
    return rejectWithValue('Failed to change active send space')
  }
})

export const removeSpace = createAsyncThunk<SpaceProfileSummary, string, { rejectValue: string }>(
  'spaces/remove',
  async (profileId, { rejectWithValue }) => {
    try {
      return await deleteSpaceProfile(profileId)
    } catch {
      return rejectWithValue('Failed to remove space')
    }
  }
)

const spacesSlice = createSlice({
  name: 'spaces',
  initialState,
  reducers: {},
  extraReducers: builder => {
    builder
      .addCase(fetchSpaces.pending, state => {
        state.listLoading = state.items.length === 0
        state.listError = null
      })
      .addCase(fetchSpaces.fulfilled, (state, action) => {
        state.items = action.payload
        state.listLoading = false
        state.listError = null
      })
      .addCase(fetchSpaces.rejected, (state, action) => {
        state.listLoading = false
        state.listError = action.payload ?? 'Failed to load spaces'
      })

    builder
      .addCase(createSpace.pending, state => {
        state.mutationError = null
      })
      .addCase(createSpace.fulfilled, (state, action) => {
        upsertSpace(state.items, action.payload)
        state.mutationError = null
      })
      .addCase(createSpace.rejected, (state, action) => {
        state.mutationError = action.payload ?? 'Failed to create space'
      })

    builder
      .addCase(joinSpace.pending, state => {
        state.mutationError = null
      })
      .addCase(joinSpace.fulfilled, (state, action) => {
        upsertSpace(state.items, action.payload)
        state.mutationError = null
      })
      .addCase(joinSpace.rejected, (state, action) => {
        state.mutationError = action.payload ?? 'Failed to join space'
      })

    builder
      .addCase(selectActiveSendSpace.pending, (state, action) => {
        state.previousActiveSendProfileId =
          state.items.find(space => space.isActiveSend)?.profileId ?? null
        state.activeSendRequestId = action.meta.requestId
        state.activeSendPendingProfileId = action.meta.arg
        state.activeSendError = null
        for (const space of state.items) {
          space.isActiveSend = space.profileId === action.meta.arg
        }
      })
      .addCase(selectActiveSendSpace.fulfilled, (state, action) => {
        if (state.activeSendRequestId !== action.meta.requestId) return
        upsertSpace(state.items, action.payload)
        for (const space of state.items) {
          space.isActiveSend = space.profileId === action.payload.profileId
        }
        state.activeSendPendingProfileId = null
        state.activeSendRequestId = null
        state.previousActiveSendProfileId = null
        state.activeSendError = null
      })
      .addCase(selectActiveSendSpace.rejected, (state, action) => {
        if (state.activeSendRequestId !== action.meta.requestId) return
        for (const space of state.items) {
          space.isActiveSend = space.profileId === state.previousActiveSendProfileId
        }
        state.activeSendPendingProfileId = null
        state.activeSendRequestId = null
        state.previousActiveSendProfileId = null
        state.activeSendError = action.payload ?? 'Failed to change active send space'
      })

    builder
      .addCase(removeSpace.pending, state => {
        state.mutationError = null
      })
      .addCase(removeSpace.fulfilled, (state, action) => {
        state.items = state.items.filter(space => space.profileId !== action.payload.profileId)
        state.mutationError = null
      })
      .addCase(removeSpace.rejected, (state, action) => {
        state.mutationError = action.payload ?? 'Failed to remove space'
      })
  },
})

export default spacesSlice.reducer
