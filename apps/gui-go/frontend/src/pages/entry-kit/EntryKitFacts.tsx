import { Text } from '@/ui-kit'

/**
 * Labelled values that must be read in full (a diagnosis, an import's source), stacked label over
 * value so long text wraps instead of being cut.
 */
export function EntryKitFacts({
  label,
  facts,
}: {
  label: string
  facts: { id: string; label: string; value: string; monospace?: boolean }[]
}) {
  return (
    <dl aria-label={label} className="m-0 flex flex-col gap-3">
      {facts.map(fact => (
        <div key={fact.id} className="flex flex-col gap-0.5">
          <dt>
            <Text role="caption" tone="secondary">
              {fact.label}
            </Text>
          </dt>
          <dd className="m-0 break-words">
            <Text monospace={fact.monospace}>{fact.value}</Text>
          </dd>
        </div>
      ))}
    </dl>
  )
}
