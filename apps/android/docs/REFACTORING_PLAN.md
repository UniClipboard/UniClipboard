# Services 文件夹重构计划

## 📋 文档信息

- **创建日期**: 2026-05-05
- **当前状态**: 规划阶段
- **预计工作量**: 3-5 次迭代

---

## 🎯 重构目标

将 `services` 文件夹重构为清晰的分层架构，遵循单一职责原则，提高代码可维护性和可测试性。

---

## 📊 当前状态分析

### 当前目录结构

```
src/services/
├── APIClient.ts                  # API 客户端基类
├── ApkDownloadService.ts         # APK 下载服务
├── AuthService.ts                # 认证服务
├── BackgroundServiceManager.ts   # 后台服务管理器
├── CacheManager.ts               # 缓存管理器
├── ClipboardManager.ts           # 剪贴板管理器
├── ClipboardMonitor.ts           # 剪贴板监听器
├── ClipboardSyncService.ts       # 剪贴板同步服务
├── ConfigStorage.ts              # 配置存储
├── HistoryAPI.ts                 # 历史记录 API（混合文件）
├── HistoryStorage.ts             # 历史记录存储
├── HistorySyncService.ts         # 历史记录同步服务
├── HistoryTransferQueue.ts       # 历史记录传输队列
├── Logger.ts                     # 日志服务
├── S3Client.ts                   # S3 客户端
├── SecureStorage.ts              # 安全存储
├── ShortcutService.ts            # 快捷方式服务
├── SyncClipboardClient.ts        # SyncClipboard 客户端
├── SyncManager.ts                # 同步管理器
├── UpdateService.ts              # 更新服务
├── WebDAVClient.ts               # WebDAV 客户端
├── errors.ts                     # 错误类定义
└── index.ts                      # 统一导出
```

### 问题识别

#### 1. 职责杂糅

**HistoryAPI.ts** - 最严重的问题文件

- ✗ 包含类型定义（`HistoryRecordDto`, `HistoryQueryParams` 等）
- ✗ 包含错误类（`SyncConflictError`, `RecordNotFoundError`）
- ✗ 包含工具函数（`dtoToClipboardItem`, `clipboardItemToDto`, `getProfileId`）
- ✗ 包含常量定义（`ProfileTypeFilter`）
- ✗ 包含接口定义（`IHistoryAPI`）

**建议**: 拆分为多个文件，移到合适的位置

#### 2. 重复功能

**formatFileSize 函数**

- `src/utils/clipboard.ts:173` - 实现版本 1
- `src/utils/index.ts:34` - 实现版本 2（带 decimals 参数）

**truncateText 函数**

- `src/utils/clipboard.ts:214` - 实现版本 1
- `src/utils/index.ts:58` - 实现版本 2

**建议**: 统一到 `utils/format.ts`，删除重复实现

#### 3. 依赖关系复杂

**ClipboardSyncService** 依赖链

```
ClipboardSyncService
  ├── SyncManager (管理同步)
  ├── HistorySyncService (历史同步)
  ├── HistoryTransferQueue (传输队列)
  ├── ConfigStorage (配置读取)
  └── useClipboardSyncServiceStore (状态管理)
```

**SyncManager** 依赖链

```
SyncManager
  ├── APIClient (API 调用)
  ├── ClipboardManager (剪贴板操作)
  ├── ClipboardMonitor (剪贴板监听)
  └── ConfigStorage (配置读取)
```

**HistorySyncService** 依赖链

```
HistorySyncService
  ├── HistoryStorage (本地存储)
  ├── HistoryAPI (API 调用)
  ├── HistoryTransferQueue (传输队列)
  └── ConfigStorage (配置读取)
```

#### 4. 文件位置不合理

以下文件应该移出 `services` 文件夹：

