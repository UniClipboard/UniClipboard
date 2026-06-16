import {
  exportLogs as exportLogsSdk,
  getDebugStatus as getDebugStatusSdk,
  updateDebugMode as updateDebugModeSdk,
} from '@/api/generated/sdk.gen'
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
}

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
