import { Heart, Star } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { fetchSponsors, type Sponsor } from '@/api/sponsors'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { createLogger } from '@/lib/logger'
import { cn } from '@/lib/utils'

const log = createLogger('sponsors-group')

/** 爱发电 sponsorship page (currently the only paid channel). */
const SPONSOR_CHANNEL_URL = 'https://afdian.com/a/mkdir700'
const GITHUB_REPOSITORY_URL = 'https://github.com/UniClipboard/UniClipboard'

/** Deterministic 1–2 char monogram for sponsors without an avatar. */
function initialsFor(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return '?'
  const isLatin = [...trimmed].every(ch => ch.charCodeAt(0) < 128)
  if (isLatin) {
    const parts = trimmed.split(/\s+/).filter(Boolean)
    return parts.length >= 2
      ? (parts[0][0] + parts[1][0]).toUpperCase()
      : trimmed.slice(0, 2).toUpperCase()
  }
  return trimmed.slice(0, 1)
}

function SponsorCard({ sponsor, goldLabel }: { sponsor: Sponsor; goldLabel: string }) {
  const isGold = sponsor.tier === 'gold'

  const card = (
    <div
      className={cn(
        'flex h-full flex-col items-center gap-2 rounded-lg border p-3 text-center transition-colors',
        isGold
          ? 'border-amber-400/50 bg-amber-400/[0.06] hover:bg-amber-400/10'
          : 'border-border/60 bg-background hover:bg-muted/40'
      )}
    >
      <Avatar size="lg">
        {sponsor.avatar && <AvatarImage src={sponsor.avatar} alt={sponsor.name} />}
        <AvatarFallback>{initialsFor(sponsor.name)}</AvatarFallback>
      </Avatar>
      <div className="w-full min-w-0 space-y-1">
        <p className="truncate text-sm font-medium">{sponsor.name}</p>
        {isGold ? (
          <Badge
            variant="secondary"
            className="border-amber-400/40 bg-amber-400/15 text-amber-700 dark:text-amber-300"
          >
            {goldLabel}
          </Badge>
        ) : (
          sponsor.note && <p className="truncate text-xs text-muted-foreground">{sponsor.note}</p>
        )}
      </div>
    </div>
  )

  if (sponsor.url) {
    return (
      <a href={sponsor.url} target="_blank" rel="noreferrer" className="block">
        {card}
      </a>
    )
  }
  return card
}

/** Placeholder card matching `SponsorCard`'s layout, shown while the feed loads. */
function SponsorCardSkeleton() {
  return (
    <div className="flex h-full flex-col items-center gap-2 rounded-lg border border-border/60 bg-background p-3">
      <Skeleton className="size-10 rounded-full" />
      <div className="w-full space-y-1.5">
        <Skeleton className="mx-auto h-3.5 w-14" />
        <Skeleton className="mx-auto h-3 w-10" />
      </div>
    </div>
  )
}

const SKELETON_CARD_COUNT = 3

/**
 * "Sponsors" group on the About settings page. Thanks current sponsors with a
 * small avatar-card grid pulled live from the public website feed; gold
 * sponsors are highlighted. The section itself renders immediately with a
 * skeleton placeholder while the feed loads; once the fetch settles, an empty
 * list / offline / fetch error hides the section instead of showing a broken
 * placeholder.
 */
export function SponsorsGroup() {
  const { t } = useTranslation()
  const [sponsors, setSponsors] = useState<Sponsor[] | null>(null)
  const isLoading = sponsors === null

  useEffect(() => {
    const controller = new AbortController()
    fetchSponsors(controller.signal)
      .then(setSponsors)
      .catch(error => {
        if (controller.signal.aborted) return
        log.error({ err: error }, 'failed to load sponsors')
        setSponsors([])
      })
    return () => controller.abort()
  }, [])

  // Loaded but empty, or failed → hide the section entirely.
  if (sponsors?.length === 0) return null

  return (
    <div className="space-y-1.5">
      <h3 className="px-1 text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {t('settings.sections.about.sponsors.title')}
      </h3>
      <div className="space-y-4 rounded-lg border border-border/60 bg-card p-4">
        <div className="flex items-center gap-2">
          <Heart className="size-4 shrink-0 fill-rose-500/20 text-rose-500" />
          <p className="text-xs leading-snug text-muted-foreground">
            {t('settings.sections.about.sponsors.thanks')}
          </p>
        </div>
        {isLoading ? (
          <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
            {Array.from({ length: SKELETON_CARD_COUNT }).map((_, i) => (
              <SponsorCardSkeleton key={i} />
            ))}
          </div>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
              {sponsors.map(sponsor => (
                <SponsorCard
                  key={sponsor.id}
                  sponsor={sponsor}
                  goldLabel={t('settings.sections.about.sponsors.goldBadge')}
                />
              ))}
            </div>
            <div className="flex flex-wrap justify-center gap-2 pt-0.5">
              <a
                href={SPONSOR_CHANNEL_URL}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 rounded-lg border border-border/60 px-3 py-1.5 text-sm text-primary transition-colors hover:bg-muted/50"
              >
                <Heart className="size-3.5" />
                {t('settings.sections.about.sponsors.becomeSponsor')}
              </a>
              <a
                href={GITHUB_REPOSITORY_URL}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 rounded-lg border border-border/60 px-3 py-1.5 text-sm text-primary transition-colors hover:bg-muted/50"
              >
                <Star className="size-3.5" />
                {t('settings.sections.about.sponsors.githubStar')}
              </a>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
