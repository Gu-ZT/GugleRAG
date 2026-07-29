<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { request } from "../api/client";
import type { SetupPayload, SetupStatus } from "../types";

defineProps<{ status: SetupStatus }>();

const steps = [
  { key: "service", label: "服务" },
  { key: "database", label: "数据库" },
  { key: "retrieval", label: "检索" },
  { key: "mcp", label: "MCP" }
] as const;

const currentStep = ref(0);
const saved = ref(false);
const error = ref("");

const form = reactive<SetupPayload>({
  server_host: "0.0.0.0",
  server_port: 8080,
  database_url: "sqlite://data/guglerag.db?mode=rwc",
  jwt_secret: crypto.randomUUID().replaceAll("-", "") + crypto.randomUUID().replaceAll("-", ""),
  embedding_provider: "stub",
  embedding_model: "BAAI/bge-m3",
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

async function saveSetup() {
  saved.value = false;
  error.value = "";
  try {
    await request("/api/setup", {
      method: "POST",
      body: JSON.stringify(form)
    });
    saved.value = true;
  } catch (err) {
    error.value = err instanceof Error ? err.message : "保存失败";
  }
}
</script>

<template>
  <main class="setup-shell">
    <aside class="setup-rail">
      <div>
        <p class="eyebrow">GugleRAG bootstrap</p>
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
          <label>SiliconFlow URL<input v-model="form.siliconflow_url" /></label>
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
        <label v-if="form.reranker_enabled && form.reranker_provider === 'custom_http'">
          重排服务 URL
          <input v-model="form.reranker_url" placeholder="http://127.0.0.1:9000/rerank" />
        </label>
      </section>

      <section v-else class="form-section">
        <label class="check"><input v-model="form.mcp_enabled" type="checkbox" />启用 MCP 端点</label>
        <label class="check"><input v-model="form.mcp_auth_required" type="checkbox" />MCP 调用需要 Bearer Token</label>
        <label>MCP 公网地址（可选）<input v-model="form.mcp_public_url" placeholder="https://kb.example.com" /></label>
        <p class="hint">公开部署时建议启用 MCP 认证。保存后需要重启后端服务读取 .env。</p>
      </section>

      <div class="form-actions">
        <button type="button" class="secondary" :disabled="currentStep === 0" @click="previousStep">上一步</button>
        <p v-if="saved" class="ok">.env 已写入，重启后端服务后生效。</p>
        <p v-else-if="error" class="bad">{{ error }}</p>
        <p v-else class="hint">目标文件：{{ status.env_path }}</p>
        <button type="submit">{{ isLastStep ? "保存 .env" : "下一步" }}</button>
      </div>
    </form>
  </main>
</template>
