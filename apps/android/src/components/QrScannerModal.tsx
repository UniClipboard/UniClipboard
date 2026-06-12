/**
 * 二维码扫码 Modal
 *
 * 用途：扫 uniclipboard://connect 协议二维码 → 解析 → 写入 pendingConnectStore → 关闭。
 *
 * 安全：本组件绝不 log QR 原文 / 解析结果 / 任何凭据；仅记录成败和错误码。
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  View,
  Text,
  StyleSheet,
  Modal,
  TouchableOpacity,
  StatusBar,
  Linking,
  ActivityIndicator,
  Platform,
  Alert,
} from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { CameraView, useCameraPermissions, type BarcodeScanningResult } from 'expo-camera';
import { useTheme } from '@/hooks/useTheme';
import { spacing, radius, typography } from '@/theme';
import {
  parseConnectUri,
  CONNECT_URI_ERROR_MESSAGES,
  type ConnectUriError,
} from '@/utils/connectUri';
import { usePendingConnectStore } from '@/stores';

interface QrScannerModalProps {
  visible: boolean;
  onClose: () => void;
  /** 解析成功后调用（已自动 set 到 pendingConnectStore） */
  onScanned?: () => void;
}

export const QrScannerModal: React.FC<QrScannerModalProps> = ({ visible, onClose, onScanned }) => {
  const { theme } = useTheme();
  const [permission, requestPermission] = useCameraPermissions();
  const [torchOn, setTorchOn] = useState(false);
  const setPendingConnect = usePendingConnectStore((s) => s.set);

  // 防止 onBarcodeScanned 在同一帧多次触发
  const scanLockRef = useRef(false);

  useEffect(() => {
    if (visible) {
      scanLockRef.current = false;
      setTorchOn(false);
    }
  }, [visible]);

  // visible 打开时主动请求权限（仅当还未决定时）
  useEffect(() => {
    if (visible && permission && !permission.granted && permission.canAskAgain) {
      requestPermission();
    }
    // 只在 visible 跳变时请求；permission 自然刷新由 hook 处理
  }, [visible]);

  const handleBarcodeScanned = useCallback(
    (result: BarcodeScanningResult) => {
      if (scanLockRef.current) return;
      scanLockRef.current = true;

      const parsed = parseConnectUri(result.data ?? '');
      if (!parsed.ok) {
        const code: ConnectUriError = parsed.error;
        console.log(`[QR] scan failed: ${code}`);
        Alert.alert('扫码失败', CONNECT_URI_ERROR_MESSAGES[code], [
          {
            text: '重新扫描',
            onPress: () => {
              scanLockRef.current = false;
            },
          },
          {
            text: '关闭',
            style: 'cancel',
            onPress: onClose,
          },
        ]);
        return;
      }

      console.log('[QR] scan succeeded');
      setPendingConnect({
        url: parsed.value.url,
        user: parsed.value.user,
        pwd: parsed.value.pwd,
        ...(parsed.value.label !== undefined ? { label: parsed.value.label } : {}),
      });
      onScanned?.();
      onClose();
    },
    [onClose, onScanned, setPendingConnect]
  );

  const renderHeader = () => (
    <View style={styles.header}>
      <TouchableOpacity
        onPress={onClose}
        style={[styles.headerButton, { backgroundColor: 'rgba(0,0,0,0.4)' }]}
      >
        <Text style={styles.headerButtonText}>取消</Text>
      </TouchableOpacity>
      <Text style={styles.headerTitle}>扫描二维码</Text>
      <TouchableOpacity
        onPress={() => setTorchOn((v) => !v)}
        style={[styles.headerButton, { backgroundColor: 'rgba(0,0,0,0.4)' }]}
      >
        <Text style={styles.headerButtonText}>{torchOn ? '关灯' : '手电'}</Text>
      </TouchableOpacity>
    </View>
  );

  const renderPermissionPending = () => (
    <SafeAreaView
      style={[styles.fullCenter, { backgroundColor: theme.colors.background }]}
      edges={['top', 'bottom']}
    >
      <ActivityIndicator color={theme.colors.primary} />
      <Text style={[styles.dimText, { color: theme.colors.text, marginTop: spacing.md }]}>
        正在请求相机权限…
      </Text>
    </SafeAreaView>
  );

  const renderPermissionDenied = () => {
    const canAskAgain = permission?.canAskAgain ?? false;
    return (
      <SafeAreaView
        style={[styles.permissionPage, { backgroundColor: theme.colors.background }]}
        edges={['top', 'bottom']}
      >
        <View style={styles.permissionHeader}>
          <TouchableOpacity onPress={onClose} style={styles.permissionBackBtn}>
            <Text style={[styles.permissionBackText, { color: theme.colors.primary }]}>取消</Text>
          </TouchableOpacity>
        </View>
        <View style={styles.permissionBody}>
          <Text style={[styles.permissionTitle, { color: theme.colors.text }]}>需要相机权限</Text>
          <Text
            style={[
              styles.permissionDesc,
              { color: theme.colors.textSecondary ?? theme.colors.text },
            ]}
          >
            UniClip 需要访问相机来扫描接入二维码。
            {canAskAgain ? '' : '\n\n权限已被永久拒绝，请在系统设置中手动开启。'}
          </Text>
          <View style={styles.permissionActions}>
            {canAskAgain ? (
              <TouchableOpacity
                style={[styles.primaryBtn, { backgroundColor: theme.colors.primary }]}
                onPress={() => requestPermission()}
              >
                <Text style={[styles.primaryBtnText, { color: theme.colors.onPrimary }]}>
                  再次请求权限
                </Text>
              </TouchableOpacity>
            ) : (
              <TouchableOpacity
                style={[styles.primaryBtn, { backgroundColor: theme.colors.primary }]}
                onPress={() => Linking.openSettings()}
              >
                <Text style={[styles.primaryBtnText, { color: theme.colors.onPrimary }]}>
                  前往系统设置
                </Text>
              </TouchableOpacity>
            )}
            <TouchableOpacity
              style={[
                styles.secondaryBtn,
                { borderColor: theme.colors.outline ?? theme.colors.divider },
              ]}
              onPress={onClose}
            >
              <Text style={[styles.secondaryBtnText, { color: theme.colors.text }]}>
                改为手动填写
              </Text>
            </TouchableOpacity>
          </View>
        </View>
      </SafeAreaView>
    );
  };

  const renderScanner = () => (
    <View style={styles.scannerRoot}>
      <CameraView
        style={StyleSheet.absoluteFill}
        facing="back"
        enableTorch={torchOn}
        barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
        onBarcodeScanned={handleBarcodeScanned}
      />
      {/* 半透明遮罩 + 中心扫码框（用四块边框遮罩实现） */}
      <View style={styles.maskTop} pointerEvents="none" />
      <View style={styles.maskMiddleRow} pointerEvents="none">
        <View style={styles.maskSide} />
        <View style={styles.scanWindow}>
          <View style={[styles.corner, styles.cornerTopLeft]} />
          <View style={[styles.corner, styles.cornerTopRight]} />
          <View style={[styles.corner, styles.cornerBottomLeft]} />
          <View style={[styles.corner, styles.cornerBottomRight]} />
        </View>
        <View style={styles.maskSide} />
      </View>
      <View style={styles.maskBottom} pointerEvents="none">
        <Text style={styles.hintText}>将二维码对准框内</Text>
      </View>
      <SafeAreaView style={styles.headerSafeArea} edges={['top']}>
        {renderHeader()}
      </SafeAreaView>
    </View>
  );

  const renderBody = () => {
    if (!permission) return renderPermissionPending();
    if (!permission.granted) return renderPermissionDenied();
    return renderScanner();
  };

  return (
    <Modal
      visible={visible}
      animationType="slide"
      presentationStyle="fullScreen"
      onRequestClose={onClose}
      statusBarTranslucent
    >
      <StatusBar barStyle="light-content" backgroundColor="transparent" translucent />
      {renderBody()}
    </Modal>
  );
};

