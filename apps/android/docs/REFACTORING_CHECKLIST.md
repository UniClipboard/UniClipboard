# 重构执行清单

## ✅ 可立即执行的任务（低风险）

### 阶段 1: 创建新目录结构

**预计时间**: 10 分钟

**任务清单**:

- [ ] 创建 `src/api/` 目录
- [ ] 创建 `src/api/clients/` 目录
- [ ] 创建 `src/api/history/` 目录
- [ ] 创建 `src/storage/` 目录
- [ ] 创建 `src/errors/` 目录
- [ ] 创建 `src/utils/format/` 目录
- [ ] 创建 `src/utils/clipboard/` 目录

**设计原则**: 单文件模块不创建子目录，直接放在父目录下

**验证命令**:

```bash
npm run type-check
npm run lint
```

**风险**: 无风险，只是创建目录

---

### 阶段 2.1: 移动存储服务

**预计时间**: 30 分钟

**任务清单**:

- [ ] 移动 `ConfigStorage.ts` → `storage/ConfigStorage.ts`
- [ ] 移动 `HistoryStorage.ts` → `storage/HistoryStorage.ts`
- [ ] 移动 `SecureStorage.ts` → `storage/SecureStorage.ts`
- [ ] 移动 `CacheManager.ts` → `storage/CacheManager.ts`
- [ ] 创建 `storage/index.ts` 导出文件
- [ ] 更新 `services/index.ts` 中的导出
- [ ] 使用 IDE 重构功能更新所有导入路径

**需要更新的文件**:

- `services/ClipboardSyncService.ts`
- `services/ClipboardManager.ts`
- `services/HistorySyncService.ts`
- `services/HistoryTransferQueue.ts`
- `services/SyncManager.ts`
- `services/BackgroundServiceManager.ts`
- `stores/settingsStore.ts`
- `stores/clipboardStore.ts`
- 其他引用这些存储的文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] 配置读取和保存
- [ ] 历史记录存储
- [ ] 安全存储
- [ ] 缓存管理

**风险**: 低风险，主要是路径更新

---

### 阶段 2.2: 移动 API 客户端

**预计时间**: 30 分钟

**任务清单**:

- [ ] 移动 `APIClient.ts` → `api/clients/APIClient.ts`
- [ ] 移动 `SyncClipboardClient.ts` → `api/clients/SyncClipboardClient.ts`
- [ ] 移动 `WebDAVClient.ts` → `api/clients/WebDAVClient.ts`
- [ ] 移动 `S3Client.ts` → `api/clients/S3Client.ts`
- [ ] 移动 `AuthService.ts` → `api/AuthService.ts`（单文件，直接放在 api 目录）
- [ ] 创建 `api/clients/index.ts` 导出文件
- [ ] 创建 `api/index.ts` 导出文件
- [ ] 更新 `services/index.ts` 中的导出
- [ ] 使用 IDE 重构功能更新所有导入路径

**需要更新的文件**:

- `services/SyncManager.ts`
- `services/ClipboardSyncService.ts`
- `services/index.ts`
- 其他引用这些客户端的文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] SyncClipboard 服务器连接
- [ ] WebDAV 连接
- [ ] S3 连接
- [ ] 认证功能

**风险**: 低风险，主要是路径更新

---

### 阶段 2.3: 移动工具服务

**预计时间**: 20 分钟

**任务清单**:

- [ ] 移动 `Logger.ts` → `utils/Logger.ts`（单文件，直接放在 utils 目录）
- [ ] 移动 `UpdateService.ts` → `utils/UpdateService.ts`（单文件，直接放在 utils 目录）
- [ ] 移动 `ApkDownloadService.ts` → `utils/ApkDownloadService.ts`（单文件，直接放在 utils 目录）
- [ ] 移动 `ShortcutService.ts` → `utils/ShortcutService.ts`（单文件，直接放在 utils 目录）
- [ ] 更新 `utils/index.ts` 导出
- [ ] 更新 `services/index.ts` 中的导出
- [ ] 使用 IDE 重构功能更新所有导入路径

