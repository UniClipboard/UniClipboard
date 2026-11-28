import AsyncStorage from '@react-native-async-storage/async-storage';
import * as Application from 'expo-application';
import * as Crypto from 'expo-crypto';
import { Linking } from 'react-native';

import type { CloudStatusSnapshot } from '@/engine/types';

export const DEFAULT_CLOUD_URL = 'https://cloud.uniclipboard.app';
const LEGACY_CLOUD_SESSION_STORAGE_KEY = '@unic/cloud-session-v1';
const AUTH_TIMEOUT_MS = 5 * 60 * 1000;

export interface CloudAuthorizationGrant {
  cloudUrl?: string;
  code: string;
  codeVerifier: string;
  redirectUri: string;
  deviceName: string;
}

export function cloudStatusLabel(status?: CloudStatusSnapshot | null): string {
  const account = status?.account?.trim();
  if (account) return account;
  const email = status?.email?.trim();
  if (email) return email;
  return 'UniClipboard Cloud';
}

export async function clearLegacyCloudSession(): Promise<void> {
  try {
    await AsyncStorage.removeItem(LEGACY_CLOUD_SESSION_STORAGE_KEY);
  } catch {
    // Ignore storage cleanup failures; legacy JS tokens are never read again.
  }
}

function getRedirectUri(): string {
  const scheme =
    Application.applicationId === 'app.uniclipboard.shell'
      ? 'uniclipboard-ui-shell'
      : 'uniclipboard';
  return `${scheme}://auth/callback`;
}

function bytesToBase64Url(bytes: Uint8Array): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const b0 = bytes[i];
    const b1 = i + 1 < bytes.length ? bytes[i + 1] : 0;
    const b2 = i + 2 < bytes.length ? bytes[i + 2] : 0;
    const triplet = (b0 << 16) | (b1 << 8) | b2;
    out += alphabet[(triplet >> 18) & 0x3f];
    out += alphabet[(triplet >> 12) & 0x3f];
    if (i + 1 < bytes.length) out += alphabet[(triplet >> 6) & 0x3f];
    if (i + 2 < bytes.length) out += alphabet[triplet & 0x3f];
  }
  return out;
}

function parseCallbackParams(rawUrl: string): { code?: string; state?: string; error?: string } | null {
  const qIndex = rawUrl.indexOf('?');
  if (qIndex < 0) return null;
  const query = rawUrl.slice(qIndex + 1).split('#')[0];
  const params: Record<string, string> = {};
  for (const part of query.split('&')) {
    if (!part) continue;
    const eq = part.indexOf('=');
    if (eq < 0) continue;
    const key = decodeURIComponent(part.slice(0, eq));
    const val = decodeURIComponent(part.slice(eq + 1).replace(/\+/g, ' '));
    params[key] = val;
  }
  return {
    code: params.code,
    state: params.state,
    error: params.error,
  };
}

/**
 * Opens UniClipboard Cloud's browser PKCE authorization flow and returns the
 * temporary authorization grant so the Go Engine host completes token exchange,
 * session storage, and credential provisioning without exposing secrets to JS.
 */
export async function authorizeCloudLogin(options?: {
  cloudUrl?: string;
  deviceName?: string;
}): Promise<CloudAuthorizationGrant> {
  const explicitUrl = options?.cloudUrl?.trim().replace(/\/+$/, '');
  const baseUrl = explicitUrl || DEFAULT_CLOUD_URL;
  const deviceName = (options?.deviceName?.trim() || 'UniClipboard Mobile').slice(0, 120);
  const redirectUri = getRedirectUri();
  const codeVerifier = bytesToBase64Url(Crypto.getRandomValues(new Uint8Array(32)));
  const state = bytesToBase64Url(Crypto.getRandomValues(new Uint8Array(24)));
  const challengeBase64 = await Crypto.digestStringAsync(
    Crypto.CryptoDigestAlgorithm.SHA256,
    codeVerifier,
    { encoding: Crypto.CryptoEncoding.BASE64 }
  );
  const challenge = challengeBase64.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

  const authUrl =
    `${baseUrl}/cli/authorize` +
    `?redirect_uri=${encodeURIComponent(redirectUri)}` +
    `&state=${encodeURIComponent(state)}` +
    `&code_challenge=${encodeURIComponent(challenge)}` +
    `&code_challenge_method=S256` +
    `&device_name=${encodeURIComponent(deviceName)}`;

  const code = await new Promise<string>((resolve, reject) => {
    let settled = false;
    const cleanup = () => {
      subscription.remove();
      clearTimeout(timer);
    };
    const handleUrl = (url: string) => {
      if (settled || !url.startsWith(redirectUri)) return;
      const parsed = parseCallbackParams(url);
      if (!parsed) return;
      settled = true;
      cleanup();
      if (parsed.error) {
        reject(new Error(`Cloud sign-in canceled or failed (${parsed.error}).`));
        return;
      }
      if (!parsed.code || parsed.state !== state) {
        reject(new Error('Cloud sign-in state verification failed.'));
        return;
      }
      resolve(parsed.code);
    };

    const subscription = Linking.addEventListener('url', ({ url }) => handleUrl(url));
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(new Error('Cloud sign-in timed out.'));
    }, AUTH_TIMEOUT_MS);

    void Linking.openURL(authUrl).catch((err) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(err instanceof Error ? err : new Error('Failed to open browser for Cloud sign-in.'));
    });
  });

  return {
    cloudUrl: explicitUrl || undefined,
    code,
    codeVerifier,
    redirectUri,
    deviceName,
  };
}
