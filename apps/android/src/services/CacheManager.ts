/**
 * Cache Manager
 * 缓存管理器 - 管理临时数据缓存
 */

import AsyncStorage from '@react-native-async-storage/async-storage';
import { CacheItem, CacheConfig, DEFAULT_CACHE_CONFIG, STORAGE_KEYS } from '../types/storage';

/**
 * 缓存管理器
 */
export class CacheManager {
  private static instance: CacheManager | null = null;
  private cache: Map<string, CacheItem> = new Map();
  private config: CacheConfig = DEFAULT_CACHE_CONFIG;
  private cleanupTimer: NodeJS.Timeout | null = null;
  private initialized = false;

  private constructor() {}

  /**
   * 获取单例实例
   */
  public static getInstance(): CacheManager {
    if (!CacheManager.instance) {
      CacheManager.instance = new CacheManager();
    }
    return CacheManager.instance;
  }

  /**
   * 初始化缓存管理器
   */
  public async initialize(config?: Partial<CacheConfig>): Promise<void> {
    if (this.initialized) {
      return;
    }

    if (config) {
      this.config = { ...DEFAULT_CACHE_CONFIG, ...config };
    }

    try {
      await this.loadCache();
      this.startCleanupTimer();
      this.initialized = true;
    } catch (error) {
      console.error('[CacheManager] Failed to initialize:', error);
      this.cache = new Map();
      this.initialized = true;
    }
  }

  /**
   * 销毁缓存管理器
   */
  public async destroy(): Promise<void> {
    this.stopCleanupTimer();
    await this.saveCache();
    this.cache.clear();
    this.initialized = false;
  }

  /**
   * 加载缓存
   */
  private async loadCache(): Promise<void> {
    try {
      const keys = await AsyncStorage.getAllKeys();
      const cacheKeys = keys.filter((key) => key.startsWith(STORAGE_KEYS.CACHE_PREFIX));

      if (cacheKeys.length > 0) {
        const entries = await AsyncStorage.multiGet(cacheKeys);

        for (const [key, value] of entries) {
          if (value) {
            const item = JSON.parse(value) as CacheItem;
            const cacheKey = key.replace(STORAGE_KEYS.CACHE_PREFIX, '');
            this.cache.set(cacheKey, item);
          }
        }
      }

      // 立即清理过期缓存
      await this.cleanup();
    } catch (error) {
      console.error('[CacheManager] Failed to load cache:', error);
    }
  }

  /**
   * 保存缓存
   */
  private async saveCache(): Promise<void> {
    try {
      const entries: [string, string][] = [];

      this.cache.forEach((item, key) => {
        entries.push([`${STORAGE_KEYS.CACHE_PREFIX}${key}`, JSON.stringify(item)]);
      });

      if (entries.length > 0) {
        await AsyncStorage.multiSet(entries);
      }
    } catch (error) {
      console.error('[CacheManager] Failed to save cache:', error);
    }
  }

  /**
   * 获取缓存项
   */
  public async get<T = unknown>(key: string): Promise<T | null> {
    if (!this.initialized) {
      await this.initialize();
    }

    const item = this.cache.get(key);

    if (!item) {
      return null;
    }

    // 检查是否过期
    if (item.expiresAt && Date.now() > item.expiresAt) {
      await this.delete(key);
      return null;
    }

    // 更新访问信息
    item.accessCount++;
    item.lastAccessedAt = Date.now();

    return item.value as T;
  }

  /**
   * 设置缓存项
   */
  public async set<T = unknown>(key: string, value: T, ttl?: number): Promise<void> {
    if (!this.initialized) {
      await this.initialize();
    }

    const now = Date.now();
    const expiresAt = ttl ? now + ttl : now + this.config.defaultTTL;

    const item: CacheItem<T> = {
      key,
      value,
      createdAt: now,
      expiresAt,
      accessCount: 0,
      lastAccessedAt: now,
    };

    this.cache.set(key, item as CacheItem);

    // 检查缓存大小限制
    if (this.cache.size > this.config.maxSize) {
      await this.evictLRU();
    }

    // 保存到持久化存储
    await AsyncStorage.setItem(`${STORAGE_KEYS.CACHE_PREFIX}${key}`, JSON.stringify(item));
  }

