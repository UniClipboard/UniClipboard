import type React from 'react'
import { useNavigate } from 'react-router'
import { useSetupFlow } from '@/hooks/useSetupFlow'
import {
  EntryScreen,
  ImportConfigScreen,
  InitializeSpaceScreen,
  JoinPendingScreen,
  JoinEndedScreen,
  PairingCompleteScreen,
  RedeemInvitationScreen,
  ShowInvitationScreen,
  SpaceReadyScreen,
} from '@/pages/setup/screens'

interface SetupPageProps {
  onCompleteSetup?: () => void
}

type SetupFlow = ReturnType<typeof useSetupFlow>

interface SetupScreenProps {
  screen: Exclude<SetupFlow['screen'], { kind: 'loading' }>
  loading: boolean
  goEntry: SetupFlow['goEntry']
  startCreateSpace: SetupFlow['startCreateSpace']
  startJoinSpace: SetupFlow['startJoinSpace']
  startImportConfig: SetupFlow['startImportConfig']
  initializeSpace: SetupFlow['initializeSpace']
  issueInvitation: SetupFlow['issueInvitation']
  cancelInvitation: SetupFlow['cancelInvitation']
  redeemInvitation: SetupFlow['redeemInvitation']
  cancelJoin: SetupFlow['cancelJoin']
  onDone: () => void
}

const SetupScreen: React.FC<SetupScreenProps> = ({
  screen,
  loading,
  goEntry,
  startCreateSpace,
  startJoinSpace,
  startImportConfig,
  initializeSpace,
  issueInvitation,
  cancelInvitation,
  redeemInvitation,
  cancelJoin,
  onDone,
}) => {
  switch (screen.kind) {
    case 'entry':
      return (
        <EntryScreen
          onCreate={startCreateSpace}
          onJoin={startJoinSpace}
          onImport={startImportConfig}
          loading={loading}
        />
      )
    case 'initialize_space':
      return <InitializeSpaceScreen onSubmit={initializeSpace} onBack={goEntry} loading={loading} />
    case 'import_config':
      return <ImportConfigScreen onBack={goEntry} />
    case 'show_invitation':
      return (
        <ShowInvitationScreen
          code={screen.code}
          expiresAtMs={screen.expiresAtMs}
          onCancel={cancelInvitation}
          loading={loading}
        />
      )
    case 'redeem_invitation':
      return (
        <RedeemInvitationScreen onSubmit={redeemInvitation} onBack={goEntry} loading={loading} />
      )
    case 'join_pending':
      return (
        <JoinPendingScreen
          status={screen.status}
          onCancel={() => void cancelJoin(screen.joinId)}
          loading={loading}
        />
      )
    case 'join_ended':
      return <JoinEndedScreen reason={screen.reason} onBack={startJoinSpace} />
    case 'space_ready':
      return <SpaceReadyScreen onInvite={issueInvitation} onDone={onDone} loading={loading} />
    case 'pairing_complete':
      return (
        <PairingCompleteScreen
          localDeviceName={screen.localDeviceName}
          peerDeviceId={screen.peerDeviceId}
          onDone={onDone}
        />
      )
  }
}

export default function SetupPage({ onCompleteSetup }: SetupPageProps = {}) {
  const navigate = useNavigate()
  const {
    screen,
    loading,
    goEntry,
    startCreateSpace,
    startJoinSpace,
    startImportConfig,
    initializeSpace,
    issueInvitation,
    cancelInvitation,
    redeemInvitation,
    cancelJoin,
    finishPairing,
  } = useSetupFlow()

  // The app-level gate owns unknown setup state and its loading presentation.
  if (screen.kind === 'loading') return null

  const handleDone = () => {
    finishPairing()
    onCompleteSetup?.()
    navigate('/', { replace: true })
  }

  return (
    <SetupScreen
      key={screen.kind}
      screen={screen}
      loading={loading}
      goEntry={goEntry}
      startCreateSpace={startCreateSpace}
      startJoinSpace={startJoinSpace}
      startImportConfig={startImportConfig}
      initializeSpace={initializeSpace}
      issueInvitation={issueInvitation}
      cancelInvitation={cancelInvitation}
      redeemInvitation={redeemInvitation}
      cancelJoin={cancelJoin}
      onDone={handleDone}
    />
  )
}
