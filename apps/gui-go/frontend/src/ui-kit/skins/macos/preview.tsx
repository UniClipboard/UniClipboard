import { Dialog } from '@base-ui/react/dialog'
import type { FactListProps, PreviewFrameProps, QuickLookProps } from '@/ui-kit/contract'
import { Icon } from './icons'
import { PushButton } from './parts'
import { usePortalContainer } from './portal'

/** The content frame of the detail (design MA2 code card, ME1 image stage). */
export function PreviewFrame({
  kind,
  caption,
  captionTrailing,
  topEnd,
  bottomStart,
  children,
}: PreviewFrameProps) {
  return (
    <div className="mac-preview" data-kind={kind}>
      {caption && (
        <div className="mac-code-header">
          <span>{caption}</span>
          {captionTrailing && <kbd>{captionTrailing}</kbd>}
        </div>
      )}
      <div className="mac-preview-body">{children}</div>
      {topEnd && (
        <div className="mac-preview-corner" data-corner="top-end">
          {topEnd}
        </div>
      )}
      {bottomStart && (
        <div className="mac-preview-corner" data-corner="bottom-start">
          {bottomStart}
        </div>
      )}
    </div>
  )
}

/** Fact cards side by side (design ME1: Dimensions, Format). */
export function FactList({ label, facts }: FactListProps) {
  return (
    <dl className="mac-facts" aria-label={label}>
      {facts.map(fact => (
        <div key={fact.id} className="mac-fact">
          <dt>{fact.label}</dt>
          <dd data-mono={fact.monospace ? '' : undefined}>{fact.value}</dd>
        </div>
      ))}
    </dl>
  )
}

/**
 * Quick Look (design ME4) drawn in the page: a dark translucent panel with its title, the image,
 * previous / next and the caller's commands. Base UI traps focus and restores it on close.
 */
export function QuickLook({
  open,
  onOpenChange,
  title,
  closeLabel,
  src,
  alt,
  previous,
  next,
  primary,
  actions,
}: QuickLookProps) {
  const container = usePortalContainer()
  return (
    <Dialog.Root open={open} onOpenChange={value => onOpenChange(value)}>
      <Dialog.Portal container={container}>
        <Dialog.Backdrop className="mac-quicklook-backdrop" />
        <Dialog.Popup
          className="mac-quicklook"
          onKeyDown={event => {
            // A focused button (Next, Close, the actions) handles its own Space and Enter.
            const onButton = (event.target as HTMLElement).closest('button, [role="button"]')
            if (onButton && (event.key === ' ' || event.key === 'Enter')) return
            if (event.key === ' ') {
              event.preventDefault()
              onOpenChange(false)
            } else if (event.key === 'ArrowLeft' && previous) {
              event.preventDefault()
              previous.onClick()
            } else if (event.key === 'ArrowRight' && next) {
              event.preventDefault()
              next.onClick()
            } else if (event.key === 'Enter' && primary) {
              event.preventDefault()
              primary.onClick()
            }
          }}
        >
          <div className="mac-quicklook-bar">
            <Dialog.Close className="mac-quicklook-close" aria-label={closeLabel}>
              <Icon name="close" size={9} strokeWidth={2.6} />
            </Dialog.Close>
            <Dialog.Title className="mac-quicklook-title">{title}</Dialog.Title>
            <span className="mac-split-spacer" />
            {actions}
          </div>
          <div className="mac-quicklook-stage">
            {previous && (
              <button
                type="button"
                className="mac-quicklook-step"
                data-side="previous"
                aria-label={previous.label}
                onClick={previous.onClick}
              >
                <Icon name="chevronLeft" size={18} strokeWidth={2} />
              </button>
            )}
            {src && <img src={src} alt={alt} className="mac-quicklook-image" draggable={false} />}
            {next && (
              <button
                type="button"
                className="mac-quicklook-step"
                data-side="next"
                aria-label={next.label}
                onClick={next.onClick}
              >
                <Icon name="chevronRight" size={18} strokeWidth={2} />
              </button>
            )}
          </div>
          {primary && (
            <div className="mac-quicklook-footer">
              <PushButton tint="primary" onClick={primary.onClick}>
                {primary.label}
              </PushButton>
            </div>
          )}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
