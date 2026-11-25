/**
 * 存储二级页(Android) — 对齐原仓库 UniClip StorageSection.tsx M3 Expressive 设计：
 * 顶部 28dp 概览卡片（缓存占用总量 + 12dp 比例条 + 图例）
 * → 缓存配额与清理（含确认弹窗）→ 文件大小上限 → 从 v1 迁移历史记录。
 */
import { memo, useCallback, useState } from 'react';
import { Alert } from 'react-native';
import { useFocusEffect } from '@react-navigation/native';
import {
  AlertDialog,
  Box,
  Column,
  Row,
  Spacer,
  Text as ComposeText,
  TextButton,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  background,
  clip,
  fillMaxHeight,
  fillMaxWidth,
  height as heightModifier,
  Shapes,
  size,
  weight,
} from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { useEngineStore } from '@/engine/store';
import { useMessageStore } from '@/stores/messageStore';
import { SettingsSectionItem } from '../SettingsSectionItem';
import { SettingsHeroCard } from './SettingsHeroCard';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSelectRow } from './SettingsSelectRow';

const ICONS = {
  migrate: require('../../../assets/icons/file_download.xml'),
};

const TOTAL_STYLE = { typography: 'headlineMedium' } as const;
const LEGEND_STYLE = { typography: 'labelMedium' } as const;
const BAR_HEIGHT = 12;
const BAR_GAP = 3;

const MB = 1024 * 1024;
const GB = 1024 * MB;

const CACHE_CAP_OPTIONS = [50 * MB, 200 * MB, 500 * MB, 1 * GB, 2 * GB];
const MAX_FILE_OPTIONS = [10 * MB, 50 * MB, 100 * MB, 250 * MB, 500 * MB, 1 * GB];
const MAX_TOTAL_OPTIONS = [500 * MB, 1 * GB, 2 * GB, 4 * GB, 8 * GB, 16 * GB];

function withCurrentBytes(presets: number[], current: number): number[] {
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b);
}

function formatCleanBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const idx = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, idx);
  const formatted = Number.isInteger(val) ? String(val) : String(parseFloat(val.toFixed(1)));
  return `${formatted} ${units[idx]}`;
}

function StorageOverviewCard({
  storedBytes,
  reservedBytes,
  maxBytes,
}: {
  storedBytes: number | null;
  reservedBytes: number | null;
  maxBytes: number;
}) {
  const { t } = useTranslation();
  const colors = useMaterialColors();
  const loading = storedBytes === null || reservedBytes === null;
  const used = loading ? 0 : storedBytes + reservedBytes;
  const remaining = Math.max(0, maxBytes - used);

  const segments = loading
    ? []
    : [
        { key: 'stored', bytes: storedBytes, color: colors.primary },
        { key: 'reserved', bytes: reservedBytes, color: colors.tertiary },
        { key: 'free', bytes: remaining, color: colors.surfaceContainerHighest },
      ].filter((s) => s.bytes > 0);
  const totalBar = segments.reduce((sum, s) => sum + s.bytes, 0);

  return (
    <SettingsHeroCard>
      <Column>
        <ComposeText style={TOTAL_STYLE}>
          {loading ? t('common:state.loading') : formatCleanBytes(used)}
        </ComposeText>
        <ComposeText color={colors.onSurfaceVariant}>
          {`${t('settingsStorage:cache.usageLabel')} · ${t('settingsStorage:cache.capLabel')} ${formatCleanBytes(maxBytes)}`}
        </ComposeText>
      </Column>
      <Row
        horizontalArrangement={{ spacedBy: BAR_GAP }}
        modifiers={[
          fillMaxWidth(),
          heightModifier(BAR_HEIGHT),
          clip(Shapes.RoundedCorner(BAR_HEIGHT / 2)),
          ...(segments.length === 0 || used === 0
            ? [background(colors.surfaceContainerHighest)]
            : []),
        ]}
      >
        {used > 0 && totalBar > 0
          ? segments.map((segment) => (
              <Box
                key={segment.key}
                modifiers={[
                  weight(segment.bytes / totalBar),
                  fillMaxHeight(),
                  background(segment.color),
                ]}
              />
            ))
          : null}
      </Row>
      <Row horizontalArrangement={{ spacedBy: 16 }}>
        <Row verticalAlignment="center">
          <Box modifiers={[size(8, 8), clip(Shapes.Circle), background(colors.primary)]} />
          <Spacer modifiers={[size(6, 1)]} />
          <ComposeText color={colors.onSurfaceVariant} style={LEGEND_STYLE}>
            {t('settingsStorage:kind.cache')}
          </ComposeText>
        </Row>
        <Row verticalAlignment="center">
          <Box
            modifiers={[
              size(8, 8),
              clip(Shapes.Circle),
              background(colors.surfaceContainerHighest),
            ]}
          />
          <Spacer modifiers={[size(6, 1)]} />
          <ComposeText color={colors.onSurfaceVariant} style={LEGEND_STYLE}>
            {t('settingsStorage:cache.capLabel')}
          </ComposeText>
        </Row>
      </Row>
    </SettingsHeroCard>
  );
}

