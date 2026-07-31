<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { request } from "../api/client";
import type { SetupPayload, SetupSaveResponse, SetupStatus } from "../types";

defineProps<{ status: SetupStatus }>();

const steps = [
  { key: "service", label: "服务" },
  { key: "database", label: "数据库" },
  { key: "retrieval", label: "检索" },
  { key: "mcp", label: "MCP" }
] as const;

const currentStep = ref(0);
const saved = ref(false);
const saving = ref(false);
const restarting = ref(false);
const error = ref("");
const notice = ref("");

const form = reactive<SetupPayload>({
  server_host: "0.0.0.0",
  server_port: 8080,
  database_url: "sqlite://data/guglerag.db?mode=rwc",
  jwt_secret: crypto.randomUUID().replaceAll("-", "") + crypto.randomUUID().replaceAll("-", ""),
  registration_enabled: true,
  embedding_provider: "siliconflow",
  embedding_model: "BAAI/bge-m3",
  embedding_url: "https://api.siliconflow.cn/v1/embeddings",
  vector_database_url: "",
  siliconflow_url: "https://api.siliconflow.cn",
  siliconflow_api_key: "",
  reranker_enabled: false,
  reranker_provider: "siliconflow",
  reranker_model: "BAAI/bge-reranker-v2-m3",
  reranker_url: "",
  mcp_enabled: true,
  mcp_auth_required: false,
  mcp_public_url: ""
});

const activeStep = computed(() => steps[currentStep.value]);
const isLastStep = computed(() => currentStep.value === steps.length - 1);

function previousStep() {
  error.value = "";
  currentStep.value = Math.max(0, currentStep.value - 1);
}

function nextStep() {
  error.value = "";
  currentStep.value = Math.min(steps.length - 1, currentStep.value + 1);
}

