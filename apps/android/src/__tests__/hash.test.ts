import * as Crypto from 'expo-crypto';
import { sha256 } from 'js-sha256';

jest.mock('expo-crypto');
jest.mock('js-sha256');

describe('Hash Utils', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('calculateTextHash', () => {
    const { calculateTextHash } = require('../utils/hash');

    it('should calculate hash for empty string', async () => {
      const mockHasher = {
        update: jest.fn().mockReturnThis(),
        hex: jest
          .fn()
          .mockReturnValue('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'),
      };
      (sha256.create as unknown as jest.Mock).mockReturnValue(mockHasher);

      const result = await calculateTextHash('');
      expect(result).toBe('E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855');
    });

    it('should return empty string for null input', async () => {
      const result = await calculateTextHash(null as unknown as string);
      expect(result).toBe('');
    });

    it('should calculate hash using js-sha256', async () => {
      const mockHasher = {
        update: jest.fn().mockReturnThis(),
        hex: jest.fn().mockReturnValue('abc123'),
      };
      (sha256.create as unknown as jest.Mock).mockReturnValue(mockHasher);

      const result = await calculateTextHash('test text');

      expect(sha256.create).toHaveBeenCalled();
      expect(mockHasher.update).toHaveBeenCalledWith('test text');
      expect(mockHasher.hex).toHaveBeenCalled();
      expect(result).toBe('ABC123');
    });

    it('should throw AbortError when signal is aborted before', async () => {
      const abortController = new AbortController();
      abortController.abort();

      await expect(calculateTextHash('test', abortController.signal)).rejects.toThrow(
        'Operation was aborted'
      );
    });

    it('should throw AbortError when signal is aborted during', async () => {
      const mockHasher = {
        update: jest.fn().mockImplementation(() => {
          throw new Error('Operation was aborted');
        }),
        hex: jest.fn().mockReturnValue('abc123'),
      };
      (sha256.create as unknown as jest.Mock).mockReturnValue(mockHasher);

      const abortController = new AbortController();

      await expect(calculateTextHash('test', abortController.signal)).rejects.toThrow();
    });
  });

  describe('calculateBase64Hash', () => {
    const { calculateBase64Hash } = require('../utils/hash');

    it('should return empty string for empty input', async () => {
      const result = await calculateBase64Hash('');
      expect(result).toBe('');
    });

    it('should calculate hash using expo-crypto', async () => {
      (Crypto.digestStringAsync as jest.Mock).mockResolvedValue('abc123');

      const result = await calculateBase64Hash('dGVzdA==');

      expect(Crypto.digestStringAsync).toHaveBeenCalledWith(
        Crypto.CryptoDigestAlgorithm.SHA256,
        'dGVzdA==',
        { encoding: Crypto.CryptoEncoding.HEX }
      );
      expect(result).toBe('ABC123');
    });

    it('should throw AbortError when signal is aborted', async () => {
      const abortController = new AbortController();
      abortController.abort();

      await expect(calculateBase64Hash('dGVzdA==', abortController.signal)).rejects.toThrow(
        'Operation was aborted'
      );
    });
  });

  describe('calculateBase64ContentHash', () => {
    const { calculateBase64ContentHash } = require('../utils/hash');

    it('should return empty string for empty input', async () => {
      const result = await calculateBase64ContentHash('');
      expect(result).toBe('');
    });

    it('should decode base64 and calculate hash', async () => {
      (sha256 as unknown as jest.Mock).mockReturnValue('abc123');

      const result = await calculateBase64ContentHash('dGVzdA==');

      expect(sha256).toHaveBeenCalled();
      expect(result).toBe('ABC123');
    });

    it('should throw AbortError when signal is aborted', async () => {
      const abortController = new AbortController();
      abortController.abort();

      await expect(calculateBase64ContentHash('dGVzdA==', abortController.signal)).rejects.toThrow(
        'Operation was aborted'
      );
    });
  });
});
