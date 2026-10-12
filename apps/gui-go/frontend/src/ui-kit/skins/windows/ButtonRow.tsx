import { makeStyles } from '@fluentui/react-components'
import type { ButtonRowProps } from '@/ui-kit/contract'

// Command buttons in page content start at the leading edge, as in Windows Settings.
const useStyles = makeStyles({
  row: { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '0.5rem' },
})

export function ButtonRow({ children }: ButtonRowProps) {
  return <div className={useStyles().row}>{children}</div>
}
