import { CustomEditor, convertToPng, type ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { installClipboardPaste } from "./editor.mjs";

export default function (pi: ExtensionAPI) {
	installClipboardPaste(pi, { CustomEditor, convertToPng });
}
