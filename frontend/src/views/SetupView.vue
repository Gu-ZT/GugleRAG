<script setup lang="ts">
import { reactive, ref } from "vue";
import { request } from "../api/client";
import type { SetupPayload, SetupStatus } from "../types";

defineProps<{ status: SetupStatus }>();

const saved = ref(false);
const error = ref("");

const form = reactive<SetupPayload>({
  server_host: "0.0.0.0",
  server_port: 8080,
  database_url: "sqlite://data/guglerag.db",
  jwt_secret: crypto.randomUUID().replaceAll("-", "") + crypto.randomUUID().replaceAll("-", ""),
  embedding_provider: "stub",
  embedding_model: "BAAI/bge-m3",
  siliconflow_url: "https://api.siliconflow.cn",
  siliconflow_api_key: "",
  mcp_enabled: true,
  mcp_auth_required: false
});

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
        <h1>写入第一份运行配置</h1>
        <p>设置监听地址、数据库连接、JWT 密钥和 MCP 认证策略。保存后重启后端服务，知识库工作台会自动接管首页。</p>
      </div>
      <div class="signal" aria-hidden="true"></div>
    </aside>

    <form class="setup-form" @submit.prevent="saveSetup">
      <section>
        <h2>服务</h2>
        <div class="field-grid">
          <label>监听地址<input v-model="form.server_host" /></label>
          <label>监听端口<input v-model.number="form.server_port" type="number" min="1" max="65535" /></label>
        </div>
      </section>

      <section>
        <h2>数据库</h2>
        <label>连接串<input v-model="form.database_url" /></label>
        <p class="hint">SQLite: sqlite://data/guglerag.db。MySQL 和 PostgreSQL 使用标准连接串。</p>
      </section>

      <section>
        <h2>安全与检索</h2>
        <label>JWT 密钥<input v-model="form.jwt_secret" /></label>
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
      </section>

      <section>
        <h2>MCP</h2>
        <label class="check"><input v-model="form.mcp_enabled" type="checkbox" />启用 MCP 端点</label>
        <label class="check"><input v-model="form.mcp_auth_required" type="checkbox" />MCP 调用需要 Bearer Token</label>
      </section>

      <div class="form-actions">
        <p v-if="saved" class="ok">.env 已写入，重启后端服务后生效。</p>
        <p v-else-if="error" class="bad">{{ error }}</p>
        <p v-else class="hint">目标文件：{{ status.env_path }}</p>
        <button type="submit">保存 .env</button>
      </div>
    </form>
  </main>
</template>
