import type { ReactNode } from 'react'
import { SetupError } from '@/api/setup'
import type { DiscoveredPeer, ScanPhase } from '@/hooks/useDeviceDiscovery'

export type { DiscoveredPeer, ScanPhase }
export interface StepProps {
  error?: SetupError | null
  loading?: boolean
  direction?: 'forward' | 'backward'
}

export interface StepLayoutProps {
  headerLeft?: ReactNode
  headerRight?: ReactNode
  title: string
  subtitle?: string
  children?: ReactNode
  footer?: ReactNode
  hint?: string
  error?: string | null
  variant?: 'default' | 'centered'
  direction?: 'forward' | 'backward'
}

export interface ProcessingJoinStepProps {
  onCancel: () => void
  loading?: boolean
  direction?: 'forward' | 'backward'
}

export interface WelcomeStepProps extends StepProps {
  onCreate: () => void
  onJoin: () => void
}

export interface CreatePassphraseStepProps extends StepProps {
  onSubmit: (pass1: string, pass2: string) => void
}

export interface JoinPickDeviceStepProps extends StepProps {
  onSelectPeer: (peerId: string) => void
  onRescan: () => void
  peers: DiscoveredPeer[]
  scanPhase: ScanPhase
}

export interface JoinVerifyPassphraseStepProps extends StepProps {
  peerId?: string
  onSubmit: (passphrase: string) => void
  onCreateNew: () => void
}

export interface PairingConfirmStepProps extends StepProps {
  shortCode: string
  peerFingerprint?: string | null
  onConfirm: () => void
  onCancel: () => void
}

export interface SetupDoneStepProps extends StepProps {
  onComplete: () => void
}
