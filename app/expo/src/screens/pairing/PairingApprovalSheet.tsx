import { ActivityIndicator, StyleSheet, Text, View } from 'react-native';
import { Check, Laptop, ShieldCheck } from 'lucide-react-native';
import { useTranslation } from 'react-i18next';

import { AppBottomSheet, AppButton, AppHost } from '@/components/ui';
import { useTheme } from '@/hooks/useTheme';
import type { PairingApprovalSheetProps } from './PairingApprovalSheet.types';
import { deviceDisplayName, formatFingerprintLines } from './pairingUtils';

/**
 * Global M3 bottom sheet that pops up automatically whenever a device needs
 * fingerprint confirmation, and transitions cleanly through waiting -> connected.
 */
export function PairingApprovalSheet({
  visible,
  phase,
  device,
  peerName,
  busy,
  errorMessage,
  onApprove,
  onReject,
  onDismiss,
}: PairingApprovalSheetProps) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const c = theme.colors;
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);

  const canReject = Boolean(device && device.trust === 'pending');
  const displayName = device ? deviceDisplayName(device) : peerName;

  return (
    <AppBottomSheet visible={visible} onDismiss={busy ? () => {} : onDismiss}>
      <View style={s.body}>
        {phase === 'verify' && device ? (
          <>
            <View style={s.headerRow}>
              <View style={[s.badge, { backgroundColor: c.warningContainer }]}>
                <Laptop size={22} color={c.warning as string} />
              </View>
              <View style={s.headerText}>
                <Text style={[s.title, { color: c.textPrimary }]}>
                  {pair('approveSheetTitle')}
                </Text>
                <Text style={[s.subtitle, { color: c.textSecondary }]} numberOfLines={2}>
                  {pair('incomingSubtitle')}
                </Text>
              </View>
            </View>

            <View style={[s.card, { backgroundColor: c.surfaceMid }]}>
              <View style={s.peerRow}>
                <Text style={[s.peerName, { color: c.textPrimary }]} numberOfLines={1}>
                  {displayName}
                </Text>
                <Text style={[s.peerId, { color: c.textSecondary }]} numberOfLines={1}>
                  {device.id.slice(0, 12)}
                </Text>
              </View>

              <View style={[s.fpContainer, { backgroundColor: c.surfaceLow }]}>
                <View style={s.fpLabelRow}>
                  <ShieldCheck size={14} color={c.accent as string} />
                  <Text style={[s.fpLabel, { color: c.textSecondary }]}>
                    {pair('fingerprintLabel')}
                  </Text>
                </View>
                <Text
                  testID="devices-approve-fingerprint"
                  selectable
                  style={[s.fpValue, { color: c.textPrimary }]}
                >
                  {formatFingerprintLines(device.fingerprint)}
                </Text>
              </View>

              <Text style={[s.hint, { color: c.textSecondary }]}>{pair('approveHint')}</Text>
            </View>

            {errorMessage ? (
              <View style={[s.errorBox, { backgroundColor: c.errorContainer }]}>
                <Text style={[s.errorText, { color: c.onErrorContainer }]}>{errorMessage}</Text>
              </View>
            ) : null}

            <View style={s.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-approve-confirm"
                  title={pair('approve')}
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={busy}
                  onPress={onApprove}
                />
              </AppHost>
              {canReject ? (
                <AppHost matchContents={{ vertical: true }}>
                  <AppButton
                    testID="devices-approve-reject"
                    title={pair('reject')}
                    variant="tonal"
                    size="large"
                    fullWidth
                    colors={{ containerColor: c.errorContainer, contentColor: c.error }}
                    disabled={busy}
                    onPress={onReject}
                  />
                </AppHost>
              ) : null}
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-approve-cancel"
                  title={pair('later')}
                  variant="text"
                  fullWidth
                  disabled={busy}
                  onPress={onDismiss}
                />
              </AppHost>
            </View>
          </>
        ) : phase === 'waiting-remote' ? (
          <View style={s.centerStage}>
            <ActivityIndicator size="large" color={c.accent} />
            <Text style={[s.centerTitle, { color: c.textPrimary }]}>
              {pair('waitingRemoteHeadline', { name: displayName })}
            </Text>
            <Text style={[s.centerBody, { color: c.textSecondary }]}>
              {pair('waitingRemoteBody')}
            </Text>
            <View style={s.centerAction}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-approve-done"
                  title={pair('waitInBackground')}
                  variant="tonal"
                  size="large"
                  fullWidth
                  onPress={onDismiss}
                />
              </AppHost>
            </View>
          </View>
        ) : (
          <View style={s.centerStage}>
            <View style={[s.checkCircle, { backgroundColor: c.successContainer }]}>
              <Check size={30} color={c.success as string} />
            </View>
            <Text style={[s.centerTitle, { color: c.textPrimary }]}>
              {pair('connectedHeadline', { name: displayName })}
            </Text>
            <Text style={[s.centerBody, { color: c.textSecondary }]}>
              {pair('connectedBody')}
            </Text>
            <View style={s.centerAction}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-approve-done"
                  title={t('common:action.done')}
                  variant="filled"
                  size="large"
                  fullWidth
                  onPress={onDismiss}
                />
              </AppHost>
            </View>
          </View>
        )}
      </View>
    </AppBottomSheet>
  );
}

const s = StyleSheet.create({
  body: {
    paddingHorizontal: 20,
    paddingBottom: 16,
    gap: 16,
  },
  headerRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
  },
  badge: {
    width: 46,
    height: 46,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'center',
  },
  headerText: {
    flex: 1,
    gap: 2,
  },
  title: {
    fontSize: 20,
    fontWeight: '700',
  },
  subtitle: {
    fontSize: 14,
    lineHeight: 20,
  },
  card: {
    borderRadius: 20,
    padding: 16,
    gap: 12,
  },
  peerRow: {
    flexDirection: 'row',
    alignItems: 'baseline',
    justifyContent: 'space-between',
    gap: 12,
  },
  peerName: {
    flex: 1,
    fontSize: 16,
    fontWeight: '600',
  },
  peerId: {
    fontSize: 12,
    fontVariant: ['tabular-nums'],
  },
  fpContainer: {
    borderRadius: 14,
    paddingVertical: 14,
    paddingHorizontal: 12,
    alignItems: 'center',
    gap: 6,
  },
  fpLabelRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
  },
  fpLabel: {
    fontSize: 12,
    fontWeight: '600',
  },
  fpValue: {
    fontSize: 18,
    lineHeight: 28,
    fontWeight: '700',
    letterSpacing: 1.4,
    textAlign: 'center',
    fontVariant: ['tabular-nums'],
  },
  hint: {
    fontSize: 13,
    lineHeight: 19,
    textAlign: 'center',
  },
  errorBox: {
    borderRadius: 12,
    padding: 12,
  },
  errorText: {
    fontSize: 14,
    lineHeight: 20,
    textAlign: 'center',
  },
  actions: {
    gap: 8,
  },
  centerStage: {
    alignItems: 'center',
    paddingTop: 12,
    gap: 12,
  },
  checkCircle: {
    width: 60,
    height: 60,
    borderRadius: 30,
    alignItems: 'center',
    justifyContent: 'center',
  },
  centerTitle: {
    fontSize: 20,
    fontWeight: '700',
    textAlign: 'center',
  },
  centerBody: {
    fontSize: 14,
    lineHeight: 21,
    textAlign: 'center',
    paddingHorizontal: 12,
  },
  centerAction: {
    width: '100%',
    paddingTop: 8,
  },
});
