const header = document.querySelector("[data-header]");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

if (!reducedMotion && "IntersectionObserver" in window) {
  document.documentElement.classList.add("motion-ready");
}

const updateHeader = () => {
  header?.classList.toggle("is-scrolled", window.scrollY > 24);
};

updateHeader();
window.addEventListener("scroll", updateHeader, { passive: true });

const revealItems = document.querySelectorAll(".reveal");

if (reducedMotion || !("IntersectionObserver" in window)) {
  revealItems.forEach((item) => item.classList.add("is-visible"));
} else {
  const revealObserver = new IntersectionObserver(
    (entries, observer) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        entry.target.classList.add("is-visible");
        observer.unobserve(entry.target);
      });
    },
    { threshold: 0.13, rootMargin: "0px 0px -40px" },
  );
  revealItems.forEach((item) => revealObserver.observe(item));
}

const modeData = {
  daily: {
    label: "DAILY DRIVER",
    signal: "host bridges available",
    command: "ags --agent codex --tmux",
    description:
      "Keep your agent state, dedicated SSH identity, selected secrets, and configured bridges. Tmux leaves a shell behind when the agent finishes, ready for inspection.",
    permissions: [
      ["on", "Repo write"],
      ["on", "Network"],
      ["on", "Scoped secrets"],
      ["on", "SSH agent"],
      ["off", "Raw host socket"],
    ],
  },
  lockdown: {
    label: "FOREIGN REPO",
    signal: "minimal host exposure",
    command: "ags --agent claude --lockdown",
    description:
      "Inspect code you do not trust with a sanitized, per-run agent home. The workspace and network remain useful; secrets, SSH, configured mounts, and host sidecars stay out.",
    permissions: [
      ["on", "Repo write"],
      ["on", "Network"],
      ["off", "Secrets"],
      ["off", "SSH agent"],
      ["off", "Host bridges"],
    ],
  },
  browser: {
    label: "BROWSER WORK",
    signal: "browser sidecar connected",
    command: "ags --agent pi --browser",
    description:
      "Launch the configured browser sidecar for browser-enabled agent workflows. AGS connects the sandbox to its debugging port while the auth proxy handles deliberate host opens.",
    permissions: [
      ["on", "Repo write"],
      ["on", "Network"],
      ["on", "Browser sidecar"],
      ["on", "OAuth relay"],
      ["off", "Wayland by default"],
    ],
  },
  containers: {
    label: "INTEGRATION TESTS",
    signal: "PSP policy active",
    command: "ags --agent codex --psp",
    description:
      "Give Docker and Testcontainers clients a per-session PSP socket. Container operations pass through deny-by-default policy instead of exposing the raw host Podman API.",
    permissions: [
      ["on", "Repo write"],
      ["on", "Network"],
      ["on", "Testcontainers"],
      ["on", "Image policy"],
      ["off", "Raw Podman socket"],
    ],
  },
};

const modePanel = document.querySelector(".mode-panel");
const modeLabel = document.querySelector("[data-mode-label]");
const modeSignal = document.querySelector("[data-mode-signal]");
const modeCommand = document.querySelector("[data-mode-command]");
const modeDescription = document.querySelector("[data-mode-description]");
const modePermissions = document.querySelector("[data-mode-permissions]");
const modeCopy = document.querySelector("[data-copy-mode]");

const renderMode = (mode) => {
  const data = modeData[mode];
  if (!data || !modePanel) return;

  modePanel.classList.remove("is-changing");
  void modePanel.offsetWidth;
  modeLabel.textContent = data.label;
  modeSignal.textContent = data.signal;
  modeCommand.textContent = data.command;
  modeDescription.textContent = data.description;
  modePermissions.innerHTML = data.permissions
    .map(
      ([state, label]) =>
        `<span class="is-${state}"><i>${state === "on" ? "✓" : "—"}</i> ${label}</span>`,
    )
    .join("");
  modeCopy.dataset.copy = data.command;
  modePanel.classList.add("is-changing");
};

document.querySelectorAll("[data-mode]").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll("[data-mode]").forEach((item) => {
      const active = item === tab;
      item.classList.toggle("is-active", active);
      item.setAttribute("aria-selected", String(active));
    });
    renderMode(tab.dataset.mode);
  });
});

