import { daemonClient } from './client'
import { DaemonApiError, DaemonErrorCode } from './errors'

export type SpaceRuntimeState =
  | { state: 'stopped' }
  | { state: 'starting' }
  | { state: 'running' }
  | { state: 'locked' }
  | { state: 'failed' }

export type SpaceIncomingSyncState =
  | { state: 'enabled' }
  | { state: 'receiving' }
  | { state: 'degraded' }
  | { state: 'disabled' }

export interface SpaceFault {
  category: string
  messageCode: string | null
}

export interface SpaceProfileSummary {
  profileId: string
  spaceId: string | null
  displayName: string | null
  deviceName: string | null
  runtimeState: SpaceRuntimeState
  incomingSyncState: SpaceIncomingSyncState
  lastFault: SpaceFault | null
  isActiveSend: boolean
}

export interface CreateSpaceProfileRequest {
  passphrase: string
  passphraseConfirm: string
  deviceName?: string | null
}

export interface JoinSpaceProfileRequest {
  code: string
  passphrase: string
  deviceName?: string | null
}

export interface SetActiveSendSpaceRequest {
  profileId: string
}

const RUNTIME_STATES = new Set(['stopped', 'starting', 'running', 'locked', 'failed'])
const INCOMING_STATES = new Set(['enabled', 'receiving', 'degraded', 'disabled'])

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function invalidResponse(endpoint: string, field: string): DaemonApiError {
  return new DaemonApiError(
    DaemonErrorCode.INTERNAL_ERROR,
    `Invalid daemon response from ${endpoint}`,
    { field }
  )
}

function requiredString(
  value: unknown,
  endpoint: string,
  field: string,
  allowEmpty = true
): string {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)) {
    throw invalidResponse(endpoint, field)
  }
  return value
}

function optionalString(value: unknown, endpoint: string, field: string): string | null {
  if (value === undefined || value === null) return null
  return requiredString(value, endpoint, field)
}

function taggedState<T extends string>(
  value: unknown,
  allowed: Set<string>,
  endpoint: string,
  field: string
): { state: T } {
  if (!isRecord(value) || typeof value.state !== 'string' || !allowed.has(value.state)) {
    throw invalidResponse(endpoint, field)
  }
  return { state: value.state as T }
}

function parseFault(value: unknown, endpoint: string): SpaceFault | null {
  if (value === undefined || value === null) return null
  if (!isRecord(value)) throw invalidResponse(endpoint, 'lastFault')
  return {
    category: requiredString(value.category, endpoint, 'lastFault.category', false),
    messageCode: optionalString(value.messageCode, endpoint, 'lastFault.messageCode'),
  }
}

function parseSpaceSummary(value: unknown, endpoint: string): SpaceProfileSummary {
  if (!isRecord(value)) throw invalidResponse(endpoint, 'data')
  if (typeof value.isActiveSend !== 'boolean') {
    throw invalidResponse(endpoint, 'isActiveSend')
  }

  return {
    profileId: requiredString(value.profileId, endpoint, 'profileId', false),
    spaceId: optionalString(value.spaceId, endpoint, 'spaceId'),
    displayName: optionalString(value.displayName, endpoint, 'displayName'),
    deviceName: optionalString(value.deviceName, endpoint, 'deviceName'),
    runtimeState: taggedState<SpaceRuntimeState['state']>(
      value.runtimeState,
      RUNTIME_STATES,
      endpoint,
      'runtimeState'
    ),
    incomingSyncState: taggedState<SpaceIncomingSyncState['state']>(
      value.incomingSyncState,
      INCOMING_STATES,
      endpoint,
      'incomingSyncState'
    ),
    lastFault: parseFault(value.lastFault, endpoint),
    isActiveSend: value.isActiveSend,
  }
}

async function requestEnvelopeData(
  endpoint: string,
  method: string,
  body?: unknown
): Promise<unknown> {
  const envelope = await daemonClient.request<unknown>(endpoint, {
    method,
    ...(body === undefined ? {} : { body }),
  })
  if (!isRecord(envelope) || !Object.prototype.hasOwnProperty.call(envelope, 'data')) {
    throw invalidResponse(endpoint, 'data')
  }
  return envelope.data
}

export async function listSpaces(): Promise<SpaceProfileSummary[]> {
  const endpoint = '/v2/spaces'
  const data = await requestEnvelopeData(endpoint, 'GET')
  if (!Array.isArray(data)) throw invalidResponse(endpoint, 'data')
  return data.map(item => parseSpaceSummary(item, endpoint))
}

export async function createSpaceProfile(
  body: CreateSpaceProfileRequest
): Promise<SpaceProfileSummary> {
  const endpoint = '/v2/spaces'
  return parseSpaceSummary(await requestEnvelopeData(endpoint, 'POST', body), endpoint)
}

export async function joinSpaceProfile(
  body: JoinSpaceProfileRequest
): Promise<SpaceProfileSummary> {
  const endpoint = '/v2/spaces/join'
  return parseSpaceSummary(await requestEnvelopeData(endpoint, 'POST', body), endpoint)
}

export async function setActiveSendSpace(profileId: string): Promise<SpaceProfileSummary> {
  const endpoint = '/v2/spaces/active-send'
  const body: SetActiveSendSpaceRequest = { profileId }
  return parseSpaceSummary(await requestEnvelopeData(endpoint, 'PUT', body), endpoint)
}

export async function deleteSpaceProfile(profileId: string): Promise<SpaceProfileSummary> {
  const endpoint = `/v2/spaces/${encodeURIComponent(profileId)}`
  return parseSpaceSummary(await requestEnvelopeData(endpoint, 'DELETE'), endpoint)
}
