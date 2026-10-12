import type { ComponentProps } from 'react'
import { useNavigate } from 'react-router'
import { useSetupFlow } from '@/hooks/useSetupFlow'
import type ExistingSetupPage from '@/pages/SetupPage'
import { SetupKitEntry } from './SetupKitEntry'
import { SetupKitImport } from './SetupKitImport'
import { SetupKitInitialize } from './SetupKitInitialize'
import { SetupKitInvitation } from './SetupKitInvitation'
import { SetupKitJoinEnded } from './SetupKitJoinEnded'
import { SetupKitJoinPending } from './SetupKitJoinPending'
import { SetupKitPaired } from './SetupKitPaired'
import { SetupKitRedeem } from './SetupKitRedeem'
import { SetupKitSpaceReady } from './SetupKitSpaceReady'

type Props = ComponentProps<typeof ExistingSetupPage>

/**
 * First-run setup on the platform UI kit (design S1–S12), replacing `SetupPage` at build time
 * through `@ui-setup-page`. Screen selection and every daemon call come from `useSetupFlow`, the
 * same owner as the existing page; each step keeps its form state in the shared setup hooks.
 */
export default function SetupPage({ onCompleteSetup }: Props = {}) {
  const navigate = useNavigate()
  const flow = useSetupFlow()
  const { screen, loading } = flow

  // The app-level gate owns unknown setup state and its loading presentation.
  if (screen.kind === 'loading') return null

  const done = () => {
    flow.finishPairing()
    onCompleteSetup?.()
    navigate('/', { replace: true })
  }

  switch (screen.kind) {
    case 'entry':
      return (
        <SetupKitEntry
          onCreate={flow.startCreateSpace}
          onJoin={flow.startJoinSpace}
          onImport={flow.startImportConfig}
          loading={loading}
        />
      )
    case 'initialize_space':
      return (
        <SetupKitInitialize
          onSubmit={flow.initializeSpace}
          onBack={flow.goEntry}
          loading={loading}
        />
      )
    case 'import_config':
      return <SetupKitImport onBack={flow.goEntry} />
    case 'show_invitation':
      return (
        <SetupKitInvitation
          key={screen.code}
          code={screen.code}
          expiresAtMs={screen.expiresAtMs}
          onCancel={flow.cancelInvitation}
          loading={loading}
        />
      )
    case 'redeem_invitation':
      return (
        <SetupKitRedeem onSubmit={flow.redeemInvitation} onBack={flow.goEntry} loading={loading} />
      )
    case 'join_pending':
      return (
        <SetupKitJoinPending
          status={screen.status}
          onCancel={() => void flow.cancelJoin(screen.joinId)}
          loading={loading}
        />
      )
    case 'join_ended':
      return <SetupKitJoinEnded reason={screen.reason} onBack={flow.startJoinSpace} />
    case 'space_ready':
      return <SetupKitSpaceReady onInvite={flow.issueInvitation} onDone={done} loading={loading} />
    case 'pairing_complete':
      return (
        <SetupKitPaired
          localDeviceName={screen.localDeviceName}
          peerDeviceId={screen.peerDeviceId}
          onDone={done}
        />
      )
  }
}
