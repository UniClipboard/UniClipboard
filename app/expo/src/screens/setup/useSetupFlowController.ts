import { useState } from "react";
import { useEngineStore } from "@/engine/store";

export function useSetupFlowController() {
  const {
    setupEntryMode,
    setupState,
    sourceDetection,
    setupViewState,
    migrationProgress,
    migrationReport,
    migrationErrorMessage,
    lastAttemptImported,
    cumulativeImported,
    busy,
    redetectSource,
    pickCustomSetupSource,
    setSetupViewState,
    startMigration,
    cancelMigration,
    completeSetupAndEnterMain,
  } = useEngineStore();

  const [passphrase, setPassphrase] = useState("");
  const [confirmSkipVisible, setConfirmSkipVisible] = useState(false);
  const [confirmStopVisible, setConfirmStopVisible] = useState(false);

  const detectedSourcePath =
    setupState?.source || sourceDetection?.source || setupState?.default_source || "";
  const detectedSourceKind =
    sourceDetection?.source_kind || setupState?.source_kind || "";
  const isMobileSQLiteSource = detectedSourceKind === "mobile_sqlite";

  const progressRatio =
    migrationProgress.total > 0
      ? Math.min(1, Math.max(0, migrationProgress.processed / migrationProgress.total))
      : undefined;

  const requestSkip = () => {
    setConfirmSkipVisible(true);
  };

  const confirmSkip = async () => {
    setConfirmSkipVisible(false);
    await completeSetupAndEnterMain();
  };

  const dismissSkip = () => {
    setConfirmSkipVisible(false);
  };

  const requestStop = () => {
    setConfirmStopVisible(true);
  };

  const confirmStop = async () => {
    setConfirmStopVisible(false);
    await cancelMigration();
  };

  const dismissStop = () => {
    setConfirmStopVisible(false);
  };

  const handleStartAutoMigration = async () => {
    await startMigration(undefined);
  };

  const handleSubmitPassphrase = async () => {
    if (!passphrase.trim()) return;
    await startMigration(passphrase);
  };

  const handleRetryMigration = async () => {
    await startMigration(passphrase.trim() ? passphrase : undefined);
  };

  const handleCancelMigration = async () => {
    await cancelMigration();
  };

  const handleEnterHistory = async () => {
    setPassphrase("");
    await completeSetupAndEnterMain();
  };

  const handleSelectCustomFolder = async () => {
    await pickCustomSetupSource();
  };

  const handleRetryDefaultDetect = async () => {
    await redetectSource("");
  };

  return {
    setupEntryMode,
    setupViewState,
    detectedSourcePath,
    detectedSourceKind,
    isMobileSQLiteSource,
    migrationProgress,
    progressRatio,
    migrationReport,
    migrationErrorMessage,
    lastAttemptImported,
    cumulativeImported,
    busy,
    passphrase,
    setPassphrase,
    confirmSkipVisible,
    requestSkip,
    confirmSkip,
    dismissSkip,
    confirmStopVisible,
    requestStop,
    confirmStop,
    dismissStop,
    setSetupViewState,
    handleStartAutoMigration,
    handleSubmitPassphrase,
    handleRetryMigration,
    handleCancelMigration,
    handleEnterHistory,
    handleSelectCustomFolder,
    handleRetryDefaultDetect,
  };
}
