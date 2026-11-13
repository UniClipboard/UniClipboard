import type Ionicons from '@expo/vector-icons/Ionicons';
import type { Color, ColorScheme } from '@/theme/colors.types';
import type { DisplayKind } from '@/utils/displayKind';

/** 内容类型的 Ionicons 图标(Android 详情页类型 chip、历史列表行共用) */
export const HISTORY_KIND_ICON: Record<DisplayKind, keyof typeof Ionicons.glyphMap> = {
  text: 'text-outline',
  url: 'link-outline',
  image: 'image-outline',
  file: 'document-outline',
  group: 'albums-outline',
};

/** 历史列表行前导图标的容器色 / 前景色:按内容类型取 M3 语义容器色,不写死色值 */
export function getHistoryKindTone(
  kind: DisplayKind,
  colors: ColorScheme
): { container: Color; content: Color } {
  switch (kind) {
    case 'url':
      return { container: colors.infoContainer, content: colors.onInfoContainer };
    case 'image':
      return { container: colors.successContainer, content: colors.onSuccessContainer };
    case 'file':
      return { container: colors.warningContainer, content: colors.onWarningContainer };
    case 'group':
      return { container: colors.surfaceHighest, content: colors.textSecondary };
    case 'text':
    default:
      return { container: colors.accentContainer, content: colors.onAccentContainer };
  }
}
