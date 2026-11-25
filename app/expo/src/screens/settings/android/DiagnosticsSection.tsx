/**
 * 诊断日志二级页(Android) — 对齐原仓库 UniClip LogSection.android.tsx M3 Expressive 设计：
 * 顶部 28dp 诊断日志导出卡片（图标 + 标题 + 脱敏说明 + 导出按钮）+ 遥测开关分组。
 * 保留 E2E 所需的 diagnostics-telemetry / diagnostics-export testID。
 */
import { memo, useEffect, useRef, useState } from 'react';
import { Alert } from 'react-native';
import {
  Column,
  FilledTonalButton,
  Icon,
  Row,
  Spacer,
  Text as ComposeText,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  fillMaxWidth,
  height as heightModifier,
  testID,
  weight,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';

import { type TelemetryState, unicEngineNative } from '@/engine/nativeModule';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsLeadingIcon } from './SettingsLeadingIcon';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const ICONS = {
  log: require('../../../assets/icons/description.xml'),
  share: require('../../../assets/icons/share.xml'),
};

const CARD_TITLE_STYLE = { typography: 'titleLarge' } as const;

export const DiagnosticsSection = memo(function DiagnosticsSection() {
  const { t } = useTranslation();
  const colors = useMaterialColors();
  const [state, setState] = useState<TelemetryState>();
  const [exporting, setExporting] = useState(false);
  const changing = useRef(false);

  useEffect(() => {
    let live = true;
    void unicEngineNative.getTelemetryState().then(
      (value) => {
        if (live) setState(value);
      },
      () => {
        if (live) Alert.alert(t('settings:mobileDiagnostics.failed'));
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
      Alert.alert(t('settings:mobileDiagnostics.failed'));
    } finally {
      changing.current = false;
    }
  };

  const exportLogs = async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const logs = await unicEngineNative.exportRecentDiagnosticLogs();
      if (logs) await unicEngineNative.shareHistoryMedia([], logs);
      else Alert.alert(t('settings:mobileDiagnostics.empty'));
    } catch {
      Alert.alert(t('settings:mobileDiagnostics.failed'));
    } finally {
      setExporting(false);
    }
  };

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsHeroCard>
        <Row verticalAlignment="center" modifiers={[fillMaxWidth()]}>
          <SettingsLeadingIcon source={ICONS.log} tone="primary" />
          <Spacer modifiers={[widthModifier(14)]} />
          <ComposeText style={CARD_TITLE_STYLE} modifiers={[weight(1)]}>
            {t('settingsAbout:log.title')}
          </ComposeText>
        </Row>
        <ComposeText color={colors.onSurfaceVariant}>
          {t('settings:live.sessionLogsFooter')}
        </ComposeText>
        <FilledTonalButton
          modifiers={[testID('diagnostics-export')]}
          enabled={!exporting}
          onClick={() => void exportLogs()}
        >
          <Icon source={ICONS.share} size={18} tint={colors.onSecondaryContainer} />
          <Spacer modifiers={[widthModifier(8)]} />
          <ComposeText>
            {exporting
              ? t('settingsIos:diagnostics.action.preparing')
              : t('settings:mobileDiagnostics.export')}
          </ComposeText>
        </FilledTonalButton>
      </SettingsHeroCard>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem
        variant="grouped"
        footer={
          state?.configured_enabled && !state.effective_enabled
            ? t('settings:mobileDiagnostics.unavailable')
            : t('settings:analytics.footer')
        }
      >
        {state ? (
          <SettingsSwitchRow
            key="diagnostics-telemetry"
            testID="diagnostics-telemetry"
            title={t('settings:mobileDiagnostics.telemetry')}
            description={t('settings:analytics.consentDescription')}
            value={state.configured_enabled}
            onValueChange={(enabled) => void change(enabled)}
          />
        ) : null}
      </SettingsSectionItem>
    </Column>
  );
});
