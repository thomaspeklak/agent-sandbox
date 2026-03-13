import { isAbsolute, normalize, relative, resolve } from "node:path";
import { isToolCallEventType, type ExtensionAPI } from "@mariozechner/pi-coding-agent";

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

type SandboxDetection = {
	enabled: boolean;
	source: string;
};

type DcgAvailability = "unknown" | "available" | "missing";

function detectSandboxAtStartup(): SandboxDetection {
	const explicit = process.env.AGS_SANDBOX;
	if (explicit === "1") {
		return { enabled: true, source: "AGS_SANDBOX=1" };
	}
	if (explicit === "0") {
		return { enabled: false, source: "AGS_SANDBOX=0" };
	}

	const hasGuardRoots = Boolean(process.env.AGS_GUARD_READ_ROOTS_JSON) || Boolean(process.env.AGS_GUARD_WRITE_ROOTS_JSON);
	if (hasGuardRoots) {
		return {
			enabled: true,
			source: "AGS_GUARD_*_ROOTS_JSON present (legacy signal)",
		};
	}

	return { enabled: false, source: "No sandbox marker env vars found" };
}

const SANDBOX_DETECTION = detectSandboxAtStartup();
let dcgAvailability: DcgAvailability = "unknown";

function dim(text: string): string {
	return `\x1b[2m${text}\x1b[22m`;
}

function yoloModeEnabled(): boolean {
	return process.env.AGS_GUARD_YOLO === "1";
}

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

async function detectDcgAvailability(pi: ExtensionAPI): Promise<boolean> {
	if (dcgAvailability === "available") return true;
	if (dcgAvailability === "missing") return false;

	try {
		const version = await pi.exec("dcg", ["--version"], { timeout: 500 });
		dcgAvailability = version.code === 0 ? "available" : "missing";
	} catch {
		dcgAvailability = "missing";
	}

	return dcgAvailability === "available";
}

async function maybeRunDcg(pi: ExtensionAPI, command: string): Promise<string | undefined> {
	if (!(await detectDcgAvailability(pi))) {
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
	pi.on("session_start", async (_event, ctx) => {
		if (!ctx.hasUI) return;
		const theme = ctx.ui.theme;
		const yolo = yoloModeEnabled();
		const status = yolo
			? theme.fg("warning", "guard:yolo")
			: SANDBOX_DETECTION.enabled
				? dim(theme.fg("success", "sandbox:on"))
				: theme.fg("error", "sandbox:off");
		ctx.ui.setWidget("ags-sandbox", [status], { placement: "aboveEditor" });
		ctx.ui.notify(
			`Sandbox ${SANDBOX_DETECTION.enabled ? "ON" : "OFF"} (${SANDBOX_DETECTION.source})`,
			SANDBOX_DETECTION.enabled ? "info" : "warning",
		);
		if (yolo) {
			ctx.ui.notify("AGS guard disabled for this run (--yolo)", "warning");
			return;
		}
		if (!(await detectDcgAvailability(pi))) {
			ctx.ui.notify(
				"destructive_command_guard (dcg) is unavailable in this sandbox; Bash classification will fail open. Run `ags doctor` or `ags update`.",
				"warning",
			);
		}
	});

	pi.on("session_shutdown", async (_event, ctx) => {
		if (!ctx.hasUI) return;
		ctx.ui.setWidget("ags-sandbox", undefined, { placement: "aboveEditor" });
	});

	pi.on("tool_call", async (event, ctx) => {
		if (yoloModeEnabled()) {
			return undefined;
		}

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
			const yolo = yoloModeEnabled();
			const dcgState = yolo ? "disabled by --yolo" : (await detectDcgAvailability(pi)) ? "available" : "missing";

			ctx.ui.notify(
				[
					yolo ? "Sandbox guard disabled for this run (--yolo)." : "Sandbox guard active.",
					`Sandbox mode (startup check): ${SANDBOX_DETECTION.enabled ? "ON" : "OFF"}`,
					`Detection source: ${SANDBOX_DETECTION.source}`,
					`dcg: ${dcgState}`,
					`Workspace root: ${ctx.cwd}`,
					`Read roots: ${readRoots.join(", ")}`,
					`Write roots: ${writeRoots.join(", ")}`,
				].join("\n"),
				"info",
			);
		},
	});
}
