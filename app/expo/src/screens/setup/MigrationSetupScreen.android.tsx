import { useEffect } from "react";
import { BackHandler, Keyboard, ScrollView, StyleSheet, Text, View, type ColorValue } from "react-native";
import { useTranslation } from "react-i18next";
import { SafeAreaView } from "react-native-safe-area-context";
import {
  AppAlertDialog,
  AppButton,
  AppHost,
  AppProgressIndicator,
  AppTextField,
} from "@/components/ui";
import { UnicBrandLogo } from "@/components/UnicBrandLogo";
import { useTheme } from "@/hooks/useTheme";
import { useSetupFlowController } from "./useSetupFlowController";

function MetricTile({
  label,
  value,
  valueColor,
  labelColor,
  bgColor,
}: {
  label: string;
  value: string;
  valueColor: ColorValue;
  labelColor: ColorValue;
  bgColor: ColorValue;
}) {
  return (
    <View style={[styles.metricTile, { backgroundColor: bgColor }]}>
      <Text style={[styles.metricValue, { color: valueColor }]}>{value}</Text>
      <Text style={[styles.metricLabel, { color: labelColor }]}>{label}</Text>
    </View>
  );
}

export function MigrationSetupScreen() {
  const { t } = useTranslation("settingsStorage");
  const { theme } = useTheme();
  const c = theme.colors;
  const ctrl = useSetupFlowController();
  const report = ctrl.migrationReport;

  useEffect(() => {
    const sub = BackHandler.addEventListener("hardwareBackPress", () => {
      if (ctrl.setupViewState === "m-importing" || ctrl.setupViewState === "m-verifying") {
        ctrl.requestStop();
        return true;
      }
      if (ctrl.setupViewState === "m-stopping") {
        return true;
      }
      return false;
    });
    return () => sub.remove();
  }, [ctrl]);

  const renderCardContent = () => {
    switch (ctrl.setupViewState) {
      case "loading":
        return (
          <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
            <View style={styles.rowCenter}>
              <AppHost matchContents={{ vertical: true }}>
                <AppProgressIndicator variant="circular" color={c.accent} />
              </AppHost>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.preparing")}
              </Text>
            </View>
          </View>
        );

      case "w-clean":
        return (
          <>
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.ready")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.readyBody")}
              </Text>
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-start-clean"
                  title={t("migration.start")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
                  variant="tonal"
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.chooseTitle")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.chooseBody")}
              </Text>
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-choose-custom-folder"
                  title={t("migration.choose")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <View style={[styles.tonalChip, { backgroundColor: c.accentContainer }]}>
                <Text style={[styles.tonalChipText, { color: c.onAccentContainer }]}>
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
                <View style={[styles.pathBlock, { backgroundColor: c.surfaceHigh }]}>
                  <Text style={[styles.pathText, { color: c.textSecondary }]} numberOfLines={2}>
                    {ctrl.detectedSourcePath}
                  </Text>
                </View>
              ) : null}
            </View>

            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-start-migration"
                  title={t("migration.startMigration")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
                  variant="tonal"
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
                  colors={{ contentColor: c.accent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text style={[styles.cardTitle, { color: c.error }]}>
                {t("migration.invalid")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.invalidBody")}
              </Text>
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-reselect-folder"
                  title={t("migration.chooseAgain")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.unlock")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.unlockBody")}
              </Text>
              <View style={styles.inputWrap}>
                <AppHost matchContents={{ vertical: true }}>
                  <AppTextField
                    testID="setup-passphrase-input"
                    value={ctrl.passphrase}
                    onChangeText={ctrl.setPassphrase}
                    placeholder={t("migration.passphraseHint")}
                    label={t("migration.passphrase")}
                    secure
                    fullWidth
                    disabled={ctrl.busy}
                  />
                </AppHost>
              </View>
              {ctrl.setupViewState === "m-auth-err" && ctrl.migrationErrorMessage ? (
                <Text testID="setup-auth-error" style={[styles.errorText, { color: c.error }]}>
                  {ctrl.migrationErrorMessage}
                </Text>
              ) : null}
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-submit-passphrase"
                  title={t("migration.confirm")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
                  colors={{ contentColor: c.accent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
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
                    trackColor={c.surfaceHigh}
                    fullWidth
                  />
                </AppHost>
                <Text style={[styles.counterText, { color: c.textSecondary }]}>
                  {ctrl.migrationProgress.total > 0
                    ? `${ctrl.migrationProgress.processed} / ${ctrl.migrationProgress.total}`
                    : t("migration.processed", { count: ctrl.migrationProgress.processed })}
                </Text>
              </View>
            </View>
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <View style={[styles.tonalChip, { backgroundColor: c.accentContainer }]}>
                <Text style={[styles.tonalChipText, { color: c.onAccentContainer }]}>
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
                  <MetricTile
                    label={t("migration.newCount")}
                    value={String(report.imported)}
                    valueColor={c.accent}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricTile
                    label={t("migration.skippedCount")}
                    value={String(report.skipped)}
                    valueColor={c.textPrimary}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricTile
                    label={t("migration.verifiedCount")}
                    value={String(report.verified)}
                    valueColor={c.accent}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricTile
                    label={t("migration.tagsCount")}
                    value={String(report.tags)}
                    valueColor={c.textPrimary}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                </View>
              ) : null}
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-enter-history"
                  title={t("migration.enter")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.warning }]}>
                {t("migration.partial")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.partialBody")}
              </Text>
              {report ? (
                <View style={styles.metricsGrid}>
                  <MetricTile
                    label={t("migration.newCount")}
                    value={String(report.imported)}
                    valueColor={c.accent}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricTile
                    label={t("migration.skippedCount")}
                    value={String(report.skipped)}
                    valueColor={c.textPrimary}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricTile
                    label={t("migration.failedCount")}
                    value={String(report.failed + report.verification_failed)}
                    valueColor={c.error}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                  <MetricTile
                    label={t("migration.missingCount")}
                    value={String(report.missing_parts)}
                    valueColor={c.warning}
                    labelColor={c.textSecondary}
                    bgColor={c.surfaceHigh}
                  />
                </View>
              ) : null}
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-partial"
                  title={t("migration.retryPartial")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.textPrimary }]}>
                {t("migration.stopped")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.stoppedBody", { count: ctrl.cumulativeImported })}
              </Text>
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-resume-migration"
                  title={t("migration.resume")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.warning }]}>
                {t("migration.conflict")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {t("migration.conflictBody")}
              </Text>
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-conflict"
                  title={t("migration.checkRetry")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
            <View style={[styles.surfaceCard, { backgroundColor: c.surface }]}>
              <Text testID="setup-result-title" style={[styles.cardTitle, { color: c.error }]}>
                {t("migration.failed")}
              </Text>
              <Text style={[styles.cardBody, { color: c.textSecondary }]}>
                {ctrl.migrationErrorMessage || t("migration.failedBody")}
              </Text>
            </View>
            <View style={styles.actions}>
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-retry-failed"
                  title={t("migration.retry")}
                  variant="filled"
                  colors={{ containerColor: c.accent, contentColor: c.onAccent }}
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
                  colors={{ contentColor: c.accent }}
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
        showsVerticalScrollIndicator={false}
      >
        <View style={[styles.hero, isAuthView ? styles.heroCompact : null]}>
          <UnicBrandLogo size={isAuthView ? 56 : 84} borderRadius={isAuthView ? 18 : 26} style={styles.logo} />
          <Text testID="setup-welcome-title" style={[styles.title, { color: c.textPrimary }]}>
            {t("migration.welcome")}
          </Text>
          <Text testID="setup-slogan" style={[styles.slogan, { color: c.textSecondary }]}>
            {t("migration.slogan")}
          </Text>
        </View>

        {renderCardContent()}
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

      <AppAlertDialog
        visible={ctrl.confirmStopVisible}
        standalone
        title={t("migration.stopConfirmTitle")}
        message={t("migration.stopConfirmBody")}
        confirmLabel={t("migration.stopConfirmAction")}
        onConfirm={() => void ctrl.confirmStop()}
        dismissLabel={t("migration.continueMigration")}
        onDismiss={ctrl.dismissStop}
        onDismissAction={ctrl.dismissStop}
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
    textAlign: "center",
  },
  slogan: {
    fontSize: 16,
    fontWeight: "500",
    textAlign: "center",
  },
  surfaceCard: {
    borderRadius: 24,
    padding: 20,
    gap: 12,
  },
  rowCenter: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  tonalChip: {
    alignSelf: "flex-start",
    paddingHorizontal: 12,
    paddingVertical: 5,
    borderRadius: 999,
  },
  tonalChipText: {
    fontSize: 12,
    fontWeight: "600",
  },
  cardTitle: {
    fontSize: 20,
    fontWeight: "700",
  },
  cardBody: {
    fontSize: 15,
    lineHeight: 22,
  },
  pathBlock: {
    borderRadius: 14,
    paddingHorizontal: 12,
    paddingVertical: 10,
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
  metricTile: {
    minWidth: "47%",
    flexGrow: 1,
    padding: 14,
    borderRadius: 18,
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
