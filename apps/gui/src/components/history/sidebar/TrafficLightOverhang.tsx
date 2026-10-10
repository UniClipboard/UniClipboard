import { useLibraryChrome } from '@/contexts/library-chrome-context'
import { usePlatform } from '@/hooks/usePlatform'
import { useWindowDragging } from '@/hooks/useWindowDragging'
import { cn } from '@/lib/utils'

/**
 * macOS, sidebar collapsed: the traffic lights (about x 13-73pt) sit in the
 * top band and reach past the 52px icon column into the first content
 * column's header. That column renders this window-drag spacer where the
 * lights land, so its own controls clear them. Renders nothing while the
 * sidebar is shown.
 */
function TrafficLightOverhang({ className }: { className: string }) {
  const { isMac } = usePlatform()
  const { hidden } = useLibraryChrome()
  const windowDragging = useWindowDragging()
  if (!isMac || !hidden) return null
  return <div data-tauri-drag-region {...windowDragging} className={cn('shrink-0', className)} />
}

export default TrafficLightOverhang