const SCAN_WINDOW_SIZE = 260;
const CORNER_LEN = 28;
const CORNER_WIDTH = 4;
const CORNER_COLOR = '#FFFFFF';
const MASK_COLOR = 'rgba(0,0,0,0.55)';

const styles = StyleSheet.create({
  scannerRoot: {
    flex: 1,
    backgroundColor: '#000',
  },
  headerSafeArea: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
  },
  header: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    paddingHorizontal: spacing.base,
    paddingTop: Platform.OS === 'android' ? spacing.md : 0,
    paddingBottom: spacing.md,
  },
  headerButton: {
    paddingVertical: spacing.sm,
    paddingHorizontal: spacing.md,
    borderRadius: radius.pill,
    minWidth: 60,
    alignItems: 'center',
  },
  headerButtonText: {
    color: '#FFFFFF',
    fontSize: typography.callout.fontSize,
    fontWeight: '500',
  },
  headerTitle: {
    color: '#FFFFFF',
    fontSize: typography.headline.fontSize,
    fontWeight: '600',
  },
  maskTop: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    height: '50%',
    backgroundColor: MASK_COLOR,
    transform: [{ translateY: -SCAN_WINDOW_SIZE / 2 }],
  },
  maskBottom: {
    position: 'absolute',
    bottom: 0,
    left: 0,
    right: 0,
    height: '50%',
    backgroundColor: MASK_COLOR,
    transform: [{ translateY: SCAN_WINDOW_SIZE / 2 }],
    alignItems: 'center',
    paddingTop: SCAN_WINDOW_SIZE / 2 + spacing.xl,
  },
  maskMiddleRow: {
    position: 'absolute',
    top: '50%',
    left: 0,
    right: 0,
    height: SCAN_WINDOW_SIZE,
    flexDirection: 'row',
    transform: [{ translateY: -SCAN_WINDOW_SIZE / 2 }],
  },
  maskSide: {
    flex: 1,
    backgroundColor: MASK_COLOR,
  },
  scanWindow: {
    width: SCAN_WINDOW_SIZE,
    height: SCAN_WINDOW_SIZE,
    position: 'relative',
  },
  corner: {
    position: 'absolute',
    width: CORNER_LEN,
    height: CORNER_LEN,
    borderColor: CORNER_COLOR,
  },
  cornerTopLeft: {
    top: 0,
    left: 0,
    borderTopWidth: CORNER_WIDTH,
    borderLeftWidth: CORNER_WIDTH,
    borderTopLeftRadius: 6,
  },
  cornerTopRight: {
    top: 0,
    right: 0,
    borderTopWidth: CORNER_WIDTH,
    borderRightWidth: CORNER_WIDTH,
    borderTopRightRadius: 6,
  },
  cornerBottomLeft: {
    bottom: 0,
    left: 0,
    borderBottomWidth: CORNER_WIDTH,
    borderLeftWidth: CORNER_WIDTH,
    borderBottomLeftRadius: 6,
  },
  cornerBottomRight: {
    bottom: 0,
    right: 0,
    borderBottomWidth: CORNER_WIDTH,
    borderRightWidth: CORNER_WIDTH,
    borderBottomRightRadius: 6,
  },
  hintText: {
    color: '#FFFFFF',
    fontSize: typography.subhead.fontSize,
    opacity: 0.9,
  },
  fullCenter: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
  },
  dimText: {
    fontSize: typography.subhead.fontSize,
  },
  permissionPage: {
    flex: 1,
  },
  permissionHeader: {
    flexDirection: 'row',
    paddingHorizontal: spacing.base,
    paddingVertical: spacing.md,
  },
  permissionBackBtn: {
    paddingVertical: spacing.sm,
    paddingHorizontal: spacing.xs,
  },
  permissionBackText: {
    fontSize: typography.headline.fontSize,
  },
  permissionBody: {
    flex: 1,
    paddingHorizontal: spacing.xl,
    justifyContent: 'center',
  },
  permissionTitle: {
    fontSize: typography.title3.fontSize,
    fontWeight: '700',
    marginBottom: spacing.md,
    textAlign: 'center',
  },
  permissionDesc: {
    fontSize: typography.callout.fontSize,
    lineHeight: typography.callout.fontSize * 1.5,
    textAlign: 'center',
    marginBottom: spacing.xl,
  },
  permissionActions: {
    gap: spacing.md,
  },
  primaryBtn: {
    paddingVertical: spacing.md,
    borderRadius: radius.pill,
    alignItems: 'center',
  },
  primaryBtnText: {
    fontSize: typography.callout.fontSize,
    fontWeight: '600',
  },
  secondaryBtn: {
    paddingVertical: spacing.md,
    borderRadius: radius.pill,
    borderWidth: 1,
    alignItems: 'center',
  },
  secondaryBtnText: {
    fontSize: typography.callout.fontSize,
    fontWeight: '500',
  },
});
