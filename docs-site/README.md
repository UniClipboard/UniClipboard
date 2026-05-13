# docs-site

This is a Next.js application generated with
[Create Fumadocs](https://github.com/fuma-nama/fumadocs).

Run development server:

```bash
npm run dev
# or
pnpm dev
# or
yarn dev
```

Open http://localhost:3000 with your browser to see the result.

## Explore

In the project, you can see:

- `lib/source.ts`: Code for content source adapter, [`loader()`](https://fumadocs.dev/docs/headless/source-api) provides the interface to access your content.
- `lib/layout.shared.tsx`: Shared options for layouts, optional but preferred to keep.

| Route                     | Description                                            |
| ------------------------- | ------------------------------------------------------ |
| `app/(home)`              | The route group for your landing page and other pages. |
| `app/docs`                | The documentation layout and pages.                    |
| `app/api/search/route.ts` | The Route Handler for search.                          |

### Fumadocs MDX

A `source.config.ts` config file has been included, you can customise different options like frontmatter schema.

Read the [Introduction](https://fumadocs.dev/docs/mdx) for further details.

## Operations

### Consistency check

```bash
bun run check:docs
```

巡检 `content/docs/{en,zh}/` —— 双语镜像（同名文件必须在两边都存在）、相对链接 `(./...#anchor)` 的目标文件 + 锚点能否解析、是否还残留 MDX 3 不接受的裸 autolink `<https://...>`。CI 与 PR 前手动跑一次。

### Sitemap & site URL

`src/app/sitemap.ts` 在构建时枚举所有页面并生成 `/sitemap.xml`，包含 hreflang 替代语言。绝对域名按以下优先级解析：

1. `NEXT_PUBLIC_SITE_URL`（手动设定，最稳）
2. `VERCEL_PROJECT_PRODUCTION_URL`（Vercel 生产部署自动注入）
3. `VERCEL_URL`（Vercel 预览）
4. `http://localhost:3000`（兜底）

部署到自定义域名后请把 `NEXT_PUBLIC_SITE_URL=https://your-domain` 写到 Vercel 的项目环境变量里。

### Ask AI

站内 Ask AI 使用 Fumadocs 的 AI 搜索组件与 Vercel AI SDK。通过 `AI_PROVIDER` 选择模型接口。

OpenAI-compatible 接口：

```bash
AI_PROVIDER=openai
OPENAI_API_KEY=...
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_MODEL=...
OPENAI_PROVIDER_NAME=openai-compatible
```

`OPENAI_BASE_URL` 必须是兼容 OpenAI Chat Completions 的 `/v1` 前缀；使用官方 OpenAI 时可以省略。`OPENAI_MODEL` 填接口服务商提供的模型 ID。

Anthropic 接口：

```bash
AI_PROVIDER=anthropic
ANTHROPIC_API_KEY=...
ANTHROPIC_BASE_URL=https://api.anthropic.com/v1
ANTHROPIC_MODEL=...
ANTHROPIC_PROVIDER_NAME=
```

`ANTHROPIC_BASE_URL` 可指向兼容 Anthropic Messages API 的代理；使用官方 Anthropic 时可以省略。`ANTHROPIC_MODEL` 填接口服务商提供的模型 ID。

### Versioning（路线建议，未实施）

当前 `content/docs/{en,zh}/` 是 **单一当前版本**。未来需要并存多版本（如 `0.7` 与 `0.8`）时，推荐路径相对克制：

- **A. 路径分版本**：`content/docs/{en,zh}/v0.8/...`，最新版另用 `current` 软链或 alias，rewrites 把 `/docs/...` 指向 current。fumadocs 原生支持。
- **B. 分支分版本**：每个 release 分支构建独立站点，主域 + 子路径（如 `/docs/0.8/`）暴露。运维更重，但隔离最干净。

在 alpha 阶段（破坏性变更频繁）建议 **先不引入版本化**，等 1.0 后再上 A 方案。引入版本化时需要同步更新：sitemap 列举（按当前版本）、check-docs.mjs（按版本树枚举）、`gitConfig.branch`（按版本 branch）。

## Learn More

To learn more about Next.js and Fumadocs, take a look at the following
resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js
  features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.
- [Fumadocs](https://fumadocs.dev) - learn about Fumadocs
