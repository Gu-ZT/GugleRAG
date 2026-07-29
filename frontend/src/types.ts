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
  mcp_enabled: boolean;
  mcp_auth_required: boolean;
}

export interface DocumentItem {
  id: string;
  title: string;
  content?: string;
  tags: string[];
  updated_at: string;
}
