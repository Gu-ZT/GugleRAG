<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { Edit3, RefreshCw, Save, Trash2, UserPlus, X } from "@lucide/vue";
import { request } from "../api/client";
import type { AdminUser, AdminUserPayload, PublicUser } from "../types";

const props = defineProps<{ token: string }>();

const users = ref<AdminUser[]>([]);
const me = ref<PublicUser | null>(null);
const loading = ref(true);
const saving = ref(false);
const deletingUserId = ref("");
const error = ref("");
const notice = ref("");

const form = reactive<AdminUserPayload & { id: string }>({
  id: "",
  username: "",
  display_name: "",
  password: "",
  role: "editor"
});

const editing = computed(() => Boolean(form.id));
const canSave = computed(() => {
  if (!form.username.trim()) return false;
  return editing.value || Boolean(form.password?.trim());
});

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${props.token}` };
}

function roleLabel(role: AdminUser["role"]) {
  return {
    admin: "管理员",
    editor: "编辑者",
    reader: "只读用户"
  }[role];
}

function workspaceKindLabel(kind: AdminUser["workspaces"][number]["kind"]) {
  return kind === "personal" ? "个人" : "团队";
}

function resetForm() {
  Object.assign(form, {
    id: "",
    username: "",
    display_name: "",
    password: "",
    role: "editor"
  });
}

async function loadUsers() {
  loading.value = true;
  error.value = "";
  try {
    const [currentUser, userList] = await Promise.all([
      request<PublicUser>("/api/me", { headers: authHeaders() }),
      request<AdminUser[]>("/api/admin/users", { headers: authHeaders() })
    ]);
    me.value = currentUser;
    users.value = userList;
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法读取用户列表";
  } finally {
    loading.value = false;
  }
}

function editUser(user: AdminUser) {
  Object.assign(form, {
    id: user.id,
    username: user.username,
    display_name: user.display_name,
    password: "",
    role: user.role
  });
  notice.value = "";
  error.value = "";
}

async function saveUser() {
  saving.value = true;
  error.value = "";
  notice.value = "";
  const payload: AdminUserPayload = {
    username: form.username.trim(),
    display_name: form.display_name?.trim() || undefined,
    password: form.password?.trim() || undefined,
    role: form.role
  };
  try {
    await request<AdminUser>(editing.value ? `/api/admin/users/${form.id}` : "/api/admin/users", {
      method: editing.value ? "PUT" : "POST",
      headers: authHeaders(),
      body: JSON.stringify(payload)
    });
    notice.value = editing.value ? "用户已更新。" : "用户已创建。";
    resetForm();
    await loadUsers();
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法保存用户";
  } finally {
    saving.value = false;
  }
}

async function deleteUser(user: AdminUser) {
  if (!window.confirm(`删除用户 ${user.username}？该用户拥有的个人工作区和团队工作区也会被删除。`)) {
    return;
  }
  deletingUserId.value = user.id;
  error.value = "";
  notice.value = "";
  try {
    await request<void>(`/api/admin/users/${user.id}`, {
      method: "DELETE",
      headers: authHeaders()
    });
    if (form.id === user.id) resetForm();
    notice.value = "用户已删除。";
    await loadUsers();
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法删除用户";
  } finally {
    deletingUserId.value = "";
  }
}

onMounted(loadUsers);
</script>

<template>
  <section class="admin-config-section admin-users-panel">
    <div class="admin-user-form">
      <div class="admin-user-form-head">
        <div>
          <h3>{{ editing ? "修改用户" : "新增用户" }}</h3>
          <p class="hint">限制注册时，新账号只能由管理员在这里创建。</p>
        </div>
        <button v-if="editing" class="icon-button" type="button" title="取消编辑" aria-label="取消编辑" @click="resetForm">
          <X :size="16" />
        </button>
      </div>
      <div class="field-grid">
        <label>用户名<input v-model.trim="form.username" autocomplete="off" /></label>
        <label>显示名称<input v-model.trim="form.display_name" autocomplete="off" /></label>
      </div>
      <div class="field-grid">
        <label>
          角色
          <select v-model="form.role">
            <option value="admin">管理员</option>
            <option value="editor">编辑者</option>
            <option value="reader">只读用户</option>
          </select>
        </label>
        <label>
          密码
          <input
            v-model="form.password"
            type="password"
            autocomplete="new-password"
            :placeholder="editing ? '留空则不修改密码' : '至少 8 个字符'"
          />
        </label>
      </div>
      <button class="btn btn-primary" type="button" :disabled="saving || !canSave" @click="saveUser">
        <Save v-if="editing" :size="16" />
        <UserPlus v-else :size="16" />
        {{ saving ? "保存中" : editing ? "保存用户" : "新增用户" }}
      </button>
    </div>

    <div class="admin-user-toolbar">
      <div>
        <h3>用户列表</h3>
        <p class="hint">{{ users.length }} 个用户</p>
      </div>
      <button class="icon-button" type="button" title="刷新用户列表" aria-label="刷新用户列表" @click="loadUsers">
        <RefreshCw :size="16" :class="{ spinning: loading }" />
      </button>
    </div>

    <p v-if="error" class="bad">{{ error }}</p>
    <p v-else-if="notice" class="ok">{{ notice }}</p>

    <div v-if="loading" class="admin-loading">正在读取用户列表…</div>
    <div v-else class="admin-user-list">
      <article v-for="item in users" :key="item.id" class="admin-user-row">
        <div class="admin-user-main">
          <div>
            <strong>{{ item.display_name }}</strong>
            <small>@{{ item.username }} · {{ roleLabel(item.role) }}</small>
          </div>
          <div class="admin-user-actions">
            <button class="icon-button" type="button" title="编辑用户" aria-label="编辑用户" @click="editUser(item)">
              <Edit3 :size="16" />
            </button>
            <button
              class="icon-button danger"
              type="button"
              title="删除用户"
              aria-label="删除用户"
              :disabled="item.id === me?.id || deletingUserId === item.id"
              @click="deleteUser(item)"
            >
              <Trash2 :size="16" />
            </button>
          </div>
        </div>
        <div class="admin-user-workspaces">
          <span v-for="workspace in item.workspaces" :key="workspace.id">
            {{ workspaceKindLabel(workspace.kind) }} · {{ workspace.name }}
          </span>
          <span v-if="item.workspaces.length === 0">无可访问工作区</span>
        </div>
      </article>
    </div>
  </section>
</template>
