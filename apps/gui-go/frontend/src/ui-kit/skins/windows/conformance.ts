// Compile-time check that this skin implements `@/ui-kit/contract`: it exports exactly the
// contract's component names, and every component accepts exactly the contract's props (both
// directions, so the skin can neither demand more nor quietly accept vendor props). Business code
// sees only the contract types either way, because tsconfig maps `@ui-skin` to the contract.
import type * as Contract from '@/ui-kit/contract'
import type * as Skin from './index'

type Assert<T extends true> = T
type Extra = Exclude<keyof typeof Skin, keyof typeof Contract>
type Missing = Exclude<keyof typeof Contract, keyof typeof Skin>
type PropsOf<F> = F extends (...args: infer A) => unknown ? A : never
type KeysOf<F> = F extends (props: infer P) => unknown ? keyof P : never
// Optional extra props are assignable both ways, so the prop names are compared as well.
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false
type PropsMatch = {
  [K in keyof typeof Contract & keyof typeof Skin]: Same<
    PropsOf<(typeof Skin)[K]>,
    PropsOf<(typeof Contract)[K]>
  > extends true
    ? Same<KeysOf<(typeof Skin)[K]>, KeysOf<(typeof Contract)[K]>>
    : false
}[keyof typeof Contract & keyof typeof Skin]

export type Conformance = [
  Assert<[Extra | Missing] extends [never] ? true : false>,
  Assert<typeof Skin extends typeof Contract ? true : false>,
  Assert<[PropsMatch] extends [true] ? true : false>,
]
