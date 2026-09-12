import type { ListProps } from 'react-virtuoso'

// Let Virtuoso use the native viewport; shared CSS handles scrollbar visibility.
export function HistoryList(props: ListProps) {
  return <div {...props} style={{ ...props.style, width: '100%', minWidth: 0 }} />
}
