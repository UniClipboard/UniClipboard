import { memo } from 'react';
import { Alert } from 'react-native';
import { Column, Spacer } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, height as heightModifier } from '@expo/ui/jetpack-compose/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { useEngineStore } from '@/engine/store';
import { SettingsSectionItem } from '../SettingsSectionItem';
import type { AndroidSettingFieldEdit } from './SettingsFieldDialog';
import { SettingsListRow } from './SettingsListRow';
import { SettingsSelectRow } from './SettingsSelectRow';
import { SettingsSwitchRow } from './SettingsSwitchRow';

const MAX_ITEMS_OPTIONS = [0, 100, 200, 500, 1000, 2000, 5000, 10000];
const RETENTION_DAY_OPTIONS = [0, 7, 30, 90, 180, 365];

function sortedWithCurrent(presets: number[], current: number): number[] {
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b);
}

/** 历史记录二级页(Android) — M3 Expressive grouped 分组，保留天数与最大条数走下拉预设，与 iOS 对齐。 */
export const HistorySection = memo(function HistorySection({
  onEdit,
}: {
  onEdit: (edit: AndroidSettingFieldEdit) => void;
}) {
  const { t } = useTranslation();
  const state = useEngineStore(
    useShallow((s) => ({
      settings: s.snapshot?.settings.value,
      updateSettings: s.updateSettings,
    }))
  );
  const settings = state.settings;

  const run = (promise: Promise<unknown>) => {
    void promise.catch((error) =>
      Alert.alert(
        t('settingsStorage:setFailed'),
        error instanceof Error ? error.message : undefined
      )
    );
  };

  if (!settings) {
    return (
      <SettingsSectionItem variant="grouped">
        <SettingsListRow testID="engine-status" title={t('settings:live.loading')} />
      </SettingsSectionItem>
    );
  }

  const tags = settings.history.excluded_tags ?? [];
  const customTags = tags.filter((tagName) => tagName !== 'starred' && tagName !== 'pinned');
  const retentionOptions = sortedWithCurrent(
    RETENTION_DAY_OPTIONS,
    settings.history.retention_days
  ).map((days) => ({
    value: String(days),
    label:
      days === 0 ? t('settings:live.unlimited') : t('settings:live.daysValue', { count: days }),
  }));
  const maxItemsOptions = sortedWithCurrent(MAX_ITEMS_OPTIONS, settings.history.max_items).map(
    (count) => ({
      value: String(count),
      label:
        count === 0
          ? t('settings:live.unlimited')
          : t('settings:live.itemsValue', { count: count.toLocaleString() }),
    })
  );

  const toggleExcludedTag = (tagName: string, enabled: boolean) => {
    run(
      state.updateSettings((old) => {
        const excluded = new Set(old.history.excluded_tags ?? []);
        if (enabled) excluded.add(tagName);
        else excluded.delete(tagName);
        return { ...old, history: { ...old.history, excluded_tags: [...excluded] } };
      })
    );
  };

  return (
    <Column modifiers={[fillMaxWidth()]}>
      <SettingsSectionItem variant="grouped">
        <SettingsSwitchRow
          key="history-cleanup"
          testID="history-cleanup"
          title={t('settings:live.cleanup')}
          value={settings.history.auto_cleanup}
          onValueChange={(auto_cleanup) =>
            run(
              state.updateSettings((old) => ({
                ...old,
                history: { ...old.history, auto_cleanup },
              }))
            )
          }
        />
        <SettingsSelectRow
          key="retention-days"
          testID="retention-days"
          title={t('settings:live.retention')}
          selectedValue={String(settings.history.retention_days)}
          options={retentionOptions}
          onSelect={(value) =>
            run(
              state.updateSettings((old) => ({
                ...old,
                history: { ...old.history, retention_days: Number(value) },
              }))
            )
          }
        />
        <SettingsSelectRow
          key="history-max-items"
          testID="history-max-items"
          title={t('settingsStorage:history.maxItemsLabel')}
          selectedValue={String(settings.history.max_items)}
          options={maxItemsOptions}
          onSelect={(value) =>
            run(
              state.updateSettings((old) => ({
                ...old,
                history: { ...old.history, max_items: Number(value) },
              }))
            )
          }
        />
      </SettingsSectionItem>

      <Spacer modifiers={[heightModifier(16)]} />

      <SettingsSectionItem
        variant="grouped"
        title={t('settings:live.excludedTags')}
        footer={t('settings:live.excludedHint')}
      >
        <SettingsSwitchRow
          key="exclude-starred"
          testID="exclude-starred"
          title={t('settings:live.starred')}
          value={tags.includes('starred')}
          onValueChange={(enabled) => toggleExcludedTag('starred', enabled)}
        />
        <SettingsSwitchRow
          key="exclude-pinned"
          testID="exclude-pinned"
          title={t('settings:live.pinned')}
          value={tags.includes('pinned')}
          onValueChange={(enabled) => toggleExcludedTag('pinned', enabled)}
        />
        <SettingsListRow
          key="excluded-tags"
          testID="excluded-tags"
          title={t('settings:live.customTags')}
          trailing={
            customTags.length > 0 ? { text: customTags.join(', '), chevron: true } : 'chevron'
          }
          onPress={() =>
            onEdit({
              id: 'excluded-tags',
              title: t('settings:live.customTags'),
              value: tags.join(', '),
              save: (next) =>
                state.updateSettings((old) => ({
                  ...old,
                  history: {
                    ...old.history,
                    excluded_tags: next
                      .split(',')
                      .map((item) => item.trim())
                      .filter(Boolean),
                  },
                })),
            })
          }
        />
      </SettingsSectionItem>
    </Column>
  );
});
