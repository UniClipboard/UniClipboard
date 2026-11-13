import { useState } from 'react';
import { ActivityIndicator, ScrollView, StyleSheet, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';
import { AppButton, AppHost } from '@/components/ui';
import { useTheme } from '@/hooks/useTheme';
import { OnboardingArtwork } from './OnboardingArtwork';
import { OnboardingCloseButton } from './OnboardingCloseButton';
import type { useOnboardingConnection } from './useOnboardingConnection';

interface Props {
  connection: ReturnType<typeof useOnboardingConnection>;
  onComplete: () => void;
  finishing: boolean;
  completionError: boolean;
}

export function OnboardingConnection({
  connection,
  onComplete,
  finishing,
  completionError,
}: Props) {
  const { t } = useTranslation('onboarding');
  const { theme } = useTheme();
  const c = theme.colors;
  const [details, setDetails] = useState(false);
  const success = connection.stage === 'success';
  const pending = connection.busy || finishing;
  const name =
    connection.intent?.name ||
    (connection.intent?.urls[0] ? new URL(connection.intent.urls[0]).hostname : '');

  return (
    <SafeAreaView style={[s.root, { backgroundColor: c.background }]} edges={['top', 'bottom']}>
      <View style={s.header}>
        <AppHost style={s.closeHost}>
          <OnboardingCloseButton
            onPress={success ? onComplete : connection.close}
            disabled={pending}
            label={t(success ? 'action.close' : 'pairing.later', {
              ns: success ? 'common' : 'onboarding',
            })}
          />
        </AppHost>
      </View>
      <ScrollView contentContainerStyle={s.content}>
        <View style={s.art}>
          <OnboardingArtwork kind="settings" />
        </View>
        <Text accessibilityRole="header" style={[s.title, { color: c.textPrimary }]}>
          {t(success ? 'pairing.success' : 'pairing.confirm')}
        </Text>
        <Text style={[s.name, { color: c.textPrimary }]}>{name}</Text>
        <Text style={[s.body, { color: c.textSecondary }]}>
          {t(success ? 'pairing.successBody' : 'pairing.method')}
        </Text>
        {!success && (
          <>
            <View style={s.secondaryActions}>
              <AppHost style={s.secondaryButton} matchContents={{ vertical: true }}>
                <AppButton
                  title={t(details ? 'pairing.hideDetails' : 'pairing.details')}
                  variant="text"
                  fullWidth
                  colors={{ contentColor: c.accent }}
                  onPress={() => setDetails(!details)}
                />
              </AppHost>
              <AppHost style={s.secondaryButton} matchContents={{ vertical: true }}>
                <AppButton
                  title={t('pairing.rescan')}
                  variant="text"
                  fullWidth
                  colors={{ contentColor: c.accent }}
                  disabled={pending}
                  onPress={() => void connection.scan()}
                />
              </AppHost>
            </View>
            {details && (
              <View style={s.details}>
                {connection.intent?.urls.map((url) => (
                  <Text key={url} selectable style={[s.address, { color: c.textSecondary }]}>
                    {url}
                  </Text>
                ))}
              </View>
            )}
          </>
        )}
        {connection.busy && (
          <View style={s.status} accessibilityLiveRegion="polite">
            <ActivityIndicator color={c.accent} />
            <Text style={[s.body, { color: c.textSecondary }]}>{t('pairing.connecting')}</Text>
          </View>
        )}
        {(connection.error || completionError) && (
          <Text accessibilityRole="alert" style={[s.error, { color: c.error }]}>
            {connection.error || t('intro.saveError')}
          </Text>
        )}
      </ScrollView>
      <View style={s.actions} testID="connection-primary-action">
        <AppHost style={s.button} matchContents={{ vertical: true }}>
          <AppButton
            title={t(
              success ? 'intro.start' : connection.error ? 'pairing.retry' : 'pairing.connect'
            )}
            fullWidth
            size="large"
            colors={{ containerColor: c.accent, contentColor: c.onAccent }}
            disabled={pending}
            onPress={success ? onComplete : () => void connection.connect()}
          />
        </AppHost>
      </View>
    </SafeAreaView>
  );
}

const s = StyleSheet.create({
  root: { flex: 1 },
  header: { height: 56, paddingHorizontal: 20, alignItems: 'flex-end', justifyContent: 'center' },
  closeHost: { width: 48, height: 48 },
  content: { flexGrow: 1, justifyContent: 'center', alignItems: 'center', padding: 24, gap: 16 },
  art: { width: '100%', maxWidth: 280, height: 180 },
  title: { fontSize: 26, lineHeight: 34, fontWeight: '700', textAlign: 'center' },
  name: { fontSize: 20, lineHeight: 28, fontWeight: '600', textAlign: 'center' },
  body: { fontSize: 15, lineHeight: 23, textAlign: 'center' },
  secondaryActions: { flexDirection: 'row', width: '100%', maxWidth: 320, gap: 12 },
  secondaryButton: { flex: 1, minHeight: 48 },
  details: { width: '100%', gap: 10 },
  address: { fontSize: 14, lineHeight: 22 },
  status: { alignItems: 'center', gap: 8 },
  error: { fontSize: 15, lineHeight: 23, textAlign: 'center' },
  actions: {
    width: '100%',
    maxWidth: 440,
    alignSelf: 'center',
    paddingHorizontal: 24,
    paddingTop: 12,
    paddingBottom: 12,
  },
  button: { width: '100%', minHeight: 48 },
});