| 文件                     | 当前位置  | 建议位置         | 原因           |
| ------------------------ | --------- | ---------------- | -------------- |
| `APIClient.ts`           | services/ | api/clients/     | API 客户端基类 |
| `SyncClipboardClient.ts` | services/ | api/clients/     | API 客户端     |
| `WebDAVClient.ts`        | services/ | api/clients/     | API 客户端     |
| `S3Client.ts`            | services/ | api/clients/     | API 客户端     |
| `ConfigStorage.ts`       | services/ | storage/         | 存储服务       |
| `HistoryStorage.ts`      | services/ | storage/         | 存储服务       |
| `SecureStorage.ts`       | services/ | storage/         | 存储服务       |
| `CacheManager.ts`        | services/ | storage/         | 存储服务       |
| `Logger.ts`              | services/ | utils/logger/    | 工具服务       |
| `UpdateService.ts`       | services/ | utils/update/    | 工具服务       |
| `ApkDownloadService.ts`  | services/ | utils/apk/       | 工具服务       |
| `ShortcutService.ts`     | services/ | utils/shortcuts/ | 工具服务       |
| `AuthService.ts`         | services/ | api/auth/        | 认证相关       |

---

## 🎯 期望状态

### 目标目录结构

**设计原则**: 如果一个功能模块只有一个文件，则不创建子目录，直接放在父目录下。

```
src/
├── api/                              # API 层
│   ├── clients/                      # API 客户端（多个文件，需要子目录）
│   │   ├── APIClient.ts             # 基础客户端
│   │   ├── SyncClipboardClient.ts   # SyncClipboard 客户端
│   │   ├── WebDAVClient.ts          # WebDAV 客户端
│   │   └── S3Client.ts              # S3 客户端
│   ├── history/                      # 历史记录 API（多个文件，需要子目录）
│   │   ├── IHistoryAPI.ts           # 历史记录 API 接口
│   │   └── index.ts                 # 导出
│   ├── AuthService.ts               # 认证服务（单文件，直接放在 api 目录）
│   ├── types.ts                      # API 相关类型
│   └── index.ts                      # 统一导出
│
├── services/                         # 业务服务层
│   ├── clipboard/                    # 剪贴板相关服务（多个文件，需要子目录）
│   │   ├── ClipboardManager.ts      # 剪贴板管理
│   │   ├── ClipboardMonitor.ts      # 剪贴板监听
│   │   └── ClipboardSyncService.ts  # 剪贴板同步
│   ├── history/                      # 历史记录相关服务（多个文件，需要子目录）
│   │   ├── HistorySyncService.ts    # 历史记录同步
│   │   └── HistoryTransferQueue.ts  # 历史记录传输队列
│   ├── sync/                         # 同步相关服务（多个文件，需要子目录）
│   │   ├── SyncManager.ts           # 同步管理器
│   │   └── BackgroundServiceManager.ts # 后台服务管理器
│   └── index.ts                      # 统一导出
│
├── storage/                          # 存储层（多个文件，平级放置）
│   ├── ConfigStorage.ts             # 配置存储
│   ├── HistoryStorage.ts            # 历史记录存储
│   ├── SecureStorage.ts             # 安全存储
│   ├── CacheManager.ts              # 缓存管理
│   └── index.ts                      # 统一导出
│
├── utils/                            # 工具函数
│   ├── format/                       # 格式化工具（多个函数，需要子目录）
│   │   ├── format.ts                # 格式化函数（统一）
│   │   └── index.ts
│   ├── clipboard/                    # 剪贴板工具函数（多个文件，需要子目录）
│   │   ├── dtoConvert.ts            # DTO 转换函数
│   │   ├── profileId.ts             # ProfileId 工具
│   │   └── index.ts
│   ├── Logger.ts                    # 日志服务（单文件，直接放在 utils 目录）
│   ├── UpdateService.ts             # 更新服务（单文件，直接放在 utils 目录）
│   ├── ApkDownloadService.ts        # APK 下载（单文件，直接放在 utils 目录）
│   ├── ShortcutService.ts           # 快捷方式（单文件，直接放在 utils 目录）
│   └── index.ts
│
├── errors/                           # 错误处理（多个文件，平级放置）
│   ├── APIError.ts                  # API 错误基类
│   ├── AuthenticationError.ts       # 认证错误
│   ├── NetworkError.ts              # 网络错误
│   ├── HistoryErrors.ts             # 历史记录相关错误
│   └── index.ts
│
└── types/                            # 类型定义（保持）
    ├── api.ts
    ├── clipboard.ts
    ├── history.ts                   # 新增：历史记录相关类型
    ├── storage.ts
    ├── sync.ts
    └── index.ts
```

