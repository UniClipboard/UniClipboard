import { use } from 'react'
import { DeviceTrustContext, type DeviceTrustContextValue } from '@/contexts/device-trust-context'

export function useDeviceTrust(): DeviceTrustContextValue {
  const value = use(DeviceTrustContext)
  if (!value) throw new Error('useDeviceTrust must be used within DeviceTrustProvider')
  return value
}
