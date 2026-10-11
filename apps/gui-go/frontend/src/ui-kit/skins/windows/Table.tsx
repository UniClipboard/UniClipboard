import type { TableProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract (plain grid with click selection); the WinUI
// ListView-with-columns design, keyboard and context menu belong to the Windows History work.
export function Table({ label, columns, rows, selection, onSelectionChange, empty }: TableProps) {
  return (
    <table aria-label={label}>
      <thead>
        <tr>
          {columns.map(column => (
            <th key={column.id}>{column.label}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map(row => (
          <tr
            key={row.id}
            aria-selected={selection.includes(row.id)}
            onClick={() => onSelectionChange([row.id])}
          >
            {columns.map(column => (
              <td key={column.id}>{row.cells[column.id]}</td>
            ))}
          </tr>
        ))}
        {rows.length === 0 && empty ? (
          <tr>
            <td colSpan={columns.length}>{empty}</td>
          </tr>
        ) : null}
      </tbody>
    </table>
  )
}
