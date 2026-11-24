import { Keyboard, ScrollView, StyleSheet, Text, View, type ColorValue } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import {
  AppAlertDialog,
  AppButton,
  AppHost,
  AppProgressIndicator,
  AppTextField,
  GlassContainer,
} from "@/components/ui";
import { UnicBrandLogo } from "@/components/UnicBrandLogo";
import { useTheme } from "@/hooks/useTheme";
import { useSetupFlowController } from "./useSetupFlowController";

function MetricPill({
  label,
  value,
  tintColor,
  textColor,
  bgColor,
}: {
  label: string;
  value: string;
  tintColor: ColorValue;
  textColor: ColorValue;
  bgColor: ColorValue;
}) {
  return (
    <View style={[styles.metricBox, { backgroundColor: bgColor }]}>
      <Text style={[styles.metricValue, { color: tintColor }]}>{value}</Text>
      <Text style={[styles.metricLabel, { color: textColor }]}>{label}</Text>
    </View>
  );
}

export function MigrationSetupScreen() {
  const { theme } = useTheme();
  const c = theme.colors;
  const ctrl = useSetupFlowController();
  const report = ctrl.migrationReport;

  const renderBody = () => {
    switch (ctrl.setupViewState) {
      case "loading":
        return (
          <GlassContainer shape="card" style={styles.card}>
            <View style={styles.centerRow}>
              <AppHost matchContents={{ vertical: true }}>
                <AppProgressIndicator variant="circular" color={c.accent} />
              </AppHost>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                正在准备本地存储…
              </Text>
            </View>
          </GlassContainer>
        );

      case "w-clean":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                全新空间已就绪
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                剪贴板历史、标签和设备信任列表全部保存在本机加密空间。若你曾将旧版 UniClipboard 数据拷贝至“文件”目录，也可手动导入。
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-start-clean"
                  title="开始使用"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleEnterHistory()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-choose-custom-folder"
                  title="手动选择旧版目录"
                  variant="outlined"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
            </View>
          </>
        );

      case "w-detected":
      case "w-custom":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <View style={[styles.badge, { backgroundColor: c.accentContainer }]}>
                <Text style={[styles.badgeText, { color: c.onAccentContainer }]}>
                  已检测到旧版数据
                </Text>
              </View>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                发现可迁移的旧版剪贴板数据
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                首次启动可将旧版文字、图片、文件和标签离线迁移到当前设备。系统不会自动写入当前剪贴板，也不会上传云端。
              </Text>
              {ctrl.detectedSourcePath ? (
                <View style={[styles.pathChip, { backgroundColor: c.surfaceHigh }]}>
                  <Text style={[styles.pathText, { color: c.textSecondary }]} numberOfLines={2}>
                    {ctrl.detectedSourcePath}
                  </Text>
                </View>
              ) : null}
            </GlassContainer>

            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-start-migration"
                  title="开始迁移"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleStartAutoMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-change-source"
                  title="选择其他旧版目录"
                  variant="outlined"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-migration"
                  title={ctrl.setupEntryMode === "later" ? "暂不迁移，返回" : "暂不迁移"}
                  variant="text"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={ctrl.requestSkip}
                />
              </AppHost>
            </View>
          </>
        );

      case "w-invalid":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text style={[styles.cardTitle, { color: c.error }]}>
                目录中未找到有效的旧版数据
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                所选目录缺少完整的旧版加密库或密钥文件（vault/keyslot.json）。请重新选择包含旧版数据的文件夹，或暂不迁移直接开始使用。
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-reselect-folder"
                  title="重新选择旧版目录"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-from-invalid"
                  title={ctrl.setupEntryMode === "later" ? "暂不迁移，返回" : "暂不迁移"}
                  variant="outlined"
                  size="large"
                  fullWidth
                  onPress={ctrl.requestSkip}
                />
              </AppHost>
            </View>
          </>
        );

      case "m-auth":
      case "m-auth-err":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                输入旧版加密口令以解锁
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                当前设备钥匙串中未读取到旧版密钥。口令仅用于本次本地解密，不会被保存或上传。
              </Text>
              <View style={styles.inputWrap}>
                <AppTextField
                  testID="setup-passphrase-input"
                  value={ctrl.passphrase}
                  onChangeText={ctrl.setPassphrase}
                  placeholder="请输入旧版加密口令"
                  label="旧版加密口令"
                  secure
                  fullWidth
                  disabled={ctrl.busy}
                  returnKeyType="done"
                  onSubmitEditing={() => {
                    Keyboard.dismiss();
                    void ctrl.handleSubmitPassphrase();
                  }}
                />
              </View>
              {ctrl.setupViewState === "m-auth-err" && ctrl.migrationErrorMessage ? (
                <Text testID="setup-auth-error" style={[styles.errorText, { color: c.error }]}>
                  {ctrl.migrationErrorMessage}
                </Text>
              ) : null}
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-submit-passphrase"
                  title="确认并开始迁移"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy || !ctrl.passphrase.trim()}
                  onPress={() => {
                    Keyboard.dismiss();
                    void ctrl.handleSubmitPassphrase();
                  }}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-back-to-detected"
                  title="返回"
                  variant="outlined"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => ctrl.setSetupViewState("w-detected")}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-from-auth"
                  title="暂不迁移"
                  variant="text"
                  size="large"
                  fullWidth
                  onPress={ctrl.requestSkip}
                />
              </AppHost>
            </View>
          </>
        );

      case "m-importing":
      case "m-verifying":
      case "m-stopping": {
        const isVerifying = ctrl.setupViewState === "m-verifying";
        const isStopping = ctrl.setupViewState === "m-stopping";
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {isStopping
                  ? "正在停止迁移…"
                  : isVerifying
                    ? "正在校验导入结果"
                    : "正在导入历史记录"}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {isStopping
                  ? "已写入本地数据库的记录会安全保留，下次可继续断点导入。"
                  : isVerifying
                    ? "正在核对导入条目、标签与颜色完整性，请保持应用在前台。"
                    : "正在离线解密并导入文字、图片、文件与标签，请保持屏幕常亮。"}
              </Text>
              <View style={styles.progressWrap}>
                <AppHost matchContents={{ vertical: true }}>
                  <AppProgressIndicator
                    variant="linear"
                    progress={ctrl.progressRatio}
                    color={c.accent}
                    fullWidth
                  />
                </AppHost>
                <Text style={[styles.counterText, { color: c.textSecondary }]}>
                  {ctrl.migrationProgress.total > 0
                    ? `${ctrl.migrationProgress.processed} / ${ctrl.migrationProgress.total}`
                    : `已处理 ${ctrl.migrationProgress.processed}`}
                </Text>
              </View>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-stop-migration"
                  title="停止迁移"
                  variant="outlined"
                  size="large"
                  fullWidth
                  disabled={isStopping}
                  onPress={() => void ctrl.handleCancelMigration()}
                />
              </AppHost>
            </View>
          </>
        );
      }

      case "r-done":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <View style={[styles.badge, { backgroundColor: c.accentContainer }]}>
                <Text style={[styles.badgeText, { color: c.onAccentContainer }]}>
                  已完整校验
                </Text>
              </View>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.textPrimary }]}>
                迁移完成
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                已将旧版历史记录导入本机，并完成结构与附件校验。
              </Text>
              {report ? (
                <View style={styles.metricsGrid}>
                  <MetricPill
                    label="本次新增导入"
                    value={String(report.imported)}
                    tintColor={c.accent}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label="此前已导入（跳过）"
                    value={String(report.skipped)}
                    tintColor={c.textPrimary}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label="校验通过"
                    value={String(report.verified)}
                    tintColor={c.accent}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label="标签保留"
                    value={String(report.tags)}
                    tintColor={c.textPrimary}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                </View>
              ) : null}
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-enter-history"
                  title="进入剪贴板历史"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleEnterHistory()}
                />
              </AppHost>
            </View>
          </>
        );

      case "r-partial":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.warning }]}>
                部分记录未能导入
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                已有记录已写入本地历史，但仍有少数条目或附件缺失/校验未通过。你可以重试未成功项，或先进入主界面稍后再试。
              </Text>
              {report ? (
                <View style={styles.metricsGrid}>
                  <MetricPill
                    label="本次新增导入"
                    value={String(report.imported)}
                    tintColor={c.accent}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label="跳过已导入"
                    value={String(report.skipped)}
                    tintColor={c.textPrimary}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label="失败条目"
                    value={String(report.failed + report.verification_failed)}
                    tintColor={c.error}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label="缺失附件"
                    value={String(report.missing_parts)}
                    tintColor={c.warning}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                </View>
              ) : null}
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-partial"
                  title="重试未成功项"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleRetryMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-enter-after-partial"
                  title="进入剪贴板历史"
                  variant="outlined"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleEnterHistory()}
                />
              </AppHost>
            </View>
          </>
        );

      case "r-cancelled":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.textPrimary }]}>
                已停止迁移
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                已写入的记录会保留在本地历史中（累计新增导入 {ctrl.cumulativeImported} 条），下次启动或点击继续迁移时会自动跳过它们。
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-resume-migration"
                  title="继续迁移"
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleRetryMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-after-cancel"
                  title="暂不迁移，直接进入"
                  variant="outlined"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleEnterHistory()}
                />
              </AppHost>
            </View>
          </>
        );

      case "r-conflict":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.warning }]}>
                检测到后台进程正在占用数据目录
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                为保证数据库写入一致性，离线迁移需要独占目标目录。请等待后台同步任务结束后重试。
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-conflict"
                  title="重新检查并继续"
                  variant="filled"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleRetryMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-conflict"
                  title="暂不迁移"
                  variant="outlined"
                  size="large"
                  fullWidth
                  onPress={ctrl.requestSkip}
                />
              </AppHost>
            </View>
          </>
        );

      case "r-failed":
      default:
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.error }]}>
                无法完成迁移
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {ctrl.migrationErrorMessage || "迁移过程中发生错误，请检查目录权限或重试。"}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-failed"
                  title="重试迁移"
                  variant="filled"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleRetryMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-reselect-failed"
                  title="选择其他目录"
                  variant="outlined"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-failed"
                  title="暂不迁移，直接进入"
                  variant="text"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleEnterHistory()}
                />
              </AppHost>
            </View>
          </>
        );
    }
  };

  const isAuthView = ctrl.setupViewState === "m-auth" || ctrl.setupViewState === "m-auth-err";

  return (
    <SafeAreaView style={[styles.root, { backgroundColor: c.background }]} edges={["top", "bottom"]}>
      <ScrollView
        contentContainerStyle={isAuthView ? styles.scrollAuth : styles.scroll}
        keyboardShouldPersistTaps="handled"
        automaticallyAdjustKeyboardInsets
        showsVerticalScrollIndicator={false}
      >
        <View style={[styles.hero, isAuthView ? styles.heroCompact : null]}>
          <UnicBrandLogo size={isAuthView ? 56 : 80} borderRadius={isAuthView ? 14 : 20} style={styles.logo} />
          <Text testID="setup-welcome-title" style={[styles.title, { color: c.textPrimary }]}>
            欢迎使用 UniClipboard
          </Text>
          <Text testID="setup-slogan" style={[styles.slogan, { color: c.textSecondary }]}>
            让每一次复制更有用
          </Text>
        </View>

        {renderBody()}
      </ScrollView>

      <AppAlertDialog
        visible={ctrl.confirmSkipVisible}
        standalone
        title="暂不迁移旧版数据？"
        message="旧版数据不会被删除。之后你可以随时在“设置 › 存储”中重新启动离线迁移。"
        confirmLabel="暂不迁移，直接进入"
        onConfirm={() => void ctrl.confirmSkip()}
        dismissLabel="取消"
        onDismiss={ctrl.dismissSkip}
        onDismissAction={ctrl.dismissSkip}
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1 },
  scroll: {
    flexGrow: 1,
    paddingHorizontal: 20,
    paddingTop: 28,
    paddingBottom: 32,
    gap: 20,
    justifyContent: "space-between",
  },
  scrollAuth: {
    flexGrow: 1,
    paddingHorizontal: 20,
    paddingTop: 12,
    paddingBottom: 24,
    gap: 12,
    justifyContent: "flex-start",
  },
  hero: {
    alignItems: "center",
    gap: 10,
    paddingVertical: 12,
  },
  heroCompact: {
    gap: 6,
    paddingVertical: 4,
  },
  logo: {
    marginBottom: 4,
  },
  title: {
    fontSize: 28,
    fontWeight: "700",
    letterSpacing: -0.4,
    textAlign: "center",
  },
  slogan: {
    fontSize: 16,
    fontWeight: "500",
    textAlign: "center",
  },
  card: {
    padding: 18,
    gap: 10,
  },
  centerRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  badge: {
    alignSelf: "flex-start",
    paddingHorizontal: 10,
    paddingVertical: 4,
    borderRadius: 999,
  },
  badgeText: {
    fontSize: 12,
    fontWeight: "600",
  },
  cardTitle: {
    fontSize: 19,
    fontWeight: "700",
  },
  cardBody: {
    fontSize: 15,
    lineHeight: 22,
  },
  pathChip: {
    paddingHorizontal: 12,
    paddingVertical: 8,
    borderRadius: 10,
  },
  pathText: {
    fontSize: 12,
  },
  inputWrap: {
    marginTop: 6,
  },
  errorText: {
    fontSize: 13,
    fontWeight: "500",
    marginTop: 4,
  },
  progressWrap: {
    marginTop: 8,
    gap: 8,
  },
  counterText: {
    fontSize: 14,
    fontVariant: ["tabular-nums"],
    textAlign: "right",
  },
  metricsGrid: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 10,
    marginTop: 6,
  },
  metricBox: {
    minWidth: "47%",
    flexGrow: 1,
    padding: 12,
    borderRadius: 12,
    gap: 4,
  },
  metricValue: {
    fontSize: 22,
    fontWeight: "700",
    fontVariant: ["tabular-nums"],
  },
  metricLabel: {
    fontSize: 12,
    fontWeight: "500",
  },
  actions: {
    gap: 12,
    paddingTop: 8,
  },
});
