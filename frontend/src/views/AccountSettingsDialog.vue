<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { Eye, EyeOff, KeyRound, Save, X } from "@lucide/vue";
import { request } from "../api/client";
import type { PublicUser } from "../types";

const props = defineProps<{ user: PublicUser; token: string }>();
const emit = defineEmits<{ close: []; saved: [user: PublicUser] }>();

const saving = ref(false);
const error = ref("");
const showCurrentPassword = ref(false);
const showNewPassword = ref(false);
const showPasswordConfirmation = ref(false);
const form = reactive({
  displayName: props.user.display_name,
  currentPassword: "",
  newPassword: "",
  passwordConfirmation: ""
});

const avatarInitial = computed(() =>
  (form.displayName.trim() || props.user.username).slice(0, 1).toUpperCase()
);

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${props.token}` };
}

function validate(): string {
  const displayNameLength = Array.from(form.displayName.trim()).length;
  if (!displayNameLength) return "显示名称不能为空";
  if (displayNameLength > 120) return "显示名称不能超过 120 个字符";

  const changingPassword = Boolean(
    form.currentPassword || form.newPassword || form.passwordConfirmation
  );
  if (!changingPassword) return "";
  if (!form.currentPassword) return "请输入当前密码";
  if (form.newPassword.length < 8) return "新密码至少需要 8 个字符";
  if (form.newPassword !== form.passwordConfirmation) return "两次输入的新密码不一致";
  return "";
}

async function saveProfile() {
  error.value = validate();
  if (error.value) return;

  saving.value = true;
  try {
    const changingPassword = Boolean(form.newPassword);
    const updatedUser = await request<PublicUser>("/api/me", {
      method: "PUT",
      headers: authHeaders(),
      body: JSON.stringify({
        display_name: form.displayName,
        current_password: changingPassword ? form.currentPassword : undefined,
        new_password: changingPassword ? form.newPassword : undefined
      })
    });
    emit("saved", updatedUser);
  } catch (err) {
    error.value = err instanceof Error ? err.message : "无法保存账户信息";
  } finally {
    saving.value = false;
  }
}

function close() {
  if (!saving.value) emit("close");
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") close();
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <Teleport to="body">
    <div class="dialog-backdrop" @click.self="close">
      <section
        class="dialog account-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="account-settings-title"
      >
        <header class="dialog-header">
          <div>
            <p class="dialog-kicker">个人账户</p>
            <h2 id="account-settings-title">账户设置</h2>
          </div>
          <button
            class="dialog-close"
            type="button"
            title="关闭"
            aria-label="关闭"
            :disabled="saving"
            @click="close"
          >
            <X :size="18" />
          </button>
        </header>

        <form class="account-form" @submit.prevent="saveProfile">
          <div class="account-identity">
            <span class="account-avatar" aria-hidden="true">{{ avatarInitial }}</span>
            <div>
              <strong>{{ form.displayName.trim() || user.display_name }}</strong>
              <span>@{{ user.username }}</span>
            </div>
          </div>

          <section class="account-section">
            <label>
              显示名称
              <input
                v-model="form.displayName"
                autofocus
                maxlength="120"
                autocomplete="name"
              />
            </label>
          </section>

          <section class="account-section account-password-section">
            <header><KeyRound :size="15" /><span>修改密码</span></header>
            <label>
              当前密码
              <span class="secret-input">
                <input
                  v-model="form.currentPassword"
                  :type="showCurrentPassword ? 'text' : 'password'"
                  autocomplete="current-password"
                />
                <button
                  type="button"
                  :title="showCurrentPassword ? '隐藏当前密码' : '显示当前密码'"
                  :aria-label="showCurrentPassword ? '隐藏当前密码' : '显示当前密码'"
                  @click="showCurrentPassword = !showCurrentPassword"
                >
                  <EyeOff v-if="showCurrentPassword" :size="16" />
                  <Eye v-else :size="16" />
                </button>
              </span>
            </label>
            <div class="field-grid">
              <label>
                新密码
                <span class="secret-input">
                  <input
                    v-model="form.newPassword"
                    :type="showNewPassword ? 'text' : 'password'"
                    minlength="8"
                    autocomplete="new-password"
                    placeholder="至少 8 个字符"
                  />
                  <button
                    type="button"
                    :title="showNewPassword ? '隐藏新密码' : '显示新密码'"
                    :aria-label="showNewPassword ? '隐藏新密码' : '显示新密码'"
                    @click="showNewPassword = !showNewPassword"
                  >
                    <EyeOff v-if="showNewPassword" :size="16" />
                    <Eye v-else :size="16" />
                  </button>
                </span>
              </label>
              <label>
                确认新密码
                <span class="secret-input">
                  <input
                    v-model="form.passwordConfirmation"
                    :type="showPasswordConfirmation ? 'text' : 'password'"
                    minlength="8"
                    autocomplete="new-password"
                  />
                  <button
                    type="button"
                    :title="showPasswordConfirmation ? '隐藏确认密码' : '显示确认密码'"
                    :aria-label="showPasswordConfirmation ? '隐藏确认密码' : '显示确认密码'"
                    @click="showPasswordConfirmation = !showPasswordConfirmation"
                  >
                    <EyeOff v-if="showPasswordConfirmation" :size="16" />
                    <Eye v-else :size="16" />
                  </button>
                </span>
              </label>
            </div>
          </section>

          <p v-if="error" class="bad" role="alert">{{ error }}</p>
          <footer class="dialog-actions">
            <button type="button" class="btn btn-ghost" :disabled="saving" @click="close">取消</button>
            <button class="btn btn-primary" :disabled="saving || !form.displayName.trim()">
              <Save :size="16" />{{ saving ? "正在保存…" : "保存更改" }}
            </button>
          </footer>
        </form>
      </section>
    </div>
  </Teleport>
</template>
