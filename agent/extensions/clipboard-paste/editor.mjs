import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readClipboard } from "./bridge.mjs";

const STATUS = "ags-clipboard";
const EXTENSIONS = { "image/png": "png", "image/jpeg": "jpg", "image/webp": "webp", "image/gif": "gif" };

function cursorKey(editor) {
	return JSON.stringify(editor.getCursor?.());
}

function spacedPath(editor, path) {
	const cursor = editor.getCursor?.();
	if (!cursor) return path;
	const line = editor.getText().split("\n")[cursor.line] ?? "";
	const before = line[cursor.col - 1];
	const after = line[cursor.col];
	return `${before && !/\s/.test(before) ? " " : ""}${path}${after && !/\s/.test(after) ? " " : ""}`;
}

/** Compose Pi's existing editor instead of injecting keystrokes or submitting prompts. */
export function installClipboardPaste(pi, { CustomEditor, convertToPng, env = process.env }) {
	let pending;
	let detach;

	function cancel() {
		if (!pending) return;
		const operation = pending;
		pending = undefined;
		operation.controller.abort();
		operation.ctx.ui.setStatus(STATUS, undefined);
	}

	pi.on("session_start", (_event, ctx) => {
		cancel();
		detach?.();
		detach = undefined;
		if (ctx.mode !== "tui" || env.AGS_SANDBOX !== "1" || env.AGS_LOCKDOWN === "1" || !env.AGS_CLIPBOARD_SOCK) return;

		const previousFactory = ctx.ui.getEditorComponent();
		let activeEditor;
		const factory = (tui, theme, keybindings) => {
			cancel();
			const editor = previousFactory?.(tui, theme, keybindings) ?? new CustomEditor(tui, theme, keybindings);
			activeEditor = editor;
			// Pi also uses this duck type: jiti can create different class identities.
			if (!(editor.actionHandlers instanceof Map) || typeof editor.insertTextAtCursor !== "function") {
				ctx.ui.notify("AGS clipboard paste needs a CustomEditor-compatible editor", "warning");
				return editor;
			}
			const originalInput = editor.handleInput.bind(editor);
			editor.handleInput = (data) => {
				if (pending) {
					// Additional paste presses share the outstanding operation.
					if (keybindings.matches(data, "app.clipboard.pasteImage")) return;
					cancel();
					// Cancel without also clearing the draft or aborting an agent turn.
					if (keybindings.matches(data, "app.interrupt") || keybindings.matches(data, "app.clear")) return;
				}
				originalInput(data);
			};
			editor.onPasteImage = () => {
				if (pending) return;
				const operation = {
					controller: new AbortController(), ctx,
					sessionId: ctx.sessionManager.getSessionId(),
					text: editor.getText(), cursor: cursorKey(editor),
				};
				pending = operation;
				ctx.ui.setStatus(STATUS, "clipboard: waiting for approval (Esc cancels)");
				const isCurrent = () => pending === operation
					&& !operation.controller.signal.aborted
					&& ctx.sessionManager.getSessionId() === operation.sessionId
					&& ctx.ui.getEditorComponent() === factory
					&& activeEditor === editor && tui.getFocusedComponent() === editor
					&& editor.getText() === operation.text && cursorKey(editor) === operation.cursor;
				void paste(operation, editor, tui, isCurrent);
			};
			return editor;
		};
		ctx.ui.setEditorComponent(factory);
		detach = () => {
			activeEditor = undefined;
			if (ctx.ui.getEditorComponent() === factory) ctx.ui.setEditorComponent(previousFactory);
		};
	});

	async function paste(operation, editor, tui, isCurrent) {
		let directory;
		try {
			const clipboard = await readClipboard(env.AGS_CLIPBOARD_SOCK, { signal: operation.controller.signal });
			if (!clipboard || !isCurrent()) return;
			let text;
			if (clipboard.mimeType === "text/plain") {
				text = clipboard.bytes.toString("utf8");
			} else {
				let { bytes, mimeType } = clipboard;
				if (!EXTENSIONS[mimeType]) {
					const converted = await convertToPng(bytes.toString("base64"), mimeType);
					if (!converted) throw new Error("Clipboard image format could not be converted to PNG");
					bytes = Buffer.from(converted.data, "base64");
					mimeType = "image/png";
				}
				if (!isCurrent()) return;
				directory = await mkdtemp(join(tmpdir(), "ags-pi-clipboard-"));
				const path = join(directory, `image.${EXTENSIONS[mimeType]}`);
				await writeFile(path, bytes, { mode: 0o600, flag: "wx" });
				text = spacedPath(editor, path);
			}
			if (!isCurrent()) return;
			// Direct insertion is Pi's native paste path: clipboard text is never input events.
			editor.insertTextAtCursor(text);
			// Keep inserted image files available for submission/draft recovery, like Pi does.
			directory = undefined;
			tui.requestRender();
		} catch (error) {
			if (isCurrent()) operation.ctx.ui.notify(`Clipboard paste failed: ${error instanceof Error ? error.message : String(error)}`, "warning");
		} finally {
			if (pending === operation) {
				pending = undefined;
				operation.ctx.ui.setStatus(STATUS, undefined);
			}
			if (directory) await rm(directory, { recursive: true, force: true }).catch(() => {});
		}
	}

	for (const event of ["session_before_switch", "session_before_fork", "session_before_tree", "ui_prompt_start", "input"]) {
		pi.on(event, () => { cancel(); });
	}
	pi.on("session_shutdown", () => {
		cancel();
		detach?.();
		detach = undefined;
	});
}
