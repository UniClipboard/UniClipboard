import { useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Image,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import {
  AlertTriangle,
  Check,
  Copy,
  Laptop,
  QrCode,
  RefreshCw,
  ShieldCheck,
} from 'lucide-react-native';
import { useTranslation } from 'react-i18next';

import { AppBottomSheet, AppButton, AppHost } from '@/components/ui';
import type { EngineDevice, PairingCode, SensitiveTicket } from '@/engine/types';
import { useTheme } from '@/hooks/useTheme';
import type { PairingFlowStage, PairingFlowTab } from './usePairingFlow';
import {
  deviceDisplayName,
  formatFingerprintLines,
  formatPairingCodeDisplay,
} from './pairingUtils';

export interface PairingFlowModalProps {
  visible: boolean;
  tab: PairingFlowTab;
  stage: PairingFlowStage;
  busy: boolean;
  errorMessage: string | null;
  creatingCode: boolean;
  codeError: string | null;
  pairingCode: PairingCode | null;
  countdown: string;
  ticket: SensitiveTicket | null;
  exportingTicket: boolean;
  verifyDevice: EngineDevice | null;
  approvedPeerName: string;
  handshakeStageIndex: number;
  onSelectTab: (tab: PairingFlowTab) => void;
  onSubmitInput: (input: string) => void;
  onSwitchToScan: () => void;
  onRegenerateCode: () => void;
  onCancelCode: () => void;
  onCopyCode: () => void;
  onExportTicket: () => void;
  onCopyTicket: () => void;
  onApprove: () => void;
  onReject: () => void;
  onRetryEntry: () => void;
  onClose: () => void;
}

/**
 * Unified M3 bottom-sheet pairing flow (Android & shared fallback):
 * - Idle: Segmented tabs ("Enter code" with 6-box OTP + ticket toggle | "This device's code" + optional offline QR)
 * - Connecting: 3-step live handshake progress
 * - Verify: Automatic inline fingerprint verification (Approve / Reject / Later)
 * - Waiting-remote / Connected / Error: Clear guided feedback
 */
export function PairingFlowModal({
  visible,
  tab,
  stage,
  busy,
  errorMessage,
  creatingCode,
  codeError,
  pairingCode,
  countdown,
  ticket,
  exportingTicket,
  verifyDevice,
  approvedPeerName,
  handshakeStageIndex,
  onSelectTab,
  onSubmitInput,
  onSwitchToScan,
  onRegenerateCode,
  onCancelCode,
  onCopyCode,
  onExportTicket,
  onCopyTicket,
  onApprove,
  onReject,
  onRetryEntry,
  onClose,
}: PairingFlowModalProps) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const c = theme.colors;
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);

  const [digits, setDigits] = useState<string[]>(['', '', '', '', '', '']);
  const [ticketMode, setTicketMode] = useState(false);
  const [ticketInput, setTicketInput] = useState('');
  const digitRefs = useRef<Array<TextInput | null>>([]);

  useEffect(() => {
    if (!visible || stage !== 'idle') {
      setDigits(['', '', '', '', '', '']);
      setTicketInput('');
      setTicketMode(false);
    }
  }, [visible, stage]);

  const handleDigitChange = (index: number, value: string) => {
    const trimmed = value.trim();
    if (trimmed.startsWith('unic-pair:')) {
      setTicketMode(true);
      setTicketInput(trimmed);
      return;
    }
    const clean = value.replace(/\D/g, '');
    if (clean.length > 1) {
      const next = [...digits];
      clean
        .slice(0, 6)
        .split('')
        .forEach((d, offset) => {
          next[offset] = d;
        });
      setDigits(next);
      digitRefs.current[Math.min(5, clean.length)]?.focus();
      return;
    }
    const next = [...digits];
    next[index] = clean.slice(-1);
    setDigits(next);
    if (next[index] && index < 5) {
      digitRefs.current[index + 1]?.focus();
    }
  };

  const isInputReady = ticketMode
    ? ticketInput.trim().length > 0
    : digits.every((d) => /^\d$/.test(d));

  const submitEntry = () => {
    if (!isInputReady || busy) return;
    onSubmitInput(ticketMode ? ticketInput : digits.join(''));
  };

  const canReject = Boolean(verifyDevice && verifyDevice.trust === 'pending');
  const handshakeSteps = [
    pair('phase.resolving'),
    pair('phase.connecting'),
    pair('phase.handshaking'),
  ];

  return (
    <AppBottomSheet visible={visible} onDismiss={busy ? () => {} : onClose}>
      <ScrollView
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
        contentContainerStyle={s.scrollContent}
      >
        {stage === 'idle' ? (
          <>
            <Text style={[s.title, { color: c.textPrimary }]}>{pair('codeSheetTitle')}</Text>

            {/* 2-tab segmented switcher */}
            <View style={[s.segmentBar, { backgroundColor: c.surfaceMid }]}>
              {(['enter', 'show'] as const).map((item) => {
                const selected = tab === item;
                return (
                  <Pressable
                    key={item}
                    testID={`devices-pairing-tab-${item}`}
                    onPress={() => onSelectTab(item)}
                    style={[
                      s.segmentBtn,
                      selected && { backgroundColor: c.accentContainer },
                    ]}
                  >
                    <Text
                      style={[
                        s.segmentText,
                        { color: selected ? c.onAccentContainer : c.textSecondary },
                        selected && s.segmentTextSelected,
                      ]}
                    >
                      {pair(item === 'enter' ? 'enterCodeTab' : 'myCodeTab')}
                    </Text>
                  </Pressable>
                );
              })}
            </View>

            {tab === 'enter' ? (
              <>
                <Text style={[s.subtitle, { color: c.textSecondary }]}>
                  {pair('enterCodeHelp')}
                </Text>

                {!ticketMode ? (
                  <View style={s.otpGrid}>
                    {digits.map((digit, idx) => (
                      <TextInput
                        key={idx}
                        ref={(el) => {
                          digitRefs.current[idx] = el;
                        }}
                        testID={idx === 0 ? 'devices-join-input' : `devices-otp-${idx}`}
                        value={digit}
                        keyboardType="number-pad"
                        maxLength={6}
                        textAlign="center"
                        selectTextOnFocus
                        onChangeText={(val) => handleDigitChange(idx, val)}
                        onKeyPress={({ nativeEvent }) => {
                          if (nativeEvent.key === 'Backspace' && !digit && idx > 0) {
                            const next = [...digits];
                            next[idx - 1] = '';
                            setDigits(next);
                            digitRefs.current[idx - 1]?.focus();
                          }
                        }}
                        style={[
                          s.otpBox,
                          {
                            backgroundColor: c.surfaceMid,
                            borderColor: digit ? c.accent : c.separator,
                            color: c.textPrimary,
                          },
                        ]}
                      />
                    ))}
                  </View>
                ) : (
                  <TextInput
                    testID="devices-join-input"
                    value={ticketInput}
                    onChangeText={setTicketInput}
                    placeholder={pair('inputHint')}
                    placeholderTextColor={c.textSecondary}
                    autoCapitalize="none"
                    autoCorrect={false}
                    multiline
                    style={[
                      s.ticketInput,
                      {
                        backgroundColor: c.surfaceMid,
                        borderColor: c.separator,
                        color: c.textPrimary,
                      },
                    ]}
                  />
                )}

                <View style={s.helperRow}>
                  <Pressable onPress={() => setTicketMode(!ticketMode)} hitSlop={8}>
                    <Text style={[s.linkText, { color: c.accent }]}>
                      {pair(ticketMode ? 'switchToCode' : 'pasteTicket')}
                    </Text>
                  </Pressable>
                  <Pressable onPress={onSwitchToScan} hitSlop={8} style={s.inlineIconLink}>
                    <QrCode size={14} color={c.accent as string} />
                    <Text style={[s.linkText, { color: c.accent }]}>{pair('scanShort')}</Text>
                  </Pressable>
                </View>

                {errorMessage ? (
                  <View style={[s.errorBox, { backgroundColor: c.errorContainer }]}>
                    <Text style={[s.errorText, { color: c.onErrorContainer }]}>{errorMessage}</Text>
                  </View>
                ) : null}

                <View style={s.actions}>
                  <AppHost matchContents={{ vertical: true }}>
                    <AppButton
                      testID="devices-join-submit"
                      title={pair('connect')}
                      variant="filled"
                      size="large"
                      fullWidth
                      disabled={!isInputReady || busy}
                      onPress={submitEntry}
                    />
                  </AppHost>
                </View>
              </>
            ) : (
              <>
                <View style={[s.codeCard, { backgroundColor: c.surfaceMid }]}>
                  {creatingCode && !pairingCode ? (
                    <ActivityIndicator size="large" color={c.accent} />
                  ) : pairingCode ? (
                    <>
                      <Text
                        testID="devices-my-code-value"
                        selectable
                        style={[s.codeDigits, { color: c.textPrimary }]}
                      >
                        {formatPairingCodeDisplay(pairingCode.code)}
                      </Text>
                      {countdown ? (
                        <Text style={[s.codeCountdown, { color: c.textSecondary }]}>
                          {pair('codeExpiresIn', { time: countdown })}
                        </Text>
                      ) : null}
                      <View style={s.codeActionRow}>
                        <Pressable
                          testID="devices-copy-code"
                          onPress={onCopyCode}
                          style={[s.pillAction, { backgroundColor: c.accentContainer }]}
                        >
                          <Copy size={14} color={c.onAccentContainer as string} />
                          <Text style={[s.pillActionText, { color: c.onAccentContainer }]}>
                            {pair('copyCode')}
                          </Text>
                        </Pressable>
                        <Pressable
                          testID="devices-create-code"
                          onPress={onRegenerateCode}
                          style={[s.pillAction, { backgroundColor: c.surfaceHigh }]}
                        >
                          <RefreshCw size={14} color={c.textPrimary as string} />
                          <Text style={[s.pillActionText, { color: c.textPrimary }]}>
                            {pair('regenerateCode')}
                          </Text>
                        </Pressable>
                      </View>
                    </>
                  ) : (
                    <>
                      <Text style={[s.subtitleCenter, { color: c.textSecondary }]}>
                        {codeError || pair('codeInstructions')}
                      </Text>
                      <AppHost matchContents={{ vertical: true }}>
                        <AppButton
                          testID="devices-create-code"
                          title={pair('generateCode')}
                          variant="filled"
                          onPress={onRegenerateCode}
                        />
                      </AppHost>
                    </>
                  )}
                </View>

                <Text style={[s.subtitleCenter, { color: c.textSecondary }]}>
                  {pair('myCodeAutoPopupHint')}
                </Text>

                {pairingCode ? (
                  <AppHost matchContents={{ vertical: true }}>
                    <AppButton
                      testID="devices-cancel-code"
                      title={pair('cancelCode')}
                      variant="text"
                      fullWidth
                      onPress={onCancelCode}
                    />
                  </AppHost>
                ) : null}

                {/* Collapsible offline QR / ticket section */}
                <View style={[s.offlineCard, { backgroundColor: c.surfaceMid }]}>
                  <Text style={[s.offlineTitle, { color: c.textPrimary }]}>
                    {pair('offlineSectionTitle')}
                  </Text>
                  <Text style={[s.offlineHint, { color: c.textSecondary }]}>
                    {pair('offlineSectionHelp')}
                  </Text>
                  {ticket ? (
                    <>
                      {ticket.qr_png ? (
                        <View style={s.qrWrap}>
                          <Image
                            testID="devices-pairing-qr"
                            accessibilityLabel={pair('ticketTitle')}
                            source={{ uri: `data:image/png;base64,${ticket.qr_png}` }}
                            style={s.qrImage}
                          />
                        </View>
                      ) : null}
                      <AppHost matchContents={{ vertical: true }}>
                        <AppButton
                          testID="devices-copy-ticket"
                          title={pair('copyTicket')}
                          variant="tonal"
                          fullWidth
                          onPress={onCopyTicket}
                        />
                      </AppHost>
                    </>
                  ) : (
                    <AppHost matchContents={{ vertical: true }}>
                      <AppButton
                        testID="devices-export-ticket"
                        title={exportingTicket ? pair('preparingQR') : pair('showOfflineQR')}
                        variant="outlined"
                        fullWidth
                        disabled={exportingTicket}
                        onPress={onExportTicket}
                      />
                    </AppHost>
                  )}
                </View>
              </>
            )}
          </>
        ) : stage === 'connecting' ? (
          <View style={s.centerStage}>
            <ActivityIndicator size="large" color={c.accent} />
            <Text testID="devices-pairing-status" style={[s.centerTitle, { color: c.textPrimary }]}>
              {pair('connectingTitle')}
            </Text>
            <View style={s.stepsList}>
              {handshakeSteps.map((label, idx) => {
                const done = idx < handshakeStageIndex;
                const active = idx === handshakeStageIndex;
                return (
                  <View key={label} style={s.stepRow}>
                    <View style={s.stepIconSlot}>
                      {done ? (
                        <Check size={16} color={c.success as string} />
                      ) : active ? (
                        <ActivityIndicator size="small" color={c.accent} />
                      ) : (
                        <View style={[s.stepDot, { backgroundColor: c.separator }]} />
                      )}
                    </View>
                    <Text
                      style={[
                        s.stepText,
                        {
                          color: done ? c.success : active ? c.textPrimary : c.textSecondary,
                          fontWeight: active ? '600' : '400',
                        },
                      ]}
                    >
                      {label}
                    </Text>
                  </View>
                );
              })}
            </View>
            <View style={s.centerAction}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-stop-waiting"
                  title={t('common:action.cancel')}
                  variant="tonal"
                  size="large"
                  fullWidth
                  onPress={onClose}
                />
              </AppHost>
            </View>
          </View>
        ) : stage === 'verify' && verifyDevice ? (
          <>
            <View style={s.verifyHeader}>
              <View style={[s.verifyBadge, { backgroundColor: c.warningContainer }]}>
                <Laptop size={22} color={c.warning as string} />
              </View>
              <View style={s.flex1}>
                <Text style={[s.title, { color: c.textPrimary }]}>
                  {pair('approveSheetTitle')}
                </Text>
                <Text style={[s.subtitle, { color: c.textSecondary }]}>
                  {pair('incomingSubtitle')}
                </Text>
              </View>
            </View>

            <View style={[s.fpCard, { backgroundColor: c.surfaceMid }]}>
              <View style={s.peerRow}>
                <Text style={[s.peerName, { color: c.textPrimary }]} numberOfLines={1}>
                  {deviceDisplayName(verifyDevice)}
                </Text>
                <Text style={[s.peerId, { color: c.textSecondary }]} numberOfLines={1}>
                  {verifyDevice.id.slice(0, 12)}
                </Text>
              </View>
              <View style={[s.fpBox, { backgroundColor: c.surfaceLow }]}>
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
                  {formatFingerprintLines(verifyDevice.fingerprint)}
                </Text>
              </View>
              <Text style={[s.subtitleCenter, { color: c.textSecondary }]}>
                {pair('approveHint')}
              </Text>
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
                  onPress={onClose}
                />
              </AppHost>
            </View>
          </>
        ) : stage === 'waiting-remote' ? (
          <View style={s.centerStage}>
            <ActivityIndicator size="large" color={c.accent} />
            <Text style={[s.centerTitle, { color: c.textPrimary }]}>
              {pair('waitingRemoteHeadline', { name: approvedPeerName })}
            </Text>
            <Text style={[s.subtitleCenter, { color: c.textSecondary }]}>
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
                  onPress={onClose}
                />
              </AppHost>
            </View>
          </View>
        ) : stage === 'connected' ? (
          <View style={s.centerStage}>
            <View style={[s.statusCircle, { backgroundColor: c.successContainer }]}>
              <Check size={30} color={c.success as string} />
            </View>
            <Text style={[s.centerTitle, { color: c.textPrimary }]}>
              {pair('connectedHeadline', { name: approvedPeerName })}
            </Text>
            <Text style={[s.subtitleCenter, { color: c.textSecondary }]}>
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
                  onPress={onClose}
                />
              </AppHost>
            </View>
          </View>
        ) : (
          <View style={s.centerStage}>
            <View style={[s.statusCircle, { backgroundColor: c.errorContainer }]}>
              <AlertTriangle size={28} color={c.error as string} />
            </View>
            <Text style={[s.centerTitle, { color: c.textPrimary }]}>{pair('phase.failed')}</Text>
            <Text style={[s.subtitleCenter, { color: c.textSecondary }]}>
              {errorMessage || pair('actionFailed')}
            </Text>
            <View style={[s.centerAction, s.actions]}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  title={pair('tryAgain')}
                  variant="filled"
                  size="large"
                  fullWidth
                  onPress={onRetryEntry}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  title={t('common:action.close')}
                  variant="text"
                  fullWidth
                  onPress={onClose}
                />
              </AppHost>
            </View>
          </View>
        )}
      </ScrollView>
    </AppBottomSheet>
  );
}

