import { getConfigImportStatus as getConfigImportStatusSdk } from '@/api/generated/sdk.gen'
import { daemonClient } from './client'

/** Absence only means there is no rejected input; it is not an import receipt. */
export async function getConfigImportStatus() {
  return await daemonClient.callEnveloped(() => getConfigImportStatusSdk({ throwOnError: true }))
}
