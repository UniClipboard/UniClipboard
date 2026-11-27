/**
 * 诊断日志二级页(Android) — 对齐原仓库 UniClip LogSection.android.tsx M3 Expressive 设计：
 * 顶部 28dp 诊断包导出卡片（分享 / 保存为文件）+ 10 分钟详细诊断记录 + 遥测开关分组。
 * 保留 E2E 所需的 diagnostics-telemetry / diagnostics-export testID。
 */
import { memo, useEffect, useRef, useState } from 'react';
import { Alert } from 'react-native';
import {
  AlertDialog,
  Column,
  FilledTonalButton,
  Icon,
  Row,
  Spacer,
  Text as ComposeText,
  TextButton,
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
import { useMessageStore } from '@/stores/messageStore';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { mapDiagnosticWarningKeys, useDiagnosticCapture } from '../useDiagnosticCapture';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsLeadingIcon } from './SettingsLeadingIcon';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const ICONS = {
  log: require('../../../assets/icons/description.xml'),
  share: require('../../../assets/icons/share.xml'),
  record: require('../../../assets/icons/fiber_manual_record.xml'),
};

const CARD_TITLE_STYLE = { typography: 'titleLarge' } as const;

export const DiagnosticsSection = memo(function DiagnosticsSection() {
  const { t } = useTranslation();
  const colors = useMaterialColors();
  const showMessage = useMessageStore((s) => s.showMessage);
  const [state, setState] = useState<TelemetryState>();
  const [exporting, setExporting] = useState(false);
  const [showExportDialog, setShowExportDialog] = useState(false);
  const changing = useRef(false);
  const capture = useDiagnosticCapture();

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

  const runArchiveExport = async (action: 'share' | 'save') => {
    setShowExportDialog(false);
    if (exporting) return;
    setExporting(true);
    try {
      const result = await unicEngineNative.exportDiagnosticArchive(action);
      const warningKeys = mapDiagnosticWarningKeys(result.warnings);
      if (action === 'save') {
        if (!result.saved) {
          showMessage(t('settingsAbout:log.exportCanceled'), 'info');
          return;
        }
        if (warningKeys.length > 0) {
          showMessage(
            warningKeys.map((key) => t(`settingsAbout:log.warning.${key}`)).join('\n'),
            'error'
          );
        } else {
          showMessage(t('settingsAbout:log.exported'), 'success');
        }
      } else if (warningKeys.length > 0) {
        showMessage(
          warningKeys.map((key) => t(`settingsAbout:log.warning.${key}`)).join('\n'),
          'error'
        );
      }
    } catch {
      showMessage(
        action === 'save'
          ? t('settingsAbout:log.exportFailed')
          : t('settingsAbout:log.shareFailed'),
        'error'
      );
    } finally {
      setExporting(false);
    }
  };

  const captureDescription = capture.failed
    ? t('settingsAbout:log.capture.failed')
    : !capture.available
      ? t('settingsAbout:log.capture.unavailable')
      : capture.active
        ? t('settingsAbout:log.capture.remaining', { count: capture.remainingMinutes })
        : undefined;

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
          onClick={() => setShowExportDialog(true)}
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

      {showExportDialog ? (
        <AlertDialog onDismissRequest={() => setShowExportDialog(false)}>
          <AlertDialog.Title>
            <ComposeText>{t('settingsAbout:log.exportLabel')}</ComposeText>
          </AlertDialog.Title>
          <AlertDialog.Text>
            <ComposeText>{t('settingsAbout:log.exportMethodPrompt')}</ComposeText>
          </AlertDialog.Text>
          <AlertDialog.ConfirmButton>
            <TextButton onClick={() => void runArchiveExport('share')}>
              <ComposeText>{t('common:action.share')}</ComposeText>
            </TextButton>
          </AlertDialog.ConfirmButton>
          <AlertDialog.DismissButton>
            <TextButton onClick={() => void runArchiveExport('save')}>
              <ComposeText>{t('settingsAbout:log.exportFile')}</ComposeText>
            </TextButton>
          </AlertDialog.DismissButton>
        </AlertDialog>
      ) : null}

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem
        variant="grouped"
        title={t('settingsAbout:log.capture.title')}
        footer={t('settingsAbout:log.capture.description')}
      >
        <SettingsListRow
          key="diagnostics-capture"
          testID="diagnostics-capture"
          icon={ICONS.record}
          title={
            capture.active
              ? t('settingsAbout:log.capture.stop')
              : t('settingsAbout:log.capture.start')
          }
          description={captureDescription}
          disabled={!capture.available || capture.busy}
          onPress={() => void capture.toggle()}
        />
      </SettingsSectionItem>

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
