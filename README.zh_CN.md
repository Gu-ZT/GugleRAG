<div align="center">

<img src=".idea/icon.png" width="256" height="256" alt="GugleRAG 图标">

# GugleRAG

**面向人与 AI 智能体的自托管团队知识库。**

[English](README.md) | 简体中文

</div>

GugleRAG 是一个自托管团队知识库，提供 Markdown 文档、REST API，以及面向 AI 智能体的 MCP JSON-RPC 端点。

## 当前结构

```text
.
├── src/
│   ├── api/             # 按职责组织的 REST 处理器
│   ├── mcp/             # MCP JSON-RPC 端点与工具
│   ├── auth.rs          # JWT、密码哈希与账户验证
│   ├── config.rs        # 运行时与初始化配置
│   ├── db.rs            # SQLx 持久化
│   ├── domain.rs        # 共享领域模型
│   ├── error.rs         # HTTP 感知的应用错误
│   ├── search.rs        # 关键词检索与排序
│   ├── lib.rs           # 应用组合
│   └── main.rs          # 精简的可执行程序入口
├── tests/               # 后端集成测试
├── frontend/            # Vue 3 + TypeScript + Vite 前端
├── PLAN.md              # 产品路线图
└── AGENTS.md            # Agent/开发说明
```

后端通过 SQLx 持久化用户、工作区、团队、成员关系、知识库、文档、版本和邀请。运行时配置接受 SQLite、MySQL 和 PostgreSQL 的 `DATABASE_URL`。

## 首次运行

初始化界面使用 Vue 实现，后端不嵌入手写 HTML。

开发模式：

```bash
cargo run
cd frontend
npm install
npm run dev
```

打开 Vite 地址，通常为 `http://127.0.0.1:5173/`。如果 `.env` 不存在，Vue 应用会显示分步初始化向导，并通过 `/api/setup` 写入包含以下配置的 `.env`：

- `SERVER_HOST` 和 `SERVER_PORT`
- SQLite、MySQL 或 PostgreSQL 的 `DATABASE_URL`
- `JWT_SECRET`
- 嵌入模型与 SiliconFlow 设置
- 可选的重排器设置
- MCP 启用状态与认证要求
- 反向代理部署可选的 `MCP_PUBLIC_URL`

保存 `.env` 后重启后端。

生产/静态模式：

```bash
cd frontend
npm install
npm run build
cd ..
cargo run
```

后端将 `frontend/dist` 作为静态文件提供，并为 SPA 路由回退到 `frontend/dist/index.html`。

## 后端

```bash
cargo run
```

后端检查与测试：

```bash
cargo fmt -- --check
cargo check
cargo test
```

常用端点：

- `GET /health`
- `GET /api/setup/status`
- `POST /api/setup`
- `POST /api/auth/register`
- `POST /api/auth/login`
- `GET/PUT /api/admin/config`（仅管理员）
- `POST /api/admin/restart`（仅管理员）
- `GET /api/workspaces`
- `GET/POST /api/workspaces/{workspace_id}/knowledge-bases`
- `GET/POST /api/teams`
- `GET /api/teams/{team_id}/members`
- `POST /api/teams/{team_id}/invitations`
- `GET /api/invitations`
- `POST /api/invitations/{token}/accept`
- `GET/POST /api/documents`
- `GET/PUT/DELETE /api/documents/{id}`
- `GET /api/search?q=...`
- `POST /mcp`
- `POST /mcp/all`
- `POST /mcp/{user|group}/{workspace_id}`

## 协作与 MCP

每个用户都会获得一个个人工作区及其默认知识库。创建团队时会同时创建团队工作区和默认知识库；团队所有者和管理员可以按用户名邀请现有用户。邀请令牌可以分享给受邀用户，由其在**加入团队**对话框中接受。一个用户可以加入多个团队。

在 Vue 工作区中，使用左上角的选择器切换个人与团队工作区。相邻的 `+` 菜单包含创建团队、邀请成员和加入团队操作。侧边栏将当前工作区的每个知识库显示为可折叠分组，文章嵌套在其中；可以直接在这棵树中创建知识库和文章。

文档归属于知识库。文档和搜索请求接受 `knowledge_base_id`；省略时，为保持向后兼容，将使用个人默认知识库。

Vue 工作区通过 `POST /api/mcp/configs` 生成并复制稳定的 MCP 配置。个人和团队配置会显式指定工作区：

```json
{
  "scope": "user",
  "workspace_id": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
}
```

团队工作区使用 `scope: "group"` 和团队 `workspace_id`；访问账户可用的全部工作区时，使用不带 `workspace_id` 的 `scope: "all"`。响应遵循所请求的 streamable HTTP 格式：

```json
{
  "type": "streamable-http",
  "url": "http://127.0.0.1:8080/mcp/user/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
  "headers": {
    "Authorization": "Bearer eyJhbGciOiJIUzI1NiJ9..."
  }
}
```

个人或团队 URL 末尾的 UUID 是工作区 ID，而不是访问令牌。全工作区 URL 为 `/mcp/all`，末尾没有 ID。认证复用用户当前登录的 JWT，因此重复复制同一配置不会创建或轮换凭据。JWT 过期与退出登录行为和普通账户会话一致，并且每次 MCP 请求都会重新检查工作区访问权限。服务位于公共域名或反向代理之后时，请设置 `MCP_PUBLIC_URL`。

MCP 客户端可以在操作文档前发现资源：

