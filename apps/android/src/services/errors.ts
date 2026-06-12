/**
 * Custom Error Classes for API Operations
 */

/**
 * 基础 API 错误类
 */
export class APIError extends Error {
  constructor(
    message: string,
    public statusCode?: number,
    public response?: unknown
  ) {
    super(message);
    this.name = 'APIError';
    Object.setPrototypeOf(this, APIError.prototype);
  }
}

/**
 * 认证错误
 */
export class AuthenticationError extends APIError {
  constructor(message: string = 'Authentication failed') {
    super(message, 401);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

/**
 * 网络错误
 */
export class NetworkError extends APIError {
  constructor(
    message: string = 'Network request failed',
    public originalError?: unknown
  ) {
    super(message);
    this.name = 'NetworkError';
    Object.setPrototypeOf(this, NetworkError.prototype);
  }
}

/**
 * 服务器错误
 */
export class ServerError extends APIError {
  constructor(message: string, statusCode: number, response?: unknown) {
    super(message, statusCode, response);
    this.name = 'ServerError';
    Object.setPrototypeOf(this, ServerError.prototype);
  }
}

/**
 * 超时错误
 */
export class TimeoutError extends APIError {
  constructor(message: string = 'Request timeout') {
    super(message);
    this.name = 'TimeoutError';
    Object.setPrototypeOf(this, TimeoutError.prototype);
  }
}

/**
 * 配置错误
 */
export class ConfigurationError extends APIError {
  constructor(message: string = 'Invalid configuration') {
    super(message);
    this.name = 'ConfigurationError';
    Object.setPrototypeOf(this, ConfigurationError.prototype);
  }
}

/**
 * 数据验证错误
 */
export class ValidationError extends APIError {
  constructor(message: string = 'Data validation failed') {
    super(message, 400);
    this.name = 'ValidationError';
    Object.setPrototypeOf(this, ValidationError.prototype);
  }
}
