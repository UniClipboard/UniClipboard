/**
 * 二级设置页(Android) — 对齐原仓库 UniClip SettingsSubScreen.android.tsx：
 * 参数化单容器，单个 <Host seedColor={MATERIAL_SEED_COLOR}> + <LazyColumn>，
 * 各二级页渲染独立的 M3 Expressive Section 组件；在 unavailable/demo 模式下回落到 DemoSettings。
 */
import { useState } from 'react';
import { StyleSheet } from 'react-native';
import { useNavigation, useRoute, type RouteProp } from '@react-navigation/native';
import { Host, LazyColumn } from '@expo/ui/jetpack-compose';
import { fillMaxSize } from '@expo/ui/jetpack-compose/modifiers';
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';

import { MessageToast } from '@/components/MessageToast.android';
import { useFloatingNavigationClearance } from '@/components/android/floatingNavigationClearance';
import { useEngineStore } from '@/engine/store';
import { useTheme } from '@/hooks/useTheme';
import type { RootStackParamList } from '@/navigation/AppNavigator.types';
import { useMessageStore } from '@/stores/messageStore';
import { MATERIAL_SEED_COLOR } from '@/theme/colors';
import { ClipboardAccessMethodSheetProvider } from './ClipboardAccessMethodSheet';
import { SettingsScreenContent } from './SettingsScreenContent';
import { AboutSection } from './android/AboutSection';
import { BackgroundSection } from './android/BackgroundSection';
import { ClipboardAccessSection } from './android/ClipboardAccessSection';
import { DiagnosticsSection } from './android/DiagnosticsSection';
import { HistorySection } from './android/HistorySection';
import { NetworkSection } from './android/NetworkSection';
import { PrivacySection } from './android/PrivacySection';
import {
  SettingsFieldDialog,
  type AndroidSettingFieldEdit,
} from './android/SettingsFieldDialog';
import { StorageSection } from './android/StorageSection';
import { SyncSection } from './android/SyncSection';

export function SettingsSubScreen() {
  const navigation = useNavigation();
  const { params } = useRoute<RouteProp<RootStackParamList, 'SettingsSub'>>();
  const mode = useEngineStore((s) => s.mode);
  const { theme } = useTheme();
  const insets = useSafeAreaInsets();
  const navClearance = useFloatingNavigationClearance();
  const [edit, setEdit] = useState<AndroidSettingFieldEdit | null>(null);
  const message = useMessageStore((s) => s.message);
  const clearMessage = useMessageStore((s) => s.clearMessage);

  if (mode === 'unavailable') {
    return <SettingsScreenContent page={params.section} onBack={() => navigation.goBack()} />;
  }

  const section = params.section;

  return (
    <ClipboardAccessMethodSheetProvider>
      <SafeAreaView
        style={[styles.container, { backgroundColor: theme.colors.background }]}
        edges={[]}
      >
        <Host
          style={styles.container}
          colorScheme={theme.isDark ? 'dark' : 'light'}
          seedColor={MATERIAL_SEED_COLOR}
        >
          <LazyColumn
            modifiers={[fillMaxSize()]}
            contentPadding={{
              start: 16,
              end: 16,
              top: 8,
              bottom: insets.bottom + navClearance + 40,
            }}
            verticalArrangement={{ spacedBy: 16 }}
          >
            {section === 'sync' ? <SyncSection onEdit={setEdit} /> : null}
            {section === 'history' ? <HistorySection onEdit={setEdit} /> : null}
            {section === 'storage' ? <StorageSection /> : null}
            {section === 'network' ? <NetworkSection onEdit={setEdit} /> : null}
            {section === 'background' ? <BackgroundSection /> : null}
            {section === 'clipboard' ? <ClipboardAccessSection /> : null}
            {section === 'diagnostics' ? <DiagnosticsSection /> : null}
            {section === 'privacy' ? <PrivacySection /> : null}
            {section === 'about' ? <AboutSection /> : null}
          </LazyColumn>
        </Host>

        <SettingsFieldDialog edit={edit} onClose={() => setEdit(null)} />
        <MessageToast message={message} onMessageShown={clearMessage} />
      </SafeAreaView>
    </ClipboardAccessMethodSheetProvider>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1 },
});