**需要更新的文件**:

- 引用这些服务的所有文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] 日志功能
- [ ] 更新检查
- [ ] APK 下载
- [ ] 快捷方式创建

**风险**: 低风险，主要是路径更新

---

### 阶段 3.2: 拆分错误类

**预计时间**: 30 分钟

**任务清单**:

- [ ] 创建 `errors/APIError.ts`
- [ ] 创建 `errors/AuthenticationError.ts`
- [ ] 创建 `errors/NetworkError.ts`
- [ ] 创建 `errors/ServerError.ts`
- [ ] 创建 `errors/ValidationError.ts`
- [ ] 创建 `errors/TimeoutError.ts`
- [ ] 创建 `errors/ConfigurationError.ts`
- [ ] 创建 `errors/index.ts` 导出文件
- [ ] 从 `services/errors.ts` 提取内容到新文件
- [ ] 更新所有导入路径
- [ ] 删除或保留 `services/errors.ts`（作为兼容层）

**需要更新的文件**:

- 所有引用 `services/errors` 的文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] 错误处理逻辑
- [ ] 错误类型判断

**风险**: 低风险，主要是路径更新

---

### 阶段 4.1: 统一 formatFileSize

**预计时间**: 15 分钟

**任务清单**:

- [ ] 创建 `utils/format/format.ts`
- [ ] 实现统一的 `formatFileSize` 函数
- [ ] 删除 `utils/clipboard.ts` 中的重复实现
- [ ] 删除 `utils/index.ts` 中的重复实现
- [ ] 创建 `utils/format/index.ts` 导出文件
- [ ] 更新所有导入路径

**统一实现**:

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

**需要更新的文件**:

- 引用 `formatFileSize` 的所有文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] 文件大小显示

**风险**: 低风险

---

### 阶段 4.2: 统一 truncateText

**预计时间**: 10 分钟

**任务清单**:

- [ ] 在 `utils/format/format.ts` 中添加 `truncateText` 函数
- [ ] 删除 `utils/clipboard.ts` 中的重复实现
- [ ] 删除 `utils/index.ts` 中的重复实现
- [ ] 更新所有导入路径

**统一实现**:

```typescript
// utils/format/format.ts
export function truncateText(text: string, maxLength: number = 100): string {
  if (text.length <= maxLength) return text;
  return text.substring(0, maxLength) + '...';
}
```

**需要更新的文件**:

- 引用 `truncateText` 的所有文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] 文本截断显示

**风险**: 低风险

---

## ⚠️ 需要仔细规划的任务（中等风险）

### 阶段 3.1: 拆分 HistoryAPI.ts

**预计时间**: 1 小时

**前置条件**:

- ✅ 完成阶段 1（创建目录）
- ✅ 完成阶段 2.2（移动 API 客户端）
- ✅ 完成阶段 3.2（拆分错误类）

**任务清单**:

- [ ] 创建 `types/history.ts` 并提取类型定义
- [ ] 创建 `errors/HistoryErrors.ts` 并提取错误类
- [ ] 创建 `utils/clipboard/dtoConvert.ts` 并提取 DTO 转换函数
- [ ] 创建 `utils/clipboard/profileId.ts` 并提取 ProfileId 工具函数
- [ ] 创建 `api/history/IHistoryAPI.ts` 并提取接口定义
- [ ] 更新 `types/history.ts` 或创建 `constants/history.ts` 存放常量
- [ ] 更新所有导入路径
- [ ] 删除或保留原 `HistoryAPI.ts`

**详细步骤**:

#### 步骤 1: 提取类型定义

