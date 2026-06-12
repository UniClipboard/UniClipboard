jest.mock('expo-crypto', () => ({
  digestStringAsync: jest.fn(),
  CryptoDigestAlgorithm: {
    SHA256: 'SHA-256',
  },
  CryptoEncoding: {
    HEX: 'hex',
  },
}));

jest.mock('expo-clipboard', () => ({
  getStringAsync: jest.fn(),
  setStringAsync: jest.fn(),
  getImageAsync: jest.fn(),
  setImageAsync: jest.fn(),
}));

jest.mock('expo-file-system', () => ({
  File: jest.fn().mockImplementation(() => ({
    info: jest.fn().mockReturnValue({ exists: true, size: 1000 }),
    open: jest.fn().mockReturnValue({
      readBytes: jest.fn().mockReturnValue(new Uint8Array(10)),
      close: jest.fn(),
    }),
  })),
  DocumentDirectory: 'file://documents/',
  CacheDirectory: 'file://cache/',
}));

jest.mock('@react-native-async-storage/async-storage', () => ({
  default: {
    getItem: jest.fn(),
    setItem: jest.fn(),
    removeItem: jest.fn(),
    clear: jest.fn(),
  },
}));

jest.mock('native-util', () => ({
  isNativeHashModuleAvailable: jest.fn().mockReturnValue(false),
  nativeCalculateFileHash: jest.fn(),
}));

jest.mock('@microsoft/signalr', () => ({
  HubConnectionBuilder: jest.fn().mockImplementation(() => ({
    withUrl: jest.fn().mockReturnThis(),
    withAutomaticReconnect: jest.fn().mockReturnThis(),
    configureLogging: jest.fn().mockReturnThis(),
    build: jest.fn(),
  })),
}));

global.setImmediate = jest.useRealTimers;
