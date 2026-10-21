import { useEffect } from 'react'
import { daemonWs } from '@/lib/daemon-ws'
import { useGetConfigImportStatusQuery } from '@/store/api'

/** Engine owns durable status. Requery on entry and after a missed-event reconnect. */
export function useConfigImportStatus() {
  const query = useGetConfigImportStatusQuery(undefined, {
    refetchOnMountOrArgChange: true,
  })
  const { refetch } = query
  useEffect(
    () =>
      daemonWs.onReconnect(() => {
        void refetch()
      }),
    [refetch]
  )
  return query
}