**设计说明**:

1. **单文件模块**: `Logger.ts`, `UpdateService.ts`, `ApkDownloadService.ts`, `ShortcutService.ts`, `AuthService.ts` 等单文件模块直接放在父目录下，不创建子目录
2. **多文件模块**: `api/clients/`, `services/clipboard/`, `services/history/`, `services/sync/`, `utils/format/`, `utils/clipboard/` 等包含多个文件的模块创建子目录
3. **平级文件**: `storage/`, `errors/` 等目录下的文件是平级的，不需要子目录

---

## 📝 分阶段重构计划

### 阶段 1: 准备工作（低风险）

**目标**: 创建新目录结构，不修改现有代码

**任务**:

1. ✅ 创建新目录结构

   - `src/api/`
   - `src/storage/`
   - `src/errors/`
   - `src/utils/format/`
   - `src/utils/clipboard/`

2. ✅ 创建占位文件
   - 在每个新目录创建 `index.ts`

**风险**: 无风险，只是创建目录

**验证**: 运行 `npm run type-check` 和 `npm run lint`

---

### 阶段 2: 移动独立模块（低风险）

**目标**: 移动没有复杂依赖的文件

#### 2.1 移动存储服务

**可移动文件**:

- `ConfigStorage.ts` → `storage/ConfigStorage.ts`
- `HistoryStorage.ts` → `storage/HistoryStorage.ts`
- `SecureStorage.ts` → `storage/SecureStorage.ts`
- `CacheManager.ts` → `storage/CacheManager.ts`

**依赖分析**:

- ✅ 这些文件主要依赖 `types/` 和第三方库
- ✅ 被其他服务依赖，但不依赖其他服务
- ⚠️ 需要更新导入路径

**操作步骤**:

1. 移动文件到 `storage/` 目录
2. 更新 `storage/index.ts` 导出
3. 更新所有导入路径（使用 IDE 的重构功能）
4. 运行测试

**风险**: 低风险，主要是路径更新

---

#### 2.2 移动 API 客户端

**可移动文件**:

- `APIClient.ts` → `api/clients/APIClient.ts`
- `SyncClipboardClient.ts` → `api/clients/SyncClipboardClient.ts`
- `WebDAVClient.ts` → `api/clients/WebDAVClient.ts`
- `S3Client.ts` → `api/clients/S3Client.ts`
- `AuthService.ts` → `api/auth/AuthService.ts`

**依赖分析**:

- ✅ API 客户端主要依赖 `types/` 和错误类
- ✅ 被其他服务依赖，但不依赖其他服务
- ⚠️ 需要更新导入路径

**操作步骤**:

1. 移动文件到 `api/` 目录
2. 更新 `api/index.ts` 导出
3. 更新所有导入路径
4. 运行测试

**风险**: 低风险，主要是路径更新

---

#### 2.3 移动工具服务

**可移动文件**:

- `Logger.ts` → `utils/logger/Logger.ts`
- `UpdateService.ts` → `utils/update/UpdateService.ts`
- `ApkDownloadService.ts` → `utils/apk/ApkDownloadService.ts`
- `ShortcutService.ts` → `utils/shortcuts/ShortcutService.ts`

**依赖分析**:

- ✅ 这些服务独立性强
- ✅ 被其他服务依赖，但不依赖其他服务
- ⚠️ 需要更新导入路径

**操作步骤**:

1. 移动文件到对应目录
2. 更新各目录的 `index.ts` 导出
3. 更新所有导入路径
4. 运行测试

**风险**: 低风险，主要是路径更新

---

### 阶段 3: 拆分混合文件（中等风险）

**目标**: 拆分职责不清晰的文件

#### 3.1 拆分 HistoryAPI.ts

**当前内容**:

```typescript
// HistoryAPI.ts 包含：
- 类型定义（HistoryRecordDto, HistoryQueryParams 等）
- 错误类（SyncConflictError, RecordNotFoundError）
- 工具函数（dtoToClipboardItem, clipboardItemToDto, getProfileId）
- 常量定义（ProfileTypeFilter）
- 接口定义（IHistoryAPI）
```