```typescript
// types/history.ts
export interface HistoryRecordDto {
  hash: string;
  type: 'Text' | 'Image' | 'File';
  text?: string;
  createTime?: string;
  lastModified?: string;
  lastAccessed?: string;
  starred?: boolean;
  pinned?: boolean;
  size?: number;
  hasData?: boolean;
  version?: number;
  isDeleted?: boolean;
}

export interface HistoryRecordUpdateDto {
  starred?: boolean;
  pinned?: boolean;
  isDelete?: boolean;
  version?: number;
  lastModified?: string;
  lastAccessed?: string;
}

export interface HistoryQueryParams {
  page?: number;
  before?: string;
  after?: string;
  modifiedAfter?: string;
  types?: number;
  searchText?: string;
  starred?: boolean;
  sortByLastAccessed?: boolean;
}

export interface HistoryStatisticsDto {
  totalCount: number;
  starredCount: number;
  deletedCount: number;
  activeCount: number;
  totalFileSizeMB: number;
}

export const ProfileTypeFilter = {
  Text: 1,
  Image: 2,
  File: 4,
  Group: 8,
  All: 15,
  FileAndGroup: 12,
};
```

#### 步骤 2: 提取错误类

```typescript
// errors/HistoryErrors.ts
import { HistoryRecordDto } from '@/types/history';

export class SyncConflictError extends Error {
  public readonly serverRecord: HistoryRecordDto;

  constructor(message: string, serverRecord: HistoryRecordDto) {
    super(message);
    this.name = 'SyncConflictError';
    this.serverRecord = serverRecord;
  }
}

export class RecordNotFoundError extends Error {
  public readonly profileId: string;

  constructor(profileId: string) {
    super(`Record not found: ${profileId}`);
    this.name = 'RecordNotFoundError';
    this.profileId = profileId;
  }
}
```

#### 步骤 3: 提取工具函数

```typescript
// utils/clipboard/dtoConvert.ts
import { HistoryRecordDto } from '@/types/history';
import { ClipboardItem, HistorySyncStatus } from '@/types/clipboard';
import { ClipboardContentType } from '@/types/api';

export function dtoToClipboardItem(dto: HistoryRecordDto): ClipboardItem {
  return {
    type: dto.type as ClipboardContentType,
    text: dto.text || '',
    profileHash: dto.hash,
    hasData: dto.hasData || false,
    size: dto.size ?? 0,
    timestamp: dto.createTime ? new Date(dto.createTime).getTime() : Date.now(),
    starred: dto.starred ?? false,
    pinned: dto.pinned ?? false,
    syncStatus: HistorySyncStatus.Synced,
    version: dto.version ?? 0,
    lastModified: dto.lastModified ? new Date(dto.lastModified).getTime() : Date.now(),
    lastAccessed: dto.lastAccessed ? new Date(dto.lastAccessed).getTime() : Date.now(),
    isDeleted: dto.isDeleted ?? false,
    hasRemoteData: dto.hasData ?? false,
    isLocalFileReady: false,
  };
}

export function clipboardItemToDto(item: ClipboardItem): HistoryRecordDto {
  const hash = item.profileHash.includes('-')
    ? item.profileHash.split('-').slice(1).join('-')
    : item.profileHash;

  return {
    hash,
    type: item.type as 'Text' | 'Image' | 'File',
    text: item.text,
    createTime: item.timestamp ? new Date(item.timestamp).toISOString() : undefined,
    lastModified: item.lastModified ? new Date(item.lastModified).toISOString() : undefined,
    lastAccessed: item.lastAccessed ? new Date(item.lastAccessed).toISOString() : undefined,
    starred: item.starred,
    pinned: item.pinned,
    size: item.size,
    hasData: item.hasData,
    version: item.version,
    isDeleted: item.isDeleted,
  };
}
```

