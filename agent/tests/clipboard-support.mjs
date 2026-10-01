import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { installClipboardPaste } from "../extensions/clipboard-paste/editor.mjs";

export async function waitFor(predicate) {
	const end = Date.now() + 6000;
	while (!predicate()) {
		assert.ok(Date.now() < end, "condition did not become true");
		await sleep(5);
	}
}

export async function bridge(t, handler) {
	const root = await mkdtemp(join(tmpdir(), "ags-paste-test-"));
	const path = join(root, "clipboard.sock");
	const requests = [];
	const sockets = new Set();
	const server = createServer((socket) => {
		sockets.add(socket);
		socket.on("error", () => {});
		socket.on("close", () => sockets.delete(socket));
		let line = "";
		socket.on("data", (chunk) => {
			line += chunk.toString();
			if (!line.endsWith("\n")) return;
			const request = JSON.parse(line);
			requests.push(request);
			handler(request, socket);
		});
	});
	await new Promise((resolve, reject) => server.listen(path, resolve).once("error", reject));
	t.after(async () => {
		for (const socket of sockets) socket.destroy();
		await new Promise((resolve) => server.close(resolve));
		await rm(root, { recursive: true, force: true });
	});
	return { path, requests, sockets };
}

export function reply(socket, value) {
	socket.end(`${JSON.stringify(value)}\n`);
}

export function imageReply(socket, bytes = Buffer.from("test-image")) {
	reply(socket, { ok: true, mime: "image/png", data_b64: bytes.toString("base64") });
}

export async function editorFixture(t, path, options = {}) {
	const events = new Map();
	const insertions = [];
	const notifications = [];
	const statuses = new Map();
	const keys = { "app.clipboard.pasteImage": "\x16", "app.interrupt": "\x1b", "app.clear": "\x03", ...options.keys };
	const keybindings = { matches: (data, action) => data === keys[action] };
	const tui = { focus: undefined, renders: 0, getFocusedComponent() { return this.focus; }, requestRender() { this.renders++; } };
	let submits = 0;
	class Editor {
		actionHandlers = new Map();
		text = "";
		cursor = 0;
		getText() { return this.text; }
		getCursor() { return { line: 0, col: this.cursor }; }
		setText(text) { this.text = text; this.cursor = text.length; }
		insertTextAtCursor(text) {
			insertions.push(text);
			this.text = this.text.slice(0, this.cursor) + text + this.text.slice(this.cursor);
			this.cursor += text.length;
		}
		handleInput(data) {
			if (this.onExtensionShortcut?.(data)) return;
			if (keybindings.matches(data, "app.clipboard.pasteImage")) this.onPasteImage?.();
			else if (data === "\r") submits++;
			else if (data === "\x1b[D") this.cursor--;
			else this.insertTextAtCursor(data);
		}
	}
	let factory = options.previousFactory?.(Editor);
	const originalFactory = factory;
	let editor = factory?.(tui, {}, keybindings) ?? new Editor();
	editor.setText(options.text ?? "draft ");
	const state = { sessionId: "first" };
	const ctx = {
		mode: options.mode ?? "tui",
		sessionManager: { getSessionId: () => state.sessionId },
		ui: {
			getEditorComponent: () => factory,
			setEditorComponent(next) {
				const text = editor.getText();
				factory = next;
				editor = next?.(tui, {}, keybindings) ?? new Editor();
				editor.setText(text);
				tui.focus = editor;
			},
			setStatus: (name, value) => { statuses.set(name, value); },
			notify: (text, type) => notifications.push({ text, type }),
		},
	};
	const pi = { on(event, callback) {
		const handlers = events.get(event) ?? [];
		handlers.push(callback);
		events.set(event, handlers);
	} };
	const emit = async (event) => {
		for (const callback of events.get(event) ?? []) await callback({}, ctx);
	};
	installClipboardPaste(pi, {
		CustomEditor: Editor,
		convertToPng: options.convertToPng ?? (async () => null),
		env: { AGS_SANDBOX: "1", AGS_CLIPBOARD_SOCK: path, ...options.env },
	});
	await emit("session_start");
	t.after(async () => {
		await emit("session_shutdown");
		for (const inserted of insertions) {
			const file = inserted.trim();
			if (file.startsWith(join(tmpdir(), "ags-pi-clipboard-"))) {
				await rm(dirname(file), { recursive: true, force: true });
			}
		}
	});
	return {
		get editor() { return editor; },
		get status() { return statuses.get("ags-clipboard"); },
		get submits() { return submits; },
		insertions, notifications, tui, ctx, state, emit, keys, originalFactory,
		paste() { editor.handleInput(keys["app.clipboard.pasteImage"]); },
	};
}