**拆分方案**:

1. **类型定义** → `types/history.ts` (新建)

   ```typescript
   // types/history.ts
   export interface HistoryRecordDto { ... }
   export interface HistoryQueryParams { ... }
   export interface HistoryStatisticsDto { ... }
   ```

2. **错误类** → `errors/HistoryErrors.ts`

   ```typescript
   // errors/HistoryErrors.ts
   export class SyncConflictError extends Error { ... }
   export class RecordNotFoundError extends Error { ... }
   ```

3. **工具函数** → `utils/clipboard/dtoConvert.ts` 和 `utils/clipboard/profileId.ts`

   ```typescript
   // utils/clipboard/dtoConvert.ts
   export function dtoToClipboardItem(dto: HistoryRecordDto): ClipboardItem { ... }
   export function clipboardItemToDto(item: ClipboardItem): HistoryRecordDto { ... }

   // utils/clipboard/profileId.ts
   export function getProfileId(type: string, hash: string): string { ... }
   export function parseProfileId(profileId: string): { type, hash } | null { ... }
   ```

4. **常量定义** → `types/history.ts` 或 `constants/history.ts`

   ```typescript
   // types/history.ts 或 constants/history.ts
   export const ProfileTypeFilter = { ... }
   ```

5. **接口定义** → `api/history/IHistoryAPI.ts`
   ```typescript
   // api/history/IHistoryAPI.ts
   export interface IHistoryAPI { ... }
   ```

**操作步骤**:

1. 创建新文件
2. 从 `HistoryAPI.ts` 提取内容到新文件
3. 更新 `HistoryAPI.ts` 为只导出接口（或删除）
4. 更新所有导入路径
5. 运行测试

**风险**: 中等风险，需要仔细处理依赖关系

---

#### 3.2 拆分错误类

**当前内容**:

```typescript
// errors.ts 包含：
-APIError -
  AuthenticationError -
  NetworkError -
  ServerError -
  ValidationError -
  TimeoutError -
  ConfigurationError;
```

**拆分方案**:

```
errors/
├── APIError.ts           # APIError 基类
├── AuthenticationError.ts # 认证错误
├── NetworkError.ts       # 网络错误
├── ServerError.ts        # 服务器错误
├── ValidationError.ts    # 验证错误
├── TimeoutError.ts       # 超时错误
├── ConfigurationError.ts # 配置错误
├── HistoryErrors.ts      # 历史记录错误（从 HistoryAPI.ts 移入）
└── index.ts              # 统一导出
```

**操作步骤**:

1. 创建新文件
2. 从 `errors.ts` 提取内容到新文件
3. 更新所有导入路径
4. 运行测试

**风险**: 低风险，主要是路径更新

---

### 阶段 4: 解决重复功能（低风险）

**目标**: 统一重复的工具函数

#### 4.1 统一 formatFileSize

**当前状态**:

- `utils/clipboard.ts:173` - 版本 1（无 decimals 参数）
- `utils/index.ts:34` - 版本 2（带 decimals 参数）

**解决方案**:

1. 创建 `utils/format/format.ts`
2. 实现统一版本（保留 decimals 参数，默认值 1）
3. 删除 `utils/clipboard.ts` 和 `utils/index.ts` 中的重复实现
4. 更新所有导入路径

**建议实现**:

