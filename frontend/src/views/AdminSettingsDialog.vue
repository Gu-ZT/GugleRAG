<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { Eye, EyeOff, Plug, RefreshCw, Save, Search, Settings, UserCog, X } from "@lucide/vue";
import { request } from "../api/client";
import AdminUsersPanel from "./AdminUsersPanel.vue";
import type {
  AdminConfigPayload,
  AdminConfigResponse,
  AdminConfigSaveResponse,
  RestartResponse
} from "../types";

const props = defineProps<{ token: string }>();
const emit = defineEmits<{ close: [] }>();

type Tab = "service" | "retrieval" | "access" | "users";

const activeTab = ref<Tab>("service");
const loading = ref(true);
const saving = ref(false);
const restarting = ref(false);
const restartConfirming = ref(false);
const restartRequired = ref(false);
const error = ref("");
const notice = ref("");
const envPath = ref("");
const jwtConfigured = ref(false);
const siliconflowKeyConfigured = ref(false);
const showJwtSecret = ref(false);
const showSiliconflowKey = ref(false);
const initialFingerprint = ref("");

const form = reactive<AdminConfigPayload>({
  server_host: "0.0.0.0",
  server_port: 8080,
  database_url: "sqlite://data/guglerag.db?mode=rwc",
  jwt_secret: "",
  registration_enabled: true,
  embedding_provider: "siliconflow",
  embedding_model: "BAAI/bge-m3",
  embedding_url: "https://api.siliconflow.cn/v1/embeddings",
  vector_database_url: "",
  siliconflow_url: "https://api.siliconflow.cn",
  siliconflow_api_key: "",
  reranker_enabled: false,
  reranker_provider: "local",
  reranker_model: "BAAI/bge-reranker-v2-m3",
  reranker_url: "",
  mcp_enabled: true,
  mcp_auth_required: false,
  mcp_public_url: ""
});

const tabs = [
  { id: "service" as const, label: "服务", icon: Settings },
  { id: "retrieval" as const, label: "检索", icon: Search },
  { id: "access" as const, label: "接入", icon: Plug },
  { id: "users" as const, label: "用户", icon: UserCog }
];

