import type { GroupedGridProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract (not virtualized, click selection only); the Win11
// grid belongs to the Windows History work.
export function GroupedGrid({
  label,
  items,
  selection,
  onSelectionChange,
  onActivate,
}: GroupedGridProps) {
  return (
    <div
      role="listbox"
      aria-label={label}
      aria-multiselectable
      style={{ display: 'flex', flexWrap: 'wrap', gap: '0.75rem' }}
    >
      {items.map(item => (
        <div
          key={item.id}
          role="option"
          aria-selected={selection.includes(item.id)}
          data-item-id={item.id}
          onClick={() => onSelectionChange([item.id])}
          onDoubleClick={() => onActivate?.(item.id)}
          style={{ width: '10rem' }}
        >
          {item.media ?? <div>{item.text}</div>}
          <div>{item.title}</div>
        </div>
      ))}
    </div>
  )
}