```typescript
// utils/format/format.ts
export function formatFileSize(bytes: number, decimals: number = 1): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(decimals)} ${sizes[i]}`;
}
```

---

#### 4.2 统一 truncateText

**当前状态**:

- `utils/clipboard.ts:214` - 版本 1
- `utils/index.ts:58` - 版本 2

**解决方案**:

1. 在 `utils/format/format.ts` 中实现统一版本
2. 删除重复实现
3. 更新所有导入路径

---

### 阶段 5: 重组业务服务（高风险）

**目标**: 按业务领域重组服务文件

#### 5.1 创建剪贴板服务目录

**操作**:

1. 创建 `services/clipboard/` 目录
2. 移动文件：
   - `ClipboardManager.ts` → `services/clipboard/ClipboardManager.ts`
   - `ClipboardMonitor.ts` → `services/clipboard/ClipboardMonitor.ts`
   - `ClipboardSyncService.ts` → `services/clipboard/ClipboardSyncService.ts`

**依赖分析**:

- ⚠️ `ClipboardMonitor` 依赖 `ClipboardManager`
- ⚠️ `ClipboardSyncService` 依赖 `SyncManager`
- ⚠️ 需要更新大量导入路径

**风险**: 高风险，需要仔细测试

---

#### 5.2 创建历史记录服务目录

**操作**:

1. 创建 `services/history/` 目录
2. 移动文件：
   - `HistorySyncService.ts` → `services/history/HistorySyncService.ts`
   - `HistoryTransferQueue.ts` → `services/history/HistoryTransferQueue.ts`

**依赖分析**:

- ⚠️ `HistorySyncService` 依赖 `HistoryStorage`, `HistoryAPI`, `HistoryTransferQueue`
- ⚠️ 需要更新大量导入路径

**风险**: 高风险，需要仔细测试

---

#### 5.3 创建同步服务目录

**操作**:

1. 创建 `services/sync/` 目录
2. 移动文件：
   - `SyncManager.ts` → `services/sync/SyncManager.ts`
   - `BackgroundServiceManager.ts` → `services/sync/BackgroundServiceManager.ts`

**依赖分析**:

- ⚠️ `SyncManager` 依赖多个服务和客户端
- ⚠️ `BackgroundServiceManager` 依赖 `ClipboardSyncService`
- ⚠️ 需要更新大量导入路径

**风险**: 高风险，需要仔细测试

---

### 阶段 6: 清理和优化（低风险）

**目标**: 清理遗留文件，优化导出

**任务**:

1. 删除空的或不再需要的文件
2. 优化 `index.ts` 导出结构
3. 添加 JSDoc 注释
4. 更新 README 文档

**风险**: 低风险

---

## 📊 风险评估

### 低风险操作（可立即执行）

- ✅ 创建新目录结构
- ✅ 移动存储服务
- ✅ 移动 API 客户端
- ✅ 移动工具服务
- ✅ 拆分错误类
- ✅ 统一重复函数

### 中等风险操作（需要仔细测试）

- ⚠️ 拆分 HistoryAPI.ts
- ⚠️ 更新复杂的导入路径

### 高风险操作（需要全面测试）

- ❗ 重组业务服务
- ❗ 处理循环依赖

---

## ✅ 验证清单

每次重构后需要验证：

1. **类型检查**

   ```bash
   npm run type-check
   ```

2. **代码检查**

   ```bash
   npm run lint
   ```

3. **功能测试**

   - 测试剪贴板同步功能
   - 测试历史记录同步功能
   - 测试文件上传下载
   - 测试后台同步

4. **导入路径检查**
   - 确保所有导入路径正确
   - 确保没有循环依赖

---

## 📝 执行记录

### 执行记录模板

```markdown
#### [日期] - 阶段 X.X: [任务名称]

**执行内容**:

- [ ] 任务 1
- [ ] 任务 2

**修改文件**:

- 文件 1: 描述
- 文件 2: 描述

**验证结果**:

- ✅ type-check 通过
- ✅ lint 通过
- ✅ 功能测试通过

**遇到的问题**:

- 问题 1: 解决方案

**下一步**:

- 下一步任务
```

---

## 🎯 成功标准

重构成功的标准：

1. ✅ 所有文件职责清晰，单一职责原则
2. ✅ 目录结构符合分层架构
3. ✅ 没有重复代码
4. ✅ 依赖关系清晰，没有循环依赖
5. ✅ 所有测试通过
6. ✅ 类型检查和 lint 通过
7. ✅ 功能完全正常

---

## 📚 参考资料

- [前端架构设计最佳实践](https://example.com)
- [单一职责原则](https://example.com)
- [依赖注入模式](https://example.com)

---

## 📞 联系方式

如有问题，请联系项目负责人。

---

**最后更新**: 2026-05-05
