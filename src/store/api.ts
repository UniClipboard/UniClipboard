import { createApi, fakeBaseQuery } from '@reduxjs/toolkit/query/react'
import { getEncryptionState } from '@/api/daemon'
import type { EncryptionStateResponse } from '@/api/daemon/encryption'

type ApiError = {
  message: string
}

/** Adapter: daemon returns camelCase, legacy consumers expect snake_case */
type EncryptionSessionStatus = {
  initialized: boolean
  session_ready: boolean
}

export const appApi = createApi({
  reducerPath: 'appApi',
  baseQuery: fakeBaseQuery<ApiError>(),
  tagTypes: ['EncryptionStatus'],
  endpoints: builder => ({
    getEncryptionSessionStatus: builder.query<EncryptionSessionStatus, void>({
      queryFn: async () => {
        try {
          const data: EncryptionStateResponse = await getEncryptionState()
          // Transform camelCase from daemon to snake_case for existing consumers
          const legacy: EncryptionSessionStatus = {
            initialized: data.initialized,
            session_ready: data.sessionReady,
          }
          return { data: legacy }
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error)
          return { error: { message } }
        }
      },
      providesTags: ['EncryptionStatus'],
    }),
  }),
})

export const { useGetEncryptionSessionStatusQuery, useLazyGetEncryptionSessionStatusQuery } = appApi
