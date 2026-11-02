import { Button } from '@/components/ui/button'
import type { SettingActionButtonProps } from './types'

export { SettingAlert } from './legacy-alert'
export { SettingSelect, SettingSwitch } from './legacy-controls'
export { SettingGroup } from './legacy-group'
export { SettingRow } from './legacy-row'

export { Fragment as SettingPresentationRoot } from 'react'
export { SettingNotice } from './legacy-notice'

export function SettingActionButton(props: SettingActionButtonProps) {
  return <Button variant="outline" size="sm" {...props} />
}