- `list_workspaces()` 返回当前 MCP 作用域可见的工作区。
- `list_knowledge_bases(workspace_id)` 返回指定工作区中可见的知识库。

文档的读取、写入和列表工具要求提供单个 `workspace_id` 与 `knowledge_base_id`；文档专用工具还要求提供已有的 `doc_id`、`folder_id` 或内容字段。`search_knowledge` 的两个资源参数均可传入单个 UUID 或 UUID 数组。省略 `workspace_id` 时，会搜索当前 MCP 作用域中全部可见工作区；省略 `knowledge_base_id` 时，会搜索所选工作区中的全部知识库；同时省略两者时，会搜索全部可访问知识库。所有显式 ID 仍会按 MCP 作用域和知识库归属校验。搜索结果会包含 `workspace_id` 与 `knowledge_base_id`，可直接用于后续文档工具调用。

```json
{
  "name": "search_knowledge",
  "arguments": {
    "workspace_id": [
      "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
      "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
    ],
    "knowledge_base_id": [
      "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"
    ],
    "query": "deployment"
  }
}
```

## 检索配置

嵌入模型由 `EMBEDDING_PROVIDER`、`EMBEDDING_MODEL`、`SILICONFLOW_URL` 和 `SILICONFLOW_API_KEY` 控制。

重排功能为可选项，由以下配置控制：

- `RERANKER_ENABLED=true|false`
- `RERANKER_PROVIDER=local|siliconflow|custom_http`
- `RERANKER_MODEL=BAAI/bge-reranker-v2-m3`
- `RERANKER_URL=http://...`，用于自定义 HTTP 重排服务

## 前端

Markdown 预览使用禁用原始 HTML 的 `markdown-it`，随后通过 DOMPurify 清理渲染结果。它支持 note/warning 容器和只读的 GitHub 风格任务列表：

```markdown
:::note
这里填写部署细节。
:::

:::warning 可选标题
运行此命令前请检查生产数据库。
:::

- [ ] 待办任务
- [x] 已完成任务
```

任务复选框反映 Markdown 源文本中的状态，并且在预览模式下有意保持禁用。

```bash
cd frontend
npm install
npm run dev
```

Vite 开发服务器会将 `/api`、`/mcp` 和 `/health` 代理到 `http://127.0.0.1:8080`。

当前 Vue 工作区支持：

- 用户注册与登录
- 在本地存储中持久化令牌
- 切换个人和团队工作区
- 每个工作区包含多个知识库
- 创建团队、查看成员列表、发出邀请和接受邀请
- 列出、新建、编辑、保存和删除文档
- 编辑标签
- 按标题、内容和标签进行关键词搜索
- 在 Markdown 文本的编辑与预览模式之间切换

## 数据库 URL

支持的 URL 前缀：

- SQLite：`sqlite://data/guglerag.db?mode=rwc`
- MySQL：`mysql://user:password@127.0.0.1:3306/guglerag`
- PostgreSQL：`postgresql://user:password@127.0.0.1:5432/guglerag`

## CI 与发布

GitHub Actions 会在每个拉取请求以及每次推送到 `main` 时验证 Rust 后端、Vue 前端、发布工具和真实 HTTP 服务器。拉取请求只运行 CI，不发布产物。`main` 构建成功且六个原生包全部通过后，会发布预发布版本。支持的发布矩阵如下：

| 平台 | Runner | Rust target | 压缩包 |
| --- | --- | --- | --- |
| Linux x64 | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Linux ARM64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| Windows x64 | `windows-latest` | `x86_64-pc-windows-msvc` | `.zip` |
| Windows ARM64 | `windows-11-arm` | `aarch64-pc-windows-msvc` | `.zip` |
| macOS Apple Silicon | `macos-15` | `aarch64-apple-darwin` | `.tar.gz` |
| macOS Intel | `macos-15-intel` | `x86_64-apple-darwin` | `.tar.gz` |

每个目标都使用匹配的 GitHub 原生托管 Runner。CI 会解析每个 ELF、PE 或 Mach-O 文件头，验证打包的 CPU 架构，然后启动该二进制文件执行服务器冒烟测试。

每个压缩包命名为 `guglerag-v<version>-<platform>-<arch>.<format>`，并提供对应的 `.sha256` 文件。压缩包包含服务器可执行文件、`frontend/dist`、`.env.example`、两份更新日志、`README.md` 和 `RELEASE-METADATA.json`。这些是未签名的便携构建，运行前必须先解压。

`main` 分支的预发布版本使用 `v<manifest-version>-dev.<run_number>`，例如 `v0.1.0-dev.42`。重新运行工作流会保留相同的 GitHub run number 并复用同一发布版本，而不会创建重复版本。上传软件包期间，发布版本保持草稿状态；只有所有目标和双语发布说明都成功后，才会以预发布版本公开显示。

发布稳定版本：

1. 保持 `Cargo.toml`、`Cargo.lock`、`frontend/package.json` 和 `frontend/package-lock.json` 中的版本一致。
2. 在 `CHANGELOG.md` 和 `CHANGELOG.zh-CN.md` 中添加匹配的 `## [x.y.z]` 小节。
3. 推送精确标签 `vx.y.z`。

发布工作流会验证全部六个原生目标，创建便携压缩包和校验和，生成双语发布说明，并且仅在所有软件包成功后发布草稿。稳定版本使用精确的清单版本号；预发布产物会附加 CI 构建标识符，同时从匹配基础版本的更新日志小节读取发布说明。工作流使用仓库提供的 `GITHUB_TOKEN`，无需额外的密钥或签名凭据。
