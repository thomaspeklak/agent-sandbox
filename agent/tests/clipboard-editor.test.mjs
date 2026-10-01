import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { dirname } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import test from "node:test";
import { bridge, editorFixture, imageReply, reply, waitFor } from "./clipboard-support.mjs";

async function pendingPaste(t, options) {
	let reader;
	const host = await bridge(t, (req, socket) => {
		if (req.op === "list") reply(socket, { ok: true, types: ["image/png", "text/plain"] });
		else reader = socket;
	});
	const fixture = await editorFixture(t, host.path, options);
	fixture.paste();
	await waitFor(() => reader);
	return { fixture, host, finish: () => imageReply(reader) };
}

test("approval completes the original paste once, privately, without submission", async (t) => {
	const { fixture, host, finish } = await pendingPaste(t);
	assert.match(fixture.status, /waiting for approval/);
	for (let i = 0; i < 4; i++) fixture.paste();
	await sleep(10);
	assert.equal(host.requests.length, 2);
	finish();
	await waitFor(() => fixture.insertions.length === 1 && fixture.status === undefined);
	const path = fixture.insertions[0].trim();
	assert.equal(await readFile(path, "utf8"), "test-image");
	assert.equal((await stat(path)).mode & 0o777, 0o600);
	assert.equal((await stat(dirname(path))).mode & 0o777, 0o700);
	assert.equal(fixture.editor.getText(), `draft ${path}`);
	assert.equal(fixture.submits, 0);
	assert.equal(fixture.notifications.length, 0);
});

test("denial does not paste, retry, or submit", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["image/png", "text/plain"] })
		: reply(socket, { ok: false, error: "clipboard access denied by user" }));
	const fixture = await editorFixture(t, host.path);
	fixture.paste();
	await waitFor(() => fixture.notifications.length === 1 && fixture.status === undefined);
	assert.match(fixture.notifications[0].text, /denied by user/);
	assert.equal(fixture.editor.getText(), "draft ");
	assert.equal(fixture.insertions.length, 0);
	assert.equal(fixture.submits, 0);
	assert.equal(host.requests.length, 2);
});

test("Escape and Ctrl-C cancel without clearing the draft", async (t) => {
	for (const key of ["\x1b", "\x03"]) {
		const { fixture, host, finish } = await pendingPaste(t);
		fixture.editor.handleInput(key);
		await waitFor(() => host.sockets.size === 0);
		finish();
		await sleep(10);
		assert.equal(fixture.status, undefined);
		assert.equal(fixture.editor.getText(), "draft ");
		assert.equal(fixture.insertions.length, 0);
		assert.equal(fixture.notifications.length, 0);
		assert.equal(fixture.submits, 0);
	}
});

test("typing, moving the cursor, or submitting cancels delayed insertion", async (t) => {
	for (const key of ["changed", "\x1b[D", "\r"]) {
		const { fixture, host, finish } = await pendingPaste(t);
		fixture.editor.handleInput(key);
		await waitFor(() => host.sockets.size === 0);
		finish();
		await sleep(10);
		assert.equal(fixture.insertions.length, key === "changed" ? 1 : 0);
		assert.equal(fixture.submits, key === "\r" ? 1 : 0);
		assert.equal(fixture.status, undefined);
	}
});

test("session changes, reload/shutdown, prompts and programmatic submission cancel", async (t) => {
	for (const event of ["session_before_switch", "session_before_fork", "session_before_tree", "session_shutdown", "ui_prompt_start", "input"]) {
		const { fixture, host, finish } = await pendingPaste(t);
		await fixture.emit(event);
		await waitFor(() => host.sockets.size === 0);
		finish();
		await sleep(10);
		assert.equal(fixture.insertions.length, 0, event);
		assert.equal(fixture.status, undefined, event);
	}
});

test("changed draft, cursor, focus, session id, or editor factory rejects a late result", async (t) => {
	const changes = [
		(f) => f.editor.setText("programmatic change"),
		(f) => { f.editor.cursor = 0; },
		(f) => { f.tui.focus = {}; },
		(f) => { f.state.sessionId = "second"; },
		(f) => f.ctx.ui.setEditorComponent(undefined),
	];
	for (const change of changes) {
		const { fixture, finish } = await pendingPaste(t);
		change(fixture);
		finish();
		await waitFor(() => fixture.status === undefined);
		assert.equal(fixture.insertions.length, 0);
		assert.equal(fixture.submits, 0);
	}
});