const isDirty = computed(() => initialFingerprint.value !== fingerprint());

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${props.token}` };
}

function fingerprint(): string {
  const { jwt_secret, siliconflow_api_key, ...config } = form;
  return JSON.stringify(config);
}

function normalizeRerankerProvider(provider: AdminConfigResponse["current"]["reranker_provider"]) {
  return provider === "none" ? "local" : provider;
}

function applyConfig(response: AdminConfigResponse) {
  Object.assign(form, {
    ...response.current,
    reranker_provider: normalizeRerankerProvider(response.current.reranker_provider),
    jwt_secret: "",
    siliconflow_api_key: ""
  });
  restartRequired.value = response.restart_required;
  envPath.value = response.env_path;
  jwtConfigured.value = response.secrets.jwt_secret_configured;
  siliconflowKeyConfigured.value = response.secrets.siliconflow_api_key_configured;
  initialFingerprint.value = fingerprint();
}

async function loadConfig() {
  loading.value = true;
  error.value = "";
  try {
    const response = await request<AdminConfigResponse>("/api/admin/config", {
      headers: authHeaders()
    });
    applyConfig(response);
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法读取系统配置";
  } finally {
    loading.value = false;
  }
}

async function saveConfig() {
  saving.value = true;
  error.value = "";
  notice.value = "";
  try {
    const response = await request<AdminConfigSaveResponse>("/api/admin/config", {
      method: "PUT",
      headers: authHeaders(),
      body: JSON.stringify(form)
    });
    form.jwt_secret = "";
    form.siliconflow_api_key = "";
    jwtConfigured.value = true;
    restartRequired.value = response.restart_required;
    envPath.value = response.env_path;
    initialFingerprint.value = fingerprint();
    notice.value = "配置已保存。";
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法保存系统配置";
  } finally {
    saving.value = false;
  }
}

function delay(milliseconds: number) {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

async function restartService() {
  restarting.value = true;
  restartConfirming.value = false;
  error.value = "";
  notice.value = "正在重启服务…";
  try {
    await request<RestartResponse>("/api/admin/restart", {
      method: "POST",
      headers: authHeaders()
    });
    const startedAt = Date.now();
    const deadline = startedAt + 30_000;
    let wasUnavailable = false;
    while (Date.now() < deadline) {
      await delay(700);
      try {
        const response = await fetch("/health", { cache: "no-store" });
        if (response.ok && (wasUnavailable || Date.now() - startedAt > 2_000)) {
          window.location.reload();
          return;
        }
      } catch {
        wasUnavailable = true;
      }
    }
    notice.value = "服务仍在重启。请稍后刷新页面；若修改了地址或端口，请打开新的服务地址。";
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法请求重启服务";
  } finally {
    restarting.value = false;
  }
}

function close() {
  if (!saving.value && !restarting.value) emit("close");
}

onMounted(loadConfig);
</script>

<template>
  <Teleport to="body">
    <div class="dialog-backdrop" @click.self="close">
      <section class="dialog admin-dialog" role="dialog" aria-modal="true" aria-labelledby="admin-settings-title">
        <header class="dialog-header">
          <div>
            <p class="dialog-kicker">系统运维</p>
            <h2 id="admin-settings-title">服务配置</h2>
          </div>
          <button class="dialog-close" title="关闭" aria-label="关闭" :disabled="saving || restarting" @click="close">
            <X :size="18" />
          </button>
        </header>

        <div v-if="loading" class="admin-loading">正在读取配置…</div>
        <div v-else class="admin-content">
          <div class="admin-status" :class="{ pending: restartRequired }">
            <span class="admin-status-dot" />
            <div>
              <strong>{{ restartRequired ? "配置待应用" : "当前配置已生效" }}</strong>
              <small>{{ envPath }}</small>
            </div>
          </div>

          <nav class="admin-tabs" role="tablist" aria-label="配置分类">
            <button
              v-for="tab in tabs"
              :key="tab.id"
              type="button"
              :class="{ active: activeTab === tab.id }"
              role="tab"
              :aria-selected="activeTab === tab.id"
              @click="activeTab = tab.id"
            >
              <component :is="tab.icon" :size="16" />
              {{ tab.label }}
            </button>
          </nav>

          <div class="admin-config-form">
            <section v-if="activeTab === 'service'" class="admin-config-section">
              <div class="field-grid">
                <label>监听地址<input v-model.trim="form.server_host" autocomplete="off" /></label>
                <label>监听端口<input v-model.number="form.server_port" type="number" min="1" max="65535" /></label>
              </div>
              <label>数据库连接串<input v-model.trim="form.database_url" autocomplete="off" /></label>
              <p class="hint">切换数据库后，重启将使用新的连接；请确认目标库包含需要的账户数据。</p>
              <label>
                JWT 密钥
                <span class="secret-input">
                  <input
                    v-model="form.jwt_secret"
                    :type="showJwtSecret ? 'text' : 'password'"
                    autocomplete="new-password"
                    :placeholder="jwtConfigured ? '留空以保留当前密钥' : '至少 32 个字符'"
                  />
                  <button
                    type="button"
                    :title="showJwtSecret ? '隐藏 JWT 密钥' : '显示 JWT 密钥'"
                    :aria-label="showJwtSecret ? '隐藏 JWT 密钥' : '显示 JWT 密钥'"
                    @click="showJwtSecret = !showJwtSecret"
                  >
                    <EyeOff v-if="showJwtSecret" :size="16" />
                    <Eye v-else :size="16" />
                  </button>
                </span>
              </label>
              <p class="hint">更换 JWT 密钥后，所有现有登录会话将在重启后失效。</p>
            </section>

            <section v-else-if="activeTab === 'retrieval'" class="admin-config-section">
              <div class="field-grid">
                <label>嵌入提供方
                  <select v-model="form.embedding_provider">
                    <option value="stub">暂不启用</option>
                    <option value="local">本地模型</option>
                    <option value="siliconflow">SiliconFlow</option>
                  </select>
                </label>
                <label>嵌入模型<input v-model.trim="form.embedding_model" /></label>
              </div>
              <div class="field-grid">
                <label>嵌入调用 URL<input v-model.trim="form.embedding_url" /></label>
                <label>SiliconFlow URL<input v-model.trim="form.siliconflow_url" /></label>
              </div>
              <label>
                PostgreSQL 向量数据库（可选）
                <input
                  v-model.trim="form.vector_database_url"
                  autocomplete="off"
                  placeholder="postgresql://user:password@127.0.0.1:5432/vectors"
                />
              </label>
              <p class="hint">填写后使用 PostgreSQL 的 pgvector 存储和检索；留空使用本地 HNSW 索引。</p>
              <div class="field-grid">
                <label>
                  SiliconFlow API Key
                  <span class="secret-input">
                    <input
                      v-model="form.siliconflow_api_key"
                      :type="showSiliconflowKey ? 'text' : 'password'"
                      autocomplete="new-password"
                      :placeholder="siliconflowKeyConfigured ? '留空以保留当前密钥' : '未配置'"
                    />
                    <button
                      type="button"
                      :title="showSiliconflowKey ? '隐藏 API Key' : '显示 API Key'"
                      :aria-label="showSiliconflowKey ? '隐藏 API Key' : '显示 API Key'"
                      @click="showSiliconflowKey = !showSiliconflowKey"
                    >
                      <EyeOff v-if="showSiliconflowKey" :size="16" />
                      <Eye v-else :size="16" />
                    </button>
                  </span>
                </label>
              </div>
              <label class="check"><input v-model="form.reranker_enabled" type="checkbox" />启用重排模型</label>
              <div v-if="form.reranker_enabled" class="field-grid">
                <label>重排提供方
                  <select v-model="form.reranker_provider">
                    <option value="siliconflow">SiliconFlow</option>
                    <option value="local">本地模型</option>
                    <option value="custom_http">自定义 HTTP 服务</option>
                  </select>
                </label>
                <label>重排模型<input v-model.trim="form.reranker_model" /></label>
              </div>
              <label v-if="form.reranker_enabled && ['local', 'custom_http'].includes(form.reranker_provider)">
                重排服务 URL
                <input v-model.trim="form.reranker_url" placeholder="http://127.0.0.1:9000/rerank" />
              </label>
            </section>

            <section v-else-if="activeTab === 'access'" class="admin-config-section">
              <label class="check"><input v-model="form.registration_enabled" type="checkbox" />允许公开注册</label>
              <label class="check"><input v-model="form.mcp_enabled" type="checkbox" />启用 MCP 端点</label>
              <label class="check">
                <input v-model="form.mcp_auth_required" type="checkbox" />MCP 调用需要 Bearer Token
              </label>
              <label>
                MCP 公网地址（可选）
                <input v-model.trim="form.mcp_public_url" placeholder="https://kb.example.com" />
              </label>
              <p class="hint">反向代理或监听地址为通配符时，使用公网地址生成 MCP 配置。</p>
            </section>

            <AdminUsersPanel v-else :token="token" />

            <footer v-if="activeTab !== 'users'" class="admin-actions">
              <p v-if="error" class="bad">{{ error }}</p>
              <p v-else-if="notice" class="ok">{{ notice }}</p>
              <p v-else-if="isDirty" class="hint">尚有未保存的修改。</p>
              <p v-else class="hint">保存后再重启以应用配置。</p>
              <div>
                <button
                  class="btn btn-ghost"
                  type="button"
                  :disabled="saving || restarting || !isDirty"
                  @click="saveConfig"
                >
                  <Save :size="16" />{{ saving ? "保存中" : "保存配置" }}
                </button>
                <button
                  class="btn admin-restart-btn"
                  type="button"
                  :disabled="saving || restarting || isDirty"
                  @click="restartConfirming = true"
                >
                  <RefreshCw :size="16" :class="{ spinning: restarting }" />{{ restarting ? "重启中" : "重启程序" }}
                </button>
              </div>
            </footer>
          </div>

          <div v-if="restartConfirming" class="restart-confirm" role="alert">
            <p>重启会短暂中断服务。已保存的配置将在服务重新就绪后生效。</p>
            <div>
              <button class="btn btn-ghost" type="button" @click="restartConfirming = false">取消</button>
              <button class="btn admin-restart-btn" type="button" @click="restartService">
                <RefreshCw :size="16" />确认重启
              </button>
            </div>
          </div>
        </div>
      </section>
    </div>
  </Teleport>
</template>
