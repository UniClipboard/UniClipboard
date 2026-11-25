import { useCallback, useState } from 'react';
import { StyleSheet } from 'react-native';
import { Host, NavigationDestination, NavigationStack, ZStack } from '@expo/ui/swift-ui';
import { frame, tint } from '@expo/ui/swift-ui/modifiers';

import { IosPageChromeProvider, type IosPageChrome } from '@/components/ui';
import { useEngineStore } from '@/engine/store';
import { useHideTabBarOnSubPage } from '@/navigation/ios/useHideTabBarOnSubPage';
import { iosAccentColor } from '@/theme/iosDesignTokens';
import { SettingsScreenContent } from './settings/SettingsScreenContent';
import { AboutPage } from './settings/ios/AboutPage';
import { ClipboardAccessPage } from './settings/ios/ClipboardAccessPage';
import { ClipboardSettingsGuideSheet } from './settings/ios/ClipboardSettingsGuideSheet';
import { DiagnosticsPage } from './settings/ios/DiagnosticsPage';
import { HistoryPage } from './settings/ios/HistoryPage';
import { NetworkPage } from './settings/ios/NetworkPage';
import { PrivacyPage } from './settings/ios/PrivacyPage';
import { SettingFieldSheet } from './settings/ios/SettingFieldSheet';
import { SettingsRootPage } from './settings/ios/SettingsRootPage';
import { ShareFavoritesGuideSheet } from './settings/ios/ShareFavoritesGuideSheet';
import { SharePage } from './settings/ios/SharePage';
import { StoragePage } from './settings/ios/StoragePage';
import { SyncPage } from './settings/ios/SyncPage';
import type { IosSettingFieldEdit, IosSettingsPage } from './settings/ios/types';

const fillModifier = frame({ maxWidth: Infinity, maxHeight: Infinity });
const NAVIGATION_CHROME: IosPageChrome = { kind: 'navigation' };

/**
 * iOS「设置」标签页（对齐 v1 SettingsScreen.ios.tsx）。
 * 全屏 Host 内持有 SwiftUI NavigationStack，子页原生推入/侧滑返回并自动隐藏底部 Tab Bar；
 * 剪贴板授权图文引导、分享收藏引导与文本编辑 sheet 均作为导航栈兄弟节点由本稳定宿主持有。
 */
export function SettingsScreen() {
  const mode = useEngineStore((s) => s.mode);
  const [path, setPath] = useState<string[]>([]);
  const [clipboardGuideVisible, setClipboardGuideVisible] = useState(false);
  const [shareGuideVisible, setShareGuideVisible] = useState(false);
  const [fieldEdit, setFieldEdit] = useState<IosSettingFieldEdit | null>(null);
  useHideTabBarOnSubPage(path);

  const openSubPage = useCallback((page: IosSettingsPage) => {
    if (page === 'root') return;
    setPath((current) => [...current, page]);
  }, []);

  if (mode === 'unavailable') {
    return <SettingsScreenContent page="root" />;
  }

  return (
    <Host style={styles.host}>
      <ZStack modifiers={[fillModifier, ...(iosAccentColor ? [tint(iosAccentColor)] : [])]}>
        <IosPageChromeProvider value={NAVIGATION_CHROME}>
          <NavigationStack path={path} onPathChange={setPath}>
            <SettingsRootPage onNavigate={openSubPage} />
            <NavigationDestination value="sync">
              <SyncPage onEdit={setFieldEdit} />
            </NavigationDestination>
            <NavigationDestination value="history">
              <HistoryPage onEdit={setFieldEdit} />
            </NavigationDestination>
            <NavigationDestination value="storage">
              <StoragePage />
            </NavigationDestination>
            <NavigationDestination value="network">
              <NetworkPage onEdit={setFieldEdit} />
            </NavigationDestination>
            <NavigationDestination value="clipboard">
              <ClipboardAccessPage onOpenSettingsGuide={() => setClipboardGuideVisible(true)} />
            </NavigationDestination>
            <NavigationDestination value="share">
              <SharePage onOpenFavoritesGuide={() => setShareGuideVisible(true)} />
            </NavigationDestination>
            <NavigationDestination value="diagnostics">
              <DiagnosticsPage />
            </NavigationDestination>
            <NavigationDestination value="privacy">
              <PrivacyPage />
            </NavigationDestination>
            <NavigationDestination value="about">
              <AboutPage />
            </NavigationDestination>
          </NavigationStack>
        </IosPageChromeProvider>

        <ClipboardSettingsGuideSheet
          visible={clipboardGuideVisible}
          onClose={() => setClipboardGuideVisible(false)}
        />
        <ShareFavoritesGuideSheet
          visible={shareGuideVisible}
          onClose={() => setShareGuideVisible(false)}
        />
        <SettingFieldSheet edit={fieldEdit} onClose={() => setFieldEdit(null)} />
      </ZStack>
    </Host>
  );
}

const styles = StyleSheet.create({
  host: { flex: 1 },
});
