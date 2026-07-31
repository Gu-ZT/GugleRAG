import DOMPurify from "dompurify";
import MarkdownIt from "markdown-it";
import container from "markdown-it-container";
import taskLists from "markdown-it-task-lists";
import Token from "markdown-it/lib/token.mjs";

type AlertKind = "note" | "tip" | "important" | "warning" | "caution" | "danger";

type AlertDefinition = {
  kind: AlertKind;
  title: string;
};

type FootnoteState = {
  definitions: Record<string, string>;
  numbers: Record<string, number>;
  referenceCounts: Record<string, number>;
  order: string[];
};

type MarkdownEnv = {
  footnotes?: FootnoteState;
};

const alertDefinitions: AlertDefinition[] = [
  { kind: "note", title: "说明" },
  { kind: "tip", title: "提示" },
  { kind: "important", title: "重要" },
  { kind: "warning", title: "警告" },
  { kind: "caution", title: "注意" },
  { kind: "danger", title: "危险" }
];

const alertByKind = new Map(alertDefinitions.map((definition) => [definition.kind, definition]));
const alertPattern = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION|DANGER)\][ \t]*(?:\n|$)/i;
const footnoteDefinitionPattern = /^\[\^([^\]\r\n]+)\]:[ \t]*(.*)$/;

const markdown = new MarkdownIt({
  breaks: true,
  html: true,
  linkify: true
});

/* 允许 GitHub 风格的内联 HTML（details/summary、kbd、mark、img、表格等），
   但统一在渲染出口处做 XSS 过滤。 */
if (typeof DOMPurify.addHook === "function") {
  DOMPurify.addHook("afterSanitizeElements", (node) => {
    // 任务列表之外的表单控件一律移除，防止在预览里伪造登录框钓鱼
    if (node instanceof HTMLInputElement && node.type !== "checkbox") {
      node.remove();
      return;
    }
    // 链接强制新窗口 + 防 opener 劫持
    if (node instanceof HTMLAnchorElement && node.getAttribute("href")) {
      node.setAttribute("target", "_blank");
      node.setAttribute("rel", "noopener noreferrer");
    }
  });
}

function hasOwnValue(source: Record<string, string>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(source, key);
}

function registerAlert({ kind, title: defaultTitle }: AlertDefinition) {
  markdown.use(container, kind, {
    validate(params: string) {
      return params.trim().split(/\s+/, 1)[0]?.toLowerCase() === kind;
    },
    render(tokens: Token[], index: number) {
      if (tokens[index].nesting === -1) return "</aside>\n";
      const params = tokens[index].info.trim();
      const [, customTitle = ""] = params.match(/^\S+\s*(.*)$/) ?? [];
      const title = customTitle.trim() || defaultTitle;
      return `<aside class="markdown-alert markdown-alert-${kind}" role="note"><p class="markdown-alert-title">${markdown.utils.escapeHtml(title)}</p>\n`;
    }
  });
}

function createTextToken(content: string): Token {
  const token = new Token("text", "", 0);
  token.content = content;
  return token;
}

function createAlertTitleTokens(title: string, level: number): Token[] {
  const titleOpen = new Token("paragraph_open", "p", 1);
  titleOpen.attrSet("class", "markdown-alert-title");
  titleOpen.level = level;

  const titleInline = new Token("inline", "", 0);
  titleInline.content = title;
  titleInline.children = [createTextToken(title)];
  titleInline.level = level + 1;

  const titleClose = new Token("paragraph_close", "p", -1);
  titleClose.level = level;

  return [titleOpen, titleInline, titleClose];
}

function findClosingBlockquote(tokens: Token[], openIndex: number): number {
  let depth = 0;

  for (let index = openIndex; index < tokens.length; index += 1) {
    if (tokens[index].type === "blockquote_open") depth += 1;
    if (tokens[index].type === "blockquote_close") depth -= 1;
    if (depth === 0) return index;
  }

  return -1;
}

function registerGitHubAlertBlockquotes() {
  markdown.core.ruler.after("block", "github_alert_blockquotes", (state) => {
    for (let index = 0; index < state.tokens.length; index += 1) {
      const openToken = state.tokens[index];
      if (openToken.type !== "blockquote_open") continue;

      const paragraphOpenIndex = index + 1;
      const inlineIndex = index + 2;
      const paragraphCloseIndex = index + 3;
      const paragraphOpen = state.tokens[paragraphOpenIndex];
      const inlineToken = state.tokens[inlineIndex];
      const paragraphClose = state.tokens[paragraphCloseIndex];
      if (
        paragraphOpen?.type !== "paragraph_open" ||
        inlineToken?.type !== "inline" ||
        paragraphClose?.type !== "paragraph_close"
      ) {
        continue;
      }

      const alertMatch = inlineToken.content.match(alertPattern);
      if (!alertMatch) continue;

      const definition = alertByKind.get(alertMatch[1].toLowerCase() as AlertKind);
      const closeIndex = findClosingBlockquote(state.tokens, index);
      if (!definition || closeIndex === -1) continue;

      const content = inlineToken.content.slice(alertMatch[0].length);
      openToken.type = "markdown_alert_open";
      openToken.tag = "aside";
      openToken.attrSet("class", `markdown-alert markdown-alert-${definition.kind}`);
      openToken.attrSet("role", "note");
      state.tokens[closeIndex].type = "markdown_alert_close";
      state.tokens[closeIndex].tag = "aside";

      if (content.trim().length === 0) {
        state.tokens.splice(paragraphOpenIndex, 3);
      } else {
        const children: Token[] = [];
        inlineToken.content = content;
        inlineToken.children = children;
        state.md.inline.parse(content, state.md, state.env, children);
      }

      state.tokens.splice(index + 1, 0, ...createAlertTitleTokens(definition.title, openToken.level + 1));
      index += 3;
    }
  });
}

