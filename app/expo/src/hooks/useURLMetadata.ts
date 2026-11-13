/** URL cards use bundled text only. No remote metadata is fetched. */
interface URLCardMetadata { title?: string; ogImageUrl?: string; faviconUrl?: string }
export function useURLMetadata(_url: string | null): URLCardMetadata | null { return null; }
