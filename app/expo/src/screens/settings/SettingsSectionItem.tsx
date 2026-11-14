/**
 * 设置分组容器:作为 LazyColumn 的单个 item。
 *
 * 顶层是单个 <Column>(ExpoComposeView),满足 @expo/ui LazyColumn「直接 child 必须是
 * Compose 组件」的硬约束(非 Compose 的 RN 节点会被原生侧静默跳过)。
 * 结构 = 分组标题 + 行容器(内含若干 ListItem 行,由调用方以 children 传入)。
 *
 * 两种行容器:
 * - `card`(默认):Material Card,行之间由调用方自行放 HorizontalDivider。
 * - `grouped`:M3 Expressive 分段列表——每行一块 surfaceContainer 色块,组首尾大圆角、
 *   组内小圆角,行间 2dp 缝隙代替分隔线。行需用 useSettingsSectionRowColors() 的容器色,
 *   否则 ListItem 默认的 surface 色会盖住色块。
 * - `plain`:不加容器,children 原样排布——分段按钮、概览卡片等自带外观的内容借用同一套
 *   分组标题与脚注样式。
 *
 * 颜色:标题用 M3 primary(Compose Text 在无 Surface 包裹时默认内容色是黑色,暗色
 * 模式下不可见,必须显式指定);Card 显式给 surface 容器色 + outlineVariant 边框,
 * 保证暗色下卡片边界与背景有对比。色板经 useMaterialColors() 读取所在 <Host> 的
 * 主题(跟随 Host 的 colorScheme)。
 */
import {
  Children,
  createContext,
  Fragment,
  isValidElement,
  memo,
  useContext,
  type ReactElement,
  type ReactNode,
} from 'react';
import {
  Card,
  Column,
  Shape,
  Spacer,
  Surface,
  Text as ComposeText,
  useMaterialColors,
  type ListItemColors,
} from '@expo/ui/jetpack-compose';
import {
  fillMaxWidth,
  height as heightModifier,
  padding,
} from '@expo/ui/jetpack-compose/modifiers';

export type SettingsSectionVariant = 'card' | 'grouped' | 'plain';

interface SettingsSectionItemProps {
  title?: string;
  children: ReactNode;
  footer?: string;
  variant?: SettingsSectionVariant;
  /**
   * 可选:该分组的弹窗(AlertDialog / ModalBottomSheet)。作为 item 内的 overlay 渲染——
   * Compose Dialog 是 window 级 overlay,不占列表布局,且弹窗打开时 item 必在视口(modal
   * 挡住背景、无法滚动),不会被 LazyColumn 回收,因此无需把弹窗状态外提到页面级。
   */
  dialogs?: ReactNode;
}

const CARD_TITLE_STYLE = { fontSize: 13, fontWeight: '600', letterSpacing: 0 } as const;
const GROUPED_TITLE_STYLE = { fontSize: 14, fontWeight: '500', letterSpacing: 0.1 } as const;
const FOOTER_STYLE = { fontSize: 12 } as const;
const GROUP_OUTER_RADIUS = 20;
const GROUP_INNER_RADIUS = 4;
const GROUP_ROW_GAP = 2;

const RowColorsContext = createContext<ListItemColors | undefined>(undefined);

/** 让非分组容器(如 SettingsHeroCard)内的设置行同样取用所在色块的颜色。 */
export const SettingsRowColorsProvider = RowColorsContext.Provider;

/** 分组内 ListItem 的容器色:grouped 下与色块一致,card 下沿用 ListItem 默认。 */
export function useSettingsSectionRowColors(): ListItemColors | undefined {
  return useContext(RowColorsContext);
}

/** 展开 Fragment 并丢弃 null / false,得到逐行元素,供 grouped 给每行分配圆角。 */
function flattenRows(children: ReactNode): ReactElement[] {
  const rows: ReactElement[] = [];
  Children.forEach(children, (child) => {
    if (!isValidElement(child)) return;
    if (child.type === Fragment) {
      rows.push(...flattenRows((child.props as { children?: ReactNode }).children));
    } else {
      rows.push(child);
    }
  });
  return rows;
}

function groupedRowShape(index: number, count: number) {
  const top = index === 0 ? GROUP_OUTER_RADIUS : GROUP_INNER_RADIUS;
  const bottom = index === count - 1 ? GROUP_OUTER_RADIUS : GROUP_INNER_RADIUS;
  return Shape.RoundedCorner({
    cornerRadii: { topStart: top, topEnd: top, bottomStart: bottom, bottomEnd: bottom },
  });
}

export const SettingsSectionItem = memo(function SettingsSectionItem({
  title,
  children,
  footer,
  variant = 'card',
  dialogs,
}: SettingsSectionItemProps) {
  const colors = useMaterialColors();
  // plain 与 grouped 共用 M3 Expressive 的标题 / 脚注样式
  const expressive = variant !== 'card';
  const rows = variant === 'grouped' ? flattenRows(children) : [];

  return (
    <Column modifiers={[fillMaxWidth()]}>
      {title ? (
        <>
          <ComposeText
            color={colors.primary}
            style={expressive ? GROUPED_TITLE_STYLE : CARD_TITLE_STYLE}
            modifiers={expressive ? [padding(4, 0, 4, 0)] : undefined}
          >
            {title}
          </ComposeText>
          <Spacer modifiers={[heightModifier(8)]} />
        </>
      ) : null}
      {variant === 'plain' ? (
        <Column modifiers={[fillMaxWidth()]}>{children}</Column>
      ) : variant === 'grouped' ? (
        <RowColorsContext.Provider value={{ containerColor: colors.surfaceContainer }}>
          <Column modifiers={[fillMaxWidth()]}>
            {rows.map((row, index) => (
              <Column key={row.key ?? index} modifiers={[fillMaxWidth()]}>
                {index > 0 ? <Spacer modifiers={[heightModifier(GROUP_ROW_GAP)]} /> : null}
                {/* Surface 按形状裁剪内容,行的点击波纹不会溢出圆角 */}
                <Surface
                  color={colors.surfaceContainer}
                  shape={groupedRowShape(index, rows.length)}
                  modifiers={[fillMaxWidth()]}
                >
                  {row}
                </Surface>
              </Column>
            ))}
          </Column>
        </RowColorsContext.Provider>
      ) : (
        // containerColor 与 ListItem 默认容器色(surface)一致,避免行与卡片色不一致的拼块感
        <Card
          colors={{ containerColor: colors.surface }}
          border={{ width: 1, color: colors.outlineVariant }}
        >
          <Column modifiers={[fillMaxWidth()]}>{children}</Column>
        </Card>
      )}
      {footer ? (
        <>
          <Spacer modifiers={[heightModifier(expressive ? 8 : 6)]} />
          <ComposeText
            color={colors.onSurfaceVariant}
            style={FOOTER_STYLE}
            modifiers={expressive ? [padding(16, 0, 16, 0)] : undefined}
          >
            {footer}
          </ComposeText>
        </>
      ) : null}
      {dialogs}
    </Column>
  );
});
