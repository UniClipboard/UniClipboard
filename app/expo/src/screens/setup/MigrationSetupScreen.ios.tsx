import { Keyboard, ScrollView, StyleSheet, Text, View, type ColorValue } from "react-native";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation("settingsStorage");
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
                {t("migration.preparing")}
              </Text>
            </View>
          </GlassContainer>
        );

      case "w-clean":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.ready")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.readyBody")}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-start-clean"
                  title={t("migration.start")}
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
                  title={t("migration.choose")}
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

      case "w-custom":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.chooseTitle")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.chooseBody")}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-choose-custom-folder"
                  title={t("migration.choose")}
                  variant="filled"
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-migration"
                  title={ctrl.setupEntryMode === "later" ? t("migration.skipReturn") : t("migration.skip")}
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

      case "w-detected":
        return (
          <>
            <GlassContainer shape="card" style={styles.card}>
              <View style={[styles.badge, { backgroundColor: c.accentContainer }]}>
                <Text style={[styles.badgeText, { color: c.onAccentContainer }]}>
                  {t("migration.detected")}
                </Text>
              </View>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {ctrl.isMobileSQLiteSource
                  ? t("migration.mobileDetectedTitle")
                  : t("migration.detectedTitle")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {ctrl.isMobileSQLiteSource
                  ? t("migration.mobileDetectedBody")
                  : t("migration.detectedBody")}
              </Text>
              {ctrl.detectedSourcePath && !ctrl.isMobileSQLiteSource ? (
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
                  title={t("migration.startMigration")}
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
                  title={t("migration.chooseOther")}
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
                  title={ctrl.setupEntryMode === "later" ? t("migration.skipReturn") : t("migration.skip")}
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
                {t("migration.invalid")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.invalidBody")}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-reselect-folder"
                  title={t("migration.chooseAgain")}
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
                  title={ctrl.setupEntryMode === "later" ? t("migration.skipReturn") : t("migration.skip")}
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
                {t("migration.unlock")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.unlockBody")}
              </Text>
              <View style={styles.inputWrap}>
                <AppTextField
                  testID="setup-passphrase-input"
                  value={ctrl.passphrase}
                  onChangeText={ctrl.setPassphrase}
                  placeholder={t("migration.passphraseHint")}
                  label={t("migration.passphrase")}
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
                  title={t("migration.confirm")}
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
                  title={t("migration.back")}
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
                  title={t("migration.skip")}
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
                  ? t("migration.stopping")
                  : isVerifying
                    ? t("migration.verifying")
                    : t("migration.importing")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {isStopping
                  ? t("migration.stoppingBody")
                  : isVerifying
                    ? t("migration.verifyingBody")
                    : ctrl.isMobileSQLiteSource
                      ? t("migration.mobileImportingBody")
                      : t("migration.importingBody")}
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
                    : t("migration.processed", { count: ctrl.migrationProgress.processed })}
                </Text>
              </View>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-stop-migration"
                  title={t("migration.stop")}
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
                  {t("migration.verified")}
                </Text>
              </View>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.done")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {ctrl.isMobileSQLiteSource ? t("migration.mobileDoneBody") : t("migration.doneBody")}
              </Text>
              {report ? (
                <View style={styles.metricsGrid}>
                  <MetricPill
                    label={t("migration.newCount")}
                    value={String(report.imported)}
                    tintColor={c.accent}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label={t("migration.skippedCount")}
                    value={String(report.skipped)}
                    tintColor={c.textPrimary}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label={t("migration.verifiedCount")}
                    value={String(report.verified)}
                    tintColor={c.accent}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label={t("migration.tagsCount")}
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
                  title={t("migration.enter")}
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
                {t("migration.partial")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.partialBody")}
              </Text>
              {report ? (
                <View style={styles.metricsGrid}>
                  <MetricPill
                    label={t("migration.newCount")}
                    value={String(report.imported)}
                    tintColor={c.accent}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label={t("migration.skippedCount")}
                    value={String(report.skipped)}
                    tintColor={c.textPrimary}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label={t("migration.failedCount")}
                    value={String(report.failed + report.verification_failed)}
                    tintColor={c.error}
                    textColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricPill
                    label={t("migration.missingCount")}
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
                  title={t("migration.retryPartial")}
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
                  title={t("migration.enter")}
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
                {t("migration.stopped")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.stoppedBody", { count: ctrl.cumulativeImported })}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-resume-migration"
                  title={t("migration.resume")}
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
                  title={t("migration.skipEnter")}
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
                {t("migration.conflict")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.conflictBody")}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-conflict"
                  title={t("migration.checkRetry")}
                  variant="filled"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleRetryMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-conflict"
                  title={t("migration.skip")}
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
                {t("migration.failed")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {ctrl.migrationErrorMessage || t("migration.failedBody")}
              </Text>
            </GlassContainer>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-failed"
                  title={t("migration.retry")}
                  variant="filled"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleRetryMigration()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-reselect-failed"
                  title={t("migration.another")}
                  variant="outlined"
                  size="large"
                  fullWidth
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-failed"
                  title={t("migration.skipEnter")}
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
            {t("migration.welcome")}
          </Text>
          <Text testID="setup-slogan" style={[styles.slogan, { color: c.textSecondary }]}>
            {t("migration.slogan")}
          </Text>
        </View>

        {renderBody()}
      </ScrollView>

      <AppAlertDialog
        visible={ctrl.confirmSkipVisible}
        standalone
        title={t("migration.skipTitle")}
        message={t("migration.skipBody")}
        confirmLabel={t("migration.skipEnter")}
        onConfirm={() => void ctrl.confirmSkip()}
        dismissLabel={t("migration.cancel")}
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
