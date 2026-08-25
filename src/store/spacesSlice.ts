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
}

interface MutationSuccess {
  items: SpaceProfileSummary[]
}

interface MutationFailure {
  message: string
  items: SpaceProfileSummary[] | null
}

const initialState: SpacesState = {
  items: [],
  listLoading: false,
  listError: null,
  mutationError: null,
  activeSendPendingProfileId: null,
  activeSendError: null,
  activeSendRequestId: null,
}

async function mutateThenRefresh(
  mutation: () => Promise<unknown>,
  mutationError: string
): Promise<MutationSuccess | MutationFailure> {
  let message: string | null = null
  try {
    await mutation()
  } catch {
    message = mutationError
  }

  let items: SpaceProfileSummary[] | null = null
  try {
    items = await listSpaces()
  } catch {
    message ??= 'spaces.errors.refresh'
  }

  return message ? { message, items } : { items: items! }
}

let activeSendQueue: Promise<void> = Promise.resolve()

function enqueueActiveSend<T>(operation: () => Promise<T>): Promise<T> {
  const result = activeSendQueue.then(operation, operation)
  activeSendQueue = result.then(
    () => undefined,
    () => undefined
  )
  return result
}

export const fetchSpaces = createAsyncThunk<SpaceProfileSummary[], void, { rejectValue: string }>(
  'spaces/fetch',
  async (_, { rejectWithValue }) => {
    try {
      return await listSpaces()
    } catch {
      return rejectWithValue('spaces.errors.load')
    }
  }
)

export const createSpace = createAsyncThunk<
  MutationSuccess,
  CreateSpaceProfileRequest,
  { rejectValue: MutationFailure }
>('spaces/create', async (request, { rejectWithValue }) => {
  const result = await mutateThenRefresh(() => createSpaceProfile(request), 'spaces.errors.create')
  return 'message' in result ? rejectWithValue(result) : result
})

export const joinSpace = createAsyncThunk<
  MutationSuccess,
  JoinSpaceProfileRequest,
  { rejectValue: MutationFailure }
>('spaces/join', async (request, { rejectWithValue }) => {
  const result = await mutateThenRefresh(() => joinSpaceProfile(request), 'spaces.errors.join')
  return 'message' in result ? rejectWithValue(result) : result
})

export const selectActiveSendSpace = createAsyncThunk<
  MutationSuccess,
  string,
  { rejectValue: MutationFailure }
>('spaces/selectActiveSend', async (profileId, { rejectWithValue }) => {
  const result = await enqueueActiveSend(() =>
    mutateThenRefresh(() => setActiveSendSpace(profileId), 'spaces.errors.activeSend')
  )
  return 'message' in result ? rejectWithValue(result) : result
})

export const removeSpace = createAsyncThunk<
  MutationSuccess,
  string,
  { rejectValue: MutationFailure }
>('spaces/remove', async (profileId, { rejectWithValue }) => {
  const result = await mutateThenRefresh(
    () => deleteSpaceProfile(profileId),
    'spaces.errors.remove'
  )
  return 'message' in result ? rejectWithValue(result) : result
})

const spacesSlice = createSlice({
  name: 'spaces',
  initialState,
  reducers: {
    clearMutationError: state => {
      state.mutationError = null
    },
  },
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
        state.listError = action.payload ?? 'spaces.errors.load'
      })

    builder
      .addCase(createSpace.pending, state => {
        state.mutationError = null
      })
      .addCase(createSpace.fulfilled, (state, action) => {
        state.items = action.payload.items
        state.mutationError = null
      })
      .addCase(createSpace.rejected, (state, action) => {
        if (action.payload?.items) state.items = action.payload.items
        state.mutationError = action.payload?.message ?? 'spaces.errors.create'
      })

    builder
      .addCase(joinSpace.pending, state => {
        state.mutationError = null
      })
      .addCase(joinSpace.fulfilled, (state, action) => {
        state.items = action.payload.items
        state.mutationError = null
      })
      .addCase(joinSpace.rejected, (state, action) => {
        if (action.payload?.items) state.items = action.payload.items
        state.mutationError = action.payload?.message ?? 'spaces.errors.join'
      })

    builder
      .addCase(selectActiveSendSpace.pending, (state, action) => {
        state.activeSendRequestId = action.meta.requestId
        state.activeSendPendingProfileId = action.meta.arg
        state.activeSendError = null
      })
      .addCase(selectActiveSendSpace.fulfilled, (state, action) => {
        state.items = action.payload.items
        if (state.activeSendRequestId !== action.meta.requestId) return
        state.activeSendPendingProfileId = null
        state.activeSendRequestId = null
        state.activeSendError = null
      })
      .addCase(selectActiveSendSpace.rejected, (state, action) => {
        if (action.payload?.items) state.items = action.payload.items
        if (state.activeSendRequestId !== action.meta.requestId) return
        state.activeSendPendingProfileId = null
        state.activeSendRequestId = null
        state.activeSendError = action.payload?.message ?? 'spaces.errors.activeSend'
      })

    builder
      .addCase(removeSpace.pending, state => {
        state.mutationError = null
      })
      .addCase(removeSpace.fulfilled, (state, action) => {
        state.items = action.payload.items
        state.mutationError = null
      })
      .addCase(removeSpace.rejected, (state, action) => {
        if (action.payload?.items) state.items = action.payload.items
        state.mutationError = action.payload?.message ?? 'spaces.errors.remove'
      })
  },
})

export const { clearMutationError } = spacesSlice.actions
export default spacesSlice.reducer
