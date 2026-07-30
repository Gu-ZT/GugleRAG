<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  BookOpen,
  ChevronDown,
  ChevronRight,
  Copy,
  FilePlus2,
  FileText,
  LogIn,
  LogOut,
  Menu,
  Plus,
  Plug,
  Search,
  Settings,
  UserPlus,
  Users,
  X
} from "@lucide/vue";
import { request } from "../api/client";
import { renderMarkdown } from "../markdown";
import AdminSettingsDialog from "./AdminSettingsDialog.vue";
import type {
  AuthResponse,
  DocumentItem,
  InvitationResponse,
  KnowledgeBase,
  McpConfig,
  PublicUser,
  Team,
  TeamInvitation,
  TeamMember,
  Workspace
} from "../types";

const activeDoc = ref<DocumentItem | null>(null);
const token = ref(localStorage.getItem("guglerag.token") ?? "");
const user = ref<PublicUser | null>(null);
const authMode = ref<"login" | "register">("login");
const editorMode = ref<"edit" | "preview">("edit");
const sidebarOpen = ref(false);
const workspaceMenuOpen = ref(false);
const adminSettingsOpen = ref(false);
const activeDialog = ref<"create-team" | "invite-member" | "join-team" | "create-knowledge-base" | "mcp" | null>(null);
const query = ref("");
const authError = ref("");

const workspaces = ref<Workspace[]>([]);
const knowledgeBases = ref<KnowledgeBase[]>([]);
const documentsByKnowledgeBase = ref<Record<string, DocumentItem[]>>({});
const expandedKnowledgeBaseIds = ref<Set<string>>(new Set());
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

interface Toast {
  id: number;
  kind: "success" | "error";
  message: string;
}

const toasts = ref<Toast[]>([]);
let toastSeq = 0;

function toast(kind: Toast["kind"], message: string) {
  const id = ++toastSeq;
  toasts.value.push({ id, kind, message });
  window.setTimeout(() => {
    toasts.value = toasts.value.filter((item) => item.id !== id);
  }, 3600);
}

const hasActiveDoc = computed(() => Boolean(activeDoc.value?.id));
const selectedWorkspace = computed(() =>
  workspaces.value.find((workspace) => workspace.id === selectedWorkspaceId.value)
);
const selectedKnowledgeBase = computed(() =>
  knowledgeBases.value.find((kb) => kb.id === selectedKnowledgeBaseId.value)
);
const selectedTeam = computed(() =>
  teams.value.find((team) => team.id === selectedWorkspace.value?.team_id)
);
const pendingInvitations = computed(() =>
  invitations.value.filter((item) => item.status === "pending")
);
const totalDocumentCount = computed(() =>
  Object.values(documentsByKnowledgeBase.value).reduce((total, items) => total + items.length, 0)
);

const dirty = computed(() => {
  if (!activeDoc.value) return false;
  return (
    editor.title !== activeDoc.value.title ||
    editor.content !== (activeDoc.value.content ?? "") ||
    editor.tags !== activeDoc.value.tags.join(", ")
  );
});

const previewHtml = computed(() => renderMarkdown(editor.content));
const tagChips = computed(() =>
  editor.tags.split(",").map((tag) => tag.trim()).filter(Boolean)
);

/* 编辑区随内容自动长高，避免页面与输入框双重滚动条 */
const editorArea = ref<HTMLTextAreaElement | null>(null);

function autogrowEditor() {
  const el = editorArea.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = `${el.scrollHeight}px`;
}

watch([editorMode, () => activeDoc.value?.id], () => {
  if (editorMode.value === "edit") nextTick(autogrowEditor);
});

function authHeaders(): Record<string, string> {
  return token.value ? { Authorization: `Bearer ${token.value}` } : {};
}

function formatTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const diff = Date.now() - date.getTime();
  const minute = 60_000;
  const hour = 3_600_000;
  const day = 86_400_000;
  if (diff < minute) return "刚刚";
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < day) return `${Math.floor(diff / hour)} 小时前`;
  if (diff < 7 * day) return `${Math.floor(diff / day)} 天前`;
  return date.toLocaleDateString();
}

function errorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