const setCopyFeedback = (button, success) => {
  const label = button.querySelector("span");
  if (!label) return;
  const original = label.textContent;
  label.textContent = success ? "Copied" : "Select";
  button.classList.toggle("is-copied", success);
  window.setTimeout(() => {
    label.textContent = original;
    button.classList.remove("is-copied");
  }, 1400);
};

document.querySelectorAll("[data-copy], [data-copy-mode]").forEach((button) => {
  button.addEventListener("click", async () => {
    const text = button.dataset.copy;
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopyFeedback(button, true);
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      const code = button.parentElement?.querySelector("code");
      if (!code || !selection) return;
      range.selectNodeContents(code);
      selection.removeAllRanges();
      selection.addRange(range);
      setCopyFeedback(button, false);
    }
  });
});

const consoleScenes = [
  {
    command: "ags --agent codex --lockdown",
    lines: [
      "✓ staged ephemeral agent home",
      "✓ mounted exact workspace",
      "✓ dropped capabilities · no new privileges",
      "→ entering sandbox",
    ],
    foot: "host exposure: minimal",
  },
  {
    command: "ags --agent pi --browser",
    lines: [
      "✓ browser sidecar ready",
      "✓ auth proxy awaiting approval",
      "✓ webview relay registered",
      "→ entering sandbox",
    ],
    foot: "bridge access: scoped",
  },
  {
    command: "ags --agent codex --psp",
    lines: [
      "✓ PSP session socket ready",
      "✓ container policy attached",
      "✓ Testcontainers host mapped",
      "→ entering sandbox",
    ],
    foot: "container access: gated",
  },
];

const terminalCommand = document.querySelector("[data-terminal-command]");
const terminalOutput = document.querySelector("[data-terminal-output]");
const terminalFoot = document.querySelector(".console-foot > span:last-child");
let sceneIndex = 0;

if (!reducedMotion && terminalCommand && terminalOutput) {
  window.setInterval(() => {
    sceneIndex = (sceneIndex + 1) % consoleScenes.length;
    const scene = consoleScenes[sceneIndex];
    terminalOutput.classList.add("is-changing");
    window.setTimeout(() => {
      terminalCommand.textContent = scene.command;
      terminalOutput.innerHTML = scene.lines
        .map((line, index) => {
          if (index === scene.lines.length - 1) {
            return `<span class="console-enter"><b>→</b>${line.replace("→ ", "")}</span>`;
          }
          return `<span><i>✓</i>${line.replace("✓ ", "")}</span>`;
        })
        .join("");
      terminalFoot.textContent = scene.foot;
      terminalOutput.classList.remove("is-changing");
    }, 180);
  }, 4800);
}

const tilt = document.querySelector("[data-tilt]");
if (tilt && !reducedMotion && window.matchMedia("(pointer: fine)").matches) {
  tilt.addEventListener("pointermove", (event) => {
    const rect = tilt.getBoundingClientRect();
    const x = (event.clientX - rect.left) / rect.width - 0.5;
    const y = (event.clientY - rect.top) / rect.height - 0.5;
    tilt.style.setProperty("--rx", `${-y * 4}deg`);
    tilt.style.setProperty("--ry", `${x * 5}deg`);
  });
  tilt.addEventListener("pointerleave", () => {
    tilt.style.setProperty("--rx", "0deg");
    tilt.style.setProperty("--ry", "0deg");
  });

  window.addEventListener(
    "pointermove",
    (event) => {
      const x = (event.clientX / window.innerWidth - 0.5) * -9;
      const y = (event.clientY / window.innerHeight - 0.5) * -6;
      document.documentElement.style.setProperty("--hero-x", `${x}px`);
      document.documentElement.style.setProperty("--hero-y", `${y}px`);
    },
    { passive: true },
  );
}

const launchSteps = document.querySelectorAll("[data-launch-step]");
if ("IntersectionObserver" in window && !reducedMotion) {
  const launchObserver = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        entry.target.classList.toggle("is-active", entry.isIntersecting);
      });
    },
    { threshold: 0.75 },
  );
  launchSteps.forEach((step) => launchObserver.observe(step));
}

document.querySelector("[data-year]").textContent = String(new Date().getFullYear());
