<div align="center">

<img src=".idea/icon.png" width="256" height="256" alt="GugleRAG 图标">

# GugleRAG

**面向人与 AI 智能体的自托管团队知识库。**

[English](README.md) | 简体中文

</div>

GugleRAG 是一个自托管团队知识库，提供 Markdown 文档、REST API，以及面向 AI 智能体的 MCP JSON-RPC 端点。

## 界面截图

### 首次初始化

| 服务                                                               | 数据库                                                               |
|--------------------------------------------------------------------|----------------------------------------------------------------------|
| <img src="docs/init-01.jpeg" alt="首次初始化服务步骤" width="420"> | <img src="docs/init-02.jpeg" alt="首次初始化数据库步骤" width="420"> |

| 检索                                                               | MCP                                                                 |
|--------------------------------------------------------------------|---------------------------------------------------------------------|
| <img src="docs/init-03.jpeg" alt="首次初始化检索步骤" width="420"> | <img src="docs/init-04.jpeg" alt="首次初始化 MCP 步骤" width="420"> |

### 账户流程

| 登录                                                   | 注册                                                      |
|--------------------------------------------------------|-----------------------------------------------------------|
| <img src="docs/login.jpeg" alt="登录界面" width="420"> | <img src="docs/register.jpeg" alt="注册界面" width="420"> |

### 知识库工作区

| 工作区                                                          | 新建文档                                                          |
|-----------------------------------------------------------------|-------------------------------------------------------------------|
| <img src="docs/empty-page.jpeg" alt="工作区空状态" width="420"> | <img src="docs/create_doc.jpeg" alt="新建文档对话框" width="420"> |

| 编辑文档                                                    | 预览文档                                                              |
|-------------------------------------------------------------|-----------------------------------------------------------------------|
| <img src="docs/edit_doc.jpeg" alt="文档编辑器" width="420"> | <img src="docs/preview_doc.jpeg" alt="Markdown 文档预览" width="420"> |

### 协作与管理

| 创建团队                                                           | 加入团队                                                         |
|--------------------------------------------------------------------|------------------------------------------------------------------|
| <img src="docs/create_team.jpeg" alt="创建团队对话框" width="420"> | <img src="docs/join_team.jpeg" alt="加入团队对话框" width="420"> |

| 服务配置                                                              | 用户管理                                                               |
|-----------------------------------------------------------------------|------------------------------------------------------------------------|
| <img src="docs/admin_settings.jpeg" alt="管理员服务配置" width="420"> | <img src="docs/user-management.jpeg" alt="管理员用户管理" width="420"> |

| MCP 配置                                                   |
|------------------------------------------------------------|
| <img src="docs/mcp.jpeg" alt="MCP 配置对话框" width="840"> |

## 当前结构

```text
.
├── src/
│   ├── api/             # 按职责组织的 REST 处理器
│   ├── mcp/             # MCP JSON-RPC 端点与工具
│   ├── auth.rs          # JWT、密码哈希与账户验证
│   ├── config.rs        # 运行时与初始化配置
│   ├── db.rs            # SQLx 持久化
│   ├── desktop.rs        # Windows 桌面启动时的系统托盘
│   ├── domain.rs        # 共享领域模型
│   ├── error.rs         # HTTP 感知的应用错误
│   ├── embedding.rs     # 嵌入服务提供方客户端
│   ├── logging.rs       # 滚动文件与控制台日志
│   ├── reranker.rs      # 可选重排服务客户端
│   ├── search.rs        # 持久化向量检索与排序
│   ├── lib.rs           # 应用组合
│   └── main.rs          # 精简的可执行程序入口
├── tests/               # 后端集成测试
├── frontend/            # Vue 3 + TypeScript + Vite 前端
├── PLAN.md              # 产品路线图
└── AGENTS.md            # Agent/开发说明
```

