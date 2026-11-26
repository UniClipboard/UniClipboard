import type { EngineDevice } from '@/engine/types';

export type DeviceStatusKind = 'connected' | 'paused' | 'connecting' | 'offline' | 'noBinding';

export function deviceDisplayName(device: EngineDevice | null | undefined): string {
  if (!device) return '';
  return device.name || device.reported_name || device.id.slice(0, 8);
}

/**
 * True when this device requires explicit user fingerprint approval:
 * either the device itself is pending trust, or a trusted device has a new pending binding.
 */
export function needsDeviceApproval(device: EngineDevice): boolean {
  if (device.trust === 'pending') return true;
  return Boolean(device.bindings?.some((b) => b.state === 'pending'));
}

/**
 * Stable snapshot key representing the exact pending approval state of a device.
 * Changes whenever the device revision or any pending binding revision changes,
 * so dismissing a stale prompt still allows a new pairing attempt to surface automatically.
 */
export function deviceApprovalKey(device: EngineDevice): string {
  const pendingBindings = (device.bindings ?? [])
    .filter((b) => b.state === 'pending')
    .map((b) => `${b.id}@${b.revision}`)
    .sort()
    .join(',');
  return `${device.id}@${device.revision}:${pendingBindings}`;
}

export function isDeviceConnected(device: EngineDevice | null | undefined): boolean {
  if (!device) return false;
  return Boolean(device.bindings?.some((b) => b.connection === 'connected'));
}

export function deviceStatusKind(device: EngineDevice): DeviceStatusKind {
  const binding = device.bindings?.find((b) => b.state === 'authorized') ?? device.bindings?.[0];
  if (!binding) return 'noBinding';
  if (binding.connection === 'connected') {
    return binding.sync_enabled ? 'connected' : 'paused';
  }
  if (binding.connection === 'connecting') return 'connecting';
  return 'offline';
}

export function sortTrustedDevices(devices: readonly EngineDevice[]): EngineDevice[] {
  return [...devices].sort((left, right) => {
    const rank = (d: EngineDevice) => {
      const kind = deviceStatusKind(d);
      if (kind === 'connected') return 0;
      if (kind === 'connecting') return 1;
      if (kind === 'paused') return 2;
      return 3;
    };
    const diff = rank(left) - rank(right);
    if (diff !== 0) return diff;
    return deviceDisplayName(left).localeCompare(deviceDisplayName(right));
  });
}

export function formatFingerprintLines(value: string): string {
  const groups = value.trim().split(/\s+/).filter(Boolean);
  if (groups.length === 8) {
    return `${groups.slice(0, 4).join(' ')}\n${groups.slice(4).join(' ')}`;
  }
  return value;
}

export function formatFingerprintShort(value: string): string {
  const groups = value.trim().split(/\s+/).filter(Boolean);
  if (groups.length >= 4) {
    return `${groups.slice(0, 2).join(' ')} ··· ${groups.slice(-2).join(' ')}`;
  }
  return value;
}

export function formatPairingCodeDisplay(code: string): string {
  const clean = code.replace(/\D/g, '');
  if (clean.length === 6) {
    return `${clean.slice(0, 3)} ${clean.slice(3)}`;
  }
  return code;
}

export function formatCountdown(expiresAtMs: number, nowMs = Date.now()): string {
  const remainMs = Math.max(0, expiresAtMs - nowMs);
  const totalSec = Math.ceil(remainMs / 1000);
  const mins = Math.floor(totalSec / 60);
  const secs = totalSec % 60;
  return `${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
}

export type PairingInputClassification =
  | { kind: 'code'; normalized: string }
  | { kind: 'ticket'; normalized: string }
  | { kind: 'invalid'; normalized: string };

export function classifyPairingInput(rawInput: string): PairingInputClassification {
  const raw = rawInput.trim();
  const compact = raw.replace(/[\s-]+/g, '');
  if (/^\d{6}$/.test(compact)) {
    return { kind: 'code', normalized: `${compact.slice(0, 3)}-${compact.slice(3)}` };
  }
  if (raw.startsWith('unic-pair:') && raw.length > 'unic-pair:'.length) {
    return { kind: 'ticket', normalized: raw };
  }
  return { kind: 'invalid', normalized: raw };
}
