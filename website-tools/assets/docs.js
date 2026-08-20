const body = document.body;
const isMac = /Mac|iPhone|iPad/.test(navigator.platform);

if (!isMac) document.documentElement.classList.add("non-mac");

const navButton = document.querySelector("[data-open-nav]");
const closeNavButtons = document.querySelectorAll("[data-close-nav]");

const setNavigation = (open) => {
  body.classList.toggle("nav-open", open);
  navButton?.setAttribute("aria-expanded", String(open));
};

navButton?.addEventListener("click", () => setNavigation(true));
closeNavButtons.forEach((button) =>
  button.addEventListener("click", () => setNavigation(false)),
);

const progress = document.querySelector("[data-reading-progress]");
const updateReadingProgress = () => {
  if (!progress) return;
  const available = document.documentElement.scrollHeight - window.innerHeight;
  const percent = available > 0 ? Math.min(100, Math.max(0, (window.scrollY / available) * 100)) : 0;
  progress.style.width = `${percent}%`;
};

updateReadingProgress();
window.addEventListener("scroll", updateReadingProgress, { passive: true });
window.addEventListener("resize", updateReadingProgress, { passive: true });

const tocLinks = [...document.querySelectorAll("[data-toc-link]")];
const tocHeadings = tocLinks
  .map((link) => document.getElementById(link.dataset.tocLink))
  .filter(Boolean);

const updateActiveToc = () => {
  if (!tocLinks.length) return;
  let activeId = tocHeadings[0]?.id;
  for (const heading of tocHeadings) {
    if (heading.getBoundingClientRect().top <= 145) activeId = heading.id;
    else break;
  }
  tocLinks.forEach((link) =>
    link.classList.toggle("is-active", link.dataset.tocLink === activeId),
  );
};

updateActiveToc();
window.addEventListener("scroll", updateActiveToc, { passive: true });

const copyText = async (text) => {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.append(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    textarea.remove();
    return copied;
  }
};

document.querySelectorAll("[data-copy-code]").forEach((button) => {
  button.addEventListener("click", async () => {
    const code = button.closest(".code-block")?.querySelector("code");
    if (!code) return;
    const copied = await copyText(code.textContent);
    const label = button.querySelector("span");
    if (!label) return;
    const original = label.textContent;
    label.textContent = copied ? "Copied" : "Select";
    button.classList.toggle("is-copied", copied);
    window.setTimeout(() => {
      label.textContent = original;
      button.classList.remove("is-copied");
    }, 1400);
  });
});

const searchLayer = document.querySelector("[data-search-layer]");
const searchInput = document.querySelector("[data-search-input]");
const searchResults = document.querySelector("[data-search-results]");
const searchState = document.querySelector("[data-search-state]");
const searchTriggers = document.querySelectorAll("[data-open-search]");
const closeSearchButtons = document.querySelectorAll("[data-close-search]");

let searchIndex;
let searchPromise;
let activeResult = -1;
let lastFocused;

const normalize = (value) =>
  String(value)
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();

const loadSearch = () => {
  if (searchIndex) return Promise.resolve(searchIndex);
  if (!searchPromise) {
    searchPromise = fetch("./search-index.json")
      .then((response) => {
        if (!response.ok) throw new Error(`Search index returned ${response.status}`);
        return response.json();
      })
      .then((data) => {
        searchIndex = data;
        return data;
      });
  }
  return searchPromise;
};

const openSearch = async () => {
  if (!searchLayer || !searchInput) return;
  lastFocused = document.activeElement;
  searchLayer.hidden = false;
  body.classList.add("search-open");
  setNavigation(false);
  searchInput.focus();
  try {
    await loadSearch();
    if (searchInput.value.trim()) renderSearch(searchInput.value);
  } catch {
    if (searchState) {
      searchState.innerHTML =
        "<strong>Search could not load</strong><p>Refresh the page and try again.</p>";
    }
  }
};

const closeSearch = () => {
  if (!searchLayer || searchLayer.hidden) return;
  searchLayer.hidden = true;
  body.classList.remove("search-open");
  activeResult = -1;
  if (lastFocused instanceof HTMLElement) lastFocused.focus();
};

searchTriggers.forEach((button) => button.addEventListener("click", openSearch));
closeSearchButtons.forEach((button) => button.addEventListener("click", closeSearch));

const scoreEntry = (entry, query, terms) => {
  const heading = normalize(entry.heading);
  const page = normalize(entry.page);
  const keywords = normalize(entry.keywords);
  const text = normalize(entry.text);
  const combined = `${heading} ${page} ${keywords} ${text}`;
  if (!terms.every((term) => combined.includes(term))) return 0;

  let score = 1;
  if (heading === query) score += 220;
  if (heading.startsWith(query)) score += 120;
  if (heading.includes(query)) score += 85;
  if (page === query) score += 100;
  if (page.includes(query)) score += 50;
  if (keywords.includes(query)) score += 34;
  if (text.includes(query)) score += 18;

  for (const term of terms) {
    if (heading.startsWith(term)) score += 35;
    else if (heading.includes(term)) score += 24;
    if (page.includes(term)) score += 16;
    if (keywords.includes(term)) score += 12;
    const occurrences = text.split(term).length - 1;
    score += Math.min(occurrences, 5) * 2;
  }
  return score;
};