async function authenticate() {
  authError.value = "";
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
    toast("success", "已进入工作区。");
  } catch (err) {
    authError.value = errorMessage(err, "登录失败");
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
  documentsByKnowledgeBase.value = {};
  expandedKnowledgeBaseIds.value = new Set();
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
  if (selectedKnowledgeBaseId.value) {
    expandedKnowledgeBaseIds.value = new Set([selectedKnowledgeBaseId.value]);
  }
  if (selectedTeam.value) {
    teamMembers.value = await request<TeamMember[]>(
      `/api/teams/${selectedTeam.value.id}/members`,
      { headers: authHeaders() }
    );
  }
  await loadAllDocuments();
}

async function selectKnowledgeBase(knowledgeBaseId: string, openFirst = false, expand = true) {
  const changed = selectedKnowledgeBaseId.value !== knowledgeBaseId;
  selectedKnowledgeBaseId.value = knowledgeBaseId;
  if (changed) activeDoc.value = null;
  if (selectedWorkspaceId.value && knowledgeBaseId) {
    localStorage.setItem(
      `guglerag.knowledge-base.${selectedWorkspaceId.value}`,
      knowledgeBaseId
    );
  }
  if (expand) {
    expandedKnowledgeBaseIds.value = new Set([
      ...expandedKnowledgeBaseIds.value,
      knowledgeBaseId
    ]);
  }
  const documents = documentsByKnowledgeBase.value[knowledgeBaseId] ?? [];
  if (openFirst && documents.length > 0) await openDocument(documents[0].id, knowledgeBaseId);
}

async function loadAllDocuments(openSelectedFirst = true) {
  let failed = false;
  const entries = await Promise.all(
    knowledgeBases.value.map(async (knowledgeBase) => {
      try {
        const documents = await request<DocumentItem[]>(
          `/api/documents?knowledge_base_id=${knowledgeBase.id}`,
          { headers: authHeaders() }
        );
        return [knowledgeBase.id, documents] as const;
      } catch {
        failed = true;
        return [knowledgeBase.id, [] as DocumentItem[]] as const;
      }
    })
  );
  documentsByKnowledgeBase.value = Object.fromEntries(entries);
  if (failed) toast("error", "部分知识库暂时无法读取");
  if (!openSelectedFirst || !selectedKnowledgeBaseId.value) return;
  const selectedDocuments = documentsByKnowledgeBase.value[selectedKnowledgeBaseId.value] ?? [];
  if (selectedDocuments.length > 0) {
    await openDocument(selectedDocuments[0].id, selectedKnowledgeBaseId.value);
  }
}

async function loadDocuments(knowledgeBaseId = selectedKnowledgeBaseId.value, openFirst = false) {
  if (!knowledgeBaseId) return;
  try {
    const documents = await request<DocumentItem[]>(
      `/api/documents?knowledge_base_id=${knowledgeBaseId}`,
      { headers: authHeaders() }
    );
    documentsByKnowledgeBase.value = {
      ...documentsByKnowledgeBase.value,
      [knowledgeBaseId]: documents
    };
    if (openFirst && documents.length > 0) await openDocument(documents[0].id, knowledgeBaseId);
  } catch (err) {
    toast("error", errorMessage(err, "无法读取文档"));
  }
}

async function searchDocuments() {
  try {
    if (!query.value.trim()) return loadAllDocuments(false);
    const entries = await Promise.all(
      knowledgeBases.value.map(async (knowledgeBase) => {
        const results = await request<Array<{
          id: string;
          title: string;
          excerpt: string;
          updated_at: string;
        }>>(
          `/api/search?q=${encodeURIComponent(query.value)}&limit=30&knowledge_base_id=${knowledgeBase.id}`,
          { headers: authHeaders() }
        );
        return [
          knowledgeBase.id,
          results.map((item) => ({
            id: item.id,
            knowledge_base_id: knowledgeBase.id,
            title: item.title,
            content: item.excerpt,
            tags: [],
            updated_at: item.updated_at
          }))
        ] as const;
      })
    );
    activeDoc.value = null;
    documentsByKnowledgeBase.value = Object.fromEntries(entries);
    expandedKnowledgeBaseIds.value = new Set(
      entries.filter(([, documents]) => documents.length > 0).map(([id]) => id)
    );
  } catch (err) {
    toast("error", errorMessage(err, "搜索失败"));
  }
}

