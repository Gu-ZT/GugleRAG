<script setup lang="ts">
import { onMounted, ref } from "vue";
import { request } from "./api/client";
import SetupView from "./views/SetupView.vue";
import WorkspaceView from "./views/WorkspaceView.vue";
import type { SetupStatus } from "./types";

const status = ref<SetupStatus | null>(null);
const loading = ref(true);
const error = ref("");

onMounted(async () => {
  try {
    status.value = await request<SetupStatus>("/api/setup/status");
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法连接后端服务";
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <div v-if="loading" class="boot"><span class="boot-mark">G</span>加载 GugleRAG…</div>
  <div v-else-if="error" class="boot"><span class="boot-mark">G</span><span class="bad">{{ error }}</span></div>
  <SetupView v-else-if="status?.setup_required" :status="status" />
  <WorkspaceView v-else />
</template>