function delay(milliseconds: number) {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

function configuredBaseUrl(): string {
  const host = form.server_host.trim();
  const browserHost = window.location.hostname;
  const targetHost = host === "" || host === "0.0.0.0" || host === "::" || host === "[::]" ? browserHost : host;
  const hostname = targetHost.includes(":") && !targetHost.startsWith("[") ? `[${targetHost}]` : targetHost;
  return `${window.location.protocol}//${hostname}:${form.server_port}`;
}

async function waitForRestart() {
  const targetBaseUrl = configuredBaseUrl();
  const currentBaseUrl = window.location.origin;
  if (targetBaseUrl !== currentBaseUrl) {
    await delay(1500);
    window.location.assign(targetBaseUrl);
    return;
  }

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
}

async function saveSetup() {
  saved.value = false;
  saving.value = true;
  restarting.value = false;
  error.value = "";
  notice.value = "";
  try {
    await request<SetupSaveResponse>("/api/setup", {
      method: "POST",
      body: JSON.stringify(form)
    });
    saved.value = true;
    restarting.value = true;
    notice.value = "配置已写入，正在重启服务…";
    await waitForRestart();
  } catch (err) {
    error.value = err instanceof Error ? err.message : "保存失败";
  } finally {
    saving.value = false;
    restarting.value = false;
  }
}
</script>

<template>
  <main class="setup-shell">
    <aside class="setup-rail">
      <div>
        <div class="sidebar-brand" style="padding:0 0 22px">
          <img class="brand-icon" src="/icon.png" alt="" />
          <div>
            <strong>GugleRAG</strong>
            <small>初始化向导</small>
          </div>
        </div>
        <p class="eyebrow">Bootstrap</p>
        <h1>按步骤完成初始化</h1>
        <p>先确定服务和数据库，再选择检索能力。重排模型可以现在启用，也可以上线后再改 .env。</p>
      </div>
      <nav class="step-list" aria-label="初始化步骤">
        <button
          v-for="(step, index) in steps"
          :key="step.key"
          type="button"
          :class="{ active: currentStep === index, done: currentStep > index }"
          @click="currentStep = index"
        >
          <span>{{ index + 1 }}</span>
          {{ step.label }}
        </button>
      </nav>
    </aside>

    <form class="setup-form" @submit.prevent="isLastStep ? saveSetup() : nextStep()">
      <div class="form-head">
        <span>步骤 {{ currentStep + 1 }} / {{ steps.length }}</span>
        <h2>{{ activeStep.label }}</h2>
      </div>

      <section v-if="activeStep.key === 'service'" class="form-section">
        <div class="field-grid">
          <label>监听地址<input v-model="form.server_host" autocomplete="off" /></label>
          <label>监听端口<input v-model.number="form.server_port" type="number" min="1" max="65535" /></label>
        </div>
        <label>JWT 密钥<input v-model="form.jwt_secret" autocomplete="off" /></label>
        <label class="check"><input v-model="form.registration_enabled" type="checkbox" />允许公开注册</label>
        <p class="hint">首个注册用户会成为管理员。JWT 密钥至少 32 个字符，生产环境不要复用默认值。</p>
      </section>

      <section v-else-if="activeStep.key === 'database'" class="form-section">
        <label>数据库连接串<input v-model="form.database_url" autocomplete="off" /></label>
        <div class="db-examples">
          <button type="button" @click="form.database_url = 'sqlite://data/guglerag.db?mode=rwc'">SQLite</button>
          <button type="button" @click="form.database_url = 'mysql://user:password@127.0.0.1:3306/guglerag'">MySQL</button>
          <button type="button" @click="form.database_url = 'postgresql://user:password@127.0.0.1:5432/guglerag'">PostgreSQL</button>
        </div>
        <p class="hint">SQLite 适合单机部署；MySQL/PostgreSQL 适合团队和容器化部署。</p>
      </section>

      <section v-else-if="activeStep.key === 'retrieval'" class="form-section">
        <div class="field-grid">
          <label>嵌入提供方
            <select v-model="form.embedding_provider">
              <option value="stub">暂不启用</option>
              <option value="local">本地模型</option>
              <option value="siliconflow">SiliconFlow</option>
            </select>
          </label>
          <label>嵌入模型<input v-model="form.embedding_model" /></label>
        </div>
        <div class="field-grid">
          <label>嵌入调用 URL<input v-model="form.embedding_url" /></label>
          <label>SiliconFlow URL<input v-model="form.siliconflow_url" /></label>
        </div>
        <label>
          PostgreSQL 向量数据库（可选）
          <input
            v-model="form.vector_database_url"
            autocomplete="off"
            placeholder="postgresql://user:password@127.0.0.1:5432/vectors"
          />
        </label>
        <p class="hint">需要目标 PostgreSQL 安装 pgvector 扩展；留空使用本地 HNSW 索引。</p>
        <div class="field-grid">
          <label>SiliconFlow API Key<input v-model="form.siliconflow_api_key" type="password" /></label>
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
          <label>重排模型<input v-model="form.reranker_model" /></label>
        </div>
        <label v-if="form.reranker_enabled && ['local', 'custom_http'].includes(form.reranker_provider)">
          重排服务 URL
          <input v-model="form.reranker_url" placeholder="http://127.0.0.1:9000/rerank" />
        </label>
      </section>

      <section v-else class="form-section">
        <label class="check"><input v-model="form.mcp_enabled" type="checkbox" />启用 MCP 端点</label>
        <label class="check"><input v-model="form.mcp_auth_required" type="checkbox" />MCP 调用需要 Bearer Token</label>
        <label>MCP 公网地址（可选）<input v-model="form.mcp_public_url" placeholder="https://kb.example.com" /></label>
        <p class="hint">公开部署时建议启用 MCP 认证。保存后会立即重启服务读取 .env。</p>
      </section>

      <div class="form-actions">
        <button type="button" class="btn btn-ghost" :disabled="currentStep === 0 || saving || restarting" @click="previousStep">上一步</button>
        <p v-if="notice" class="ok">{{ notice }}</p>
        <p v-else-if="saved" class="ok">.env 已写入，正在应用配置。</p>
        <p v-else-if="error" class="bad">{{ error }}</p>
        <p v-else class="hint">目标文件：{{ status.env_path }}</p>
        <button type="submit" class="btn btn-primary" :disabled="saving || restarting">
          {{ isLastStep ? (restarting ? "重启中" : saving ? "保存中" : "保存 .env") : "下一步" }}
        </button>
      </div>
    </form>
  </main>
</template>