const s = StyleSheet.create({
  scrollContent: {
    paddingHorizontal: 20,
    paddingBottom: 20,
    gap: 14,
  },
  flex1: { flex: 1 },
  title: {
    fontSize: 20,
    fontWeight: '700',
  },
  subtitle: {
    fontSize: 14,
    lineHeight: 20,
  },
  subtitleCenter: {
    fontSize: 14,
    lineHeight: 20,
    textAlign: 'center',
  },
  segmentBar: {
    flexDirection: 'row',
    padding: 4,
    borderRadius: 16,
    gap: 4,
  },
  segmentBtn: {
    flex: 1,
    paddingVertical: 10,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
  },
  segmentText: {
    fontSize: 14,
    fontWeight: '500',
  },
  segmentTextSelected: {
    fontWeight: '700',
  },
  otpGrid: {
    flexDirection: 'row',
    gap: 8,
    justifyContent: 'space-between',
  },
  otpBox: {
    flex: 1,
    height: 54,
    borderRadius: 14,
    borderWidth: 1.5,
    fontSize: 22,
    fontWeight: '700',
    fontVariant: ['tabular-nums'],
  },
  ticketInput: {
    minHeight: 84,
    borderRadius: 14,
    borderWidth: 1,
    padding: 12,
    fontSize: 14,
    textAlignVertical: 'top',
  },
  helperRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
  },
  inlineIconLink: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 5,
  },
  linkText: {
    fontSize: 14,
    fontWeight: '600',
  },
  codeCard: {
    borderRadius: 20,
    padding: 20,
    alignItems: 'center',
    gap: 10,
  },
  codeDigits: {
    fontSize: 34,
    fontWeight: '700',
    letterSpacing: 3,
    fontVariant: ['tabular-nums'],
  },
  codeCountdown: {
    fontSize: 13,
    fontVariant: ['tabular-nums'],
  },
  codeActionRow: {
    flexDirection: 'row',
    gap: 10,
    marginTop: 6,
  },
  pillAction: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 14,
    paddingVertical: 9,
    borderRadius: 20,
  },
  pillActionText: {
    fontSize: 13,
    fontWeight: '600',
  },
  offlineCard: {
    borderRadius: 18,
    padding: 16,
    gap: 10,
  },
  offlineTitle: {
    fontSize: 15,
    fontWeight: '600',
  },
  offlineHint: {
    fontSize: 13,
    lineHeight: 19,
  },
  qrWrap: {
    alignSelf: 'center',
    backgroundColor: '#FFFFFF',
    padding: 12,
    borderRadius: 16,
  },
  qrImage: {
    width: 210,
    height: 210,
  },
  actions: {
    gap: 8,
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
  centerStage: {
    alignItems: 'center',
    paddingTop: 12,
    gap: 12,
  },
  statusCircle: {
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
  centerAction: {
    width: '100%',
    paddingTop: 8,
  },
  stepsList: {
    width: '100%',
    paddingHorizontal: 12,
    gap: 10,
    marginTop: 4,
  },
  stepRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  stepIconSlot: {
    width: 20,
    height: 20,
    alignItems: 'center',
    justifyContent: 'center',
  },
  stepDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
  stepText: {
    fontSize: 14,
  },
  verifyHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
  },
  verifyBadge: {
    width: 46,
    height: 46,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'center',
  },
  fpCard: {
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
  fpBox: {
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
});
