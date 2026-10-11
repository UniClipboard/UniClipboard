import type { PreviewFrameProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract; styling belongs to the Windows History work.
export function PreviewFrame({ caption, topEnd, bottomStart, children }: PreviewFrameProps) {
  return (
    <div style={{ position: 'relative', display: 'flex', flexDirection: 'column' }}>
      {caption ? <div>{caption}</div> : null}
      <div style={{ position: 'relative', flexGrow: 1 }}>{children}</div>
      {topEnd ? <div>{topEnd}</div> : null}
      {bottomStart ? <div>{bottomStart}</div> : null}
    </div>
  )
}
