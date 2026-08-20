import { access, readdir, readFile, stat } from "node:fs/promises";
import { execFile } from "node:child_process";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const toolsDir = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(toolsDir, "..");
const siteDir = join(rootDir, "site");
const docsDir = join(siteDir, "docs");
const execFileAsync = promisify(execFile);

const failures = [];

async function walk(directory, predicate) {
  const entries = await readdir(directory, { withFileTypes: true });
  const matches = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) matches.push(...(await walk(path, predicate)));
    else if (predicate(path)) matches.push(path);
  }
  return matches;
}

function idsIn(html) {
  return new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((match) => match[1]));
}

async function resolveSiteTarget(sourceFile, rawTarget) {
  const target = rawTarget.replaceAll("&amp;", "&");
  if (
    !target ||
    target.startsWith("#") ||
    /^(?:https?:|mailto:|tel:|data:|javascript:)/i.test(target)
  ) {
    return null;
  }
  if (target.startsWith("/")) {
    failures.push(
      `${relative(rootDir, sourceFile)} uses root-relative URL ${JSON.stringify(target)}`,
    );
    return null;
  }

  const [pathPart] = target.split(/[?#]/, 1);
  let path = resolve(dirname(sourceFile), decodeURI(pathPart));
  try {
    if ((await stat(path)).isDirectory()) path = join(path, "index.html");
  } catch {
    // The existence check below reports a useful error.
  }
  return path;
}

async function validateHtmlLinks(htmlFiles, htmlByPath) {
  for (const file of htmlFiles) {
    const html = htmlByPath.get(file);
    const targets = [
      ...html.matchAll(/\s(?:href|src)="([^"]+)"/g),
    ].map((match) => match[1]);

    for (const target of targets) {
      const [pathPart, fragment] = target.split("#", 2);
      const resolved = pathPart
        ? await resolveSiteTarget(file, target)
        : file;
      if (!resolved) continue;

      try {
        await access(resolved);
      } catch {
        failures.push(
          `${relative(rootDir, file)} references missing ${JSON.stringify(target)}`,
        );
        continue;
      }

      if (fragment && extname(resolved) === ".html") {
        const targetHtml = htmlByPath.get(resolved) ?? (await readFile(resolved, "utf8"));
        if (!idsIn(targetHtml).has(decodeURIComponent(fragment))) {
          failures.push(
            `${relative(rootDir, file)} references missing anchor ${JSON.stringify(target)}`,
          );
        }
      }
    }
  }
}

async function validateSourceCoverage(generatedPages) {
  const { stdout } = await execFileAsync("git", ["ls-files", "*.md"], {
    cwd: rootDir,
  });
  const publicSources = stdout
    .trim()
    .split(/\r?\n/)
    .filter(
      (path) =>
        ["README.md", "CHANGELOG.md", "CONTRIBUTING.md"].includes(path) ||
        /^docs\/[^/]+\.md$/.test(path),
    )
    .sort();
  const pages = await Promise.all(
    generatedPages.map(async (path) => ({
      path,
      html: await readFile(path, "utf8"),
    })),
  );

  for (const source of publicSources) {
    const owners = pages.filter(({ html }) => html.includes(`<span>${source}</span>`));
    if (owners.length !== 1) {
      failures.push(
        `${source} is represented by ${owners.length} generated documentation pages (expected 1)`,
      );
    }
  }

  if (generatedPages.length !== publicSources.length) {
    failures.push(
      `${generatedPages.length} generated documentation pages do not match ${publicSources.length} public Markdown sources`,
    );
  }
}

async function validateSearchIndex(htmlByPath) {
  const indexPath = join(docsDir, "search-index.json");
  const entries = JSON.parse(await readFile(indexPath, "utf8"));
  if (!Array.isArray(entries) || entries.length < 100) {
    failures.push("search-index.json does not contain the expected section-level entries");
    return;
  }

  for (const entry of entries) {
    if (!entry.page || !entry.heading || !entry.url) {
      failures.push(`Malformed search entry: ${JSON.stringify(entry)}`);
      continue;
    }
    const [pathPart, fragment] = entry.url.split("#", 2);
    const targetPath = resolve(docsDir, pathPart);
    const html = htmlByPath.get(targetPath);
    if (!html) {
      failures.push(`Search entry references missing page ${entry.url}`);
    } else if (fragment && !idsIn(html).has(decodeURIComponent(fragment))) {
      failures.push(`Search entry references missing anchor ${entry.url}`);
    }
  }

  console.log(`Validated ${entries.length} section-level search entries.`);
}

async function validateSitemap(generatedPages) {
  const sitemap = await readFile(join(siteDir, "sitemap.xml"), "utf8");
  const expectedUrls = [
    "https://thomaspeklak.github.io/agent-sandbox/",
    "https://thomaspeklak.github.io/agent-sandbox/docs/",
    ...generatedPages.map(
      (path) =>
        `https://thomaspeklak.github.io/agent-sandbox/docs/${relative(docsDir, path)}`,
    ),
  ];
  for (const url of expectedUrls) {
    if (!sitemap.includes(`<loc>${url}</loc>`)) {
      failures.push(`sitemap.xml is missing ${url}`);
    }
  }
}

async function main() {
  const htmlFiles = await walk(siteDir, (path) => extname(path) === ".html");
  const htmlByPath = new Map(
    await Promise.all(
      htmlFiles.map(async (path) => [path, await readFile(path, "utf8")]),
    ),
  );
  const generatedPages = htmlFiles.filter(
    (path) => dirname(path) === docsDir && path !== join(docsDir, "index.html"),
  );

  await validateSourceCoverage(generatedPages);
  await validateHtmlLinks(htmlFiles, htmlByPath);
  await validateSearchIndex(htmlByPath);
  await validateSitemap(generatedPages);

  if (failures.length) {
    for (const failure of failures) console.error(`- ${failure}`);
    throw new Error(`Documentation validation failed with ${failures.length} issue(s).`);
  }

  console.log(
    `Validated ${generatedPages.length} Markdown pages and ${htmlFiles.length} site pages with no broken local links.`,
  );
}

await main();
