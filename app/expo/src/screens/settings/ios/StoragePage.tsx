import { useCallback, useEffect, useState } from 'react';
import { Alert } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Picker,
  Section,
  Spacer,
  Text as SwiftUIText,
} from '@expo/ui/swift-ui';
import {
  buttonStyle,
  disabled as disabledMod,
  foregroundStyle,
  frame,
  monospacedDigit,
  pickerStyle,
  tag,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { useMessageStore } from '@/stores/messageStore';
import { SettingsNavRow, settingsTileColors } from './common';

const MB = 1024 * 1024;
const GB = 1024 * MB;

const CACHE_CAP_OPTIONS = [50 * MB, 200 * MB, 500 * MB, 1 * GB, 2 * GB];
const MAX_FILE_OPTIONS = [10 * MB, 50 * MB, 100 * MB, 250 * MB, 500 * MB, 1 * GB];
const MAX_TOTAL_OPTIONS = [500 * MB, 1 * GB, 2 * GB, 4 * GB, 8 * GB, 16 * GB];

function withCurrentBytes(presets: number[], current: number): number[] {
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b);
}

/** 整数容量不保留多余 .0（如 500 MB、4 GB），非整数保留一位小数。 */
function formatCleanBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const idx = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, idx);
  const formatted = Number.isInteger(val) ? String(val) : String(parseFloat(val.toFixed(1)));
  return `${formatted} ${units[idx]}`;
}

/** 存储设置（统一左对齐二级页排版、整数容量去 .0、0 B 时禁用清除按钮） */
export function StoragePage() {
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

  useEffect(() => {
    const engine = useEngineStore.getState();
    void engine.refreshFileCache();
    const timer = setInterval(() => void engine.refreshFileCache(), 3000);
    return () => clearInterval(timer);
  }, []);

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

  const handleClearPreviews = useCallback(async () => {
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
  }, [state, t]);

  const settings = state.settings;
  if (!settings) {
    return (
      <IosSheetPage title={t('settingsStorage:title')}>
        <IosSheetForm>
          <Section>
            <SwiftUIText>{t('settings:live.loading')}</SwiftUIText>
          </Section>
        </IosSheetForm>
      </IosSheetPage>
    );
  }

  const usedBytes = state.fileCache
    ? state.fileCache.stored_bytes + state.fileCache.reserved_bytes
    : null;
  const cacheSizeLabel = purging
    ? t('settingsStorage:cache.clearing')
    : usedBytes !== null
      ? formatCleanBytes(usedBytes)
      : '—';

  const cacheCapOptions = withCurrentBytes(CACHE_CAP_OPTIONS, settings.file_cache.max_bytes);
  const maxFileOptions = withCurrentBytes(MAX_FILE_OPTIONS, settings.limits.max_file_bytes);
  const maxTotalOptions = withCurrentBytes(MAX_TOTAL_OPTIONS, settings.limits.max_total_bytes);

  return (
    <IosSheetPage title={t('settingsStorage:title')}>
      <IosSheetForm>
        <Section header={<SwiftUIText>{t('settingsStorage:cache.sectionHeader')}</SwiftUIText>}>
          <Picker
            testID="file-cache-limit"
            label={<SwiftUIText>{t('settingsStorage:cache.capLabel')}</SwiftUIText>}
            selection={settings.file_cache.max_bytes}
            onSelectionChange={(v) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  file_cache: { max_bytes: Number(v) },
                }))
              )
            }
            modifiers={[pickerStyle('menu')]}
          >
            {cacheCapOptions.map((bytes) => (
              <SwiftUIText key={bytes} modifiers={[tag(bytes)]}>
                {formatCleanBytes(bytes)}
              </SwiftUIText>
            ))}
          </Picker>

          <HStack testID="file-cache-usage" modifiers={[frame({ maxWidth: Infinity })]}>
            <SwiftUIText>{t('settingsStorage:cache.iosUsageLabel')}</SwiftUIText>
            <Spacer />
            <SwiftUIText modifiers={[foregroundStyle('secondary'), monospacedDigit()]}>
              {cacheSizeLabel}
            </SwiftUIText>
            <Spacer modifiers={[frame({ width: 8 })]} />
            <SwiftUIButton
              testID="clear-previews"
              label={t('common:action.clear')}
              onPress={() => void handleClearPreviews()}
              modifiers={[
                buttonStyle('borderless'),
                foregroundStyle('red'),
                disabledMod(purging || (usedBytes ?? 0) === 0),
              ]}
            />
          </HStack>
        </Section>

        <Section>
          <Picker
            testID="max_file_bytes"
            label={<SwiftUIText>{t('settings:live.max_file_bytes')}</SwiftUIText>}
            selection={settings.limits.max_file_bytes}
            onSelectionChange={(v) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  limits: { ...old.limits, max_file_bytes: Number(v) },
                }))
              )
            }
            modifiers={[pickerStyle('menu')]}
          >
            {maxFileOptions.map((bytes) => (
              <SwiftUIText key={bytes} modifiers={[tag(bytes)]}>
                {formatCleanBytes(bytes)}
              </SwiftUIText>
            ))}
          </Picker>
          <Picker
            testID="max_total_bytes"
            label={<SwiftUIText>{t('settings:live.max_total_bytes')}</SwiftUIText>}
            selection={settings.limits.max_total_bytes}
            onSelectionChange={(v) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  limits: { ...old.limits, max_total_bytes: Number(v) },
                }))
              )
            }
            modifiers={[pickerStyle('menu')]}
          >
            {maxTotalOptions.map((bytes) => (
              <SwiftUIText key={bytes} modifiers={[tag(bytes)]}>
                {formatCleanBytes(bytes)}
              </SwiftUIText>
            ))}
          </Picker>
        </Section>

        <Section>
          <SettingsNavRow
            testID="storage-migrate-v1"
            icon="arrow.down.circle.fill"
            iconColor={settingsTileColors.blue}
            title={t('settingsStorage:migration.title')}
            subtitle={t('settingsStorage:migration.description')}
            onPress={() => run(state.beginLaterMigration())}
          />
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