async function openDocument(id: string, knowledgeBaseId?: string, mode: "edit" | "preview" = "preview") {
  try {
    activeDoc.value = await request<DocumentItem>(`/api/documents/${id}`, { headers: authHeaders() });
    const ownerId = knowledgeBaseId ?? activeDoc.value.knowledge_base_id;
    selectedKnowledgeBaseId.value = ownerId;
    if (selectedWorkspaceId.value) {
      localStorage.setItem(`guglerag.knowledge-base.${selectedWorkspaceId.value}`, ownerId);
    }
    editor.title = activeDoc.value.title;
    editor.content = activeDoc.value.content ?? "";
    editor.tags = activeDoc.value.tags.join(", ");
    editorMode.value = mode;
    sidebarOpen.value = false;
  } catch (err) {
    toast("error", errorMessage(err, "无法打开文档"));
  }
}

async function createDocument(knowledgeBaseId = selectedKnowledgeBaseId.value) {
  if (!knowledgeBaseId) return;
  try {
    const created = await request<DocumentItem>("/api/documents", {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({
        knowledge_base_id: knowledgeBaseId,
        title: "未命名文档",
        content: "# 未命名文档\n\n开始记录知识。",
        tags: []
      })
    });
    documentsByKnowledgeBase.value = {
      ...documentsByKnowledgeBase.value,
      [knowledgeBaseId]: [created, ...(documentsByKnowledgeBase.value[knowledgeBaseId] ?? [])]
    };
    expandedKnowledgeBaseIds.value = new Set([...expandedKnowledgeBaseIds.value, knowledgeBaseId]);
    await openDocument(created.id, knowledgeBaseId, "edit");
    toast("success", "已创建文档。");
  } catch (err) {
    toast("error", errorMessage(err, "创建文档失败"));
  }
}

async function saveDocument() {
  if (!activeDoc.value) return;
  try {
    const knowledgeBaseId = activeDoc.value.knowledge_base_id;
    const saved = await request<DocumentItem>(`/api/documents/${activeDoc.value.id}`, {
      method: "PUT",
      headers: authHeaders(),
      body: JSON.stringify({
        knowledge_base_id: knowledgeBaseId,
        title: editor.title,
        content: editor.content,
        tags: editor.tags.split(",").map((tag) => tag.trim()).filter(Boolean)
      })
    });
    await loadDocuments(knowledgeBaseId);
    await openDocument(saved.id, knowledgeBaseId, editorMode.value);
    toast("success", "文档已保存。");
  } catch (err) {
    toast("error", errorMessage(err, "保存失败"));
  }
}

async function deleteDocument() {
  if (!activeDoc.value || !window.confirm("删除当前文档？")) return;
  try {
    const knowledgeBaseId = activeDoc.value.knowledge_base_id;
    await request(`/api/documents/${activeDoc.value.id}`, {
      method: "DELETE",
      headers: authHeaders()
    });
    activeDoc.value = null;
    await loadDocuments(knowledgeBaseId, true);
    toast("success", "文档已删除。");
  } catch (err) {
    toast("error", errorMessage(err, "删除失败"));
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
    closeDialog();
    toast("success", "团队已创建。");
  } catch (err) {
    toast("error", errorMessage(err, "创建团队失败"));
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
    await selectKnowledgeBase(knowledgeBase.id);
    expandedKnowledgeBaseIds.value = new Set([...expandedKnowledgeBaseIds.value, knowledgeBase.id]);
    closeDialog();
    toast("success", "知识库已创建。");
  } catch (err) {
    toast("error", errorMessage(err, "创建知识库失败"));
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
    toast("success", "邀请码已复制。");
  } catch (err) {
    toast("error", errorMessage(err, "邀请失败"));
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
    closeDialog();
    toast("success", "已加入团队。");
  } catch (err) {
    toast("error", errorMessage(err, "加入团队失败"));
  }
}

async function createMcpConfig(scope: "user" | "group" | "all") {
  try {
    let workspaceId: string | undefined;
    if (scope === "user") {
      workspaceId = workspaces.value.find((workspace) => workspace.kind === "personal")?.id;
    } else if (scope === "group") {
      workspaceId = selectedWorkspace.value?.id;
    }
    if (scope !== "all" && !workspaceId) {
      toast("error", "未找到对应工作区");
      return;
    }
    const config = await request<McpConfig>("/api/mcp/configs", {
      method: "POST",
      headers: authHeaders(),
      body: JSON.stringify({ scope, workspace_id: workspaceId })
    });
    mcpConfig.value = JSON.stringify(config, null, 2);
    await copyText(mcpConfig.value);
    toast("success", "MCP 配置已复制。");
  } catch (err) {
    toast("error", errorMessage(err, "生成 MCP 配置失败"));
  }
}

