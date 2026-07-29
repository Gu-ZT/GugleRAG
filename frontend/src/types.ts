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
    mcp_enabled: boolean;
    mcp_auth_required: boolean;
    embedding_provider: string;
    embedding_model: string;
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
  embedding_provider: "stub" | "local" | "siliconflow";
  embedding_model: string;
  siliconflow_url: string;
  siliconflow_api_key: string;
  reranker_enabled: boolean;
  reranker_provider: "local" | "siliconflow" | "custom_http";
  reranker_model: string;
  reranker_url: string;
  mcp_enabled: boolean;
  mcp_auth_required: boolean;
}

export interface DocumentItem {
  id: string;
  title: string;
  content?: string;
  tags: string[];
  versions?: Array<{
    content: string;
    saved_at: string;
  }>;
  created_at?: string;
  updated_at: string;
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
