import {
  ActivityIndicator,
  Keyboard,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  type ColorValue,
} from "react-native";
import { useTranslation } from "react-i18next";
import { SafeAreaView } from "react-native-safe-area-context";
import {
  AlertTriangle,
  ArrowRight,
  Check,
  ChevronRight,
  Database,
  Folder,
  Pause,
  ShieldCheck,
  X,
} from "lucide-react-native";
import {
  AppAlertDialog,
  AppButton,
  AppHost,
  AppProgressIndicator,
  AppTextField,
} from "@/components/ui";
import { UnicBrandLogo } from "@/components/UnicBrandLogo";
import { useTheme } from "@/hooks/useTheme";
import { OnboardingBackButton } from "@/screens/onboarding/OnboardingBackButton";
import { ONBOARDING_PLATFORM_CONFIG } from "@/screens/onboarding/onboardingPlatformCopy";
import { useSetupFlowController } from "./useSetupFlowController";

function MetricTile({
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
    <View style={[s.metricBox, { backgroundColor: bgColor }]}>
      <Text style={[s.metricValue, { color: tintColor }]}>{value}</Text>
      <Text style={[s.metricLabel, { color: textColor }]} numberOfLines={1}>
        {label}
      </Text>
    </View>
  );
}

export function MigrationSetupContent({
  ctrl,
  standaloneAlerts = false,
}: {
  ctrl: ReturnType<typeof useSetupFlowController>;
  standaloneAlerts?: boolean;
}) {
  const { t } = useTranslation("settingsStorage");
  const { theme } = useTheme();
  const c = theme.colors;
  const segmented = ONBOARDING_PLATFORM_CONFIG.segmentedCards;
  const report = ctrl.migrationReport;

  const pageBg = segmented ? c.background : c.surface;
  const cardBg = segmented ? c.surfaceMid : c.surfaceLowest;
  const softBg = segmented ? c.surfaceHigh : c.fillQuaternary;
  const iconBg = (iosColor: string) => (segmented ? c.accentContainer : iosColor);
  const iconFg = () => (segmented ? (c.onAccentContainer as string) : "#FFFFFF");

  const rowStyle = (index: number, total: number) => {
    if (!segmented) {
      return [
        s.iosRow,
        index > 0 && { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: c.separator },
      ];
    }
    const isFirst = index === 0;
    const isLast = index === total - 1;
    return [
      s.m3Row,
      {
        backgroundColor: c.surfaceMid,
        borderTopLeftRadius: isFirst ? 20 : 4,
        borderTopRightRadius: isFirst ? 20 : 4,
        borderBottomLeftRadius: isLast ? 20 : 4,
        borderBottomRightRadius: isLast ? 20 : 4,
      },
    ];
  };

  const skipLabel =
    ctrl.setupEntryMode === "later" ? t("migration.skipReturn") : t("migration.skip");

  const renderTopBar = ({
    showBack = false,
    onBack,
    backTestID,
    badge,
    rightLabel,
    onRightPress,
    rightTestID,
  }: {
    showBack?: boolean;
    onBack?: () => void;
    backTestID?: string;
    badge?: string;
    rightLabel?: string;
    onRightPress?: () => void;
    rightTestID?: string;
  }) => (
    <View style={s.topBar}>
      <View style={s.topSide}>
        {showBack && onBack ? (
          <View testID={backTestID}>
            <OnboardingBackButton
              onPress={onBack}
              disabled={ctrl.busy}
              label={t("migration.back")}
            />
          </View>
        ) : null}
      </View>

      {badge ? (
        <View style={[s.stepPill, { backgroundColor: softBg }]}>
          <View style={[s.stepDot, { backgroundColor: c.accent }]} />
          <Text style={[s.stepPillText, { color: c.textSecondary }]}>{badge}</Text>
        </View>
      ) : (
        <View />
      )}

      <View style={[s.topSide, s.topSideRight]}>
        {rightLabel && onRightPress ? (
          <Pressable
            testID={rightTestID}
            onPress={onRightPress}
            disabled={ctrl.busy}
            hitSlop={10}
            style={({ pressed }) => [s.skipBtn, pressed && { opacity: 0.6 }]}
          >
            <Text style={[s.skipText, { color: segmented ? c.accent : c.textSecondary }]}>
              {rightLabel}
            </Text>
          </Pressable>
        ) : null}
      </View>
    </View>
  );

  const renderScreen = () => {
    switch (ctrl.setupViewState) {
      case "loading":
        return (
          <View style={s.centerStage}>
            <ActivityIndicator size="large" color={c.accent} />
            <Text style={[s.centerSub, { color: c.textSecondary }]}>
              {t("migration.preparing")}
            </Text>
          </View>
        );

      case "w-clean":
        return (
          <>
            {renderTopBar({ badge: t("migration.stepBadge") })}
            <View style={s.centerStage}>
              <UnicBrandLogo size={76} borderRadius={22} />
              <Text
                testID="setup-welcome-title"
                accessibilityRole="header"
                style={[s.centerTitle, { color: c.textPrimary }]}
              >
                {t("migration.welcome")}
              </Text>
              <Text testID="setup-slogan" style={[s.centerSub, { color: c.textSecondary }]}>
                {t("migration.slogan")}
              </Text>
            </View>
            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
              <AppHost style={s.secondaryHost} matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-choose-custom-folder"
                  title={t("migration.choose")}
                  variant="text"
                  colors={{ contentColor: segmented ? c.accent : c.textSecondary }}
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
            {renderTopBar({
              badge: t("migration.stepBadge"),
              rightLabel: skipLabel,
              onRightPress: () => void ctrl.handleEnterHistory(),
              rightTestID: "setup-skip-migration",
            })}
            <View style={s.centerStage}>
              <View style={[s.circleBadge, { backgroundColor: c.accentContainer }]}>
                <Folder size={30} color={c.onAccentContainer as string} />
              </View>
              <Text accessibilityRole="header" style={[s.centerTitle, { color: c.textPrimary }]}>
                {t("migration.chooseTitle")}
              </Text>
              <Text style={[s.centerSub, { color: c.textSecondary }]}>
                {t("migration.chooseBody")}
              </Text>
            </View>
            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
            </View>
          </>
        );

      case "w-detected": {
        const sourceTitle = ctrl.isMobileSQLiteSource
          ? t("migration.sourceMobile")
          : t("migration.sourceV1");
        const sourceSub =
          !ctrl.isMobileSQLiteSource && ctrl.detectedSourcePath
            ? ctrl.detectedSourcePath
            : t("migration.detected");

        return (
          <>
            {renderTopBar({
              badge: t("migration.stepBadge"),
              rightLabel: skipLabel,
              onRightPress: ctrl.requestSkip,
              rightTestID: "setup-skip-migration",
            })}

            <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
              {/* Compact transfer visual */}
              <View style={s.upgradeVisual}>
                <View style={[s.legacyTile, { backgroundColor: softBg }]}>
                  <Database size={24} color={c.textSecondary as string} />
                </View>
                <ArrowRight size={18} color={c.textTertiary as string} />
                <UnicBrandLogo size={54} borderRadius={15} />
              </View>

              <View style={s.pageHeadCenter}>
                <Text
                  testID="setup-welcome-title"
                  accessibilityRole="header"
                  style={[s.pageTitleCenter, { color: c.textPrimary }]}
                >
                  {ctrl.isMobileSQLiteSource
                    ? t("migration.mobileDetectedShortTitle")
                    : t("migration.detectedShortTitle")}
                </Text>
                <Text style={[s.pageSubCenter, { color: c.textSecondary }]}>
                  {ctrl.isMobileSQLiteSource
                    ? t("migration.mobileDetectedShortSub")
                    : t("migration.detectedShortSub")}
                </Text>
              </View>

              <View
                style={[
                  s.groupWrap,
                  !segmented && { backgroundColor: cardBg },
                  segmented && { gap: 2 },
                ]}
              >
                <Pressable
                  testID="setup-change-source"
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                  disabled={ctrl.busy}
                  style={({ pressed }) => [rowStyle(0, 2), pressed && { opacity: 0.72 }]}
                >
                  <View
                    style={[
                      segmented ? s.m3IconCircle : s.iosIconTile,
                      { backgroundColor: iconBg("#007AFF") },
                    ]}
                  >
                    <Folder size={18} color={iconFg()} />
                  </View>
                  <View style={s.flex1}>
                    <Text style={[s.rowTitle, { color: c.textPrimary }]} numberOfLines={1}>
                      {sourceTitle}
                    </Text>
                    <Text style={[s.rowSub, { color: c.textSecondary }]} numberOfLines={1}>
                      {sourceSub}
                    </Text>
                  </View>
                  <Text style={[s.changeText, { color: c.textSecondary }]}>
                    {t("migration.changeSource")}
                  </Text>
                  <ChevronRight size={16} color={c.textTertiary as string} />
                </Pressable>

                <View style={rowStyle(1, 2)}>
                  <View
                    style={[
                      segmented ? s.m3IconCircle : s.iosIconTile,
                      { backgroundColor: iconBg("#34C759") },
                    ]}
                  >
                    <ShieldCheck size={18} color={iconFg()} />
                  </View>
                  <View style={s.flex1}>
                    <Text style={[s.rowTitle, { color: c.textPrimary }]} numberOfLines={1}>
                      {t("migration.scopeTitle")}
                    </Text>
                    <Text style={[s.rowSub, { color: c.textSecondary }]} numberOfLines={1}>
                      {t("migration.scopeSub")}
                    </Text>
                  </View>
                </View>
              </View>
            </ScrollView>

            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
                <AppButton
                  title={skipLabel}
                  variant="tonal"
                  colors={{ containerColor: c.accentContainer, contentColor: c.onAccentContainer }}
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={ctrl.requestSkip}
                />
              </AppHost>
            </View>
          </>
        );
      }

      case "w-invalid":
        return (
          <>
            {renderTopBar({
              badge: t("migration.stepBadge"),
              rightLabel: skipLabel,
              onRightPress: ctrl.requestSkip,
              rightTestID: "setup-skip-from-invalid",
            })}
            <View style={s.centerStage}>
              <View style={[s.circleBadge, { backgroundColor: c.errorContainer }]}>
                <AlertTriangle size={30} color={c.error as string} />
              </View>
              <Text accessibilityRole="header" style={[s.centerTitle, { color: c.textPrimary }]}>
                {t("migration.invalid")}
              </Text>
              <Text style={[s.centerSub, { color: c.textSecondary }]}>
                {t("migration.invalidBody")}
              </Text>
            </View>
            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
            </View>
          </>
        );

      case "m-auth":
      case "m-auth-err":
        return (
          <>
            {renderTopBar({
              showBack: true,
              onBack: () => ctrl.setSetupViewState("w-detected"),
              backTestID: "setup-back-to-detected",
              badge: t("migration.stepBadge"),
              rightLabel: t("migration.skip"),
              onRightPress: ctrl.requestSkip,
              rightTestID: "setup-skip-from-auth",
            })}

            <ScrollView
              contentContainerStyle={s.scrollContent}
              keyboardShouldPersistTaps="handled"
              automaticallyAdjustKeyboardInsets
              showsVerticalScrollIndicator={false}
            >
              <View style={s.pageHead}>
                <Text accessibilityRole="header" style={[s.pageTitle, { color: c.textPrimary }]}>
                  {t("migration.unlock")}
                </Text>
                <Text style={[s.pageSubtitle, { color: c.textSecondary }]}>
                  {t("migration.unlockBody")}
                </Text>
              </View>

              <View style={[s.panelCard, { backgroundColor: cardBg }]}>
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
                {ctrl.setupViewState === "m-auth-err" && ctrl.migrationErrorMessage ? (
                  <View style={[s.errBox, { backgroundColor: c.errorContainer }]}>
                    <Text testID="setup-auth-error" style={[s.errBoxText, { color: c.onErrorContainer }]}>
                      {ctrl.migrationErrorMessage}
                    </Text>
                  </View>
                ) : null}
              </View>
            </ScrollView>

            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
            {renderTopBar({
              badge: isStopping
                ? t("migration.stopping")
                : isVerifying
                  ? t("migration.verifying")
                  : t("migration.importing"),
            })}

            <View style={s.centerStage}>
              <View style={[s.progressCard, { backgroundColor: cardBg }]}>
                <ActivityIndicator size="large" color={isVerifying ? c.success : c.accent} />
                <Text style={[s.progressTitle, { color: c.textPrimary }]}>
                  {isStopping
                    ? t("migration.stopping")
                    : isVerifying
                      ? t("migration.verifying")
                      : t("migration.importing")}
                </Text>

                <View style={s.stepList}>
                  <View style={s.stepRow}>
                    <View style={s.stepIconSlot}>
                      {isVerifying ? (
                        <Check size={16} color={c.success as string} />
                      ) : (
                        <ActivityIndicator size="small" color={c.accent} />
                      )}
                    </View>
                    <Text
                      style={[
                        s.stepText,
                        {
                          color: isVerifying ? c.success : c.textPrimary,
                          fontWeight: isVerifying ? "500" : "600",
                        },
                      ]}
                    >
                      {t("migration.stepImport")}
                    </Text>
                  </View>
                  <View style={s.stepRow}>
                    <View style={s.stepIconSlot}>
                      {isVerifying ? (
                        <ActivityIndicator size="small" color={c.success} />
                      ) : (
                        <View style={[s.stepBullet, { backgroundColor: c.separator }]} />
                      )}
                    </View>
                    <Text
                      style={[
                        s.stepText,
                        {
                          color: isVerifying ? c.textPrimary : c.textSecondary,
                          fontWeight: isVerifying ? "600" : "400",
                        },
                      ]}
                    >
                      {t("migration.stepVerify")}
                    </Text>
                  </View>
                </View>

                <View style={s.barWrap}>
                  <AppHost matchContents={{ vertical: true }}>
                    <AppProgressIndicator
                      variant="linear"
                      progress={ctrl.progressRatio}
                      color={isVerifying ? c.success : c.accent}
                      fullWidth
                    />
                  </AppHost>
                  <View style={s.counterRow}>
                    <Text style={[s.counterHint, { color: c.textSecondary }]}>
                      {t("migration.keepForeground")}
                    </Text>
                    <Text style={[s.counterText, { color: c.textPrimary }]}>
                      {ctrl.migrationProgress.total > 0
                        ? `${ctrl.migrationProgress.processed} / ${ctrl.migrationProgress.total}`
                        : t("migration.processed", { count: ctrl.migrationProgress.processed })}
                    </Text>
                  </View>
                </View>
              </View>
            </View>

            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-stop-migration"
                  title={t("migration.stop")}
                  variant="tonal"
                  colors={{ containerColor: c.accentContainer, contentColor: c.onAccentContainer }}
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

      case "r-done": {
        const tiles: Array<{ label: string; value: string; tint: ColorValue }> = [];
        if (report) {
          tiles.push({ label: t("migration.newCount"), value: String(report.imported), tint: c.accent });
          tiles.push({ label: t("migration.verifiedCount"), value: String(report.verified), tint: c.success });
          if (report.skipped > 0) {
            tiles.push({ label: t("migration.skippedCount"), value: String(report.skipped), tint: c.textPrimary });
          }
          if (report.tags > 0) {
            tiles.push({ label: t("migration.tagsCount"), value: String(report.tags), tint: c.textPrimary });
          }
        }
        return (
          <>
            {renderTopBar({ badge: t("migration.verified") })}
            <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
              <View style={s.resultHero}>
                <View style={[s.circleBadge, { backgroundColor: c.successContainer }]}>
                  <Check size={34} color={c.success as string} />
                </View>
                <Text testID="setup-result-title" style={[s.centerTitle, { color: c.textPrimary }]}>
                  {t("migration.done")}
                </Text>
                <Text style={[s.centerSub, { color: c.textSecondary }]}>
                  {ctrl.isMobileSQLiteSource ? t("migration.mobileDoneBody") : t("migration.doneBody")}
                </Text>
              </View>

              {tiles.length > 0 ? (
                <View style={s.metricsGrid}>
                  {tiles.map((tile) => (
                    <MetricTile
                      key={tile.label}
                      label={tile.label}
                      value={tile.value}
                      tintColor={tile.tint}
                      textColor={c.textSecondary}
                      bgColor={cardBg}
                    />
                  ))}
                </View>
              ) : null}
            </ScrollView>

            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
      }

      case "r-partial": {
        const failedTotal = (report?.failed ?? 0) + (report?.verification_failed ?? 0);
        const tiles: Array<{ label: string; value: string; tint: ColorValue }> = [];
        if (report) {
          tiles.push({ label: t("migration.newCount"), value: String(report.imported), tint: c.accent });
          if (failedTotal > 0) {
            tiles.push({ label: t("migration.failedCount"), value: String(failedTotal), tint: c.error });
          }
          if (report.missing_parts > 0) {
            tiles.push({ label: t("migration.missingCount"), value: String(report.missing_parts), tint: c.warning });
          }
          if (report.skipped > 0) {
            tiles.push({ label: t("migration.skippedCount"), value: String(report.skipped), tint: c.textPrimary });
          }
        }
        return (
          <>
            {renderTopBar({ badge: t("migration.stepBadge") })}
            <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
              <View style={s.resultHero}>
                <View style={[s.circleBadge, { backgroundColor: c.warningContainer }]}>
                  <AlertTriangle size={32} color={c.warning as string} />
                </View>
                <Text testID="setup-result-title" style={[s.centerTitle, { color: c.textPrimary }]}>
                  {t("migration.partial")}
                </Text>
                <Text style={[s.centerSub, { color: c.textSecondary }]}>
                  {t("migration.partialBody")}
                </Text>
              </View>

              {tiles.length > 0 ? (
                <View style={s.metricsGrid}>
                  {tiles.map((tile) => (
                    <MetricTile
                      key={tile.label}
                      label={tile.label}
                      value={tile.value}
                      tintColor={tile.tint}
                      textColor={c.textSecondary}
                      bgColor={cardBg}
                    />
                  ))}
                </View>
              ) : null}
            </ScrollView>

            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-enter-after-partial"
                  title={t("migration.enter")}
                  variant="tonal"
                  colors={{ containerColor: c.accentContainer, contentColor: c.onAccentContainer }}
                  size="large"
                  fullWidth
                  disabled={ctrl.busy}
                  onPress={() => void ctrl.handleEnterHistory()}
                />
              </AppHost>
            </View>
          </>
        );
      }

      case "r-cancelled":
        return (
          <>
            {renderTopBar({ badge: t("migration.stepBadge") })}
            <View style={s.centerStage}>
              <View style={[s.circleBadge, { backgroundColor: softBg }]}>
                <Pause size={30} color={c.textPrimary as string} />
              </View>
              <Text testID="setup-result-title" style={[s.centerTitle, { color: c.textPrimary }]}>
                {t("migration.stopped")}
              </Text>
              <Text style={[s.centerSub, { color: c.textSecondary }]}>
                {t("migration.stoppedBody", { count: ctrl.cumulativeImported })}
              </Text>
            </View>
            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-skip-after-cancel"
                  title={t("migration.skipEnter")}
                  variant="tonal"
                  colors={{ containerColor: c.accentContainer, contentColor: c.onAccentContainer }}
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
            {renderTopBar({
              badge: t("migration.stepBadge"),
              rightLabel: t("migration.skip"),
              onRightPress: ctrl.requestSkip,
              rightTestID: "setup-skip-conflict",
            })}
            <View style={s.centerStage}>
              <View style={[s.circleBadge, { backgroundColor: c.warningContainer }]}>
                <AlertTriangle size={30} color={c.warning as string} />
              </View>
              <Text testID="setup-result-title" style={[s.centerTitle, { color: c.textPrimary }]}>
                {t("migration.conflict")}
              </Text>
              <Text style={[s.centerSub, { color: c.textSecondary }]}>
                {t("migration.conflictBody")}
              </Text>
            </View>
            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
            </View>
          </>
        );

      case "r-failed":
      default:
        return (
          <>
            {renderTopBar({
              badge: t("migration.stepBadge"),
              rightLabel: t("migration.skip"),
              onRightPress: () => void ctrl.handleEnterHistory(),
              rightTestID: "setup-skip-failed",
            })}
            <View style={s.centerStage}>
              <View style={[s.circleBadge, { backgroundColor: c.errorContainer }]}>
                <X size={32} color={c.error as string} />
              </View>
              <Text testID="setup-result-title" style={[s.centerTitle, { color: c.textPrimary }]}>
                {t("migration.failed")}
              </Text>
              <Text style={[s.centerSub, { color: c.textSecondary }]}>
                {ctrl.migrationErrorMessage || t("migration.failedBody")}
              </Text>
            </View>
            <View style={s.footer}>
              <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
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
              <AppHost style={s.secondaryHost} matchContents={{ vertical: true }}>
                <AppButton
                  testID="setup-reselect-failed"
                  title={t("migration.another")}
                  variant="text"
                  colors={{ contentColor: segmented ? c.accent : c.textSecondary }}
                  fullWidth
                  onPress={() => void ctrl.handleSelectCustomFolder()}
                />
              </AppHost>
            </View>
          </>
        );
    }
  };

  return (
    <SafeAreaView style={[s.root, { backgroundColor: pageBg }]} edges={["top", "bottom"]}>
      {renderScreen()}

      <AppAlertDialog
        visible={ctrl.confirmSkipVisible}
        standalone={standaloneAlerts}
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
        standalone={standaloneAlerts}
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

const s = StyleSheet.create({
  root: { flex: 1 },
  flex1: { flex: 1 },

  topBar: {
    minHeight: 52,
    paddingHorizontal: 16,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  topSide: { width: 84, alignItems: "flex-start", justifyContent: "center" },
  topSideRight: { alignItems: "flex-end" },
  stepPill: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: 11,
    paddingVertical: 5,
    borderRadius: 99,
  },
  stepDot: { width: 6, height: 6, borderRadius: 3 },
  stepPillText: { fontSize: 12, fontWeight: "600" },
  skipBtn: { paddingVertical: 8, paddingHorizontal: 4 },
  skipText: { fontSize: 15, fontWeight: "600" },

  scrollContent: {
    flexGrow: 1,
    paddingHorizontal: 18,
    paddingTop: 10,
    paddingBottom: 20,
    alignSelf: "center",
    width: "100%",
    maxWidth: 480,
  },
  upgradeVisual: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 16,
    paddingTop: 22,
    paddingBottom: 22,
  },
  legacyTile: {
    width: 54,
    height: 54,
    borderRadius: 15,
    alignItems: "center",
    justifyContent: "center",
  },
  pageHead: { paddingHorizontal: 2, paddingBottom: 20 },
  pageTitle: { fontSize: 26, lineHeight: 33, fontWeight: "700" },
  pageSubtitle: { fontSize: 14.5, lineHeight: 21, marginTop: 6 },
  pageHeadCenter: { alignItems: "center", paddingBottom: 24, paddingHorizontal: 12 },
  pageTitleCenter: { fontSize: 26, lineHeight: 33, fontWeight: "700", textAlign: "center" },
  pageSubCenter: { fontSize: 14.5, lineHeight: 21, textAlign: "center", marginTop: 6 },

  groupWrap: { borderRadius: 20, overflow: "hidden" },
  iosRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    paddingHorizontal: 15,
    paddingVertical: 14,
    minHeight: 66,
  },
  m3Row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 14,
    paddingHorizontal: 16,
    paddingVertical: 14,
    minHeight: 72,
  },
  iosIconTile: {
    width: 32,
    height: 32,
    borderRadius: 8,
    alignItems: "center",
    justifyContent: "center",
  },
  m3IconCircle: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: "center",
    justifyContent: "center",
  },
  rowTitle: { fontSize: 15.5, lineHeight: 21, fontWeight: "600" },
  rowSub: { fontSize: 12.5, lineHeight: 17, marginTop: 2 },
  changeText: { fontSize: 13.5, fontWeight: "500" },

  panelCard: { borderRadius: 20, padding: 18, gap: 12 },
  errBox: { padding: 12, borderRadius: 12 },
  errBoxText: { fontSize: 13, lineHeight: 18.5 },

  centerStage: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 26,
    gap: 10,
  },
  resultHero: {
    alignItems: "center",
    paddingTop: 22,
    paddingBottom: 24,
    gap: 8,
  },
  circleBadge: {
    width: 68,
    height: 68,
    borderRadius: 34,
    alignItems: "center",
    justifyContent: "center",
    marginBottom: 8,
  },
  centerTitle: { fontSize: 25, lineHeight: 32, fontWeight: "700", textAlign: "center" },
  centerSub: { fontSize: 14.5, lineHeight: 21, textAlign: "center", maxWidth: 320 },

  progressCard: {
    width: "100%",
    maxWidth: 380,
    borderRadius: 22,
    padding: 24,
    alignItems: "center",
    gap: 16,
  },
  progressTitle: { fontSize: 19, fontWeight: "700" },
  stepList: { width: "100%", gap: 10 },
  stepRow: { flexDirection: "row", alignItems: "center", gap: 10 },
  stepIconSlot: { width: 20, height: 20, alignItems: "center", justifyContent: "center" },
  stepBullet: { width: 6, height: 6, borderRadius: 3 },
  stepText: { fontSize: 14 },
  barWrap: { width: "100%", gap: 8, marginTop: 4 },
  counterRow: { flexDirection: "row", alignItems: "center", justifyContent: "space-between" },
  counterHint: { fontSize: 12.5 },
  counterText: { fontSize: 13.5, fontWeight: "600", fontVariant: ["tabular-nums"] },

  metricsGrid: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 10,
  },
  metricBox: {
    minWidth: "47%",
    flexGrow: 1,
    paddingVertical: 14,
    paddingHorizontal: 16,
    borderRadius: 18,
    gap: 4,
  },
  metricValue: {
    fontSize: 24,
    fontWeight: "700",
    fontVariant: ["tabular-nums"],
  },
  metricLabel: { fontSize: 12.5, fontWeight: "500" },

  footer: {
    paddingHorizontal: 20,
    paddingTop: 10,
    paddingBottom: 12,
    alignSelf: "center",
    width: "100%",
    maxWidth: 480,
    gap: 10,
  },
  primaryHost: { minHeight: 54, width: "100%" },
  secondaryHost: { minHeight: 44, width: "100%" },
});
