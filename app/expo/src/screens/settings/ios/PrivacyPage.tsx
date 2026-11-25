import { useEffect, useRef, useState } from 'react';
import { Alert } from 'react-native';
import { Section, Text as SwiftUIText, VStack } from '@expo/ui/swift-ui';
import { font, foregroundStyle } from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { type TelemetryState, unicEngineNative } from '@/engine/nativeModule';
import { SettingsToggle } from './common';

/** 隐私页（无重复“隐私”分组头，统一左对齐二级页排版） */
export function PrivacyPage() {
  const { t } = useTranslation('settings');
  const [state, setState] = useState<TelemetryState>();
  const changing = useRef(false);

  useEffect(() => {
    let live = true;
    void unicEngineNative.getTelemetryState().then(
      (value) => {
        if (live) setState(value);
      },
      () => {
        if (live) Alert.alert(t('analytics.error'));
      }
    );
    return () => {
      live = false;
    };
  }, [t]);

  const change = async (enabled: boolean) => {
    if (changing.current) return;
    changing.current = true;
    try {
      setState(await unicEngineNative.setTelemetryEnabled(enabled));
    } catch {
      Alert.alert(t('analytics.error'));
    } finally {
      changing.current = false;
    }
  };

  return (
    <IosSheetPage title={t('category.privacy')}>
      <IosSheetForm>
        <Section
          footer={
            <SwiftUIText>
              {state?.configured_enabled && !state.effective_enabled
                ? `${t('mobileDiagnostics.unavailable')}\n${t('analytics.footer')}`
                : t('analytics.footer')}
            </SwiftUIText>
          }
        >
          {state ? (
            <SettingsToggle
              testID="privacy-telemetry"
              isOn={state.configured_enabled}
              onIsOnChange={(enabled) => void change(enabled)}
            >
              <VStack alignment="leading" spacing={2}>
                <SwiftUIText>{t('analytics.consentTitle')}</SwiftUIText>
                <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                  {t('analytics.consentDescription')}
                </SwiftUIText>
              </VStack>
            </SettingsToggle>
          ) : null}
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
