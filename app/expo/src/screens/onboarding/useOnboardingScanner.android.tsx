import { useCallback, useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { AppBottomSheet, AppButton, AppHost } from '@/components/ui';
import { useTheme } from '@/hooks/useTheme';
import { PairingScanner } from '@/screens/PairingScanner';
import { showUnavailable } from '@/shell/notice';
import type { OnboardingScannerController } from './useOnboardingScanner.ios';

export function useOnboardingScanner({
  isLive,
  onRead,
  onFallbackToCode,
}: {
  isLive: boolean;
  onRead: (value: string) => void;
  onFallbackToCode: () => void;
}): OnboardingScannerController {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const c = theme.colors;
  const [visible, setVisible] = useState(false);

  const openScanner = useCallback(() => {
    if (!isLive) {
      showUnavailable();
      return;
    }
    setVisible(true);
  }, [isLive]);

  const renderScannerOverlay = () => (
    <AppBottomSheet visible={visible} onDismiss={() => setVisible(false)}>
      <View style={s.sheetBody}>
        <Text style={[s.title, { color: c.textPrimary }]}>
          {t('onboarding:guide.pair.scanAction')}
        </Text>
        <PairingScanner
          onRead={(value) => {
            setVisible(false);
            onRead(value);
          }}
        />
        <AppHost style={s.fallbackHost} matchContents={{ vertical: true }}>
          <AppButton
            title={t('onboarding:guide.pair.switchToCode')}
            variant="text"
            fullWidth
            colors={{ contentColor: c.accent }}
            onPress={() => {
              setVisible(false);
              onFallbackToCode();
            }}
          />
        </AppHost>
      </View>
    </AppBottomSheet>
  );

  return { openScanner, renderScannerOverlay };
}

const s = StyleSheet.create({
  sheetBody: { paddingHorizontal: 20, paddingBottom: 16, gap: 14 },
  title: { fontSize: 18, fontWeight: '700' },
  fallbackHost: { minHeight: 48 },
});
