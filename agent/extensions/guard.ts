import { isAbsolute, normalize, relative, resolve } from "node:path";
import { isToolCallEventType, type ExtensionAPI } from "@mariozechner/pi-coding-agent";

const DANGEROUS_BASH_PATTERNS: Array<{ pattern: RegExp; reason: string }> = [
	{ pattern: /\brm\s+-[a-zA-Z]*r[a-zA-Z]*f\b.*\s\//, reason: "Refusing recursive force delete on absolute path" },
	{ pattern: /\bgit\s+reset\s+--hard\b/i, reason: "Refusing git reset --hard" },
	{ pattern: /\bgit\s+clean\b[^\n]*\b-f\b/i, reason: "Refusing git clean with force flag" },
	{ pattern: /\bmkfs(\.[a-z0-9]+)?\b/i, reason: "Refusing filesystem formatting command" },
	{ pattern: /\bdd\b[^\n]*\bof=\/dev\//i, reason: "Refusing dd writes to block devices" },
	{ pattern: /\b(shutdown|reboot|poweroff|halt)\b/i, reason: "Refusing system power command" },
	{ pattern: /:\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:/, reason: "Refusing fork bomb" },
];

const SENSITIVE_PATH_PREFIXES = [
	"/home/dev/.ssh",
	"/home/dev/.gnupg",
	"/home/dev/.aws",
	"/home/dev/.config/gcloud",
	"/home/dev/.git-credentials",
	"/home/dev/.npmrc",
	"/home/dev/.pi/agent/auth.json",
	"/home/dev/.pi/agent-host/auth.json",
	"/home/dev/.pi/agent-host/sandbox.json",
];

const DENY_WRITE_GLOBS = [/\.env(\..*)?$/i, /\.pem$/i, /\.key$/i, /id_[a-z0-9_-]+$/i];

function expandHome(inputPath: string, home: string): string {
	if (inputPath === "~") return home;
	if (inputPath.startsWith("~/")) return `${home}/${inputPath.slice(2)}`;
	return inputPath;
}

function parseRootsFromEnv(envName: string): string[] | undefined {
	const raw = process.env[envName];
	if (!raw) return undefined;
	try {
		const parsed = JSON.parse(raw);
		if (!Array.isArray(parsed)) return undefined;
		return parsed.filter((v): v is string => typeof v === "string" && v.length > 0);
	} catch {
		return undefined;
	}
}

function asAbsolutePath(inputPath: string, cwd: string, home: string): string {
	const expanded = expandHome(inputPath, home);
	const absolute = isAbsolute(expanded) ? expanded : resolve(cwd, expanded);
	return normalize(absolute);
}

function isPathInside(targetPath: string, rootPath: string): boolean {
	const rel = relative(rootPath, targetPath);
	return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function isSensitivePath(targetPath: string): boolean {
	return SENSITIVE_PATH_PREFIXES.some((prefix) => isPathInside(targetPath, prefix));
}

function matchesDeniedWrite(targetPath: string): boolean {
	return DENY_WRITE_GLOBS.some((re) => re.test(targetPath));
}

async function maybeRunDcg(pi: ExtensionAPI, command: string): Promise<string | undefined> {
	try {
		const version = await pi.exec("dcg", ["--version"], { timeout: 500 });
		if (version.code !== 0) return undefined;
	} catch {
		return undefined;
	}

	try {
		const result = await pi.exec("dcg", ["test", "--format", "json", command], { timeout: 2000 });
		if (result.code === 0) return undefined;
		if (result.code !== 1) return undefined; // fail-open for dcg runtime issues

		try {
			const parsed = JSON.parse(result.stdout || "{}");
			return parsed.reason || parsed.explanation || parsed.rule_id || "Blocked by destructive_command_guard";
		} catch {
			return "Blocked by destructive_command_guard";
		}
	} catch {
		return undefined;
	}
}

export default function (pi: ExtensionAPI) {
	pi.on("tool_call", async (event, ctx) => {
		const home = process.env.HOME ?? "/home/dev";
		const cwd = ctx.cwd;
		const workspace = normalize(cwd);

		const readRootsFromEnv = parseRootsFromEnv("AGS_GUARD_READ_ROOTS_JSON");
		const writeRootsFromEnv = parseRootsFromEnv("AGS_GUARD_WRITE_ROOTS_JSON");

		const allowedReadRoots = (readRootsFromEnv ?? [workspace, "/tmp", "/home/dev/.pi/agent"]).map((p) => asAbsolutePath(p, cwd, home));

		const allowedWriteRoots = (writeRootsFromEnv ?? [workspace, "/tmp", "/home/dev/.pi/agent"]).map((p) => asAbsolutePath(p, cwd, home));

		const inputPath =
			typeof event.input === "object" && event.input !== null && "path" in event.input && typeof event.input.path === "string"
				? asAbsolutePath(event.input.path, cwd, home)
				: undefined;

		if (event.toolName === "read" || event.toolName === "grep" || event.toolName === "find" || event.toolName === "ls") {
			if (inputPath) {
				if (isSensitivePath(inputPath)) {
					return { block: true, reason: `Sensitive path is not readable: ${inputPath}` };
				}
				if (!allowedReadRoots.some((root) => isPathInside(inputPath, root))) {
					return { block: true, reason: `Read outside sandbox roots denied: ${inputPath}` };
				}
			}
		}

		if (event.toolName === "write" || event.toolName === "edit") {
			if (!inputPath) {
				return { block: true, reason: `${event.toolName} requires a file path` };
			}
			if (isSensitivePath(inputPath)) {
				return { block: true, reason: `Sensitive path is not writable: ${inputPath}` };
			}
			if (!allowedWriteRoots.some((root) => isPathInside(inputPath, root))) {
				return { block: true, reason: `Write outside sandbox roots denied: ${inputPath}` };
			}
			if (matchesDeniedWrite(inputPath)) {
				return { block: true, reason: `Refusing writes to secret-like file: ${inputPath}` };
			}
		}

		if (isToolCallEventType("bash", event)) {
			const command = event.input.command;

			if (SENSITIVE_PATH_PREFIXES.some((p) => command.includes(p))) {
				return { block: true, reason: "Command references sensitive host path" };
			}

			for (const { pattern, reason } of DANGEROUS_BASH_PATTERNS) {
				if (pattern.test(command)) {
					return { block: true, reason };
				}
			}

			const dcgReason = await maybeRunDcg(pi, command);
			if (dcgReason) {
				return { block: true, reason: dcgReason };
			}
		}

		return undefined;
	});

	pi.registerCommand("guard", {
		description: "Show active sandbox guard roots",
		handler: async (_args, ctx) => {
			const readRoots = parseRootsFromEnv("AGS_GUARD_READ_ROOTS_JSON") ?? [ctx.cwd, "/tmp", "/home/dev/.pi/agent"];
			const writeRoots = parseRootsFromEnv("AGS_GUARD_WRITE_ROOTS_JSON") ?? [ctx.cwd, "/tmp", "/home/dev/.pi/agent"];

			ctx.ui.notify(
				[
					"Sandbox guard active.",
					`Workspace root: ${ctx.cwd}`,
					`Read roots: ${readRoots.join(", ")}`,
					`Write roots: ${writeRoots.join(", ")}`,
				].join("\n"),
				"info",
			);
		},
	});
}
