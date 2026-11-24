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

  const detectedSourcePath =
    setupState?.source || sourceDetection?.source || setupState?.default_source || "";

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