后端通过 SQLx 持久化用户、工作区、团队、成员关系、知识库、文档、版本、邀请和文档元数据。运行时配置接受 SQLite、MySQL 和
PostgreSQL 的 `DATABASE_URL`。活动向量检索默认使用 Rust 内嵌的 HNSW 索引：每个知识库在 `VECTOR_INDEX_PATH`（默认
`data/vector-index`）下保存一个二进制索引文件，因此不需要额外部署向量数据库服务。目标 PostgreSQL 安装 `pgvector` 扩展后，
也可以通过独立的 `VECTOR_DATABASE_URL` 将向量存入 PostgreSQL。SQL 仍是文档内容和权限的事实来源，两种向量后端都是可重建的派生索引。

## 首次运行

初始化界面使用 Vue 实现，后端不嵌入手写 HTML。

开发模式：

```bash
cargo run
cd frontend
npm install
npm run dev
```

打开 Vite 地址，通常为 `http://127.0.0.1:5173/`。如果 `.env` 不存在，Vue 应用会显示分步初始化向导，并通过 `/api/setup
` 写入包含以下配置的 `.env`：

- `SERVER_HOST` 和 `SERVER_PORT`
- SQLite、MySQL 或 PostgreSQL 的 `DATABASE_URL`
- `JWT_SECRET`
- 嵌入模型、完整的 SiliconFlow 调用地址与相关设置
- 可选的 PostgreSQL `pgvector` 向量数据库连接串
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

### Windows 桌面启动

在 Windows 资源管理器中双击 `GugleRAG.exe` 会以无命令行窗口的方式启动。HTTP 服务成功绑定监听地址后，GugleRAG 会创建系统托盘图标；鼠标悬浮时可查看实际监听 URL，右键菜单中的“退出 GugleRAG”会优雅停止服务。通过终端启动（包括 `cargo run`）时保留原有的控制台行为，不会创建托盘图标。

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

## 日志

服务会同时将结构化日志写入控制台和 `logs/latest.log`。每次进程启动时，如果上一次的 `latest.log` 非空，就会压缩为
`logs/log-YY-MM-dd-HH:mm:ss:ms.log.gz`，然后创建新的 `latest.log`。当前日志文件在下一次写入会使大小超过 500 KiB 时滚动。
Windows 文件名不允许使用冒号，因此 Windows 下归档文件使用 `log-YY-MM-dd-HH-mm-ss-ms.log.gz`。

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
- `GET /api/mcp/tokens`
- `DELETE /api/mcp/tokens/{token_id}`
- `GET/POST /api/documents`
- `GET/PUT/DELETE /api/documents/{id}`
- `GET /api/search?q=...`
- `POST /mcp`
- `POST /mcp/all`
- `POST /mcp/{user|group}/{workspace_id}`

## 协作与 MCP

每个用户都会获得一个个人工作区及其默认知识库。创建团队时会同时创建团队工作区和默认知识库；团队所有者和管理员可以按用户名邀请现有用户。邀请令牌可以分享给受邀用户，由其在
**加入团队**对话框中接受。一个用户可以加入多个团队。

在 Vue 工作区中，使用左上角的选择器切换个人与团队工作区。相邻的 `+`
菜单包含创建团队、邀请成员和加入团队操作。侧边栏将当前工作区的每个知识库显示为可折叠分组，文章嵌套在其中；可以直接在这棵树中创建知识库和文章。

文档归属于知识库。文档和搜索请求接受 `knowledge_base_id`；省略时，为保持向后兼容，将使用个人默认知识库。

Vue 工作区通过 `POST /api/mcp/configs` 生成并复制 MCP 配置。每次复制都会创建一个独立且带范围限制的 MCP
访问令牌，不会把当前登录 JWT 写入配置。个人和团队配置会显式指定工作区：

```json
{
  "scope": "user",
  "workspace_id": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
  "expires_in_days": 30
}
```

团队工作区使用 `scope: "group"` 和团队 `workspace_id`；访问账户可用的全部工作区时，使用不带 `workspace_id` 的
`scope: "all"`。`expires_in_days` 可选，默认有效期为 30 天。响应使用支持发送 `Authorization` 请求头的 HTTP 格式：

```json
{
  "type": "http",
  "url": "http://127.0.0.1:8080/mcp/user/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
  "headers": {
    "Authorization": "Bearer ggr_..."
  }
}
```

