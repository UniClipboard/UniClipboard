import type { FactListProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract; styling belongs to the Windows History work.
export function FactList({ label, facts }: FactListProps) {
  return (
    <dl aria-label={label} style={{ display: 'flex', gap: '1rem', margin: 0 }}>
      {facts.map(fact => (
        <div key={fact.id}>
          <dt>{fact.label}</dt>
          <dd style={{ margin: 0 }}>{fact.value}</dd>
        </div>
      ))}
    </dl>
  )
}
