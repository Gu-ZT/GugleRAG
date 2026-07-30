import DOMPurify from "dompurify";
import MarkdownIt from "markdown-it";

const markdown = new MarkdownIt({
  breaks: true,
  html: false,
  linkify: true
});

export function renderMarkdown(source: string): string {
  return DOMPurify.sanitize(markdown.render(source));
}
