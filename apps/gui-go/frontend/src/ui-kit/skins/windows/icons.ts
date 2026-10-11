import {
  AddRegular,
  ArrowRightRegular,
  ChevronLeftRegular,
  ChevronDownRegular,
  ChevronRightRegular,
  CopyRegular,
  DeleteRegular,
  DesktopRegular,
  DismissRegular,
  EditRegular,
  FilterRegular,
  HistoryRegular,
  InfoRegular,
  MoreHorizontalRegular,
  PanelLeftRegular,
  PhoneLaptopRegular,
  PinFilled,
  PinRegular,
  SearchRegular,
  SettingsRegular,
  TagRegular,
  WeatherMoonRegular,
  WeatherSunnyRegular,
} from '@fluentui/react-icons'
import type { FluentIcon } from '@fluentui/react-icons'
import type { IconName } from '@/ui-kit/contract'

// Fluent System Icons for each public icon name. Toggle buttons switch to the filled variant when
// pressed (WinUI ToggleButton convention), so only icons used in toggles need one.
const regular: Record<IconName, FluentIcon> = {
  add: AddRegular,
  back: ChevronLeftRegular,
  chevronDown: ChevronDownRegular,
  chevronRight: ChevronRightRegular,
  copy: CopyRegular,
  delete: DeleteRegular,
  desktop: DesktopRegular,
  devices: PhoneLaptopRegular,
  dismiss: DismissRegular,
  edit: EditRegular,
  filter: FilterRegular,
  history: HistoryRegular,
  info: InfoRegular,
  moon: WeatherMoonRegular,
  more: MoreHorizontalRegular,
  pin: PinRegular,
  search: SearchRegular,
  send: ArrowRightRegular,
  settings: SettingsRegular,
  sidebar: PanelLeftRegular,
  sun: WeatherSunnyRegular,
  tag: TagRegular,
}

const filled: Partial<Record<IconName, FluentIcon>> = { pin: PinFilled }

export function iconFor(name: IconName, pressed = false): FluentIcon {
  return (pressed && filled[name]) || regular[name]
}
