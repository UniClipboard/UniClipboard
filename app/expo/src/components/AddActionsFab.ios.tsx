import React, { useCallback, useEffect, useState } from 'react';
import { Pressable, StyleSheet, Text, useColorScheme, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import {
  Camera,
  Clipboard,
  FileText,
  Image as ImageIcon,
  Plus,
  RefreshCw,
} from 'lucide-react-native';
import Animated, {
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withSpring,
  withTiming,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import * as Haptics from 'expo-haptics';
import { BlurView } from 'expo-blur';
import { GlassContainer } from '@/components/ui';
import { hexToRgba, iosAccent, iosColors, iosKindTints } from '@/theme/iosDesignTokens';
import { FAB_SIZE, type AddActionsFabProps } from './AddActionsFab.types';

/**
 * 右下/左下角融合操作按钮。
 *
 * 注意:这里不使用 SwiftUI Menu —— 原生 Menu 的点击手势由系统接管,RN 拿不到回调,
 * 无法在展开瞬间给出触感反馈。改为与 Android 同构的自绘浮层(毛玻璃 + 连续圆角,
 * 行样式对齐 CardContextOverlay 的原生 UIMenu 质感),点击 FAB 立即 impact 震动;
 * 相机/相册入口依旧是原生 picker。
 */
export function AddActionsFab({
  open,
  onOpenChange,
  onTakePhoto,
  onPickImage,
  onPickFile,
  onUploadClipboard,
  onSync,
  anchor = 'end',
  horizontalInset = 16,
}: AddActionsFabProps) {
  const { t } = useTranslation('home');
  const insets = useSafeAreaInsets();
  const isDark = useColorScheme() === 'dark';
  const anchorEnd = anchor === 'end';
  // 贴右→菜单向左上展开;贴左→向右上展开(缩放锚点随之翻转)。
  const anchorStyle = anchorEnd ? { right: horizontalInset } : { left: horizontalInset };
  const [mounted, setMounted] = useState(open);
  const progress = useSharedValue(0);

  const unmount = useCallback(() => setMounted(false), []);

  useEffect(() => {
    if (open) {
      setMounted(true);
      progress.value = withSpring(1, { damping: 18, stiffness: 240, mass: 0.7 });
    } else if (mounted) {
      progress.value = withTiming(0, { duration: 150, easing: Easing.in(Easing.quad) }, (f) => {
        if (f) scheduleOnRN(unmount);
      });
    }
    // mounted 不入依赖:开→挂载,关→播完退场再卸载
  }, [open, progress, unmount]);

  const fabIconStyle = useAnimatedStyle(() => ({
    transform: [{ rotate: `${progress.value * 45}deg` }],
  }));
  const popStyle = useAnimatedStyle(() => ({
    opacity: progress.value,
    transform: [{ translateY: (1 - progress.value) * 12 }, { scale: 0.9 + progress.value * 0.1 }],
  }));
  const scrimStyle = useAnimatedStyle(() => ({ opacity: progress.value * 0.28 }));

  const toggleOpen = useCallback(() => {
    Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium).catch(() => {});
    onOpenChange(!open);
  }, [open, onOpenChange]);

  const runItem = useCallback(
    (fn: () => void) => {
      Haptics.selectionAsync().catch(() => {});
      onOpenChange(false);
      // 等自绘浮层播完收起动画再 present 相机或 picker。
      setTimeout(fn, 200);
    },
    [onOpenChange]
  );

  const rows: {
    key: string;
    icon: typeof Camera;
    color: string;
    label: string;
    onPress: () => void;
  }[] = [
    {
      key: 'photo',
      icon: Camera,
      color: iosKindTints.image,
      label: t('fab.takePhoto'),
      onPress: onTakePhoto,
    },
    {
      key: 'image',
      icon: ImageIcon,
      color: iosKindTints.image,
      label: t('fab.pickImage'),
      onPress: onPickImage,
    },
    {
      key: 'file',
      icon: FileText,
      color: iosKindTints.file,
      label: t('fab.pickFile'),
      onPress: onPickFile,
    },
    {
      key: 'clip',
      icon: Clipboard,
      color: iosKindTints.text,
      label: t('fab.uploadClipboard'),
      onPress: onUploadClipboard,
    },
  ];

  const fabBottom = Math.max(0, insets.bottom - 4);
  const popBottom = fabBottom + FAB_SIZE + 12;

  return (
    <>
      {mounted && (
        <>
          <Animated.View
            style={[StyleSheet.absoluteFill, s.scrim, scrimStyle]}
            pointerEvents="none"
          />
          <Pressable
            style={[StyleSheet.absoluteFill, s.scrimTouch]}
            onPress={() => onOpenChange(false)}
            accessibilityRole="button"
            accessibilityLabel={t('a11y.closeMenu')}
          />
        </>
      )}

      <Pressable
        onPress={toggleOpen}
        style={[s.fab, anchorStyle, { bottom: fabBottom }]}
        accessibilityRole="button"
        accessibilityLabel={t('a11y.addContent')}
      >
        <GlassContainer shape="circle" interactive style={s.glass}>
          <Animated.View style={fabIconStyle}>
            <Plus size={28} color={isDark ? iosAccent.dark : iosAccent.light} />
          </Animated.View>
        </GlassContainer>
      </Pressable>

      {mounted && (
        <Animated.View
          style={[
            s.pop,
            anchorStyle,
            {
              bottom: popBottom,
              transformOrigin: anchorEnd ? 'bottom right' : 'bottom left',
            },
            popStyle,
          ]}
        >
          <BlurView intensity={90} tint="systemMaterial" style={StyleSheet.absoluteFill} />
          {rows.map((row, index) => (
            <React.Fragment key={row.key}>
              {index > 0 && (
                <View style={[s.rowSeparator, { backgroundColor: iosColors!.separator }]} />
              )}
              <Pressable
                onPress={() => runItem(row.onPress)}
                accessibilityRole="button"
                style={({ pressed }) => [
                  s.row,
                  pressed && { backgroundColor: iosColors!.tertiarySystemFill },
                ]}
              >
                <Text style={[s.rowLabel, { color: iosColors!.label }]}>{row.label}</Text>
                <row.icon size={19} color={row.color} />
              </Pressable>
            </React.Fragment>
          ))}
          <View style={[s.groupSeparator, { backgroundColor: iosColors!.tertiarySystemFill }]} />
          <Pressable
            onPress={() => runItem(onSync)}
            accessibilityRole="button"
            style={({ pressed }) => [
              s.row,
              pressed && { backgroundColor: iosColors!.tertiarySystemFill },
            ]}
          >
            <Text style={[s.rowLabel, { color: iosColors!.label }]}>{t('fab.syncNow')}</Text>
            <RefreshCw size={19} color={hexToRgba(iosKindTints.text, 0.7)} />
          </Pressable>
        </Animated.View>
      )}
    </>
  );
}

const s = StyleSheet.create({
  scrim: { backgroundColor: '#000000', zIndex: 15 },
  scrimTouch: { zIndex: 16 },
  fab: {
    position: 'absolute',
    width: FAB_SIZE,
    height: FAB_SIZE,
    zIndex: 20,
  },
  glass: {
    width: FAB_SIZE,
    height: FAB_SIZE,
    justifyContent: 'center',
    alignItems: 'center',
  },
  pop: {
    position: 'absolute',
    width: 220,
    borderRadius: 13,
    borderCurve: 'continuous',
    overflow: 'hidden',
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 6 },
    shadowOpacity: 0.15,
    shadowRadius: 18,
    zIndex: 21,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    height: 44,
    paddingHorizontal: 16,
  },
  rowSeparator: {
    height: StyleSheet.hairlineWidth,
    marginHorizontal: 12,
  },
  groupSeparator: {
    height: 7,
  },
  rowLabel: {
    fontSize: 16,
  },
});