export const StorageSection = memo(function StorageSection() {
  const { t } = useTranslation();
  const state = useEngineStore(
    useShallow((s) => ({
      settings: s.snapshot?.settings.value,
      fileCache: s.fileCache,
      updateSettings: s.updateSettings,
      clearPreviews: s.clearPreviews,
      beginLaterMigration: s.beginLaterMigration,
    }))
  );
  const [purging, setPurging] = useState(false);
  const [showClearDialog, setShowClearDialog] = useState(false);

  useFocusEffect(
    useCallback(() => {
      const engine = useEngineStore.getState();
      void engine.refreshFileCache();
      const timer = setInterval(() => void engine.refreshFileCache(), 3000);
      return () => clearInterval(timer);
    }, [])
  );

  const run = useCallback(
    (promise: Promise<unknown>) => {
      void promise.catch((error) =>
        Alert.alert(
          t('settingsStorage:setFailed'),
          error instanceof Error ? error.message : undefined
        )
      );
    },
    [t]
  );

  const handleClearConfirm = async () => {
    setPurging(true);
    try {
      await state.clearPreviews();
      useMessageStore.getState().showMessage(t('settingsStorage:cache.cleared'), 'success');
    } catch (error) {
      Alert.alert(
        t('settingsStorage:cache.clearFailed'),
        error instanceof Error ? error.message : undefined
      );
    } finally {
      setPurging(false);
    }
  };

  const settings = state.settings;
  if (!settings) {
    return (
      <SettingsSectionItem variant="grouped">
        <SettingsListRow testID="engine-status" title={t('settings:live.loading')} />
      </SettingsSectionItem>
    );
  }

  const storedBytes = state.fileCache ? state.fileCache.stored_bytes : null;
  const reservedBytes = state.fileCache ? state.fileCache.reserved_bytes : null;
  const usedBytes = storedBytes !== null && reservedBytes !== null ? storedBytes + reservedBytes : null;
  const usageLabel = purging
    ? t('settingsStorage:cache.clearing')
    : usedBytes !== null
      ? formatCleanBytes(usedBytes)
      : '—';

  const cacheCapOptions = withCurrentBytes(CACHE_CAP_OPTIONS, settings.file_cache.max_bytes).map(
    (bytes) => ({ value: String(bytes), label: formatCleanBytes(bytes) })
  );
  const maxFileOptions = withCurrentBytes(MAX_FILE_OPTIONS, settings.limits.max_file_bytes).map(
    (bytes) => ({ value: String(bytes), label: formatCleanBytes(bytes) })
  );
  const maxTotalOptions = withCurrentBytes(MAX_TOTAL_OPTIONS, settings.limits.max_total_bytes).map(
    (bytes) => ({ value: String(bytes), label: formatCleanBytes(bytes) })
  );

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <StorageOverviewCard
        storedBytes={storedBytes}
        reservedBytes={reservedBytes}
        maxBytes={settings.file_cache.max_bytes}
      />

      <Spacer modifiers={[heightModifier(20)]} />

      <SettingsSectionItem
        variant="grouped"
        title={t('settingsStorage:cache.sectionHeader')}
        dialogs={
          showClearDialog ? (
            <AlertDialog onDismissRequest={() => setShowClearDialog(false)}>
              <AlertDialog.Title>
                <ComposeText>{t('settingsStorage:cache.clearDialogTitle')}</ComposeText>
              </AlertDialog.Title>
              <AlertDialog.Text>
                <ComposeText>{t('settingsStorage:cache.clearDialogMessage')}</ComposeText>
              </AlertDialog.Text>
              <AlertDialog.ConfirmButton>
                <TextButton
                  onClick={() => {
                    setShowClearDialog(false);
                    void handleClearConfirm();
                  }}
                >
                  <ComposeText>{t('common:action.confirm')}</ComposeText>
                </TextButton>
              </AlertDialog.ConfirmButton>
              <AlertDialog.DismissButton>
                <TextButton onClick={() => setShowClearDialog(false)}>
                  <ComposeText>{t('common:action.cancel')}</ComposeText>
                </TextButton>
              </AlertDialog.DismissButton>
            </AlertDialog>
          ) : undefined
        }
      >
        <SettingsSelectRow
          key="file-cache-limit"
          testID="file-cache-limit"
          title={t('settingsStorage:cache.capLabel')}
          selectedValue={String(settings.file_cache.max_bytes)}
          options={cacheCapOptions}
          onSelect={(value) =>
            run(
              state.updateSettings((old) => ({
                ...old,
                file_cache: { max_bytes: Number(value) },
              }))
            )
          }
        />
        <SettingsListRow
          key="file-cache-usage"
          testID="file-cache-usage"
          title={t('settingsStorage:cache.usageLabel')}
          description={usageLabel}
          trailing={{ action: t('settingsStorage:cleanUp') }}
          disabled={purging || (usedBytes ?? 0) === 0}
          onPress={() => setShowClearDialog(true)}
        />
        <SettingsListRow
          key="clear-previews"
          testID="clear-previews"
          title={t('settings:live.clearPreviews')}
          trailing={{ action: t('settingsStorage:cleanUp') }}
          disabled={purging}
          onPress={() => setShowClearDialog(true)}
        />
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsSelectRow
          key="max_file_bytes"
          testID="max_file_bytes"
          title={t('settings:live.max_file_bytes')}
          selectedValue={String(settings.limits.max_file_bytes)}
          options={maxFileOptions}
          onSelect={(value) =>
            run(
              state.updateSettings((old) => ({
                ...old,
                limits: { ...old.limits, max_file_bytes: Number(value) },
              }))
            )
          }
        />
        <SettingsSelectRow
          key="max_total_bytes"
          testID="max_total_bytes"
          title={t('settings:live.max_total_bytes')}
          selectedValue={String(settings.limits.max_total_bytes)}
          options={maxTotalOptions}
          onSelect={(value) =>
            run(
              state.updateSettings((old) => ({
                ...old,
                limits: { ...old.limits, max_total_bytes: Number(value) },
              }))
            )
          }
        />
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem variant="grouped">
        <SettingsListRow
          key="storage-migrate-v1"
          testID="storage-migrate-v1"
          icon={ICONS.migrate}
          title={t('settingsStorage:migration.title')}
          description={t('settingsStorage:migration.description')}
          trailing="chevron"
          onPress={() => run(state.beginLaterMigration())}
        />
      </SettingsSectionItem>
    </Column>
  );
});
