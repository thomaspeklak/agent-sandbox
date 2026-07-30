import { readFile, mkdir, rm, writeFile, copyFile } from "node:fs/promises";
import { dirname, join, normalize, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { Marked } from "marked";

const toolsDir = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(toolsDir, "..");
const siteDir = join(rootDir, "site");
const outputDir = join(siteDir, "docs");
const outputAssetsDir = join(outputDir, "assets");
const publicBaseUrl = "https://thomaspeklak.github.io/agent-sandbox";

const groups = [
  { id: "start", label: "Start here" },
  { id: "operate", label: "Operate" },
  { id: "integrate", label: "Integrate" },
  { id: "understand", label: "Understand" },
  { id: "project", label: "Project" },
];

const documents = [
  {
    slug: "overview",
    source: "README.md",
    title: "Overview & quick start",
    navTitle: "Overview",
    group: "start",
    description:
      "Install Agent Sandbox, prepare your environment, and launch your first isolated coding agent.",
    keywords: ["install", "quick start", "requirements", "first run", "agents"],
  },
  {
    slug: "commands",
    source: "docs/COMMANDS.md",
    title: "Commands & runtime behavior",
    navTitle: "Commands",
    group: "operate",
    description:
      "Understand every command, run flag, side effect, and runtime service Agent Sandbox can activate.",
    keywords: ["cli", "flags", "run", "setup", "doctor", "install", "aliases"],
  },
  {
    slug: "configuration",
    source: "docs/CONFIG.md",
    title: "Configuration reference",
    navTitle: "Configuration",
    group: "operate",
    description:
      "Configure mounts, secrets, browser integration, clipboard access, host UI, and update behavior.",
    keywords: ["config", "toml", "mount", "secret", "browser", "clipboard"],
  },
  {
    slug: "onepassword",
    source: "docs/ONEPASSWORD.md",
    title: "1Password Secure Note sets",
    navTitle: "1Password Secure Notes",
    group: "operate",
    description:
      "Inject a CLI-selected 1Password Secure Note into only the final agent process tree.",
    keywords: ["1password", "op", "secure note", "secret", "environment", "postgres"],
  },
  {
    slug: "troubleshooting",
    source: "docs/TROUBLESHOOTING.md",
    title: "Troubleshooting",
    navTitle: "Troubleshooting",
    group: "operate",
    description:
      "Diagnose common image, mount, terminal, SSH, browser, clipboard, and host-service problems.",
    keywords: ["error", "fix", "missing", "debug", "problem", "doctor"],
  },
  {
    slug: "glimpse",
    source: "docs/GLIMPSE.md",
    title: "Glimpse host UI",
    navTitle: "Glimpse host UI",
    group: "integrate",
    description:
      "Open host-owned Glimpse windows from sandboxed code without exposing a browser or compositor.",
    keywords: ["glimpse", "host ui", "window", "webview", "prompt"],
  },
  {
    slug: "psp-mode",
    source: "docs/PSP_MODE.md",
    title: "PSP container mode",
    navTitle: "PSP mode",
    group: "integrate",
    description:
      "Give Docker and Testcontainers clients policy-gated access to container operations.",
    keywords: ["psp", "podman", "docker", "testcontainers", "containers"],
  },
  {
    slug: "architecture",
    source: "docs/ARCHITECTURE.md",
    title: "Architecture overview",
    navTitle: "Architecture",
    group: "understand",
    description:
      "Follow the pipeline from CLI parsing and validated configuration to a rootless Podman launch plan.",
    keywords: ["architecture", "launch plan", "modules", "execution"],
  },
  {
    slug: "host-ui-bridge",
    source: "docs/GLIMPSE_HOST_UI_BRIDGE.md",
    title: "Glimpse host UI bridge",
    navTitle: "Host UI bridge",
    group: "understand",
    status: "RFC",
    statusTone: "cyan",
    description:
      "The protocol, lifecycle, security boundaries, and ownership model behind sandbox-safe host windows.",
    keywords: ["rfc", "protocol", "socket", "glimpse", "host ui", "security"],
  },
  {
    slug: "app-origin-relay",
    source: "docs/GLIMPSE_SANDBOX_APP_ORIGIN_RELAY.md",
    title: "Sandbox app origin relay",
    navTitle: "App origin relay",
    group: "understand",
    description:
      "How host-owned webviews reach temporary HTTP applications running on sandbox-local ports.",
    keywords: ["relay", "localhost", "webview", "http", "origin", "socket"],
  },
  {
    slug: "config-editor-research",
    source: "docs/CONFIG_EDITOR_TOML_RESEARCH.md",
    title: "Config editor research",
    navTitle: "Config editor research",
    group: "understand",
    status: "Research",
    statusTone: "sand",
    description:
      "Design research for comment-preserving TOML editing, layered configuration, backups, and TUI structure.",
    keywords: ["research", "toml", "config editor", "ratatui", "backup"],
  },
  {
    slug: "contributing",
    source: "CONTRIBUTING.md",
    title: "Contributing",
    navTitle: "Contributing",
    group: "project",
    description:
      "Set up a development environment, run quality checks, and prepare changes for review.",
    keywords: ["contribute", "development", "tests", "pull request", "rust"],
  },
  {
    slug: "changelog",
    source: "CHANGELOG.md",
    title: "Changelog",
    navTitle: "Changelog",
    group: "project",
    description: "Follow Agent Sandbox releases, features, fixes, and maintenance changes.",
    keywords: ["release", "version", "changes", "history"],
  },
];

const sourceToDocument = new Map(
  documents.map((document) => [normalize(resolve(rootDir, document.source)), document]),
);

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function escapeAttribute(value) {
  return escapeHtml(value).replaceAll("`", "&#096;");
}

function cleanGeneratedHtml(value) {
  return value.replace(/^[\t ]+$/gm, "");
}

function stripHtml(value) {
  return String(value)
    .replace(/<[^>]*>/g, " ")
    .replace(/&(?:#\d+|#x[\da-f]+|\w+);/gi, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function plainMarkdown(value) {
  return String(value)
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]+\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/<[^>]+>/g, " ")
    .replace(/[*_~>#|]/g, " ")
    .replace(/^\s*(?:[-+]|\d+\.)\s+/gm, "")
    .replace(/\s+/g, " ")
    .trim();
}

class Slugger {
  constructor() {
    this.seen = new Map();
  }

  slug(value) {
    const base =
      plainMarkdown(value)
        .normalize("NFKD")
        .replace(/[\u0300-\u036f]/g, "")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "") || "section";
    const count = this.seen.get(base) ?? 0;
    this.seen.set(base, count + 1);
    return count === 0 ? base : `${base}-${count + 1}`;
  }
}

function rewriteHref(href, sourcePath) {
  if (!href || href.startsWith("#")) return href;
  if (/^(?:https?:|mailto:|tel:)/i.test(href)) return href;

  const [pathPart, fragment = ""] = href.split("#", 2);
  if (!pathPart) return `#${fragment}`;

  const target = normalize(resolve(dirname(sourcePath), pathPart));
  const targetDocument = sourceToDocument.get(target);
  if (targetDocument) {
    return `${targetDocument.slug}.html${fragment ? `#${fragment}` : ""}`;
  }

  if (target === resolve(rootDir, "agent-sandbox-logo.webp")) {
    return `../assets/agent-sandbox-logo.webp${fragment ? `#${fragment}` : ""}`;
  }

  return href;
}

function assignHeadingIds(tokens) {
  const slugger = new Slugger();
  const headings = [];

  function visit(list) {
    for (const token of list ?? []) {
      if (token.type === "heading") {
        const text = plainMarkdown(token.text);
        token.docId = slugger.slug(text);
        headings.push({ depth: token.depth, text, id: token.docId });
      }
      if (Array.isArray(token.tokens)) visit(token.tokens);
      if (Array.isArray(token.items)) visit(token.items);
    }
  }

  visit(tokens);
  return headings;
}

function renderMarkdown(markdown, document) {
  const sourcePath = resolve(rootDir, document.source);
  const marked = new Marked({ gfm: true, breaks: false });
  const tokens = marked.lexer(markdown);
  const headings = assignHeadingIds(tokens);
  const firstTitleIndex = tokens.findIndex(
    (token) => token.type === "heading" && token.depth === 1,
  );
  if (firstTitleIndex !== -1) tokens.splice(firstTitleIndex, 1);

  const renderer = {
    heading(token) {
      const content = this.parser.parseInline(token.tokens);
      const label = plainMarkdown(token.text);
      return `<h${token.depth} id="${escapeAttribute(token.docId)}">${content}<a class="heading-anchor" href="#${escapeAttribute(token.docId)}" aria-label="Link to ${escapeAttribute(label)}">#</a></h${token.depth}>`;
    },
    code(token) {
      const language = (token.lang ?? "").trim().split(/\s+/)[0];
      const label = language || "code";
      return `<div class="code-block"><div class="code-toolbar"><span>${escapeHtml(label)}</span><button type="button" class="code-copy" data-copy-code aria-label="Copy ${escapeAttribute(label)} code"><svg aria-hidden="true" viewBox="0 0 20 20"><rect x="7" y="7" width="9" height="9" rx="2"/><path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2"/></svg><span>Copy</span></button></div><pre><code${language ? ` class="language-${escapeAttribute(language)}"` : ""}>${escapeHtml(token.text)}</code></pre></div>`;
    },
    link(token) {
      const href = rewriteHref(token.href, sourcePath);
      const external = /^(?:https?:)/i.test(href);
      const title = token.title ? ` title="${escapeAttribute(token.title)}"` : "";
      const attrs = external ? ' target="_blank" rel="noreferrer"' : "";
      return `<a href="${escapeAttribute(href)}"${title}${attrs}>${this.parser.parseInline(token.tokens)}${external ? '<span class="external-mark" aria-hidden="true">↗</span>' : ""}</a>`;
    },
    image(token) {
      const src = rewriteHref(token.href, sourcePath);
      const title = token.title ? ` title="${escapeAttribute(token.title)}"` : "";
      return `<img src="${escapeAttribute(src)}" alt="${escapeAttribute(token.text)}"${title} loading="lazy">`;
    },
  };

  marked.use({ renderer });
  return {
    html: marked.parser(tokens),
    headings: headings.filter((heading) => heading.depth > 1),
  };
}

function buildSearchEntries(document, markdown, headings) {
  const searchableHeadings = headings.filter((heading) => heading.depth === 2 || heading.depth === 3);
  const headingQueue = [...searchableHeadings];
  const entries = [];
  let active = {
    heading: document.title,
    id: "",
    lines: [],
  };

  function flush() {
    const text = plainMarkdown(active.lines.join("\n")).slice(0, 4000);
    if (!text && active.id) return;
    entries.push({
      page: document.title,
      heading: active.heading,
      group: groups.find((group) => group.id === document.group)?.label ?? "",
      status: document.status ?? "",
      url: `${document.slug}.html${active.id ? `#${active.id}` : ""}`,
      text,
      keywords: document.keywords.join(" "),
    });
  }

  let inFence = false;
  for (const line of markdown.split(/\r?\n/)) {
    if (line.startsWith("```")) {
      inFence = !inFence;
      active.lines.push(line.replace(/^```/, ""));
      continue;
    }

    if (!inFence) {
      const match = /^(#{2,3})\s+(.+?)\s*$/.exec(line);
      if (match) {
        flush();
        const heading = headingQueue.shift();
        active = {
          heading: plainMarkdown(match[2]),
          id: heading?.id ?? "",
          lines: [],
        };
        continue;
      }
      if (/^#\s+/.test(line)) continue;
    }

    active.lines.push(line);
  }
  flush();
  return entries;
}

function renderBrand() {
  return `<a class="docs-brand" href="../index.html" aria-label="Agent Sandbox home">
    <span class="docs-brand-mark" aria-hidden="true"><span></span></span>
    <span>agent<span>/</span>sandbox</span>
  </a>`;
}

function renderTopbar() {
  return `<header class="docs-topbar">
    <button class="mobile-menu-button" type="button" data-open-nav aria-label="Open documentation navigation" aria-expanded="false">
      <svg aria-hidden="true" viewBox="0 0 20 20"><path d="M3 5h14M3 10h14M3 15h14"/></svg>
    </button>
    ${renderBrand()}
    <span class="docs-divider" aria-hidden="true"></span>
    <a class="docs-label" href="./index.html">Documentation</a>
    <button class="search-trigger" type="button" data-open-search>
      <svg aria-hidden="true" viewBox="0 0 20 20"><circle cx="9" cy="9" r="5.5"/><path d="m13 13 4 4"/></svg>
      <span>Search documentation</span>
      <kbd><span class="command-key">⌘</span><span class="control-key">Ctrl</span> K</kbd>
    </button>
    <nav class="docs-top-links" aria-label="Documentation utilities">
      <a href="../index.html">Showcase</a>
      <a href="https://github.com/thomaspeklak/agent-sandbox" target="_blank" rel="noreferrer">GitHub <span aria-hidden="true">↗</span></a>
    </nav>
  </header>`;
}

function renderSidebar(activeSlug) {
  const groupMarkup = groups
    .map((group) => {
      const links = documents
        .filter((document) => document.group === group.id)
        .map(
          (document) => `<a href="./${document.slug}.html" class="sidebar-link${activeSlug === document.slug ? " is-active" : ""}"${activeSlug === document.slug ? ' aria-current="page"' : ""}>
            <span>${escapeHtml(document.navTitle)}</span>
            ${document.status ? `<small class="nav-status nav-status-${escapeAttribute(document.statusTone)}">${escapeHtml(document.status)}</small>` : ""}
          </a>`,
        )
        .join("");
      return `<div class="sidebar-group"><div class="sidebar-group-label">${escapeHtml(group.label)}</div>${links}</div>`;
    })
    .join("");

  return `<aside class="docs-sidebar" data-sidebar>
    <div class="sidebar-mobile-head">
      <span>Documentation</span>
      <button type="button" data-close-nav aria-label="Close documentation navigation">×</button>
    </div>
    <nav aria-label="Documentation pages">
      <a href="./index.html" class="sidebar-home${activeSlug === "home" ? " is-active" : ""}"${activeSlug === "home" ? ' aria-current="page"' : ""}>
        <svg aria-hidden="true" viewBox="0 0 20 20"><path d="m3 9 7-6 7 6v8H3Z"/><path d="M8 17v-5h4v5"/></svg>
        Documentation home
      </a>
      ${groupMarkup}
    </nav>
    <div class="sidebar-foot">
      <span><i></i> ${documents.length} source documents</span>
      <a href="https://github.com/thomaspeklak/agent-sandbox/tree/main/docs" target="_blank" rel="noreferrer">Edit on GitHub ↗</a>
    </div>
  </aside><button class="sidebar-scrim" type="button" data-close-nav aria-label="Close navigation"></button>`;
}

function renderSearchDialog() {
  return `<div class="search-layer" data-search-layer hidden>
    <button class="search-backdrop" type="button" data-close-search aria-label="Close search"></button>
    <section class="search-dialog" role="dialog" aria-modal="true" aria-labelledby="search-title">
      <h2 id="search-title" class="visually-hidden">Search documentation</h2>
      <div class="search-input-wrap">
        <svg aria-hidden="true" viewBox="0 0 20 20"><circle cx="9" cy="9" r="5.5"/><path d="m13 13 4 4"/></svg>
        <input type="search" placeholder="Search commands, configuration, errors…" autocomplete="off" spellcheck="false" data-search-input>
        <kbd>Esc</kbd>
      </div>
      <div class="search-state" data-search-state>
        <div class="search-empty-graphic" aria-hidden="true"><span></span><span></span><i></i></div>
        <strong>Search the complete Agent Sandbox manual</strong>
        <p>Try “lockdown”, “clipboard approval”, “mount”, or “OAuth callback”.</p>
      </div>
      <div class="search-results" data-search-results role="listbox" aria-label="Search results"></div>
      <div class="search-footer">
        <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
        <span><kbd>↵</kbd> open</span>
        <span>Section-level search</span>
      </div>
    </section>
  </div>`;
}

function renderToc(headings) {
  const items = headings
    .filter((heading) => heading.depth === 2 || heading.depth === 3)
    .map(
      (heading) =>
        `<a href="#${escapeAttribute(heading.id)}" class="toc-link toc-depth-${heading.depth}" data-toc-link="${escapeAttribute(heading.id)}">${escapeHtml(heading.text)}</a>`,
    )
    .join("");
  if (!items) return "";
  return `<aside class="page-toc" aria-label="On this page">
    <div class="toc-label">On this page</div>
    <nav>${items}</nav>
    <a class="toc-top" href="#doc-top"><span>↑</span> Back to top</a>
  </aside>`;
}

function statusMessage(document) {
  if (document.status === "Draft") {
    return "Draft architecture: this page describes a proposed direction, not guaranteed shipped behavior.";
  }
  if (document.status === "RFC") {
    return "Architecture RFC: this page defines the bridge contract and records follow-up decisions.";
  }
  if (document.status === "Research") {
    return "Research artifact: use this page for design rationale; use the configuration reference for current behavior.";
  }
  return "";
}

function renderPageFooter(index) {
  const previous = documents[index - 1];
  const next = documents[index + 1];
  return `<nav class="page-neighbors" aria-label="Adjacent documentation pages">
    ${
      previous
        ? `<a class="neighbor previous" href="./${previous.slug}.html"><span>← Previous</span><strong>${escapeHtml(previous.navTitle)}</strong></a>`
        : `<span></span>`
    }
    ${
      next
        ? `<a class="neighbor next" href="./${next.slug}.html"><span>Next →</span><strong>${escapeHtml(next.navTitle)}</strong></a>`
        : `<span></span>`
    }
  </nav>`;
}

function shell({ activeSlug, title, description, main, toc = "", pageClass = "" }) {
  const canonical =
    activeSlug === "home"
      ? "https://thomaspeklak.github.io/agent-sandbox/docs/"
      : `https://thomaspeklak.github.io/agent-sandbox/docs/${activeSlug}.html`;
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="description" content="${escapeAttribute(description)}">
    <meta name="theme-color" content="#0b0f10">
    <meta property="og:title" content="${escapeAttribute(title)} · Agent Sandbox Docs">
    <meta property="og:description" content="${escapeAttribute(description)}">
    <meta property="og:image" content="https://thomaspeklak.github.io/agent-sandbox/assets/agent-sandbox-logo.webp">
    <meta property="og:type" content="website">
    <meta property="og:url" content="${canonical}">
    <link rel="canonical" href="${canonical}">
    <link rel="icon" href="../assets/favicon.svg" type="image/svg+xml">
    <link rel="stylesheet" href="./assets/docs.css">
    <script src="./assets/docs.js" defer></script>
    <title>${escapeHtml(title)} · Agent Sandbox Docs</title>
  </head>
  <body class="docs-page ${escapeAttribute(pageClass)}" data-doc-page="${escapeAttribute(activeSlug)}">
    <a class="skip-link" href="#docs-main">Skip to documentation</a>
    <div class="reading-progress" aria-hidden="true"><span data-reading-progress></span></div>
    ${renderTopbar()}
    ${renderSidebar(activeSlug)}
    <div class="docs-content-shell">
      <div class="docs-content-grid${toc ? "" : " no-toc"}">
        ${main}
        ${toc}
      </div>
      <footer class="docs-footer">
        <span>Agent Sandbox documentation</span>
        <span>Generated from version-controlled Markdown</span>
        <a href="https://github.com/thomaspeklak/agent-sandbox" target="_blank" rel="noreferrer">MIT licensed · GitHub ↗</a>
      </footer>
    </div>
    ${renderSearchDialog()}
  </body>
</html>`;
}

function renderDocumentPage(document, markdown, rendered, index) {
  const group = groups.find((item) => item.id === document.group);
  const wordCount = plainMarkdown(markdown).split(/\s+/).filter(Boolean).length;
  const readingMinutes = Math.max(1, Math.round(wordCount / 220));
  const status = document.status
    ? `<span class="doc-status doc-status-${escapeAttribute(document.statusTone)}">${escapeHtml(document.status)}</span>`
    : `<span class="doc-status doc-status-stable">Reference</span>`;
  const callout = statusMessage(document);
  const sourceUrl = `https://github.com/thomaspeklak/agent-sandbox/blob/main/${document.source}`;

  const main = `<main id="docs-main" class="doc-main" tabindex="-1">
    <article id="doc-top">
      <div class="doc-breadcrumb"><a href="./index.html">Docs</a><span>/</span><span>${escapeHtml(group.label)}</span></div>
      <header class="doc-header">
        <div class="doc-meta">${status}<span>${readingMinutes} min read</span><span>${escapeHtml(document.source)}</span></div>
        <h1>${escapeHtml(document.title)}</h1>
        <p>${escapeHtml(document.description)}</p>
        <a class="source-link" href="${sourceUrl}" target="_blank" rel="noreferrer">
          View source Markdown
          <span aria-hidden="true">↗</span>
        </a>
      </header>
      ${callout ? `<aside class="status-callout status-callout-${escapeAttribute(document.statusTone)}"><strong>${escapeHtml(document.status)}</strong><p>${escapeHtml(callout)}</p></aside>` : ""}
      <div class="markdown-body">${rendered.html}</div>
      <div class="doc-feedback">
        <div><span>Found what you needed?</span><p>Improve this page where the source lives.</p></div>
        <a href="${sourceUrl}" target="_blank" rel="noreferrer">Suggest an edit ↗</a>
      </div>
      ${renderPageFooter(index)}
    </article>
  </main>`;

  return shell({
    activeSlug: document.slug,
    title: document.title,
    description: document.description,
    main,
    toc: renderToc(rendered.headings),
  });
}

function taskLink(documentSlug, hash, label, description, icon) {
  return `<a class="task-link" href="./${documentSlug}.html${hash}">
    <span class="task-icon" aria-hidden="true">${icon}</span>
    <span><strong>${escapeHtml(label)}</strong><small>${escapeHtml(description)}</small></span>
    <i aria-hidden="true">→</i>
  </a>`;
}

function renderDocsHome(searchEntryCount, latestRelease) {
  const main = `<main id="docs-main" class="docs-home" tabindex="-1">
    <section class="docs-home-hero">
      <div class="home-hero-copy">
        <div class="home-kicker"><span></span> Agent Sandbox manual</div>
        <h1>Build freely.<br><em>Know the boundary.</em></h1>
        <p>Everything you need to install, operate, configure, troubleshoot, and extend Agent Sandbox—generated directly from the repository documentation.</p>
        <button class="home-search-button" type="button" data-open-search>
          <svg aria-hidden="true" viewBox="0 0 20 20"><circle cx="9" cy="9" r="5.5"/><path d="m13 13 4 4"/></svg>
          Search ${searchEntryCount} documented sections
          <kbd><span class="command-key">⌘</span><span class="control-key">Ctrl</span> K</kbd>
        </button>
      </div>
      <div class="home-hero-object" aria-hidden="true">
        <div class="home-boundary boundary-back"></div>
        <div class="home-boundary boundary-front">
          <div class="home-terminal">
            <span>❯</span><code>ags --agent codex</code><i></i>
          </div>
          <div class="home-sand"></div>
          <span class="home-tool tool-one">{ }</span>
          <span class="home-tool tool-two">_</span>
          <span class="home-tool tool-three">git</span>
        </div>
        <div class="home-orbit"><span></span></div>
      </div>
    </section>

    <section class="home-section path-section" aria-labelledby="paths-title">
      <div class="home-section-heading"><span>Choose a path</span><h2 id="paths-title">Start with the work in front of you.</h2></div>
      <div class="path-grid">
        <a class="path-card path-card-featured" href="./overview.html">
          <span class="path-number">01</span>
          <div class="path-art path-art-start" aria-hidden="true"><i></i><i></i><i></i></div>
          <div><small>New to AGS</small><h3>Get running</h3><p>Requirements, installation, first-time setup, and your first agent launch.</p></div>
          <strong>Open the overview <span>→</span></strong>
        </a>
        <a class="path-card" href="./configuration.html">
          <span class="path-number">02</span>
          <div class="path-art path-art-config" aria-hidden="true"><i></i><i></i><i></i></div>
          <div><small>Daily operation</small><h3>Shape the environment</h3><p>Mounts, secrets, repo overlays, browser access, and clipboard controls.</p></div>
          <strong>Configure AGS <span>→</span></strong>
        </a>
        <a class="path-card" href="./architecture.html">
          <span class="path-number">03</span>
          <div class="path-art path-art-understand" aria-hidden="true"><i></i><i></i><i></i></div>
          <div><small>Going deeper</small><h3>Understand the system</h3><p>Launch plans, host bridges, socket protocols, relays, and security boundaries.</p></div>
          <strong>Explore architecture <span>→</span></strong>
        </a>
      </div>
    </section>

    <section class="home-section task-section" aria-labelledby="tasks-title">
      <div class="home-section-heading"><span>Common tasks</span><h2 id="tasks-title">Go straight to the answer.</h2></div>
      <div class="task-grid">
        ${taskLink("overview", "#first-time-setup", "Set up AGS", "Keys, config, image, and verification", "01")}
        ${taskLink("commands", "#run-mode-agent", "Launch an agent", "Modes, passthrough flags, and runtime flow", "02")}
        ${taskLink("configuration", "#mount", "Configure mounts", "Read-only, read-write, optional, and per-repo", "03")}
        ${taskLink("commands", "#notes", "Harden a foreign repo", "What lockdown keeps and what it cuts", "04")}
        ${taskLink("troubleshooting", "#pi-ctrl-v-image-paste-or-copy-clipboard-actions-fail", "Fix clipboard paste", "Approval windows, shims, and diagnostics", "05")}
        ${taskLink("psp-mode", "#quick-start", "Run Testcontainers", "Policy-gated access through PSP", "06")}
      </div>
    </section>

    <section class="home-section library-section" aria-labelledby="library-title">
      <div class="home-section-heading library-heading">
        <div><span>The complete library</span><h2 id="library-title">${documents.length} source documents. One coherent manual.</h2></div>
        <div class="library-stats"><span><strong>${searchEntryCount}</strong> searchable sections</span><span><strong>${escapeHtml(latestRelease)}</strong> latest release</span></div>
      </div>
      <div class="library-groups">
        ${groups
          .map(
            (group) => `<section class="library-group">
              <h3>${escapeHtml(group.label)}</h3>
              <div>${documents
                .filter((document) => document.group === group.id)
                .map(
                  (document) => `<a href="./${document.slug}.html"><span><strong>${escapeHtml(document.navTitle)}</strong><small>${escapeHtml(document.description)}</small></span>${document.status ? `<b class="nav-status nav-status-${escapeAttribute(document.statusTone)}">${escapeHtml(document.status)}</b>` : '<i>→</i>'}</a>`,
                )
                .join("")}</div>
            </section>`,
          )
          .join("")}
      </div>
    </section>

    <section class="home-source-banner">
      <div><span>Docs that move with the code.</span><p>Every page is rebuilt from repository Markdown on each documentation change.</p></div>
      <a href="https://github.com/thomaspeklak/agent-sandbox/tree/main/docs" target="_blank" rel="noreferrer">Browse the source <span>↗</span></a>
    </section>
  </main>`;

  return shell({
    activeSlug: "home",
    title: "Documentation",
    description:
      "The complete Agent Sandbox manual: installation, commands, configuration, troubleshooting, integrations, and architecture.",
    main,
    pageClass: "docs-home-page",
  });
}

async function main() {
  await rm(outputDir, { recursive: true, force: true });
  await mkdir(outputAssetsDir, { recursive: true });

  const searchIndex = [];
  let latestRelease = "current";

  for (const [index, document] of documents.entries()) {
    const sourcePath = resolve(rootDir, document.source);
    const markdown = await readFile(sourcePath, "utf8");
    if (document.slug === "changelog") {
      latestRelease = /\n## \[?([^\]\s]+)\]?/.exec(markdown)?.[1] ?? "current";
    }
    const rendered = renderMarkdown(markdown, document);
    searchIndex.push(...buildSearchEntries(document, markdown, rendered.headings));
    const html = renderDocumentPage(document, markdown, rendered, index);
    await writeFile(
      join(outputDir, `${document.slug}.html`),
      cleanGeneratedHtml(html),
    );
  }

  await writeFile(
    join(outputDir, "search-index.json"),
    `${JSON.stringify(searchIndex)}\n`,
  );
  await writeFile(
    join(outputDir, "index.html"),
    cleanGeneratedHtml(renderDocsHome(searchIndex.length, latestRelease)),
  );
  const sitemapUrls = [
    `${publicBaseUrl}/`,
    `${publicBaseUrl}/docs/`,
    ...documents.map(
      (document) => `${publicBaseUrl}/docs/${document.slug}.html`,
    ),
  ];
  await writeFile(
    join(siteDir, "sitemap.xml"),
    `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${sitemapUrls.map((url) => `  <url><loc>${url}</loc></url>`).join("\n")}\n</urlset>\n`,
  );
  await copyFile(join(toolsDir, "assets", "docs.css"), join(outputAssetsDir, "docs.css"));
  await copyFile(join(toolsDir, "assets", "docs.js"), join(outputAssetsDir, "docs.js"));

  console.log(
    `Generated ${documents.length + 1} pages and ${searchIndex.length} search entries in ${relative(rootDir, outputDir).split(sep).join("/")}`,
  );
}

await main();
