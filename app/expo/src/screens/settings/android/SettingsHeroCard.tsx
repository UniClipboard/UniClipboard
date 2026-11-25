/**
 * 设置页顶部的大圆角卡片(M3 Expressive):主开关、状态概览、应用信息等页面主角内容。
 *
 * `primary` 用 primaryContainer 强调「已开启 / 进行中」,`neutral` 用 surfaceContainer 与分组
 * 行同色。只提供容器(28dp 圆角 + 内边距 + 12dp 行距),内容由各页面自行组合;卡片内的
 * 设置行(如主开关 SettingsSwitchRow)经 SettingsRowColorsProvider 取卡片色,不会露出
 * ListItem 默认的 surface 底色。
 */
import { memo, type ReactNode } from 'react';
import { Column, Shape, Surface, useMaterialColors } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, paddingAll } from '@expo/ui/jetpack-compose/modifiers';

import { SettingsRowColorsProvider } from '../SettingsSectionItem';

export type SettingsHeroCardTone = 'primary' | 'neutral' | 'error';

const HERO_SHAPE = Shape.RoundedCorner({
  cornerRadii: { topStart: 28, topEnd: 28, bottomStart: 28, bottomEnd: 28 },
});

export const SettingsHeroCard = memo(function SettingsHeroCard({
  tone = 'neutral',
  horizontalAlignment,
  contentPadding = 20,
  children,
}: {
  tone?: SettingsHeroCardTone;
  horizontalAlignment?: 'start' | 'center' | 'end';
  /** 内容是自带内边距的 ListItem 时调小,避免双重留白。 */
  contentPadding?: number;
  children: ReactNode;
}) {
  const colors = useMaterialColors();
  const [container, content, supporting] =
    tone === 'error'
      ? [colors.errorContainer, colors.onErrorContainer, colors.onErrorContainer]
      : tone === 'primary'
        ? [colors.primaryContainer, colors.onPrimaryContainer, colors.onPrimaryContainer]
        : [colors.surfaceContainer, colors.onSurface, colors.onSurfaceVariant];

  return (
    <Surface
      color={container}
      contentColor={content}
      shape={HERO_SHAPE}
      modifiers={[fillMaxWidth()]}
    >
      <SettingsRowColorsProvider
        value={{
          containerColor: container,
          contentColor: content,
          supportingContentColor: supporting,
          trailingContentColor: supporting,
        }}
      >
        <Column
          modifiers={[fillMaxWidth(), paddingAll(contentPadding)]}
          verticalArrangement={{ spacedBy: 12 }}
          horizontalAlignment={horizontalAlignment}
        >
          {children}
        </Column>
      </SettingsRowColorsProvider>
    </Surface>
  );
});
