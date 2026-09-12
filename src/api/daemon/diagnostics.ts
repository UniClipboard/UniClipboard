import {
  exportLogs as exportLogsSdk,
  getDebugStatus as getDebugStatusSdk,
  getDiagnosticCaptureStatus as getDiagnosticCaptureStatusSdk,
  startDiagnosticCapture as startDiagnosticCaptureSdk,
  stopDiagnosticCapture as stopDiagnosticCaptureSdk,
  updateDebugMode as updateDebugModeSdk,
} from '@/api/generated/sdk.gen'
import type {
  DiagnosticArchiveCollectionDto,
  DiagnosticCaptureStopResultDto,
  DiagnosticExportPreparationDto,
  DiagnosticStatusDto,
} from '@/api/generated/types.gen'
import { daemonClient } from './client'

export interface DebugStatus {
  debugMode: boolean
  effectiveLogProfile: string
  restartRequired: boolean
}

export interface UpdateDebugModeResult {
  debugMode: boolean
  restartRequired: boolean
}

export interface LogExportResult {
  path: string
  includedFiles: string[]
  since: string
  enginePreparation: DiagnosticExportPreparationDto
  collection: DiagnosticArchiveCollectionDto
}

export type DiagnosticCaptureStatus = DiagnosticStatusDto

export async function getDebugStatus(): Promise<DebugStatus> {
  return await daemonClient.callEnveloped(() => getDebugStatusSdk({ throwOnError: true }))
}

export async function updateDebugMode(enabled: boolean): Promise<UpdateDebugModeResult> {
  return await daemonClient.callEnveloped(() =>
    updateDebugModeSdk({ body: { enabled }, throwOnError: true })
  )
}

export async function exportLogs(sinceHours = 24): Promise<LogExportResult> {
  return await daemonClient.callEnveloped(() =>
    exportLogsSdk({ body: { sinceHours }, throwOnError: true })
  )
}

export async function getDiagnosticCaptureStatus(): Promise<DiagnosticCaptureStatus> {
  return await daemonClient.callEnveloped(() =>
    getDiagnosticCaptureStatusSdk({ throwOnError: true })
  )
}

export async function startDiagnosticCapture(
  durationSeconds = 600
): Promise<DiagnosticCaptureStatus> {
  return await daemonClient.callEnveloped(() =>
    startDiagnosticCaptureSdk({ body: { durationSeconds }, throwOnError: true })
  )
}

export async function stopDiagnosticCapture(
  captureId: string
): Promise<DiagnosticCaptureStopResultDto> {
  return await daemonClient.callEnveloped(() =>
    stopDiagnosticCaptureSdk({ body: { captureId }, throwOnError: true })
  )
}