```typescript
// utils/clipboard/profileId.ts
export function getProfileId(type: string, hash: string): string {
  return `${type}-${hash}`;
}

export function parseProfileId(
  profileId: string
): { type: 'Text' | 'Image' | 'File'; hash: string } | null {
  const parts = profileId.split('-');
  if (parts.length < 2) {
    return null;
  }
  const type = parts[0] as 'Text' | 'Image' | 'File';
  const hash = parts.slice(1).join('-');
  if (!['Text', 'Image', 'File'].includes(type)) {
    return null;
  }
  return { type, hash };
}
```

#### 步骤 4: 提取接口定义

```typescript
// api/history/IHistoryAPI.ts
import {
  HistoryRecordDto,
  HistoryRecordUpdateDto,
  HistoryQueryParams,
  HistoryStatisticsDto,
} from '@/types/history';
import { ProgressInfo } from 'native-util';

export interface IHistoryAPI {
  getHistory(params?: HistoryQueryParams): Promise<HistoryRecordDto[]>;
  getRecord(profileId: string): Promise<HistoryRecordDto>;
  createRecord(record: HistoryRecordDto): Promise<HistoryRecordDto>;
  updateRecord(profileId: string, update: HistoryRecordUpdateDto): Promise<HistoryRecordDto>;
  deleteRecord(profileId: string): Promise<void>;
  getStatistics(): Promise<HistoryStatisticsDto>;
  downloadData(profileId: string, onProgress?: (progress: ProgressInfo) => void): Promise<string>;
  uploadData(
    profileId: string,
    localPath: string,
    onProgress?: (progress: ProgressInfo) => void
  ): Promise<void>;
}
```

**需要更新的文件**:

- `services/HistorySyncService.ts`
- `services/HistoryTransferQueue.ts`
- `services/SyncClipboardClient.ts`
- 其他引用 `HistoryAPI` 的文件

**验证命令**:

```bash
npm run type-check
npm run lint
```

**测试功能**:

- [ ] 历史记录同步
- [ ] DTO 转换
- [ ] ProfileId 生成和解析
- [ ] 错误处理

**风险**: 中等风险，需要仔细处理依赖关系

---

## ❗ 需要全面测试的任务（高风险）

### 阶段 5: 重组业务服务

**预计时间**: 2-3 小时

**前置条件**:

- ✅ 完成所有低风险和中等风险任务
- ✅ 全面测试通过

**任务清单**:

- [ ] 创建 `services/clipboard/` 目录
- [ ] 创建 `services/history/` 目录
- [ ] 创建 `services/sync/` 目录
- [ ] 移动剪贴板相关服务
- [ ] 移动历史记录相关服务
- [ ] 移动同步相关服务
- [ ] 更新所有导入路径
- [ ] 全面功能测试

**详细步骤**: 参见 REFACTORING_PLAN.md 阶段 5

**风险**: 高风险，需要全面测试

---

## 📝 执行记录模板

每次执行任务后，记录执行情况：

```markdown
### 执行记录 - [日期]

**执行阶段**: 阶段 X.X
**任务名称**: [任务名称]
**执行人**: [姓名]
**执行时间**: [开始时间] - [结束时间]

**完成任务**:

- [x] 任务 1
- [x] 任务 2
- [ ] 任务 3（未完成，原因：...）

**修改文件**:

- 文件 1: 描述修改内容
- 文件 2: 描述修改内容

**验证结果**:

- ✅ type-check 通过
- ✅ lint 通过
- ✅ 功能测试通过
  - [ ] 测试项 1
  - [ ] 测试项 2

**遇到的问题**:

1. 问题描述
   - 解决方案：...
   - 影响：...

**下一步计划**:

- 下一步要执行的任务
- 预计时间

**备注**:

- 其他需要记录的信息
```

---

## 🎯 完成标准

每个阶段完成的标志：

1. ✅ 所有任务清单项完成
2. ✅ `npm run type-check` 通过
3. ✅ `npm run lint` 通过
4. ✅ 功能测试通过
5. ✅ 执行记录已填写
6. ✅ 代码已提交（可选）

---

**最后更新**: 2026-05-05