test("uses configured paste keys and preserves the existing custom editor", async (t) => {
	const { fixture, finish } = await pendingPaste(t, {
		keys: { "app.clipboard.pasteImage": "custom-paste-key" },
		previousFactory: (Editor) => () => {
			const editor = new Editor();
			editor.customMarker = true;
			return editor;
		},
	});
	assert.equal(fixture.editor.customMarker, true);
	finish();
	await waitFor(() => fixture.insertions.length === 1);
	await fixture.emit("session_shutdown");
	assert.equal(fixture.ctx.ui.getEditorComponent(), fixture.originalFactory);
});

test("extension shortcuts retain precedence over native paste", async (t) => {
	const host = await bridge(t, () => assert.fail("clipboard bridge should not be called"));
	const fixture = await editorFixture(t, host.path);
	let calls = 0;
	fixture.editor.onExtensionShortcut = () => { calls++; return true; };
	fixture.paste();
	await sleep(10);
	assert.equal(calls, 1);
	assert.equal(fixture.status, undefined);
	assert.equal(host.requests.length, 0);
});

test("text including newlines, terminal escapes and commands is only inserted, never submitted", async (t) => {
	const text = "!echo never execute\n\x1b[201~\r\n";
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["text/plain;charset=utf-8"] })
		: reply(socket, { ok: true, data_b64: Buffer.from(text).toString("base64") }));
	const fixture = await editorFixture(t, host.path);
	fixture.paste();
	await waitFor(() => fixture.insertions.length === 1);
	assert.equal(fixture.insertions[0], text);
	assert.equal(fixture.editor.getText(), `draft ${text}`);
	assert.equal(fixture.submits, 0);
});

test("unsupported images are converted before insertion and separated from adjacent draft text", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["image/bmp"] })
		: imageReply(socket));
	let conversions = 0;
	const fixture = await editorFixture(t, host.path, {
		text: "describe",
		convertToPng: async (data, mime) => {
			assert.equal(mime, "image/bmp");
			assert.equal(Buffer.from(data, "base64").toString(), "test-image");
			conversions++;
			return { data: Buffer.from("converted").toString("base64"), mimeType: "image/png" };
		},
	});
	fixture.paste();
	await waitFor(() => fixture.insertions.length === 1);
	assert.equal(conversions, 1);
	assert.equal(await readFile(fixture.insertions[0].trim(), "utf8"), "converted");
	assert.match(fixture.editor.getText(), /^describe \/.*\/image\.png$/);
});

test("cancellation during asynchronous image conversion prevents a late paste", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["image/bmp"] })
		: imageReply(socket));
	let resolve;
	const fixture = await editorFixture(t, host.path, {
		convertToPng: () => new Promise((finish) => { resolve = finish; }),
	});
	fixture.paste();
	await waitFor(() => resolve);
	fixture.editor.handleInput("\x1b");
	resolve({ data: Buffer.from("converted").toString("base64"), mimeType: "image/png" });
	await sleep(15);
	assert.equal(fixture.insertions.length, 0);
	assert.equal(fixture.notifications.length, 0);
});

test("headless, outside-sandbox, lockdown, and missing-bridge runs remain untouched", async (t) => {
	for (const options of [
		{ mode: "rpc" }, { mode: "print" },
		{ env: { AGS_SANDBOX: "0" } }, { env: { AGS_LOCKDOWN: "1" } },
		{ env: { AGS_CLIPBOARD_SOCK: undefined } },
	]) {
		const fixture = await editorFixture(t, "/unused-clipboard.sock", options);
		assert.equal(fixture.ctx.ui.getEditorComponent(), undefined);
		assert.equal(fixture.status, undefined);
	}
});

test("repeated session starts do not accumulate wrappers or duplicate pastes", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["image/png"] }) : imageReply(socket));
	const fixture = await editorFixture(t, host.path);
	await fixture.emit("session_start");
	await fixture.emit("session_start");
	fixture.paste();
	await waitFor(() => fixture.insertions.length === 1);
	assert.equal(host.requests.length, 2);
	assert.equal(fixture.submits, 0);
});