function extractFootnotes(source: string): { body: string; definitions: Record<string, string> } {
  const normalized = source.replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n");
  const body: string[] = [];
  const definitions: Record<string, string> = Object.create(null) as Record<string, string>;

  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(footnoteDefinitionPattern);
    if (!match) {
      body.push(lines[index]);
      continue;
    }

    const label = match[1].trim();
    const contentLines = [match[2]];
    index += 1;

    while (index < lines.length) {
      const line = lines[index];
      const continuation = line.match(/^(?: {4}|\t)(.*)$/);
      if (continuation) {
        contentLines.push(continuation[1]);
        index += 1;
        continue;
      }

      if (line.trim() === "" && index + 1 < lines.length && /^(?: {4}|\t)/.test(lines[index + 1])) {
        contentLines.push("");
        index += 1;
        continue;
      }

      break;
    }

    index -= 1;
    if (label.length > 0) definitions[label] = contentLines.join("\n").trim();
  }

  return { body: body.join("\n"), definitions };
}

function registerFootnoteRefs() {
  markdown.inline.ruler.before("emphasis", "github_footnote_ref", (state, silent) => {
    if (state.src.charCodeAt(state.pos) !== 0x5b || state.src.charCodeAt(state.pos + 1) !== 0x5e) return false;

    const labelEnd = state.src.indexOf("]", state.pos + 2);
    if (labelEnd === -1) return false;

    const label = state.src.slice(state.pos + 2, labelEnd).trim();
    const footnotes = (state.env as MarkdownEnv).footnotes;
    if (!label || !footnotes || !hasOwnValue(footnotes.definitions, label)) return false;

    if (!silent) {
      const token = state.push("github_footnote_ref", "sup", 0);
      token.meta = { label };
    }

    state.pos = labelEnd + 1;
    return true;
  });

  markdown.renderer.rules.github_footnote_ref = (tokens, index, _options, env) => {
    const footnotes = (env as MarkdownEnv).footnotes;
    const label = tokens[index].meta?.label as string | undefined;
    if (!footnotes || !label) return "";

    if (footnotes.numbers[label] === undefined) {
      footnotes.numbers[label] = footnotes.order.length + 1;
      footnotes.order.push(label);
    }

    const number = footnotes.numbers[label];
    const referenceCount = (footnotes.referenceCounts[label] ?? 0) + 1;
    footnotes.referenceCounts[label] = referenceCount;
    const referenceId = referenceCount === 1 ? `fnref-${number}` : `fnref-${number}-${referenceCount}`;

    return `<sup class="footnote-ref" id="${referenceId}"><a href="#fn-${number}">${number}</a></sup>`;
  };
}

function renderFootnotes(footnotes: FootnoteState): string {
  if (footnotes.order.length === 0) return "";

  const items = footnotes.order.map((label) => {
    const number = footnotes.numbers[label];
    const content = markdown.renderInline(footnotes.definitions[label]);
    return `<li id="fn-${number}"><p>${content} <a class="footnote-backref" href="#fnref-${number}" aria-label="返回引用">返回</a></p></li>`;
  });

  return `<section class="footnotes" aria-label="Footnotes"><hr><ol>${items.join("\n")}</ol></section>`;
}

alertDefinitions.forEach(registerAlert);
registerGitHubAlertBlockquotes();
registerFootnoteRefs();
markdown.use(taskLists, { enabled: false });

export function renderMarkdown(source: string): string {
  const { body, definitions } = extractFootnotes(source);
  const footnotes: FootnoteState = {
    definitions,
    numbers: Object.create(null) as Record<string, number>,
    referenceCounts: Object.create(null) as Record<string, number>,
    order: []
  };
  const env: MarkdownEnv = {
    footnotes
  };
  const html = `${markdown.render(body, env)}${renderFootnotes(footnotes)}`;
  if (typeof DOMPurify.sanitize !== "function") return html; // 非浏览器环境（测试/SSR）不过滤
  return DOMPurify.sanitize(html, {
    ADD_ATTR: ["target", "align", "open"],
    FORBID_TAGS: ["style", "form", "button", "select", "textarea", "iframe", "object", "embed"],
    FORBID_ATTR: ["style"]
  });
}
