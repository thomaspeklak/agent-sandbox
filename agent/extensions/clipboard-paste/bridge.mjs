import { createConnection } from "node:net";

const MAX_PAYLOAD_BYTES = 50 * 1024 * 1024;
const MAX_RESPONSE_BYTES = Math.ceil(MAX_PAYLOAD_BYTES / 3) * 4 + 65536;
export const APPROVAL_TIMEOUT_MS = 5 * 60 * 1000;
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];

function baseMime(mime) {
	return mime.split(";")[0].trim().toLowerCase();
}

/** One bounded bridge exchange. Reads remain subject to the host approval gate. */
export function request(socketPath, payload, { signal, timeoutMs = APPROVAL_TIMEOUT_MS } = {}) {
	return new Promise((resolve, reject) => {
		if (signal?.aborted) {
			reject(signal.reason);
			return;
		}
		const socket = createConnection(socketPath);
		const chunks = [];
		let length = 0;
		let settled = false;
		const finish = (error, result) => {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			signal?.removeEventListener("abort", onAbort);
			socket.destroy();
			if (error) reject(error);
			else resolve(result);
		};
		const onAbort = () => finish(signal.reason);
		const timer = setTimeout(() => finish(new Error("Clipboard approval timed out")), timeoutMs);
		signal?.addEventListener("abort", onAbort, { once: true });
		socket.on("error", (error) => finish(error));
		socket.on("close", () => finish(new Error("Clipboard bridge closed without a response")));
		socket.on("connect", () => {
			if (!settled) socket.write(`${JSON.stringify({ v: 1, ...payload })}\n`);
		});
		socket.on("data", (chunk) => {
			if (settled) return;
			length += chunk.length;
			if (length > MAX_RESPONSE_BYTES) {
				finish(new Error("Clipboard response exceeds the paste size limit"));
				return;
			}
			chunks.push(chunk);
			if (!chunk.includes(10)) return;
			try {
				const buffer = Buffer.concat(chunks, length);
				const response = JSON.parse(buffer.subarray(0, buffer.indexOf(10)).toString("utf8"));
				if (response?.ok !== true) {
					throw new Error(response?.error || "Clipboard access was denied");
				}
				finish(undefined, response);
			} catch (error) {
				finish(error);
			}
		});
	});
}

/** Select one MIME type and perform just one approved read; never retry a denial. */
export async function readClipboard(socketPath, options) {
	const listing = await request(socketPath, { op: "list" }, { ...options, timeoutMs: options?.timeoutMs ?? 5000 });
	if (!Array.isArray(listing.types) || listing.types.some((type) => typeof type !== "string")) {
		throw new Error("Invalid clipboard MIME listing");
	}
	const types = listing.types.map((raw) => ({ raw, base: baseMime(raw) }));
	const selected = IMAGE_TYPES.map((mime) => types.find((type) => type.base === mime)).find(Boolean)
		?? types.find((type) => type.base.startsWith("image/"))
		?? types.find((type) => type.base === "text/plain");
	if (!selected) return undefined;
	const response = await request(socketPath, { op: "read", mime: selected.raw }, options);
	const encoded = response.data_b64;
	if (typeof encoded !== "string") throw new Error("Invalid clipboard image/text payload");
	const bytes = Buffer.from(encoded, "base64");
	if (bytes.length > MAX_PAYLOAD_BYTES) throw new Error("Clipboard exceeds the paste size limit");
	if (bytes.toString("base64") !== encoded) throw new Error("Invalid clipboard image/text payload");
	if (!bytes.length) return undefined;
	return { bytes, mimeType: selected.base };
}
