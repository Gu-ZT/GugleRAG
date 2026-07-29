# GugleRAG - Team Knowledge Base with MCP — 项目开发计划 (PLAN.md)

## 0. 当前初始化状态

- 已建立 Rust 后端 MVP：健康检查、注册/登录、文档 CRUD、简单搜索、MCP HTTP JSON-RPC 端点。
- 已加入首次启动配置流程：当 `.env` 不存在时，Vue 前端展示分步配置向导并通过 `/api/setup` 写入 `.env`。
- 已加入可选重排模型配置：`RERANKER_ENABLED`、`RERANKER_PROVIDER`、`RERANKER_MODEL`、`RERANKER_URL`。
- 配置层已识别 SQLite、MySQL、PostgreSQL 三类 `DATABASE_URL`，后续持久化层基于 SQLx 接入。
- 已新增 `frontend/` Vue 3 + TypeScript + Vite 项目骨架，开发时通过 Vite proxy 调用后端。
- 用户、文档和版本历史已通过 SQLx 持久化到配置的数据库。
- Vue 工作台已支持注册/登录、文档列表、新建、编辑、保存、删除、标签和搜索。
- Rust 后端已拆分为配置、领域模型、数据库、认证、REST API、MCP、搜索和错误处理模块，`main.rs` 仅保留启动入口。
- 后端配置、账号校验和关键词排序已由顶层 `tests/` 集成测试覆盖。
- 已加入个人/团队工作区、多知识库、团队成员邀请与加入，以及按用户/团队/全部可访问知识库隔离的 MCP 配置。

## 1. 项目概述

**目标**：开发一个轻量级、可自行部署的团队知识库系统，核心能力包括：

- 多用户团队协作
- Markdown 文档管理与全文搜索
- 内置 MCP (Model Context Protocol) 服务，供 AI Agent 调用
- 嵌入向量化支持（本地推理 / SiliconFlow API 切换）
- 全栈自包含：Rust 后端托管 Vue3 前端构建产物，后续可切换为二进制资源嵌入
- 可选重排模型集成，提升 RAG 检索质量

**交付形式**：一个可执行文件，通过环境变量配置即可运行。

---

## 2. 技术栈

| 层级             | 技术选型                                                   |
|------------------|------------------------------------------------------------|
| **后端语言**     | Rust（稳定版）                                             |
| **Web 框架**     | Axum（或 Actix-web）                                       |
| **数据库**       | SQLite（开发/测试），MySQL / PostgreSQL（通过 `sqlx`）     |
| **全文搜索**     | Tantivy（Rust 原生全文检索引擎）                           |
| **嵌入模型**     | `fastembed-rs`（本地 ONNX 推理） + SiliconFlow API（远程） |
| **前端框架**     | Vue 3 + TypeScript                                         |
| **前端构建工具** | Vite                                                       |
| **前端静态托管** | `rust-embed` 嵌入到二进制文件                              |
| **MCP 协议**     | 基于 JSON-RPC 自行实现（支持 stdio / HTTP 传输）           |
| **协作（可选）** | WebSocket（通知/在线状态）                                 |

---

## 3. 核心功能模块

### 3.1 用户与团队管理

- [x] 用户注册 / 登录（JWT 认证）
- [x] 工作区（Workspace）创建与切换
- [x] 工作区成员邀请与管理（团队 Owner/Admin/Member）
- [ ] 个人资料与密码修改

### 3.2 知识库文档管理

- [x] 文档的 CRUD（支持 Markdown）
- [ ] 文件夹/目录树结构（无限层级）
- [ ] 文档版本历史（基于 Git 或简单的快照）
- [x] 文档标签与元数据
- [ ] 文档间双向链接（支持知识图谱可视化）
- [ ] 附件上传（图片、文件）

### 3.3 搜索与检索

- [ ] 基于 Tantivy 的全文搜索（标题、正文）
- [ ] 向量语义搜索（基于嵌入模型）
- [ ] 混合检索（全文 + 向量）与重排（可选）
- [ ] 搜索结果高亮与摘要

### 3.4 嵌入服务（可切换）

