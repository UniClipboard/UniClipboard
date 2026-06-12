# Bug 修复工作流：测试先行（TDD 风格）

本文档定义了修复 Bug 时的标准工作流程。**核心原则：先写失败测试，再改代码，最后验证。**

## 流程概览

```
1. 复现与定位 → 2. 编写失败测试 → 3. 修复代码 → 4. 验证全部通过 → 5. Lint + Type-check
```

## 详细步骤

### 步骤 1：复现与定位

1. 理解 Bug 现象：用户描述的预期行为 vs 实际行为
2. 追踪代码路径：从触发点出发，沿调用链定位到 Bug 所在的具体代码
3. 确认根因：明确是哪段逻辑导致了错误行为

**输出**：明确的 Bug 根因和涉及的代码位置。

### 步骤 2：编写失败测试

在修改任何业务代码之前，先创建测试用例：

1. **评估可测试性**：如果 Bug 逻辑嵌在组件闭包或难以直接调用的函数中，先提取为独立的可测试模块
2. **提取核心逻辑**（按需）：
   - 将 Bug 所在的核心逻辑提取到 `src/utils/` 下的独立函数
   - 使用依赖注入接口（接口参数）隔离外部依赖（存储、网络、服务等）
   - **提取时保持 Bug 原样**，不修复任何行为
3. **编写测试**：
   - 在 `src/__tests__/` 下创建对应的测试文件
   - 测试描述**期望的正确行为**，而非当前的错误行为
   - 至少包含：Bug 场景的核心测试、边界情况、正常路径的回归测试
4. **运行测试，确认失败**：Bug 相关的测试必须失败，其他测试应通过

```bash
npx jest src/__tests__/<测试文件>.test.ts --no-coverage
# 期望：Bug 场景测试 FAIL，其他 PASS
```

**输出**：失败的测试用例，明确展示了 Bug 行为与预期行为的差异。

### 步骤 3：修复代码

1. 修改提取出的工具函数，使 Bug 场景的测试通过
2. 更新原调用方（如组件）使用修复后的工具函数
3. 清理不再需要的 import

**原则**：

- 最小化修改范围，只改必要的代码
- 不引入不相关的重构或优化

### 步骤 4：运行测试

```bash
npx jest src/__tests__/<测试文件>.test.ts --no-coverage
# 期望：全部 PASS
```

如果仍有失败，回到步骤 3 继续修复。

### 步骤 5：Lint + Type-check

对所有修改和新增的文件运行检查：

```bash
# Lint（自动修复格式问题）
npx eslint --fix <修改的文件列表>

# 类型检查
npx tsc --noEmit
```

两项检查必须全部通过。

## 提取核心逻辑的指导原则

当 Bug 逻辑位于以下位置时，需要提取：

| 场景                     | 做法                                |
| ------------------------ | ----------------------------------- |
| React 组件内部的闭包函数 | 提取为 `src/utils/` 下的纯函数      |
| 依赖全局状态或服务的逻辑 | 通过依赖注入接口传入，测试时用 mock |
| 多个职责混合在一起       | 仅提取与 Bug 相关的判断逻辑         |

**提取模板**：

```typescript
// src/utils/xxxLogic.ts

// 依赖注入接口
export interface XxxDeps {
  getSomeState: () => SomeType;
  queryStorage: (key: string) => Promise<Result | null>;
}

// 纯函数，可独立测试
export async function xxxLogic(input: InputType, deps: XxxDeps): Promise<OutputType> {
  // 核心逻辑
}
```

## 测试编写指导

```typescript
// src/__tests__/xxxLogic.test.ts

describe('xxxLogic', () => {
  let deps: XxxDeps;

  beforeEach(() => {
    deps = {
      getSomeState: jest.fn().mockReturnValue(defaultValue),
      queryStorage: jest.fn().mockResolvedValue(null),
    };
  });

  describe('Bug 场景', () => {
    it('应该表现出正确行为（修复前会失败）', async () => {
      // Arrange: 设置触发 Bug 的条件
      // Act: 调用被测函数
      // Assert: 验证期望的正确行为
    });
  });

  describe('正常路径（回归测试）', () => {
    it('应保持原有正确行为不变', async () => {
      // 确保修复不会破坏其他功能
    });
  });

  describe('边界情况', () => {
    it('应优雅处理异常', async () => {
      // 错误处理、空值、极端输入等
    });
  });
});
```

## 检查清单

修复完成前，确认以下所有项：

- [ ] Bug 根因已明确，能用一句话解释
- [ ] 核心逻辑已提取为可测试的纯函数（如需要）
- [ ] 测试在修复前确实失败
- [ ] 测试在修复后全部通过
- [ ] 原调用方已更新为使用修复后的函数
- [ ] `npx eslint` 无错误
- [ ] `npx tsc --noEmit` 无错误