async function copyText(value: string) {
  if (!navigator.clipboard) return;
  try {
    await navigator.clipboard.writeText(value);
  } catch {
    // 非安全上下文下剪贴板可能不可用。
  }
}

function toggleKnowledgeBase(knowledgeBaseId: string) {
  const next = new Set(expandedKnowledgeBaseIds.value);
  if (next.has(knowledgeBaseId)) next.delete(knowledgeBaseId);
  else next.add(knowledgeBaseId);
  expandedKnowledgeBaseIds.value = next;
}

function openDialog(dialog: NonNullable<typeof activeDialog.value>) {
  workspaceMenuOpen.value = false;
  lastInviteToken.value = "";
  mcpConfig.value = "";
  activeDialog.value = dialog;
}

function closeDialog() {
  activeDialog.value = null;
}

function logout() {
  adminSettingsOpen.value = false;
  token.value = "";
  user.value = null;
  documentsByKnowledgeBase.value = {};
  workspaces.value = [];
  knowledgeBases.value = [];
  activeDoc.value = null;
  localStorage.removeItem("guglerag.token");
}

function onKeydown(event: KeyboardEvent) {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
    if (!user.value || !hasActiveDoc.value) return;
    event.preventDefault();
    if (dirty.value) saveDocument();
  }
  if (event.key === "Escape") {
    sidebarOpen.value = false;
    workspaceMenuOpen.value = false;
    closeDialog();
    adminSettingsOpen.value = false;
  }
}

function onWindowClick() {
  workspaceMenuOpen.value = false;
}

function onBeforeUnload(event: BeforeUnloadEvent) {
  if (dirty.value) event.preventDefault();
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("click", onWindowClick);
  window.addEventListener("beforeunload", onBeforeUnload);
  loadMe();
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("click", onWindowClick);
  window.removeEventListener("beforeunload", onBeforeUnload);
});
</script>

