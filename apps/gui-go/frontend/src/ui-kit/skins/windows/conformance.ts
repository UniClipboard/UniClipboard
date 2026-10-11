// Compile-time proof that this skin implements `@/ui-kit/contract` exactly: every contract
// component exists with a compatible signature, and the skin exports nothing else.
import type * as Contract from '@/ui-kit/contract'
import type * as Skin from './index'

type Assert<T extends true> = T
type Extra = Exclude<keyof typeof Skin, keyof typeof Contract>
type Missing = Exclude<keyof typeof Contract, keyof typeof Skin>

export type Conformance = [
  Assert<typeof Skin extends typeof Contract ? true : false>,
  Assert<[Extra | Missing] extends [never] ? true : false>,
]