  /**
   * 检查缓存是否存在
   */
  public async has(key: string): Promise<boolean> {
    if (!this.initialized) {
      await this.initialize();
    }

    const item = this.cache.get(key);

    if (!item) {
      return false;
    }

    // 检查是否过期
    if (item.expiresAt && Date.now() > item.expiresAt) {
      await this.delete(key);
      return false;
    }

    return true;
  }

  /**
   * 删除缓存项
   */
  public async delete(key: string): Promise<void> {
    this.cache.delete(key);
    await AsyncStorage.removeItem(`${STORAGE_KEYS.CACHE_PREFIX}${key}`);
  }

  /**
   * 批量删除缓存项
   */
  public async deleteMany(keys: string[]): Promise<void> {
    keys.forEach((key) => this.cache.delete(key));

    const storageKeys = keys.map((key) => `${STORAGE_KEYS.CACHE_PREFIX}${key}`);
    await AsyncStorage.multiRemove(storageKeys);
  }

  /**
   * 清空所有缓存
   */
  public async clear(): Promise<void> {
    const keys = await AsyncStorage.getAllKeys();
    const cacheKeys = keys.filter((key) => key.startsWith(STORAGE_KEYS.CACHE_PREFIX));

    if (cacheKeys.length > 0) {
      await AsyncStorage.multiRemove(cacheKeys);
    }

    this.cache.clear();
  }

  /**
   * 获取所有缓存键
   */
  public async keys(): Promise<string[]> {
    if (!this.initialized) {
      await this.initialize();
    }

    return Array.from(this.cache.keys());
  }

  /**
   * 获取缓存大小
   */
  public size(): number {
    return this.cache.size;
  }

  /**
   * 清理过期缓存
   */
  private async cleanup(): Promise<number> {
    const now = Date.now();
    const expiredKeys: string[] = [];

    this.cache.forEach((item, key) => {
      if (item.expiresAt && now > item.expiresAt) {
        expiredKeys.push(key);
      }
    });

    if (expiredKeys.length > 0) {
      await this.deleteMany(expiredKeys);
    }

    return expiredKeys.length;
  }

  /**
   * 启动清理定时器
   */
  private startCleanupTimer(): void {
    this.stopCleanupTimer();

    this.cleanupTimer = setInterval(() => {
      this.cleanup().catch((error) => {
        console.error('[CacheManager] Cleanup failed:', error);
      });
    }, this.config.cleanupInterval);
  }

  /**
   * 停止清理定时器
   */
  private stopCleanupTimer(): void {
    if (this.cleanupTimer) {
      clearInterval(this.cleanupTimer);
      this.cleanupTimer = null;
    }
  }

  /**
   * 驱逐最少使用的缓存项（LRU）
   */
  private async evictLRU(): Promise<void> {
    if (this.cache.size === 0) {
      return;
    }

    let lruKey: string | null = null;
    let lruTime = Infinity;

    this.cache.forEach((item, key) => {
      if (item.lastAccessedAt < lruTime) {
        lruTime = item.lastAccessedAt;
        lruKey = key;
      }
    });

    if (lruKey) {
      await this.delete(lruKey);
    }
  }

  /**
   * 获取缓存统计信息
   */
  public getStats(): {
    size: number;
    maxSize: number;
    hitRate: number;
    totalAccess: number;
    avgAccessCount: number;
  } {
    let totalAccess = 0;
    let totalAccessCount = 0;

    this.cache.forEach((item) => {
      totalAccess++;
      totalAccessCount += item.accessCount;
    });

    return {
      size: this.cache.size,
      maxSize: this.config.maxSize,
      hitRate: totalAccess > 0 ? totalAccessCount / totalAccess : 0,
      totalAccess,
      avgAccessCount: totalAccess > 0 ? totalAccessCount / totalAccess : 0,
    };
  }

  /**
   * 获取缓存项信息（用于调试）
   */
  public getItemInfo(key: string): CacheItem | null {
    return this.cache.get(key) || null;
  }

  /**
   * 更新配置
   */
  public async updateConfig(config: Partial<CacheConfig>): Promise<void> {
    this.config = { ...this.config, ...config };

    // 重启清理定时器
    this.startCleanupTimer();

    // 检查缓存大小
    while (this.cache.size > this.config.maxSize) {
      await this.evictLRU();
    }
  }
}

// 导出单例
export const cacheManager = CacheManager.getInstance();