<template>
  <!-- 顶部 toast -->
  <div class="toast-stack" aria-live="polite">
    <div v-for="item in toasts" :key="item.id" class="toast" :class="item.kind">{{ item.message }}</div>
  </div>

  <!-- 登录 / 注册 -->
  <main v-if="!user" class="auth-screen">
    <section class="auth-panel">
      <div class="auth-brand">
        <span class="boot-mark">G</span>
        <div>
          <strong>GugleRAG</strong>
          <p class="hint">团队知识库与 AI 检索</p>
        </div>
      </div>
      <h1>{{ authMode === "login" ? "欢迎回来" : "创建账号" }}</h1>
      <div class="auth-tabs">
        <button :class="{ active: authMode === 'login' }" @click="authMode = 'login'">登录</button>
        <button :class="{ active: authMode === 'register' }" @click="authMode = 'register'">注册</button>
      </div>
      <label>用户名<input v-model="authForm.username" autocomplete="username" @keydown.enter="authenticate" /></label>
      <label>密码<input v-model="authForm.password" type="password" autocomplete="current-password" @keydown.enter="authenticate" /></label>
      <label v-if="authMode === 'register'">显示名称<input v-model="authForm.display_name" @keydown.enter="authenticate" /></label>
      <button class="btn btn-primary" @click="authenticate">{{ authMode === "login" ? "登录" : "注册并登录" }}</button>
      <p v-if="authError" class="bad">{{ authError }}</p>
      <p v-else class="hint">首个注册的账号将成为管理员。</p>
    </section>
  </main>

  <!-- 工作区 -->
  <main v-else class="workspace">
    <button v-if="sidebarOpen" class="backdrop" aria-label="关闭侧栏" @click="sidebarOpen = false" />

    <aside class="sidebar" :class="{ open: sidebarOpen }">
      <div class="sidebar-brand">
        <span class="brand-mark">G</span>
        <div>
          <strong>GugleRAG</strong>
          <small>团队知识库</small>
        </div>
      </div>

      <div class="sidebar-context" @click.stop>
        <select v-model="selectedWorkspaceId" class="rail-select" aria-label="选择工作区" @change="loadKnowledgeBases">
          <option v-for="workspace in workspaces" :key="workspace.id" :value="workspace.id">
            {{ workspace.kind === "personal" ? "个人 · " : "团队 · " }}{{ workspace.name }}
          </option>
        </select>
        <button
          class="workspace-add-btn"
          title="团队协作"
          aria-label="团队协作"
          :aria-expanded="workspaceMenuOpen"
          @click="workspaceMenuOpen = !workspaceMenuOpen"
        >
          <Plus :size="18" />
        </button>
        <div v-if="workspaceMenuOpen" class="workspace-menu">
          <button @click="openDialog('create-team')"><Users :size="16" /><span>创建团队</span></button>
          <button v-if="selectedTeam" @click="openDialog('invite-member')"><UserPlus :size="16" /><span>邀请成员</span></button>
          <button @click="openDialog('join-team')"><LogIn :size="16" /><span>加入团队</span></button>
        </div>
      </div>

      <div class="sidebar-body">
        <div class="rail-search">
          <Search :size="15" />
          <input v-model="query" class="rail-input" placeholder="搜索当前工作区…" @keydown.enter="searchDocuments" />
          <button v-if="query" title="清除搜索" aria-label="清除搜索" @click="query = ''; searchDocuments()">
            <X :size="14" />
          </button>
        </div>

        <section class="knowledge-tree">
          <div class="sidebar-heading">
            <span>知识库 · {{ knowledgeBases.length }}</span>
            <button class="heading-icon-btn" title="新建知识库" aria-label="新建知识库" @click="openDialog('create-knowledge-base')">
              <Plus :size="15" />
            </button>
          </div>

          <div v-if="knowledgeBases.length" class="knowledge-groups">
            <section v-for="knowledgeBase in knowledgeBases" :key="knowledgeBase.id" class="knowledge-group">
              <div class="knowledge-row" :class="{ active: selectedKnowledgeBaseId === knowledgeBase.id }">
                <button
                  class="knowledge-toggle"
                  :aria-expanded="expandedKnowledgeBaseIds.has(knowledgeBase.id)"
                  @click="toggleKnowledgeBase(knowledgeBase.id); selectKnowledgeBase(knowledgeBase.id, false, false)"
                >
                  <ChevronDown v-if="expandedKnowledgeBaseIds.has(knowledgeBase.id)" :size="15" />
                  <ChevronRight v-else :size="15" />
                  <BookOpen :size="15" />
                  <span>{{ knowledgeBase.name }}</span>
                  <small>{{ documentsByKnowledgeBase[knowledgeBase.id]?.length ?? 0 }}</small>
                </button>
                <button
                  class="knowledge-add-btn"
                  title="在此知识库中新建文章"
                  aria-label="在此知识库中新建文章"
                  @click="createDocument(knowledgeBase.id)"
                >
                  <Plus :size="14" />
                </button>
              </div>

              <div v-if="expandedKnowledgeBaseIds.has(knowledgeBase.id)" class="article-list">
                <button
                  v-for="doc in documentsByKnowledgeBase[knowledgeBase.id] ?? []"
                  :key="doc.id"
                  class="article-item"
                  :class="{ active: activeDoc?.id === doc.id }"
                  @click="openDocument(doc.id, knowledgeBase.id)"
                >
                  <FileText :size="14" />
                  <span>{{ doc.title }}</span>
                  <small>{{ formatTime(doc.updated_at) }}</small>
                </button>
                <p v-if="!(documentsByKnowledgeBase[knowledgeBase.id]?.length)" class="article-empty">暂无文章</p>
              </div>
            </section>
          </div>
          <div v-else class="rail-empty">
            <BookOpen :size="22" />
            <p>当前工作区还没有知识库</p>
            <button @click="openDialog('create-knowledge-base')">新建知识库</button>
          </div>
        </section>

        <p v-if="knowledgeBases.length" class="tree-summary">{{ totalDocumentCount }} 篇文章</p>
      </div>

      <div class="sidebar-user">
        <span class="avatar">{{ (user.display_name || user.username).slice(0, 1).toUpperCase() }}</span>
        <div class="user-meta">
          <strong>{{ user.display_name }}</strong>
          <small>{{ user.role }}</small>
        </div>
        <button class="rail-icon-btn" title="MCP 配置" aria-label="MCP 配置" @click="openDialog('mcp')">
          <Plug :size="17" />
        </button>
        <button
          v-if="user.role === 'admin'"
          class="rail-icon-btn"
          title="系统设置"
          aria-label="系统设置"
          @click="adminSettingsOpen = true"
        >
          <Settings :size="17" />
        </button>
        <button class="rail-icon-btn" title="退出登录" @click="logout">
          <LogOut :size="17" />
        </button>
      </div>
    </aside>

    <section class="main-pane">
      <div class="topbar">
        <div style="display:flex; align-items:center; gap:12px; min-width:0">
          <button class="menu-toggle" aria-label="打开侧栏" @click="sidebarOpen = true">
            <Menu :size="17" />
          </button>
          <nav class="breadcrumb">
            <span class="crumb">{{ selectedWorkspace?.name ?? "工作区" }}</span>
            <span class="sep">/</span>
            <span class="crumb">{{ selectedKnowledgeBase?.name ?? "知识库" }}</span>
            <template v-if="hasActiveDoc">
              <span class="sep">/</span>
              <span class="crumb current">{{ editor.title || "未命名文档" }}</span>
            </template>
          </nav>
        </div>
        <div v-if="hasActiveDoc" class="topbar-actions">
          <template v-if="editorMode === 'edit'">
            <span class="save-state" :class="{ dirty }">{{ dirty ? "未保存" : "已保存" }}</span>
            <button class="btn btn-primary" :disabled="!dirty" @click="saveDocument">保存</button>
          </template>
          <div class="seg">
            <button :class="{ active: editorMode === 'edit' }" @click="editorMode = 'edit'">编辑</button>
            <button :class="{ active: editorMode === 'preview' }" @click="editorMode = 'preview'">预览</button>
          </div>
          <button class="btn btn-danger" @click="deleteDocument">删除</button>
        </div>
      </div>

      <div v-if="hasActiveDoc" class="editor-scroll">
        <div class="editor-sheet">
          <input v-if="editorMode === 'edit'" v-model="editor.title" class="doc-title-input" placeholder="未命名文档" />
          <h1 v-else class="doc-title-static">{{ editor.title || "未命名文档" }}</h1>
          <div class="doc-meta-row">
            <span v-if="activeDoc">更新于 {{ formatTime(activeDoc.updated_at) }}</span>
            <span class="dot">·</span>
            <span>{{ activeDoc?.versions?.length ?? 0 }} 个历史版本</span>
            <template v-if="editorMode === 'edit'">
              <span class="dot">·</span>
              <span class="tags-editor">
                <span v-for="tag in tagChips" :key="tag" class="tag-chip">{{ tag }}</span>
                <input v-model="editor.tags" placeholder="添加标签，逗号分隔" />
              </span>
            </template>
            <template v-else-if="tagChips.length">
              <span class="dot">·</span>
              <span v-for="tag in tagChips" :key="tag" class="tag-chip">{{ tag }}</span>
            </template>
          </div>
          <textarea
            v-if="editorMode === 'edit'"
            ref="editorArea"
            v-model="editor.content"
            class="editor-area"
            placeholder="用 Markdown 记录知识…"
            @input="autogrowEditor"
          />
          <div v-else class="markdown" v-html="previewHtml" />
        </div>
      </div>

      <div v-else class="empty-state">
        <span class="empty-mark">
          <FilePlus2 :size="28" />
        </span>
        <h2>{{ selectedKnowledgeBaseId ? "选择一篇文档，或开始新的记录" : "先创建一个知识库" }}</h2>
        <p>{{ selectedKnowledgeBaseId ? "从左侧知识库中选择文章" : "从左侧知识库标题旁创建后即可开始写作" }}</p>
        <button v-if="selectedKnowledgeBaseId" class="btn btn-primary" @click="createDocument()">新建文档</button>
      </div>
    </section>
  </main>

  <Teleport to="body">
    <div v-if="activeDialog" class="dialog-backdrop" @click.self="closeDialog">
      <section class="dialog" :class="{ 'dialog-wide': activeDialog === 'mcp' }" role="dialog" aria-modal="true">
        <header class="dialog-header">
          <div>
            <p class="dialog-kicker">{{ selectedWorkspace?.name ?? "GugleRAG" }}</p>
            <h2 v-if="activeDialog === 'create-team'">创建团队</h2>
            <h2 v-else-if="activeDialog === 'invite-member'">邀请成员</h2>
            <h2 v-else-if="activeDialog === 'join-team'">加入团队</h2>
            <h2 v-else-if="activeDialog === 'create-knowledge-base'">新建知识库</h2>
            <h2 v-else>MCP 配置</h2>
          </div>
          <button class="dialog-close" title="关闭" aria-label="关闭" @click="closeDialog"><X :size="18" /></button>
        </header>

        <form v-if="activeDialog === 'create-team'" class="dialog-form" @submit.prevent="createTeam">
          <label>团队名称<input v-model="collaborationForm.teamName" autofocus placeholder="例如：产品研发" /></label>
          <p class="hint">创建后会自动生成团队工作区和默认知识库。</p>
          <footer class="dialog-actions">
            <button type="button" class="btn btn-ghost" @click="closeDialog">取消</button>
            <button class="btn btn-primary" :disabled="!collaborationForm.teamName.trim()"><Users :size="16" />创建团队</button>
          </footer>
        </form>

        <form v-else-if="activeDialog === 'invite-member'" class="dialog-form" @submit.prevent="inviteMember">
          <label>用户名<input v-model="collaborationForm.inviteUsername" autofocus placeholder="输入已注册用户的用户名" /></label>
          <div v-if="teamMembers.length" class="dialog-member-list">
            <div v-for="member in teamMembers" :key="member.user_id" class="member-row">
              <span>{{ member.display_name }} <small>@{{ member.username }}</small></span>
              <small>{{ member.role }}</small>
            </div>
          </div>
          <div v-if="lastInviteToken" class="token-output">
            <code>{{ lastInviteToken }}</code>
            <button type="button" title="复制邀请码" aria-label="复制邀请码" @click="copyText(lastInviteToken)"><Copy :size="15" /></button>
          </div>
          <footer class="dialog-actions">
            <button type="button" class="btn btn-ghost" @click="closeDialog">关闭</button>
            <button class="btn btn-primary" :disabled="!collaborationForm.inviteUsername.trim()"><UserPlus :size="16" />生成邀请</button>
          </footer>
        </form>

        <form v-else-if="activeDialog === 'join-team'" class="dialog-form" @submit.prevent="acceptInvitation">
          <label>邀请码<input v-model="collaborationForm.inviteToken" autofocus placeholder="粘贴团队邀请码" /></label>
          <p v-if="pendingInvitations.length" class="hint">待加入：{{ pendingInvitations.map((item) => item.team_name).join("、") }}</p>
          <footer class="dialog-actions">
            <button type="button" class="btn btn-ghost" @click="closeDialog">取消</button>
            <button class="btn btn-primary" :disabled="!collaborationForm.inviteToken.trim()"><LogIn :size="16" />加入团队</button>
          </footer>
        </form>

        <form v-else-if="activeDialog === 'create-knowledge-base'" class="dialog-form" @submit.prevent="createKnowledgeBase">
          <label>知识库名称<input v-model="collaborationForm.knowledgeBaseName" autofocus placeholder="例如：产品文档" /></label>
          <p class="hint">知识库将创建在“{{ selectedWorkspace?.name }}”工作区。</p>
          <footer class="dialog-actions">
            <button type="button" class="btn btn-ghost" @click="closeDialog">取消</button>
            <button class="btn btn-primary" :disabled="!collaborationForm.knowledgeBaseName.trim()"><BookOpen :size="16" />创建知识库</button>
          </footer>
        </form>

        <div v-else class="mcp-panel">
          <button class="mcp-scope" @click="createMcpConfig('user')">
            <span><strong>个人工作区</strong><small>仅访问你的个人知识库</small></span><Copy :size="16" />
          </button>
          <button v-if="selectedTeam" class="mcp-scope" @click="createMcpConfig('group')">
            <span><strong>{{ selectedTeam.name }}</strong><small>访问当前团队工作区的知识库</small></span><Copy :size="16" />
          </button>
          <button class="mcp-scope" @click="createMcpConfig('all')">
            <span><strong>全部可用工作区</strong><small>访问个人与所有已加入团队的知识库</small></span><Copy :size="16" />
          </button>
          <pre v-if="mcpConfig" class="code-block">{{ mcpConfig }}</pre>
        </div>
      </section>
    </div>
  </Teleport>

  <AdminSettingsDialog v-if="adminSettingsOpen" :token="token" @close="adminSettingsOpen = false" />
</template>
