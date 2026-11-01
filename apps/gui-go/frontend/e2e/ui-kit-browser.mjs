import { runEntry } from './tester-army/entry.mjs'

process.env.UC_UI_PLATFORM ??= process.env.UI_KIT_PLATFORM
runEntry('ui-kit')
