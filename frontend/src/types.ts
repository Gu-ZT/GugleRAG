export type DatabaseEngine = "sqlite" | "mysql" | "postgres";

export interface SetupStatus {
  setup_required: boolean;
  env_path: string;
  supported_databases: DatabaseEngine[];
  current: {
    server_host: string;
    server_port: number;
    database: {
      engine: DatabaseEngine;
      url: string;
    };
    registration_enabled: boolean;
    mcp_enabled: boolean;
    mcp_auth_required: boolean;
    embedding_provider: string;
    embedding_model: string;
    embedding_url: string;
    reranker_enabled: boolean;
    reranker_provider: string;
    reranker_model: string;
    reranker_url: string;
  };
}

export interface SetupPayload {
  server_host: string;
  server_port: number;
  database_url: string;
  jwt_secret: string;
  registration_enabled: boolean;
  embedding_provider: "stub" | "local" | "siliconflow";
  embedding_model: string;
  embedding_url: string;
  siliconflow_url: string;
  siliconflow_api_key: string;
  reranker_enabled: boolean;
  reranker_provider: "local" | "siliconflow" | "custom_http";
  reranker_model: string;
  reranker_url: string;
  mcp_enabled: boolean;
  mcp_auth_required: boolean;
  mcp_public_url: string;
}

export interface SetupSaveResponse {
  ok: boolean;
  env_path: string;
  restart_required: boolean;
  restarting: boolean;
}

export interface AdminConfigValues {
  server_host: string;
  server_port: number;
  database_url: string;
  registration_enabled: boolean;
  embedding_provider: "stub" | "local" | "siliconflow";
  embedding_model: string;
  embedding_url: string;
  siliconflow_url: string;
  reranker_enabled: boolean;
  reranker_provider: "none" | "local" | "siliconflow" | "custom_http";
  reranker_model: string;
  reranker_url: string;
  mcp_enabled: boolean;
  mcp_auth_required: boolean;
  mcp_public_url: string;
}

export interface AdminConfigResponse {
  env_path: string;
  restart_required: boolean;
  secrets: {
    jwt_secret_configured: boolean;
    siliconflow_api_key_configured: boolean;
  };
  current: AdminConfigValues;
}

export interface AdminConfigPayload extends AdminConfigValues {
  jwt_secret: string;
  siliconflow_api_key: string;
}

export interface AdminConfigSaveResponse {
  ok: boolean;
  env_path: string;
  restart_required: boolean;
}

export interface RestartResponse {
  ok: boolean;
  restarting: boolean;
}

export interface RegistrationStatus {
  registration_enabled: boolean;
}

export interface AdminUser {
  id: string;
  username: string;
  display_name: string;
  role: "admin" | "editor" | "reader";
  created_at: string;
  workspaces: Workspace[];
}

export interface AdminUserPayload {
  username: string;
  display_name?: string;
  password?: string;
  role: "admin" | "editor" | "reader";
}

export interface DocumentItem {
  id: string;
  knowledge_base_id: string;
  title: string;
  content?: string;
  parent_id: string | null;
  is_folder: boolean;
  tags: string[];
  versions?: Array<{
    content: string;
    saved_at: string;
  }>;
  created_at?: string;
  updated_at: string;
}

export interface ZipImportResult {
  imported_files: number;
  created_folders: number;
  skipped_entries: number;
  skips: Array<{
    path: string;
    reason: string;
  }>;
}

export interface SearchResult {
  id: string;
  title: string;
  excerpt: string;
  score: number;
  updated_at: string;
}

export interface PublicUser {
  id: string;
  username: string;
  display_name: string;
  role: "admin" | "editor" | "reader";
  created_at: string;
}

export interface AuthResponse {
  token: string;
  user: PublicUser;
}

export interface Workspace {
  id: string;
  name: string;
  kind: "personal" | "team";
  owner_id?: string;
  team_id?: string;
}

export interface KnowledgeBase {
  id: string;
  workspace_id: string;
  name: string;
  description: string;
  created_at: string;
}

export interface Team {
  id: string;
  name: string;
  owner_id: string;
  workspace_id: string;
  created_at: string;
}

export interface TeamMember {
  user_id: string;
  username: string;
  display_name: string;
  role: "owner" | "admin" | "member";
  joined_at: string;
}

export interface TeamInvitation {
  id: string;
  team_id: string;
  team_name: string;
  inviter_id: string;
  invitee_id: string;
  status: string;
  created_at: string;
}

export interface InvitationResponse {
  invitation: TeamInvitation;
  invite_token: string;
}

export interface McpConfig {
  type: "http";
  url: string;
  headers: {
    Authorization: string;
  };
}

export interface McpToken {
  id: string;
  token_prefix: string;
  scope: "user" | "group" | "all";
  workspace_id: string | null;
  workspace_name: string | null;
  expires_at: string;
  revoked_at: string | null;
  created_at: string;
}
