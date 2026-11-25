import { Alert } from 'react-native';
import { Picker, Section, Text as SwiftUIText } from '@expo/ui/swift-ui';
import { pickerStyle, tag } from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';
import { useShallow } from 'zustand/react/shallow';

import { IosSheetForm, IosSheetPage } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { SettingsNavRow, SettingsToggle } from './common';
import type { IosSettingFieldEdit } from './types';

const MAX_ITEMS_OPTIONS = [0, 100, 200, 500, 1000, 2000, 5000, 10000];
const RETENTION_DAY_OPTIONS = [0, 7, 30, 90, 180, 365];

function sortedWithCurrent(presets: number[], current: number): number[] {
  return presets.includes(current) ? presets : [...presets, current].sort((a, b) => a - b);
}

/**
 * 历史记录（对齐 iOS 设置二级页统一左对齐排版；0 显示「不限」，非零带单位）。
 */
export function HistoryPage({ onEdit }: { onEdit: (edit: IosSettingFieldEdit) => void }) {
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
      <IosSheetPage title={t('settings:category.history')}>
        <IosSheetForm>
          <Section>
            <SwiftUIText>{t('settings:live.loading')}</SwiftUIText>
          </Section>
        </IosSheetForm>
      </IosSheetPage>
    );
  }

  const tags = settings.history.excluded_tags ?? [];
  const customTags = tags.filter((tagName) => tagName !== 'starred' && tagName !== 'pinned');
  const maxItemsOptions = sortedWithCurrent(MAX_ITEMS_OPTIONS, settings.history.max_items);
  const retentionOptions = sortedWithCurrent(RETENTION_DAY_OPTIONS, settings.history.retention_days);

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
    <IosSheetPage title={t('settings:category.history')}>
      <IosSheetForm>
        <Section>
          <SettingsToggle
            testID="history-cleanup"
            label={t('settings:live.cleanup')}
            isOn={settings.history.auto_cleanup}
            onIsOnChange={(auto_cleanup) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  history: { ...old.history, auto_cleanup },
                }))
              )
            }
          />
          <Picker
            testID="retention-days"
            selection={settings.history.retention_days}
            onSelectionChange={(value) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  history: { ...old.history, retention_days: Number(value) },
                }))
              )
            }
            modifiers={[pickerStyle('menu')]}
            label={<SwiftUIText>{t('settings:live.retention')}</SwiftUIText>}
          >
            {retentionOptions.map((days) => (
              <SwiftUIText key={days} modifiers={[tag(days)]}>
                {days === 0
                  ? t('settings:live.unlimited')
                  : t('settings:live.daysValue', { count: days })}
              </SwiftUIText>
            ))}
          </Picker>
          <Picker
            testID="history-max-items"
            selection={settings.history.max_items}
            onSelectionChange={(value) =>
              run(
                state.updateSettings((old) => ({
                  ...old,
                  history: { ...old.history, max_items: Number(value) },
                }))
              )
            }
            modifiers={[pickerStyle('menu')]}
            label={<SwiftUIText>{t('settingsStorage:history.maxItemsLabel')}</SwiftUIText>}
          >
            {maxItemsOptions.map((count) => (
              <SwiftUIText key={count} modifiers={[tag(count)]}>
                {count === 0
                  ? t('settings:live.unlimited')
                  : t('settings:live.itemsValue', { count: count.toLocaleString() })}
              </SwiftUIText>
            ))}
          </Picker>
        </Section>

        <Section
          header={<SwiftUIText>{t('settings:live.excludedTags')}</SwiftUIText>}
          footer={<SwiftUIText>{t('settings:live.excludedHint')}</SwiftUIText>}
        >
          <SettingsToggle
            testID="exclude-starred"
            label={t('settings:live.starred')}
            isOn={tags.includes('starred')}
            onIsOnChange={(enabled) => toggleExcludedTag('starred', enabled)}
          />
          <SettingsToggle
            testID="exclude-pinned"
            label={t('settings:live.pinned')}
            isOn={tags.includes('pinned')}
            onIsOnChange={(enabled) => toggleExcludedTag('pinned', enabled)}
          />
          <SettingsNavRow
            testID="excluded-tags"
            title={t('settings:live.customTags')}
            value={customTags.length > 0 ? customTags.join(', ') : undefined}
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
        </Section>
      </IosSheetForm>
    </IosSheetPage>
  );
}
