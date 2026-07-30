import DOMPurify from "dompurify";
import MarkdownIt from "markdown-it";
import container from "markdown-it-container";
import taskLists from "markdown-it-task-lists";
import type Token from "markdown-it/lib/token.mjs";

const markdown = new MarkdownIt({
  breaks: true,
  html: false,
  linkify: true
});

function registerAlert(kind: "note" | "warning", defaultTitle: string) {
  markdown.use(container, kind, {
    validate(params: string) {
      const value = params.trim();
      return value === kind || value.startsWith(`${kind} `);
    },
    render(tokens: Token[], index: number) {
      if (tokens[index].nesting === -1) return "</aside>\n";
      const params = tokens[index].info.trim();
      const title = params.slice(kind.length).trim() || defaultTitle;
      return `<aside class="markdown-alert markdown-alert-${kind}" role="note"><p class="markdown-alert-title">${markdown.utils.escapeHtml(title)}</p>\n`;
    }
  });
}

registerAlert("note", "说明");
registerAlert("warning", "警告");
markdown.use(taskLists, { enabled: false });

export function renderMarkdown(source: string): string {
  return DOMPurify.sanitize(markdown.render(source));
}
