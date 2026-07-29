<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { request } from "../api/client";
import type { AuthResponse, DocumentItem, PublicUser, SearchResult } from "../types";

const docs = ref<DocumentItem[]>([]);
const activeDoc = ref<DocumentItem | null>(null);
const token = ref(localStorage.getItem("guglerag.token") ?? "");
const user = ref<PublicUser | null>(null);
const authMode = ref<"login" | "register">("login");
const editorMode = ref<"edit" | "preview">("edit");
const query = ref("");
const error = ref("");
const notice = ref("");

const authForm = reactive({
  username: "",
  password: "",
  display_name: ""
});

const editor = reactive({
  title: "",
  content: "",
  tags: ""
});

const authHeaders = computed<Record<string, string>>(() => {
  if (!token.value) {
    return {};
  }
  return { Authorization: `Bearer ${token.value}` };
});
const hasActiveDoc = computed(() => Boolean(activeDoc.value?.id));

function setMessage(kind: "error" | "notice", message: string) {
  if (kind === "error") {
    error.value = message;
    notice.value = "";
  } else {
    notice.value = message;
    error.value = "";
  }
}

async function authenticate() {
  setMessage("notice", "");
  const path = authMode.value === "login" ? "/api/auth/login" : "/api/auth/register";
  try {
    const body = {
      username: authForm.username,
      password: authForm.password,
      display_name: authForm.display_name || undefined
    };
    const response = await request<AuthResponse>(path, {
      method: "POST",
      body: JSON.stringify(body)
    });
    token.value = response.token;
    user.value = response.user;
    localStorage.setItem("guglerag.token", response.token);
    await loadDocuments();
    setMessage("notice", "已进入知识库。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "登录失败");
  }
}

async function loadMe() {
  if (!token.value) {
    return;
  }
  try {
    user.value = await request<PublicUser>("/api/me", { headers: authHeaders.value });
    await loadDocuments();
  } catch {
    logout();
  }
}

async function loadDocuments() {
  try {
    docs.value = await request<DocumentItem[]>("/api/documents", { headers: authHeaders.value });
    if (!activeDoc.value && docs.value.length > 0) {
      await openDocument(docs.value[0].id);
    }
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "无法读取文档");
  }
}

async function searchDocuments() {
  try {
    if (!query.value.trim()) {
      activeDoc.value = null;
      await loadDocuments();
      return;
    }
    const results = await request<SearchResult[]>(
      `/api/search?q=${encodeURIComponent(query.value)}&limit=30`,
      { headers: authHeaders.value }
    );
    docs.value = results.map((item) => ({
      id: item.id,
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
    activeDoc.value = await request<DocumentItem>(`/api/documents/${id}`, { headers: authHeaders.value });
    editor.title = activeDoc.value.title;
    editor.content = activeDoc.value.content ?? "";
    editor.tags = activeDoc.value.tags.join(", ");
    editorMode.value = "edit";
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "无法打开文档");
  }
}

async function createDocument() {
  try {
    const created = await request<DocumentItem>("/api/documents", {
      method: "POST",
      headers: authHeaders.value,
      body: JSON.stringify({
        title: "未命名文档",
        content: "# 未命名文档\n\n开始记录团队知识。",
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
  if (!activeDoc.value) {
    return;
  }
  try {
    const saved = await request<DocumentItem>(`/api/documents/${activeDoc.value.id}`, {
      method: "PUT",
      headers: authHeaders.value,
      body: JSON.stringify({
        title: editor.title,
        content: editor.content,
        tags: editor.tags.split(",").map((tag) => tag.trim()).filter(Boolean)
      })
    });
    activeDoc.value = saved;
    await loadDocuments();
    activeDoc.value = saved;
    setMessage("notice", "文档已保存。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "保存失败");
  }
}

async function deleteDocument() {
  if (!activeDoc.value || !window.confirm("删除当前文档？")) {
    return;
  }
  try {
    await request(`/api/documents/${activeDoc.value.id}`, {
      method: "DELETE",
      headers: authHeaders.value
    });
    activeDoc.value = null;
    editor.title = "";
    editor.content = "";
    editor.tags = "";
    await loadDocuments();
    setMessage("notice", "文档已删除。");
  } catch (err) {
    setMessage("error", err instanceof Error ? err.message : "删除失败");
  }
}

function logout() {
  token.value = "";
  user.value = null;
  docs.value = [];
  activeDoc.value = null;
  localStorage.removeItem("guglerag.token");
}

onMounted(loadMe);
</script>

<template>
  <main v-if="!user" class="auth-screen">
    <section class="auth-panel">
      <p class="eyebrow">GugleRAG workspace</p>
      <h1>{{ authMode === "login" ? "登录知识库" : "创建第一个账号" }}</h1>
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
        <div>
          <div class="brand">GugleRAG</div>
          <p>{{ user.display_name }} · {{ user.role }}</p>
        </div>
        <button class="icon-button" title="退出登录" @click="logout">退出</button>
      </div>

      <div class="search-row">
        <input v-model="query" placeholder="搜索标题、正文或标签" @keydown.enter="searchDocuments" />
        <button class="secondary" @click="searchDocuments">搜索</button>
      </div>
      <button @click="createDocument">新建文档</button>

      <div class="doc-list">
        <button
          v-for="doc in docs"
          :key="doc.id"
          class="doc-row"
          :class="{ active: activeDoc?.id === doc.id }"
          @click="openDocument(doc.id)"
        >
          <strong>{{ doc.title }}</strong>
          <span>{{ new Date(doc.updated_at).toLocaleString() }}</span>
        </button>
      </div>
    </aside>

    <section class="editor-pane">
      <div class="toolbar">
        <div>
          <h1>{{ hasActiveDoc ? editor.title || "未命名文档" : "选择或新建文档" }}</h1>
          <span v-if="activeDoc">{{ activeDoc.versions?.length ?? 0 }} 个历史版本</span>
        </div>
        <div class="toolbar-actions">
          <button class="secondary" :class="{ active: editorMode === 'edit' }" @click="editorMode = 'edit'">编辑</button>
          <button class="secondary" :class="{ active: editorMode === 'preview' }" @click="editorMode = 'preview'">预览</button>
          <button :disabled="!hasActiveDoc" @click="saveDocument">保存</button>
          <button class="danger" :disabled="!hasActiveDoc" @click="deleteDocument">删除</button>
        </div>
      </div>

      <p v-if="error" class="bad">{{ error }}</p>
      <p v-if="notice" class="ok">{{ notice }}</p>

      <div v-if="hasActiveDoc" class="editor-grid">
        <label>标题<input v-model="editor.title" /></label>
        <label>标签<input v-model="editor.tags" placeholder="用逗号分隔" /></label>
        <textarea v-if="editorMode === 'edit'" v-model="editor.content" />
        <pre v-else class="preview">{{ editor.content }}</pre>
      </div>
      <div v-else class="empty-state">
        <h2>还没有打开的文档</h2>
        <p>创建一篇文档，或从左侧列表选择已有内容。</p>
        <button @click="createDocument">新建文档</button>
      </div>
    </section>
  </main>
</template>
