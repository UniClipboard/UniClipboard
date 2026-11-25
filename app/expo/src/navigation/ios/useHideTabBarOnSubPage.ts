import { useLayoutEffect } from 'react';
import { useNavigation } from '@react-navigation/native';

/**
 * iOS 标签页内的 SwiftUI NavigationStack 推入二级页面时隐藏底部原生标签栏,回到根页再显示。
 * 标签栏只属于各标签的根页,二级页面全屏展示自己的内容。
 */
export function useHideTabBarOnSubPage(path: readonly string[]) {
  const navigation = useNavigation();
  const onSubPage = path.length > 0;

  useLayoutEffect(() => {
    navigation.setOptions({ tabBarStyle: { display: onSubPage ? 'none' : 'flex' } });
  }, [navigation, onSubPage]);
}
