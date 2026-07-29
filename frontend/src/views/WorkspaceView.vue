<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { request } from "../api/client";
import type {
  AuthResponse,
  DocumentItem,
  InvitationResponse,
  KnowledgeBase,
  McpConfig,
  PublicUser,
  SearchResult,
  Team,
  TeamInvitation,
  TeamMember,
  Workspace
} from "../types";

const docs = ref<DocumentItem[]>([]);
const activeDoc = ref<DocumentItem | null>(null);
const token = ref(localStorage.getItem("guglerag.token") ?? "");
const user = ref<PublicUser | null>(null);
const authMode = ref<"login" | "register">("login");
const editorMode = ref<"edit" | "preview">("edit");
const sidebarMode = ref<"documents" | "teams" | "mcp">("documents");
const query = ref("");
const error = ref("");
const notice = ref("");

const workspaces = ref<Workspace[]>([]);
const knowledgeBases = ref<KnowledgeBase[]>([]);
const teams = ref<Team[]>([]);
const teamMembers = ref<TeamMember[]>([]);
const invitations = ref<TeamInvitation[]>([]);
const selectedWorkspaceId = ref("");
const selectedKnowledgeBaseId = ref("");
const mcpConfig = ref("");
const lastInviteToken = ref("");

const authForm = reactive({ username: "", password: "", display_name: "" });
const editor = reactive({ title: "", content: "", tags: "" });
const collaborationForm = reactive({ teamName: "", knowledgeBaseName: "", inviteUsername: "", inviteToken: "" });

const hasActiveDoc = computed(() => Boolean(activeDoc.value?.id));
const selectedWorkspace = computed(() =>
  workspaces.value.find((workspace) => workspace.id === selectedWorkspaceId.value)
);
const selectedTeam = computed(() =>
  teams.value.find((team) => team.id === selectedWorkspace.value?.team_id)
);

function authHeaders(): Record<string, string> {
  return token.value ? { Authorization: `Bearer ${token.value}` } : {};
}

function setMessage(kind: "error" | "notice", message: string) {
  error.value = kind === "error" ? message : "";
  notice.value = kind === "notice" ? message : "";
}

async function authenticate() {
  setMessage("notice", "");
  const path = authMode.value === "login" ? "/api/auth/login" : "/api/auth/register";
  try {
    const response = await request<AuthResponse>(path, {
      method: "POST",
      body: JSON.stringify({
        username: authForm.username,
        password: authForm.password,
        display_name: authForm.display_name || undefined
      })
    });
    token.value = response.token;
    user.value = response.user;
    localStorage.setItem("guglerag.token", response.token);
    await loadContext();
    setMessage("notice", "已进入工作区。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "登录失败");
  }
}

async function loadMe() {
  if (!token.value) return;
  try {
    user.value = await request<PublicUser>("/api/me", { headers: authHeaders() });
    await loadContext();
  } catch {
    logout();
  }
}

async function loadContext() {
  const [workspaceList, teamList, invitationList] = await Promise.all([
    request<Workspace[]>("/api/workspaces", { headers: authHeaders() }),
    request<Team[]>("/api/teams", { headers: authHeaders() }),
    request<TeamInvitation[]>("/api/invitations", { headers: authHeaders() })
  ]);
  workspaces.value = workspaceList;
  teams.value = teamList;
  invitations.value = invitationList;
  const saved = localStorage.getItem("guglerag.workspace");
  selectedWorkspaceId.value = workspaceList.some((item) => item.id === saved)
    ? saved ?? ""
    : workspaceList.find((item) => item.kind === "personal")?.id ?? workspaceList[0]?.id ?? "";
  await loadKnowledgeBases();
}

async function loadKnowledgeBases() {
  activeDoc.value = null;
  docs.value = [];
  teamMembers.value = [];
  if (!selectedWorkspaceId.value) return;
  localStorage.setItem("guglerag.workspace", selectedWorkspaceId.value);
  knowledgeBases.value = await request<KnowledgeBase[]>(
    `/api/workspaces/${selectedWorkspaceId.value}/knowledge-bases`,
    { headers: authHeaders() }
  );
  const saved = localStorage.getItem(`guglerag.knowledge-base.${selectedWorkspaceId.value}`);
  selectedKnowledgeBaseId.value = knowledgeBases.value.some((item) => item.id === saved)
    ? saved ?? ""
    : knowledgeBases.value[0]?.id ?? "";
  if (selectedTeam.value) {
    teamMembers.value = await request<TeamMember[]>(
      `/api/teams/${selectedTeam.value.id}/members`,
      { headers: authHeaders() }
    );
  }
  await loadDocuments();
}

async function selectKnowledgeBase() {
  activeDoc.value = null;
  if (selectedWorkspaceId.value && selectedKnowledgeBaseId.value) {
    localStorage.setItem(
      `guglerag.knowledge-base.${selectedWorkspaceId.value}`,
      selectedKnowledgeBaseId.value
    );
  }
  await loadDocuments();
}

async function loadDocuments() {
  if (!selectedKnowledgeBaseId.value) {
    docs.value = [];
    return;
  }
  try {
    docs.value = await request<DocumentItem[]>(
      `/api/documents?knowledge_base_id=${selectedKnowledgeBaseId.value}`,
      { headers: authHeaders() }
    );
    if (docs.value.length > 0) await openDocument(docs.value[0].id);
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "无法读取文档");
  }
}

