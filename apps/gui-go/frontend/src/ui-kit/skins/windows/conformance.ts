// Compile-time proof that this skin implements `@/ui-kit/contract` (see `@/ui-kit/conformance`).
import type { Assert, ConformsToContract } from '@/ui-kit/conformance'
import type * as Skin from './index'

export type Conformance = Assert<ConformsToContract<typeof Skin>>
