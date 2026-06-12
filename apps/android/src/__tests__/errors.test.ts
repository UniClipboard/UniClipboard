import {
  APIError,
  AuthenticationError,
  NetworkError,
  ServerError,
  TimeoutError,
  ConfigurationError,
  ValidationError,
} from '../services/errors';

describe('Error Classes', () => {
  describe('APIError', () => {
    it('should create an APIError with message only', () => {
      const error = new APIError('Something went wrong');
      expect(error.message).toBe('Something went wrong');
      expect(error.name).toBe('APIError');
      expect(error.statusCode).toBeUndefined();
      expect(error.response).toBeUndefined();
    });

    it('should create an APIError with message and statusCode', () => {
      const error = new APIError('Bad Request', 400);
      expect(error.message).toBe('Bad Request');
      expect(error.statusCode).toBe(400);
    });

    it('should create an APIError with all parameters', () => {
      const response = { error: 'details' };
      const error = new APIError('Server Error', 500, response);
      expect(error.message).toBe('Server Error');
      expect(error.statusCode).toBe(500);
      expect(error.response).toEqual(response);
    });

    it('should be instanceof Error', () => {
      const error = new APIError('Test');
      expect(error instanceof Error).toBe(true);
    });
  });

  describe('AuthenticationError', () => {
    it('should create an AuthenticationError with default message', () => {
      const error = new AuthenticationError();
      expect(error.message).toBe('Authentication failed');
      expect(error.statusCode).toBe(401);
      expect(error.name).toBe('AuthenticationError');
    });

    it('should create an AuthenticationError with custom message', () => {
      const error = new AuthenticationError('Invalid token');
      expect(error.message).toBe('Invalid token');
      expect(error.statusCode).toBe(401);
    });

    it('should be instanceof APIError', () => {
      const error = new AuthenticationError();
      expect(error instanceof APIError).toBe(true);
    });
  });

  describe('NetworkError', () => {
    it('should create a NetworkError with default message', () => {
      const error = new NetworkError();
      expect(error.message).toBe('Network request failed');
      expect(error.name).toBe('NetworkError');
    });

    it('should create a NetworkError with custom message', () => {
      const error = new NetworkError('Connection refused');
      expect(error.message).toBe('Connection refused');
    });

    it('should store original error', () => {
      const originalError = new Error('Original');
      const error = new NetworkError('Failed', originalError);
      expect(error.originalError).toBe(originalError);
    });

    it('should be instanceof APIError', () => {
      const error = new NetworkError();
      expect(error instanceof APIError).toBe(true);
    });
  });

  describe('ServerError', () => {
    it('should create a ServerError with message and statusCode', () => {
      const error = new ServerError('Internal Server Error', 500);
      expect(error.message).toBe('Internal Server Error');
      expect(error.statusCode).toBe(500);
      expect(error.name).toBe('ServerError');
    });

    it('should create a ServerError with response', () => {
      const response = { message: 'Error details' };
      const error = new ServerError('Error', 500, response);
      expect(error.response).toEqual(response);
    });

    it('should be instanceof APIError', () => {
      const error = new ServerError('Error', 500);
      expect(error instanceof APIError).toBe(true);
    });
  });

  describe('TimeoutError', () => {
    it('should create a TimeoutError with default message', () => {
      const error = new TimeoutError();
      expect(error.message).toBe('Request timeout');
      expect(error.name).toBe('TimeoutError');
    });

    it('should create a TimeoutError with custom message', () => {
      const error = new TimeoutError('Connection timeout');
      expect(error.message).toBe('Connection timeout');
    });

    it('should be instanceof APIError', () => {
      const error = new TimeoutError();
      expect(error instanceof APIError).toBe(true);
    });
  });

  describe('ConfigurationError', () => {
    it('should create a ConfigurationError with default message', () => {
      const error = new ConfigurationError();
      expect(error.message).toBe('Invalid configuration');
      expect(error.name).toBe('ConfigurationError');
    });

    it('should create a ConfigurationError with custom message', () => {
      const error = new ConfigurationError('Missing API key');
      expect(error.message).toBe('Missing API key');
    });

    it('should be instanceof APIError', () => {
      const error = new ConfigurationError();
      expect(error instanceof APIError).toBe(true);
    });
  });

  describe('ValidationError', () => {
    it('should create a ValidationError with default message', () => {
      const error = new ValidationError();
      expect(error.message).toBe('Data validation failed');
      expect(error.statusCode).toBe(400);
      expect(error.name).toBe('ValidationError');
    });

    it('should create a ValidationError with custom message', () => {
      const error = new ValidationError('Invalid email format');
      expect(error.message).toBe('Invalid email format');
      expect(error.statusCode).toBe(400);
    });

    it('should be instanceof APIError', () => {
      const error = new ValidationError();
      expect(error instanceof APIError).toBe(true);
    });
  });
});