async function searchDocuments() {
  if (!selectedKnowledgeBaseId.value) return;
  try {
    if (!query.value.trim()) return loadDocuments();
    const results = await request<SearchResult[]>(
      `/api/search?q=${encodeURIComponent(query.value)}&limit=30&knowledge_base_id=${selectedKnowledgeBaseId.value}`,
      { headers: authHeaders() }
    );
    activeDoc.value = null;
    docs.value = results.map((item) => ({
      id: item.id,
      knowledge_base_id: selectedKnowledgeBaseId.value,
      title: item.title,
      content: item.excerpt,
      tags: [],
      updated_at: item.updated_at
    }));
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "搜索失败");
  }
}

async function openDocument(id: string) {
  try {
    activeDoc.value = await request<DocumentItem>(`/api/documents/${id}`, { headers: authHeaders() });
    editor.title = activeDoc.value.title;
    editor.content = activeDoc.value.content ?? "";
    editor.tags = activeDoc.value.tags.join(", ");
    editorMode.value = "edit";
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "无法打开文档");
  }
}

async function createDocument() {
  if (!selectedKnowledgeBaseId.value) return;
  try {
    const created = await request<DocumentItem>("/api/documents", {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({
        knowledge_base_id: selectedKnowledgeBaseId.value,
        title: "未命名文档",
        content: "# 未命名文档\n\n开始记录知识。",
        tags: []
      })
    });
    docs.value = [created, ...docs.value];
    await openDocument(created.id);
    setMessage("notice", "已创建文档。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "创建文档失败");
  }
}

async function saveDocument() {
  if (!activeDoc.value) return;
  try {
    const saved = await request<DocumentItem>(`/api/documents/${activeDoc.value.id}`, {
      method: "PUT",
      headers: authHeaders(),
      body: JSON.stringify({
        knowledge_base_id: selectedKnowledgeBaseId.value,
        title: editor.title,
        content: editor.content,
        tags: editor.tags.split(",").map((tag) => tag.trim()).filter(Boolean)
      })
    });
    await loadDocuments();
    await openDocument(saved.id);
    setMessage("notice", "文档已保存。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "保存失败");
  }
}

async function deleteDocument() {
  if (!activeDoc.value || !window.confirm("删除当前文档？")) return;
  try {
    await request(`/api/documents/${activeDoc.value.id}`, {
      method: "DELETE",
      headers: authHeaders()
    });
    activeDoc.value = null;
    await loadDocuments();
    setMessage("notice", "文档已删除。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "删除失败");
  }
}

async function createTeam() {
  if (!collaborationForm.teamName.trim()) return;
  try {
    const team = await request<Team>("/api/teams", {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({ name: collaborationForm.teamName })
    });
    collaborationForm.teamName = "";
    await loadContext();
    selectedWorkspaceId.value = team.workspace_id;
    await loadKnowledgeBases();
    setMessage("notice", "团队已创建。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "创建团队失败");
  }
}

async function createKnowledgeBase() {
  if (!selectedWorkspaceId.value || !collaborationForm.knowledgeBaseName.trim()) return;
  try {
    const knowledgeBase = await request<KnowledgeBase>(
      `/api/workspaces/${selectedWorkspaceId.value}/knowledge-bases`,
      {
        method: "POST",
        headers: authHeaders(),
        body: JSON.stringify({ name: collaborationForm.knowledgeBaseName, description: "" })
      }
    );
    collaborationForm.knowledgeBaseName = "";
    await loadKnowledgeBases();
    selectedKnowledgeBaseId.value = knowledgeBase.id;
    await selectKnowledgeBase();
    setMessage("notice", "知识库已创建。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "创建知识库失败");
  }
}

async function inviteMember() {
  if (!selectedTeam.value || !collaborationForm.inviteUsername.trim()) return;
  try {
    const result = await request<InvitationResponse>(
      `/api/teams/${selectedTeam.value.id}/invitations`,
      {
        method: "POST",
        headers: authHeaders(),
        body: JSON.stringify({ username: collaborationForm.inviteUsername })
      }
    );
    collaborationForm.inviteUsername = "";
    lastInviteToken.value = result.invite_token;
    await copyText(result.invite_token);
    setMessage("notice", "邀请码已复制。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "邀请失败");
  }
}

async function acceptInvitation() {
  const inviteToken = collaborationForm.inviteToken.trim();
  if (!inviteToken) return;
  try {
    await request<Team>(`/api/invitations/${encodeURIComponent(inviteToken)}/accept`, {
      method: "POST",
      headers: authHeaders()
    });
    collaborationForm.inviteToken = "";
    await loadContext();
    setMessage("notice", "已加入团队。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "加入团队失败");
  }
}

async function createMcpConfig(scope: "user" | "group" | "all") {
  try {
    const config = await request<McpConfig>("/api/mcp/configs", {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({ scope, team_id: scope === "group" ? selectedTeam.value?.id : undefined })
    });
    mcpConfig.value = JSON.stringify(config, null, 2);
    await copyText(mcpConfig.value);
    setMessage("notice", "MCP 配置已复制。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "生成 MCP 配置失败");
  }
}

async function copyText(value: string) {
  if (!navigator.clipboard) return;
  try {
    await navigator.clipboard.writeText(value);
  } catch {
    // Clipboard access can be unavailable on non-secure development origins.
  }
}

function logout() {
  token.value = "";
  user.value = null;
  docs.value = [];
  workspaces.value = [];
  knowledgeBases.value = [];
  activeDoc.value = null;
  localStorage.removeItem("guglerag.token");
}

onMounted(loadMe);
</script>

<template>
  <main v-if="!user" class="auth-screen">
    <section class="auth-panel">
      <p class="eyebrow">GugleRAG workspace</p>
      <h1>{{ authMode === "login" ? "登录知识库" : "创建账号" }}</h1>
      <div class="auth-tabs">
        <button :class="{ active: authMode === 'login' }" @click="authMode = 'login'">登录</button>
        <button :class="{ active: authMode === 'register' }" @click="authMode = 'register'">注册</button>
      </div>
      <label>用户名<input v-model="authForm.username" autocomplete="username" /></label>
      <label>密码<input v-model="authForm.password" type="password" autocomplete="current-password" /></label>
      <label v-if="authMode === 'register'">显示名称<input v-model="authForm.display_name" /></label>
      <button @click="authenticate">{{ authMode === "login" ? "登录" : "注册并登录" }}</button>
      <p v-if="error" class="bad">{{ error }}</p>
    </section>
  </main>

  <main v-else class="workspace">
    <aside>
      <div class="brand-row">
        <div><div class="brand">GugleRAG</div><p>{{ user.display_name }} · {{ user.role }}</p></div>
        <button class="icon-button" title="退出登录" @click="logout">退出</button>
      </div>

      <div class="context-selectors">
        <label>工作区
          <select v-model="selectedWorkspaceId" @change="loadKnowledgeBases">
            <option v-for="workspace in workspaces" :key="workspace.id" :value="workspace.id">
              {{ workspace.kind === "personal" ? "个人 · " : "团队 · " }}{{ workspace.name }}
            </option>
          </select>
        </label>
        <label>知识库
          <select v-model="selectedKnowledgeBaseId" @change="selectKnowledgeBase">
            <option v-for="knowledgeBase in knowledgeBases" :key="knowledgeBase.id" :value="knowledgeBase.id">
              {{ knowledgeBase.name }}
            </option>
          </select>
        </label>
      </div>

      <div class="sidebar-tabs">
        <button :class="{ active: sidebarMode === 'documents' }" @click="sidebarMode = 'documents'">文档</button>
        <button :class="{ active: sidebarMode === 'teams' }" @click="sidebarMode = 'teams'">协作</button>
        <button :class="{ active: sidebarMode === 'mcp' }" @click="sidebarMode = 'mcp'">MCP</button>
      </div>

      <template v-if="sidebarMode === 'documents'">
        <div class="search-row">
          <input v-model="query" placeholder="搜索知识库" @keydown.enter="searchDocuments" />
          <button class="secondary" @click="searchDocuments">搜索</button>
        </div>
        <button :disabled="!selectedKnowledgeBaseId" @click="createDocument">新建文档</button>
        <div class="doc-list">
          <button v-for="doc in docs" :key="doc.id" class="doc-row" :class="{ active: activeDoc?.id === doc.id }" @click="openDocument(doc.id)">
            <strong>{{ doc.title }}</strong><span>{{ new Date(doc.updated_at).toLocaleString() }}</span>
          </button>
        </div>
      </template>

      <div v-else-if="sidebarMode === 'teams'" class="sidebar-section-list">
        <section>
          <h2>新建知识库</h2>
          <div class="inline-form"><input v-model="collaborationForm.knowledgeBaseName" placeholder="知识库名称" /><button @click="createKnowledgeBase">创建</button></div>
        </section>
        <section>
          <h2>新建团队</h2>
          <div class="inline-form"><input v-model="collaborationForm.teamName" placeholder="团队名称" /><button @click="createTeam">创建</button></div>
        </section>
        <section v-if="selectedTeam">
          <h2>{{ selectedTeam.name }} 成员</h2>
          <div class="member-list"><div v-for="member in teamMembers" :key="member.user_id"><span>{{ member.display_name }}</span><small>{{ member.role }}</small></div></div>
          <div class="inline-form"><input v-model="collaborationForm.inviteUsername" placeholder="用户名" /><button @click="inviteMember">邀请</button></div>
          <pre v-if="lastInviteToken" class="invite-output">{{ lastInviteToken }}</pre>
        </section>
        <section>
          <h2>加入团队</h2>
          <div class="inline-form"><input v-model="collaborationForm.inviteToken" placeholder="邀请码" /><button @click="acceptInvitation">加入</button></div>
          <p v-if="invitations.some((item) => item.status === 'pending')" class="hint">{{ invitations.filter((item) => item.status === "pending").map((item) => item.team_name).join("、") }}</p>
        </section>
      </div>

      <div v-else class="sidebar-section-list">
        <section>
          <h2>个人工作区</h2><button @click="createMcpConfig('user')">复制配置</button>
        </section>
        <section v-if="selectedTeam">
          <h2>{{ selectedTeam.name }}</h2><button @click="createMcpConfig('group')">复制团队配置</button>
        </section>
        <section>
          <h2>全部可访问知识库</h2><button @click="createMcpConfig('all')">复制账户配置</button>
        </section>
        <pre v-if="mcpConfig" class="mcp-output">{{ mcpConfig }}</pre>
      </div>
    </aside>

    <section class="editor-pane">
      <div class="toolbar">
        <div><h1>{{ hasActiveDoc ? editor.title || "未命名文档" : "选择或新建文档" }}</h1><span v-if="activeDoc">{{ activeDoc.versions?.length ?? 0 }} 个历史版本</span></div>
        <div class="toolbar-actions">
          <button class="secondary" :class="{ active: editorMode === 'edit' }" @click="editorMode = 'edit'">编辑</button>
          <button class="secondary" :class="{ active: editorMode === 'preview' }" @click="editorMode = 'preview'">预览</button>
          <button :disabled="!hasActiveDoc" @click="saveDocument">保存</button>
          <button class="danger" :disabled="!hasActiveDoc" @click="deleteDocument">删除</button>
        </div>
      </div>
      <p v-if="error" class="bad">{{ error }}</p><p v-if="notice" class="ok">{{ notice }}</p>
      <div v-if="hasActiveDoc" class="editor-grid">
        <label>标题<input v-model="editor.title" /></label><label>标签<input v-model="editor.tags" placeholder="用逗号分隔" /></label>
        <textarea v-if="editorMode === 'edit'" v-model="editor.content" /><pre v-else class="preview">{{ editor.content }}</pre>
      </div>
      <div v-else class="empty-state"><h2>还没有打开的文档</h2><p>{{ selectedKnowledgeBaseId ? "选择已有内容或新建文档" : "先创建一个知识库" }}</p><button :disabled="!selectedKnowledgeBaseId" @click="createDocument">新建文档</button></div>
    </section>
  </main>
</template>
