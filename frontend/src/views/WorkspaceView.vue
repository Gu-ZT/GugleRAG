<script setup lang="ts">
import { onMounted, ref } from "vue";
import { request } from "../api/client";
import type { DocumentItem } from "../types";

const docs = ref<DocumentItem[]>([]);
const token = ref(localStorage.getItem("guglerag.token") ?? "");
const error = ref("");

async function loadDocuments() {
  error.value = "";
  try {
    docs.value = await request<DocumentItem[]>("/api/documents", {
      headers: token.value ? { Authorization: `Bearer ${token.value}` } : {}
    });
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法读取文档";
  }
}

onMounted(loadDocuments);

function persistTokenAndRefresh() {
  localStorage.setItem("guglerag.token", token.value);
  void loadDocuments();
}
</script>

<template>
  <main class="workspace">
    <aside>
      <div class="brand">GugleRAG</div>
      <label>Bearer Token<input v-model="token" placeholder="登录 API 返回的 token" /></label>
      <button @click="persistTokenAndRefresh">刷新文档</button>
    </aside>
    <section>
      <div class="toolbar">
        <h1>知识库</h1>
        <span>{{ docs.length }} 篇文档</span>
      </div>
      <p v-if="error" class="bad">{{ error }}</p>
      <div v-else class="doc-grid">
        <article v-for="doc in docs" :key="doc.id">
          <h2>{{ doc.title }}</h2>
          <p>{{ new Date(doc.updated_at).toLocaleString() }}</p>
        </article>
      </div>
    </section>
  </main>
</template>
