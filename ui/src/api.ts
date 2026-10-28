// One call path for every backend method. Inside the desktop app it goes through
// Tauri's IPC; in a plain browser (development, screenshots) it goes to the local
// dev bridge, which runs the very same Rust dispatcher.
import type { ApiError } from "./types";

const DEV_BRIDGE = "http://127.0.0.1:8787/api";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export class CallError extends Error {
  code: string;
  constructor(err: ApiError) {
    super(err.message);
    this.code = err.code;
  }
}

function asApiError(e: unknown): ApiError {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as ApiError;
  return { code: "bridge", message: String(e) };
}

export async function call<T>(method: string, params: unknown = {}): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      return await invoke<T>("api", { method, params });
    } catch (e) {
      throw new CallError(asApiError(e));
    }
  }
  let res: Response;
  try {
    res = await fetch(DEV_BRIDGE, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ method, params }),
    });
  } catch (e) {
    throw new CallError({ code: "bridge", message: String(e) });
  }
  const body = (await res.json()) as { ok: boolean; result?: T; error?: ApiError };
  if (!body.ok) throw new CallError(body.error ?? { code: "bridge", message: "unknown error" });
  return body.result as T;
}

/** Opens an external URL or settings page (ms-settings:...) in the OS. */
export async function openExternal(url: string) {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

/** Saves text to a file chosen by the user. Falls back to a download in the browser. */
export async function saveTextFile(defaultName: string, content: string, filters?: { name: string; extensions: string[] }[]) {
  if (isTauri) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ defaultPath: defaultName, filters });
    if (!path) return null;
    await call("fs.writeText", { path, content });
    return path;
  }
  const blob = new Blob([content], { type: "application/octet-stream" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = defaultName;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  return defaultName;
}

/** Lets the user pick a file and returns its text (or data URL for images). */
export async function openFile(accept: string, asDataUrl = false): Promise<{ name: string; content: string } | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = () => {
      const file = input.files?.[0];
      if (!file) return resolve(null);
      const reader = new FileReader();
      reader.onload = () => resolve({ name: file.name, content: String(reader.result) });
      reader.onerror = () => resolve(null);
      if (asDataUrl) reader.readAsDataURL(file);
      else reader.readAsText(file);
    };
    input.click();
  });
}
