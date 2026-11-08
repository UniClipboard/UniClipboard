import { commands } from '@/lib/ipc'
import { appApi } from '@/store/api'

export async function fetchReleaseAnnouncements(locale: string) {
  return (await commands.getReleaseAnnouncements(locale)) ?? []
}

export type ReleaseAnnouncement = Awaited<ReturnType<typeof fetchReleaseAnnouncements>>[number]

export const announcementsApi = appApi.injectEndpoints({
  endpoints: builder => ({
    getReleaseAnnouncements: builder.query<
      Awaited<ReturnType<typeof fetchReleaseAnnouncements>>,
      string
    >({
      queryFn: async locale => {
        try {
          return { data: await fetchReleaseAnnouncements(locale) }
        } catch {
          return { error: { message: 'Failed to load release announcements' } }
        }
      },
    }),
  }),
})

export const { useGetReleaseAnnouncementsQuery } = announcementsApi