- [ ] **本地模式**：通过 `fastembed-rs` 加载 `BAAI/bge-large-zh-v1.5` 或 `BAAI/bge-large-en-v1.5` 或 `BAAI/bge-m3`
- [ ] **远程模式**：调用 [SiliconFlow (https://api-docs.siliconflow.cn/docs/api/embeddings-post)]
  的 [Embeddings API](https://api.siliconflow.cn/v1/embeddings)
- [ ] 环境变量切换（`EMBEDDING_PROVIDER=local|siliconflow`，`SILICONFLOW_API_KEY=sk-xxxxxx`）
- [ ] 统一的 `Embedder` trait，支持策略模式

### 3.5 MCP 服务（核心）

- [x] 实现 MCP 协议的服务器端点（HTTP 路径 `/mcp`）
- [x] 暴露至少以下工具（Tools）供 Agent 调用：
    - `search_knowledge(query, limit)` — 全文/语义混合搜索
    - `read_document(doc_id)` — 获取文档完整内容
    - `create_document(title, content, parent_id)` — 新建文档
    - `update_document(doc_id, content)` — 更新文档
    - `list_documents(folder_id)` — 列出目录结构
    - `get_document_metadata(doc_id)` — 获取元数据
- [x] 工具返回结构符合 MCP 规范（JSON-RPC 响应）
- [x] 支持通过 MCP 进行用户认证（可配置）
- [x] 支持个人、团队和全部可访问知识库的独立 MCP 配置

### 3.6 前端界面（Vue3 + TS）

- [ ] 布局：侧边栏目录树 + 文档列表 + 主编辑/预览区
- [ ] Markdown 编辑器（支持实时预览、代码高亮）
- [x] 搜索框（当前为关键词搜索，语义切换待接入嵌入索引）
- [ ] 知识图谱可视化（使用 `vis-network` 或 `ECharts`）
- [ ] AI 交互侧边栏（展示 Agent 操作日志，或直接与 Agent 对话）
- [ ] 用户管理界面（邀请成员、角色分配）

### 3.7 运维与部署

- [ ] 单一二进制，前端静态资源嵌入
- [ ] 通过环境变量配置：数据库路径、监听端口、嵌入模式、SiliconFlow API Key 等
- [ ] 健康检查端点 `/health`
- [ ] 日志与可观测性（`tracing` 生态）

---

## 4. 开发阶段划分

### Phase 1：基础架构与 MVP（2-3 周）

- [x] Rust 项目骨架（Axum + SQLx + 静态文件托管）
- [x] 用户认证（注册/登录/JWT）
- [x] 文档 CRUD + 目录层级 API
- [x] 前端 Vue3 框架搭建（Vite + TypeScript）
- [x] 前端核心页面：登录、文档列表、编辑器
- [ ] 生产构建：前端构建产物嵌入到二进制

**交付**：可运行的单机知识库，支持多用户、文档管理。

### Phase 2：搜索与嵌入（2-3 周）

- [ ] 集成 Tantivy 全文搜索
- [ ] 实现嵌入服务（本地 `fastembed-rs` 和 SiliconFlow API 切换）
- [ ] 向量索引与语义搜索 API
- [ ] 前端搜索界面（支持两种模式切换）

**交付**：具备混合搜索能力的知识库。

### Phase 3：MCP 集成（1-2 周）

- [ ] 实现 MCP 协议服务器（HTTP 端点）
- [ ] 注册核心工具（search/read/create/update/list）
- [ ] 测试与 AI Agent（如 Claude Desktop）的联调
- [ ] 提供 MCP 工具使用文档

**交付**：AI Agent 可以通过 MCP 调用知识库所有功能。

### Phase 4：团队协作增强（2-3 周）

- [ ] 工作区与成员管理
- [ ] 权限控制（角色划分）
- [ ] 文档版本历史
- [ ] WebSocket 实时通知（在线状态、文档更新提醒）

**交付**：完整的多用户协作系统。

### Phase 5：高级功能（可选，2-3 周）

- [ ] 重排模型集成（通过单独服务或本地 `reranker`）
- [ ] 知识图谱可视化
- [ ] AI 对话侧边栏（直接使用 MCP 工具与 Agent 交互）
- [ ] 文档导出（PDF/HTML）
- [ ] 性能优化与压力测试

**交付**：功能完备的企业级知识库。

---

## 5. 配置管理

通过环境变量或 `.env` 文件配置：

| 变量名                | 说明                                    | 默认值                       |
|-----------------------|-----------------------------------------|------------------------------|
| `SERVER_HOST`         | 监听地址                                | `0.0.0.0`                    |
| `SERVER_PORT`         | 监听端口                                | `8080`                       |
| `DATABASE_URL`        | 数据库连接串（SQLite/MySQL/PostgreSQL） | `sqlite://data/guglerag.db?mode=rwc` |
| `EMBEDDING_PROVIDER`  | `local` 或 `siliconflow`                | `local`                      |
| `EMBEDDING_MODEL`     | 模型名称（如 `BAAI/bge-large-zh-v1.5`） | `BAAI/bge-large-zh-v1.5`     |
| `SILICONFLOW_URL`     | SiliconFlow API 地址（远程模式必填）    | `https://api.siliconflow.cn` |
| `SILICONFLOW_API_KEY` | SiliconFlow API Key（远程模式必填）     | 空                           |
| `RERANKER_ENABLED`    | 是否启用重排模型                        | `false`                      |
| `RERANKER_PROVIDER`   | `local` / `siliconflow` / `custom_http` | `siliconflow`                |
| `RERANKER_MODEL`      | 重排模型名称                            | `BAAI/bge-reranker-v2-m3`    |
| `RERANKER_URL`        | 自定义重排 HTTP 服务地址                | 空                           |
| `JWT_SECRET`          | JWT 签名密钥                            | （必须设置）                 |
| `MCP_ENABLED`         | 是否启用 MCP 端点                       | `true`                       |

---

## 6. 项目目录结构

```
my-knowledge-base/
├── backend/ # Rust 项目
│ ├── src/
│ │ ├── api/ # REST API 路由
│ │ │ ├── auth.rs
│ │ │ ├── docs.rs
│ │ │ ├── search.rs
│ │ │ └── workspace.rs
│ │ ├── mcp/ # MCP 协议实现
│ │ │ ├── server.rs
│ │ │ └── tools.rs
│ │ ├── embedder/ # 嵌入服务（策略模式）
│ │ │ ├── mod.rs
│ │ │ ├── local.rs
│ │ │ └── siliconflow.rs
│ │ ├── db/ # 数据库操作（sqlx）
│ │ ├── search/ # Tantivy 索引
│ │ ├── static/ # 前端构建产物（gitignore）
│ │ ├── main.rs
│ │ └── lib.rs
│ ├── Cargo.toml
│ └── .env.example
├── frontend/ # Vue3 + TS 项目
│ ├── src/
│ │ ├── api/ # 调用后端 API
│ │ ├── components/ # Vue 组件
│ │ ├── views/ # 页面视图
│ │ ├── stores/ # Pinia 状态管理
│ │ ├── types/ # TypeScript 类型定义
│ │ ├── App.vue
│ │ └── main.ts
│ ├── index.html
│ ├── vite.config.ts
│ ├── package.json
│ └── tsconfig.json
├── README.md
└── PLAN.md # 本文件
```

---

## 7. 里程碑与验收标准

| 里程碑               | 预计时间   | 验收标准                                       |
|----------------------|------------|------------------------------------------------|
| M1: MVP 可用         | 第 3 周末  | 多用户登录、文档增删改查、目录树，前端可访问   |
| M2: 搜索上线         | 第 6 周末  | 全文+语义搜索，支持本地/云端嵌入切换           |
| M3: MCP 服务联调     | 第 8 周末  | 至少 5 个 MCP 工具通过 Claude Desktop 验证可用 |
| M4: 协作功能         | 第 11 周末 | 工作区成员管理、权限控制、版本历史             |
| M5（可选）: 增强特性 | 第 14 周末 | 重排、图谱、AI 对话面板                        |

---

## 8. 风险与应对

- **嵌入模型资源消耗**：本地推理需至少 16GB 内存或 GPU。建议默认使用 SiliconFlow API 降低启动门槛。
- **MCP 协议演进**：关注官方规范更新，保持协议实现的灵活性。
- **前端实时协作**：若协同编辑实现复杂，可推迟到 Phase 5，初版仅支持手动刷新。
- **数据迁移**：SQLite 单文件便于备份，未来迁移到 PostgreSQL 需提供迁移脚本。

---

## 9. 开始开发

建议按 Phase 1 开始，逐步迭代。每日构建可运行版本，保持主干稳定。

**立即启动命令**：

```bash
# 后端
cargo new backend --lib
cd backend && cargo add axum tokio sqlx tantivy fastembed-rs serde

# 前端
npm create vite@latest frontend -- --template vue-ts
```
