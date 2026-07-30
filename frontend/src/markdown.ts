import DOMPurify from "dompurify";
import { marked } from "marked";

export function renderMarkdown(source: string): string {
  const html = marked.parse(source, {
    async: false,
    breaks: true,
    gfm: true
  });

  return DOMPurify.sanitize(html);
}