个人或团队 URL 末尾的 UUID 是工作区 ID，而不是访问令牌。全工作区 URL 为 `/mcp/all`，末尾没有 ID。生成的令牌只以哈希形式
存储，并且独立于登录会话。每次 MCP 请求都会检查令牌有效期、弃用状态、令牌工作区范围和当前成员关系。
`GET /api/mcp/tokens` 会列出令牌前缀及元数据，但不会暴露完整令牌；`DELETE /api/mcp/tokens/{token_id}` 可以弃用自己的令牌。
服务位于公共域名或反向代理之后时，请设置 `MCP_PUBLIC_URL`。

MCP 客户端可以在操作文档前发现资源：

- `list_workspaces()` 返回当前 MCP 作用域可见的工作区。
- `list_knowledge_bases(workspace_id)` 返回指定工作区中可见的知识库。

文档的读取、写入和列表工具要求提供单个 `workspace_id` 与 `knowledge_base_id`；文档专用工具还要求提供已有的 `doc_id`、
`folder_id` 或内容字段。`search_knowledge` 的两个资源参数均可传入单个 UUID 或 UUID 数组。省略 `workspace_id` 时，会搜索当前
MCP 作用域中全部可见工作区；省略 `knowledge_base_id` 时，会搜索所选工作区中的全部知识库；同时省略两者时，会搜索全部可访问知识库。所有显式
ID 仍会按 MCP 作用域和知识库归属校验。搜索结果会包含 `workspace_id` 与 `knowledge_base_id`，可直接用于后续文档工具调用。

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

嵌入模型与向量索引由以下配置控制：

- `EMBEDDING_PROVIDER=stub|local|siliconflow`
- `EMBEDDING_MODEL=BAAI/bge-m3`
- `EMBEDDING_URL=https://api.siliconflow.cn/v1/embeddings`
- `SILICONFLOW_URL=https://api.siliconflow.cn`
- SiliconFlow 嵌入或重排使用 `SILICONFLOW_API_KEY=sk-...`
- `VECTOR_INDEX_PATH=data/vector-index`
- `VECTOR_DATABASE_URL=postgresql://user:password@127.0.0.1:5432/vectors`（可选）

初始化向导默认选择 SiliconFlow 与 `BAAI/bge-m3`。默认嵌入请求地址是完整的
`https://api.siliconflow.cn/v1/embeddings`；`SILICONFLOW_URL` 仍是用于推导重排地址的 API 根地址。`local`
提供方会把 `EMBEDDING_URL` 作为 OpenAI 兼容 HTTP 嵌入端点调用。`stub` 是确定性离线提供方，用于测试或暂时不接入模型服务的部署。

`VECTOR_DATABASE_URL` 留空时使用内嵌 HNSW；填写后，目标 PostgreSQL 必须提供 `vector` 扩展，GugleRAG 会在首次使用时创建向量表、过滤索引和适用的 HNSW 索引。它与
`DATABASE_URL` 相互独立，因此文档元数据和向量可以存放在不同数据库中。

每个非文件夹文档都会保留标题和标签作为上下文，并根据已配置模型的输入上限使用保守的字符窗口切分正文：512 token 的 BGE-large/BCE
模型使用 384 字符，8,192 token 的 BGE-M3 模型使用 6,144 字符，32,768 token 的 Qwen3-Embedding 模型使用 8,192 字符，其他模型使用
4,000 字符回退值；文本块之间保留少量重叠。文本块向量、文本、内容哈希、提供方和模型会保存到所选后端。只要文档集合、文本块哈希以及提供方、
模型没有变化，后续搜索就会复用已有索引或 PostgreSQL 记录；否则会重建对应知识库。搜索按命中的最高分文本块为文档排序，可选重排服务也接收最佳匹配
文本块。服务启动时会补建缺失或过期索引；首次搜索也会按需建立索引。旧版本 SQL 向量会在可以匹配当前单块布局时自动迁移到所选后端，不匹配时重新生成，
不需要单独导出数据。超过 pgvector HNSW 维度上限的模型仍可使用 PostgreSQL 精确向量检索。

