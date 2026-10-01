import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";
import test from "node:test";
import { APPROVAL_TIMEOUT_MS, readClipboard, request } from "../extensions/clipboard-paste/bridge.mjs";
import { bridge, imageReply, reply, waitFor } from "./clipboard-support.mjs";

const imageTypes = (socket) => reply(socket, { ok: true, types: ["image/png", "text/plain"] });

test("one read can wait longer than Pi's native three-second timeout", async (t) => {
	assert.equal(APPROVAL_TIMEOUT_MS, 300000);
	const host = await bridge(t, (req, socket) => {
		if (req.op === "list") imageTypes(socket);
		else {
			const timer = setTimeout(() => imageReply(socket), 3100);
			socket.once("close", () => clearTimeout(timer));
		}
	});
	const clipboard = await readClipboard(host.path);
	assert.equal(clipboard.mimeType, "image/png");
	assert.equal(clipboard.bytes.toString(), "test-image");
	assert.deepEqual(host.requests.map((req) => req.op), ["list", "read"]);
});

test("prefers supported images and preserves raw MIME parameters", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["text/plain", "image/bmp", "image/jpeg", "image/png;charset=binary"] })
		: imageReply(socket));
	assert.equal((await readClipboard(host.path)).mimeType, "image/png");
	assert.equal(host.requests[1].mime, "image/png;charset=binary");
});

test("selects plain text without an unsuccessful image read", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? reply(socket, { ok: true, types: ["text/html", "text/plain;charset=utf-8"] })
		: reply(socket, { ok: true, data_b64: Buffer.from("line one\nline two").toString("base64") }));
	const clipboard = await readClipboard(host.path);
	assert.equal(clipboard.mimeType, "text/plain");
	assert.equal(clipboard.bytes.toString(), "line one\nline two");
	assert.equal(host.requests[1].mime, "text/plain;charset=utf-8");
});

test("empty or unsupported clipboard types do not trigger an approved read", async (t) => {
	for (const types of [[], ["text/html"]]) {
		const host = await bridge(t, (_req, socket) => reply(socket, { ok: true, types }));
		assert.equal(await readClipboard(host.path), undefined);
		assert.deepEqual(host.requests.map((req) => req.op), ["list"]);
	}
});

test("denial is terminal: no retry and no text fallback read", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? imageTypes(socket)
		: reply(socket, { ok: false, error: "clipboard access denied by user" }));
	await assert.rejects(readClipboard(host.path), /denied by user/);
	assert.deepEqual(host.requests.map((req) => req.op), ["list", "read"]);
});

test("invalid JSON, invalid MIME lists and invalid base64 fail closed", async (t) => {
	for (const listing of [{ ok: true, types: [42] }, { ok: true }]) {
		const host = await bridge(t, (_req, socket) => reply(socket, listing));
		await assert.rejects(readClipboard(host.path), /Invalid clipboard MIME/);
	}
	const invalidJson = await bridge(t, (_req, socket) => socket.end("not-json\n"));
	await assert.rejects(readClipboard(invalidJson.path), SyntaxError);
	for (const data_b64 of ["!not-base64", "aGVsbG8", 42]) {
		const host = await bridge(t, (req, socket) => req.op === "list"
			? imageTypes(socket)
			: reply(socket, { ok: true, data_b64 }));
		await assert.rejects(readClipboard(host.path), /Invalid clipboard image\/text payload/);
	}
});

test("empty payload is a successful no-op", async (t) => {
	const host = await bridge(t, (req, socket) => req.op === "list"
		? imageTypes(socket)
		: reply(socket, { ok: true, data_b64: "" }));
	assert.equal(await readClipboard(host.path), undefined);
});

test("fragmented responses including split Unicode decode correctly", async (t) => {
	const host = await bridge(t, (_req, socket) => {
		const response = Buffer.from(JSON.stringify({ ok: true, value: "🤖" }) + "\n");
		socket.write(response.subarray(0, response.length - 5));
		setImmediate(() => socket.end(response.subarray(response.length - 5)));
	});
	assert.equal((await request(host.path, { op: "list" })).value, "🤖");
});

test("abort and deadline close the pending socket", async (t) => {
	const host = await bridge(t, () => {});
	const controller = new AbortController();
	const read = request(host.path, { op: "read" }, { signal: controller.signal });
	await waitFor(() => host.requests.length === 1);
	controller.abort();
	await assert.rejects(read, { name: "AbortError" });
	await waitFor(() => host.sockets.size === 0);
	await assert.rejects(request(host.path, { op: "read" }, { timeoutMs: 15 }), /timed out/);
	await waitFor(() => host.sockets.size === 0);
	await assert.rejects(request(host.path, { op: "read" }, { signal: controller.signal }), { name: "AbortError" });
	await sleep(10);
	assert.equal(host.requests.length, 2);
});

test("disconnect without a response does not retry", async (t) => {
	const host = await bridge(t, (_req, socket) => socket.end());
	await assert.rejects(readClipboard(host.path), /closed without a response/);
	assert.equal(host.requests.length, 1);
});