const excerptFor = (text, terms) => {
  const clean = String(text).replace(/\s+/g, " ").trim();
  if (!clean) return "";
  const lower = normalize(clean);
  const positions = terms.map((term) => lower.indexOf(term)).filter((index) => index >= 0);
  const first = positions.length ? Math.min(...positions) : 0;
  const start = Math.max(0, first - 90);
  const end = Math.min(clean.length, start + 230);
  return `${start > 0 ? "…" : ""}${clean.slice(start, end).trim()}${end < clean.length ? "…" : ""}`;
};

const appendHighlighted = (element, value, terms) => {
  const text = String(value);
  if (!terms.length) {
    element.textContent = text;
    return;
  }
  const pattern = new RegExp(
    `(${terms
      .map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .join("|")})`,
    "gi",
  );
  for (const part of text.split(pattern)) {
    if (terms.some((term) => normalize(part) === term)) {
      const mark = document.createElement("mark");
      mark.textContent = part;
      element.append(mark);
    } else {
      element.append(document.createTextNode(part));
    }
  }
};

const setActiveResult = (index) => {
  const results = [...searchResults.querySelectorAll(".search-result")];
  if (!results.length) {
    activeResult = -1;
    return;
  }
  activeResult = (index + results.length) % results.length;
  results.forEach((result, resultIndex) => {
    const active = resultIndex === activeResult;
    result.classList.toggle("is-active", active);
    result.setAttribute("aria-selected", String(active));
    if (active) result.scrollIntoView({ block: "nearest" });
  });
};

const renderSearch = (rawQuery) => {
  if (!searchIndex || !searchResults || !searchState) return;
  const query = normalize(rawQuery.trim());
  const terms = [...new Set(query.split(/\s+/).filter((term) => term.length > 1))];
  searchResults.replaceChildren();
  activeResult = -1;

  if (!query || !terms.length) {
    searchResults.hidden = true;
    searchState.hidden = false;
    searchState.innerHTML =
      '<div class="search-empty-graphic" aria-hidden="true"><span></span><span></span><i></i></div><strong>Search the complete Agent Sandbox manual</strong><p>Try “lockdown”, “clipboard approval”, “mount”, or “OAuth callback”.</p>';
    return;
  }

  const matches = searchIndex
    .map((entry) => ({ entry, score: scoreEntry(entry, query, terms) }))
    .filter((item) => item.score > 0)
    .sort((a, b) => b.score - a.score || a.entry.heading.localeCompare(b.entry.heading))
    .slice(0, 12);

  if (!matches.length) {
    searchResults.hidden = true;
    searchState.hidden = false;
    searchState.innerHTML =
      "<strong>No documented section matched</strong><p>Try a shorter term, a CLI flag, or the exact error fragment.</p>";
    return;
  }

  searchState.hidden = true;
  searchResults.hidden = false;
  const fragment = document.createDocumentFragment();

  matches.forEach(({ entry }, index) => {
    const link = document.createElement("a");
    link.className = "search-result";
    link.href = entry.url;
    link.setAttribute("role", "option");
    link.setAttribute("aria-selected", "false");
    link.addEventListener("pointerenter", () => setActiveResult(index));

    const path = document.createElement("span");
    path.className = "result-path";
    path.textContent = `${entry.group} / ${entry.page}${entry.status ? ` / ${entry.status}` : ""}`;

    const title = document.createElement("span");
    title.className = "result-title";
    appendHighlighted(title, entry.heading, terms);

    const excerpt = document.createElement("p");
    excerpt.className = "result-excerpt";
    appendHighlighted(excerpt, excerptFor(entry.text, terms), terms);

    const arrow = document.createElement("span");
    arrow.className = "result-arrow";
    arrow.setAttribute("aria-hidden", "true");
    arrow.textContent = "→";

    link.append(path, title, excerpt, arrow);
    fragment.append(link);
  });

  searchResults.append(fragment);
  setActiveResult(0);
};

searchInput?.addEventListener("input", () => renderSearch(searchInput.value));

document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    searchLayer?.hidden ? openSearch() : closeSearch();
    return;
  }

  if (event.key === "Escape") {
    if (body.classList.contains("nav-open")) setNavigation(false);
    else closeSearch();
    return;
  }

  if (!searchLayer || searchLayer.hidden) return;
  const resultLinks = [...searchResults.querySelectorAll(".search-result")];
  if (event.key === "ArrowDown") {
    event.preventDefault();
    setActiveResult(activeResult + 1);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    setActiveResult(activeResult - 1);
  } else if (event.key === "Enter" && document.activeElement === searchInput) {
    const active = resultLinks[activeResult];
    if (active) {
      event.preventDefault();
      active.click();
    }
  } else if (event.key === "Tab") {
    const focusable = [
      searchInput,
      ...resultLinks,
      ...searchLayer.querySelectorAll("button:not([disabled])"),
    ].filter((element) => element instanceof HTMLElement && element.offsetParent !== null);
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
});
