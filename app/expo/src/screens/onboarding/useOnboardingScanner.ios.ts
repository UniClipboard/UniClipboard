import { useCallback, useRef } from 'react';
import { Alert, Linking } from 'react-native';
import { useTranslation } from 'react-i18next';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { showUnavailable } from '@/shell/notice';

export interface OnboardingScannerController {
  openScanner: () => void;
  renderScannerOverlay: () => React.ReactNode;
}

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
  const scanningRef = useRef(false);

  const openScanner = useCallback(() => {
    if (!isLive) {
      showUnavailable();
      return;
    }
    if (scanningRef.current) return;
    scanningRef.current = true;
    void unicEngineNative
      .scanQRCode(t('common:action.cancel'), t('settingsSync:lan.qr.hint'))
      .then((scanned) => {
        if (scanned) onRead(scanned);
      })
      .catch((error) => {
        const parsed = parseEngineError(error);
        if (parsed.code === 'permission_denied') {
          Alert.alert(
            t('settingsSync:lan.qr.permissionTitle'),
            t('settingsSync:pairing.cameraPermission'),
            [
              { text: t('onboarding:guide.pair.switchToCode'), onPress: onFallbackToCode },
              {
                text: t('settingsSync:pairing.openSettings'),
                onPress: () => void Linking.openSettings(),
              },
            ],
          );
        } else if (parsed.code === 'unsupported') {
          Alert.alert(t('settingsSync:pairing.actionFailed'), t('home:toast.cameraUnavailable'));
        } else {
          const msg = t(`errors:engine.${parsed.code}`, {
            defaultValue: t('errors:engine.internal'),
          });
          Alert.alert(t('settingsSync:pairing.actionFailed'), msg);
        }
      })
      .finally(() => {
        scanningRef.current = false;
      });
  }, [isLive, onRead, onFallbackToCode, t]);

  return {
    openScanner,
    renderScannerOverlay: () => null,
  };
}
