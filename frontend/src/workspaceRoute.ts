export interface WorkspaceRoute {
  workspaceId?: string;
  knowledgeBaseId?: string;
  documentId?: string;
}

export type WorkspaceHistoryMode = "push" | "replace" | "none";

export function parseWorkspaceRoute(pathname: string): WorkspaceRoute {
  const segments = pathname.split("/").filter(Boolean).map(decodeSegment);
  if (segments.some((segment) => segment === null)) return {};

  const values = segments as string[];
  if (values[0] !== "workspaces" || !values[1]) return {};
  if (values.length === 2) return { workspaceId: values[1] };
  if (values[2] !== "knowledge-bases" || !values[3]) return {};
  if (values.length === 4) {
    return { workspaceId: values[1], knowledgeBaseId: values[3] };
  }
  if (values.length !== 6 || values[4] !== "documents" || !values[5]) return {};
  return {
    workspaceId: values[1],
    knowledgeBaseId: values[3],
    documentId: values[5]
  };
}

export function buildWorkspaceRoute(route: WorkspaceRoute): string {
  if (!route.workspaceId) return "/";
  const workspacePath = `/workspaces/${encodeURIComponent(route.workspaceId)}`;
  if (!route.knowledgeBaseId) return workspacePath;
  const knowledgeBasePath = `${workspacePath}/knowledge-bases/${encodeURIComponent(route.knowledgeBaseId)}`;
  if (!route.documentId) return knowledgeBasePath;
  return `${knowledgeBasePath}/documents/${encodeURIComponent(route.documentId)}`;
}

function decodeSegment(segment: string): string | null {
  try {
    return decodeURIComponent(segment);
  } catch {
    return null;
  }
}