重排功能为可选项，由以下配置控制：

- `RERANKER_ENABLED=true|false`
- `RERANKER_PROVIDER=local|siliconflow|custom_http`
- `RERANKER_MODEL=BAAI/bge-reranker-v2-m3`
- `RERANKER_URL=http://...`，用于本地或自定义 HTTP 重排服务

SiliconFlow 重排地址为 `SILICONFLOW_URL/v1/rerank`；本地和自定义 HTTP 重排使用 `RERANKER_URL`。请求包含
`{ model, query, documents, top_n, return_documents: false }`，响应可使用 `results` 或 `data`，每项包含 `index` 以及
`score` 或 `relevance_score`。

## 前端

Markdown 预览使用禁用原始 HTML 的 `markdown-it`，随后通过 DOMPurify 清理渲染结果。它支持 note/warning 容器和只读的 GitHub
风格任务列表：

```markdown
:::note 这里填写部署细节。
:::

:::warning 可选标题 运行此命令前请检查生产数据库。
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
- 基于标题、内容和标签的持久化嵌入搜索，并可选用模型重排
- 在 Markdown 文本的编辑与预览模式之间切换

## 数据库 URL

支持的 URL 前缀：

- SQLite：`sqlite://data/guglerag.db?mode=rwc`
- MySQL：`mysql://user:password@127.0.0.1:3306/guglerag`
- PostgreSQL：`postgresql://user:password@127.0.0.1:5432/guglerag`

## CI 与发布

GitHub Actions 会在每个拉取请求以及每次推送到 `main` 时验证 Rust 后端、Vue 前端、发布工具和真实 HTTP 服务器。拉取请求只运行
CI，不发布产物。`main` 构建成功且六个原生包全部通过后，会发布预发布版本。支持的发布矩阵如下：

| 平台                | Runner             | Rust target                 | 压缩包    |
|---------------------|--------------------|-----------------------------|-----------|
| Linux x64           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  | `.tar.gz` |
| Linux ARM64         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| Windows x64         | `windows-latest`   | `x86_64-pc-windows-msvc`    | `.zip`    |
| Windows ARM64       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   | `.zip`    |
| macOS Apple Silicon | `macos-15`         | `aarch64-apple-darwin`      | `.tar.gz` |
| macOS Intel         | `macos-15-intel`   | `x86_64-apple-darwin`       | `.tar.gz` |

每个目标都使用匹配的 GitHub 原生托管 Runner。CI 会解析每个 ELF、PE 或 Mach-O 文件头，验证打包的 CPU 架构，然后启动该二进制文件执行服务器冒烟测试。

每个压缩包命名为 `guglerag-v<version>-<platform>-<arch>.<format>`，并提供对应的 `.sha256` 文件。压缩包包含服务器可执行文件、
`frontend/dist`、`.env.example`、两份更新日志、`README.md` 和 `RELEASE-METADATA.json`。这些是未签名的便携构建，运行前必须先解压。

`main` 分支的预发布版本使用 `v<manifest-version>-dev.<run_number>`，例如 `v0.1.0-dev.42`。重新运行工作流会保留相同的
GitHub run number 并复用同一发布版本，而不会创建重复版本。上传软件包期间，发布版本保持草稿状态；只有所有目标和双语发布说明都成功后，才会以预发布版本公开显示。

发布稳定版本：

1. 保持 `Cargo.toml`、`Cargo.lock`、`frontend/package.json` 和 `frontend/package-lock.json` 中的版本一致。
2. 在 `CHANGELOG.md` 和 `CHANGELOG.zh-CN.md` 中添加匹配的 `## [x.y.z]` 小节。
3. 推送精确标签 `vx.y.z`。

发布工作流会验证全部六个原生目标，创建便携压缩包和校验和，生成双语发布说明，并且仅在所有软件包成功后发布草稿。稳定版本使用精确的清单版本号；预发布产物会附加
CI 构建标识符，同时从匹配基础版本的更新日志小节读取发布说明。工作流使用仓库提供的 `GITHUB_TOKEN`，无需额外的密钥或签名凭据。
