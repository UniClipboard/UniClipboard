// Compile-time check that a skin implements this contract. A skin's own `conformance.ts` states
// `export type Conformance = Assert<ConformsToContract<typeof Skin>>` for its `index.ts`.
//
// A skin conforms when it exports exactly the contract's component names and every component
// accepts exactly the contract's props: the parameter types are assignable both ways and the prop
// names are equal, so a skin can neither demand more nor quietly accept vendor props (optional
// extra props are assignable both ways and are only caught by the name comparison).
import type * as Contract from './contract'

type Components = typeof Contract
type PropsOf<F> = F extends (...args: infer A) => unknown ? A : never
type KeysOf<F> = F extends (props: infer P) => unknown ? keyof P : never
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false

type SameNames<Skin> = Same<keyof Skin, keyof Components>
type SameProps<Skin> = {
  [K in keyof Components & keyof Skin]: Same<PropsOf<Skin[K]>, PropsOf<Components[K]>> extends true
    ? Same<KeysOf<Skin[K]>, KeysOf<Components[K]>>
    : false
}[keyof Components & keyof Skin]

export type ConformsToContract<Skin> =
  SameNames<Skin> extends true ? ([SameProps<Skin>] extends [true] ? true : false) : false

export type Assert<T extends true> = T
