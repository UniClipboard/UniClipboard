// Host adapter for `@tauri-apps/plugin-opener`.
import { Browser } from '@wailsio/runtime'

export const openUrl = (url: string | URL): Promise<void> => Browser.OpenURL(String(url))
