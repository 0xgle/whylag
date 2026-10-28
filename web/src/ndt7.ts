// Minimal browser client for M-Lab's ndt7 protocol.
// https://github.com/m-lab/ndt-server/blob/main/spec/ndt7-protocol.md

export interface Progress {
  phase: "locate" | "download" | "upload" | "done";
  /** 0..1 within the whole test */
  progress: number;
  downloadMbps?: number;
  uploadMbps?: number;
  /** Smallest RTT the server saw (ms): our idle latency. */
  idleMs?: number;
  /** Median RTT under load (ms). */
  loadedDownMs?: number;
  loadedUpMs?: number;
}

export interface Result {
  server: string;
  downloadMbps: number;
  uploadMbps: number;
  idleMs: number | null;
  loadedDownMs: number | null;
  loadedUpMs: number | null;
}

const PROTOCOL = "net.measurementlab.ndt.v7";
const LOCATE = "https://locate.measurementlab.net/v2/nearest/ndt/ndt7?client_name=whylag-web&client_version=0.1";
const PHASE_MS = 10_000;

interface TcpInfo {
  RTT?: number;
  MinRTT?: number;
  BytesReceived?: number;
  ElapsedTime?: number;
}

function median(v: number[]) {
  if (!v.length) return null;
  const s = [...v].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

async function locate() {
  const res = await fetch(LOCATE);
  if (!res.ok) throw new Error(`locate ${res.status}`);
  const body = await res.json();
  const r = body.results?.[0];
  if (!r) throw new Error("no server");
  const where = r.location ? `${r.location.city}, ${r.location.country}` : r.machine;
  return { down: r.urls["wss:///ndt/v7/download"] as string, up: r.urls["wss:///ndt/v7/upload"] as string, server: where as string };
}

function download(url: string, report: (mbps: number, rtts: number[], minRtt: number | null, elapsed: number) => void): Promise<{ mbps: number; rtts: number[]; minRtt: number | null }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, PROTOCOL);
    ws.binaryType = "arraybuffer";
    let bytes = 0;
    let start = 0;
    let last = 0;
    const rtts: number[] = [];
    let minRtt: number | null = null;
    const finish = () => {
      const elapsed = (performance.now() - start) / 1000;
      resolve({ mbps: (bytes * 8) / Math.max(0.001, elapsed) / 1e6, rtts, minRtt });
    };
    const guard = setTimeout(() => ws.close(), PHASE_MS + 5000);
    ws.onopen = () => {
      start = performance.now();
      last = start;
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        bytes += ev.data.length;
        try {
          const m = JSON.parse(ev.data) as { TCPInfo?: TcpInfo };
          if (m.TCPInfo?.RTT) rtts.push(m.TCPInfo.RTT / 1000);
          if (m.TCPInfo?.MinRTT) minRtt = m.TCPInfo.MinRTT / 1000;
        } catch {
          /* not a measurement */
        }
      } else {
        bytes += (ev.data as ArrayBuffer).byteLength;
      }
      const now = performance.now();
      if (now - last > 250) {
        last = now;
        report((bytes * 8) / ((now - start) / 1000) / 1e6, rtts, minRtt, now - start);
      }
    };
    ws.onerror = () => reject(new Error("download failed"));
    ws.onclose = () => {
      clearTimeout(guard);
      if (start) finish();
      else reject(new Error("download failed"));
    };
  });
}

function upload(url: string, report: (mbps: number, rtts: number[], elapsed: number) => void): Promise<{ mbps: number; rtts: number[] }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url, PROTOCOL);
    let start = 0;
    let sent = 0;
    let serverBytes = 0;
    let serverElapsedUs = 0;
    let size = 1 << 13;
    const rtts: number[] = [];
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      const clientSecs = (performance.now() - start) / 1000;
      // Prefer the server's count: the browser counts bytes still in its buffers.
      const mbps = serverBytes && serverElapsedUs ? (serverBytes * 8) / (serverElapsedUs / 1e6) / 1e6 : (sent * 8) / Math.max(0.001, clientSecs) / 1e6;
      resolve({ mbps, rtts });
    };
    const pump = () => {
      if (ws.readyState !== WebSocket.OPEN) return;
      const elapsed = performance.now() - start;
      if (elapsed >= PHASE_MS) {
        ws.close();
        return;
      }
      while (ws.bufferedAmount < 7 * size) {
        ws.send(new Uint8Array(size));
        sent += size;
        if (size < 1 << 20 && sent >= size * 16) size *= 2;
      }
      const mbps = serverBytes && serverElapsedUs ? (serverBytes * 8) / (serverElapsedUs / 1e6) / 1e6 : ((sent - ws.bufferedAmount) * 8) / Math.max(0.001, elapsed / 1000) / 1e6;
      report(mbps, rtts, elapsed);
      setTimeout(pump, 30);
    };
    ws.onopen = () => {
      start = performance.now();
      pump();
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data !== "string") return;
      try {
        const m = JSON.parse(ev.data) as { TCPInfo?: TcpInfo };
        if (m.TCPInfo?.RTT) rtts.push(m.TCPInfo.RTT / 1000);
        if (m.TCPInfo?.BytesReceived) serverBytes = m.TCPInfo.BytesReceived;
        if (m.TCPInfo?.ElapsedTime) serverElapsedUs = m.TCPInfo.ElapsedTime;
      } catch {
        /* ignore */
      }
    };
    ws.onerror = () => reject(new Error("upload failed"));
    ws.onclose = () => (start ? finish() : reject(new Error("upload failed")));
  });
}

export async function runTest(onProgress: (p: Progress) => void): Promise<Result> {
  onProgress({ phase: "locate", progress: 0.02 });
  const loc = await locate();
  let idle: number | null = null;
  const down = await download(loc.down, (mbps, rtts, minRtt, elapsed) => {
    idle = minRtt;
    onProgress({ phase: "download", progress: 0.05 + 0.45 * Math.min(1, elapsed / PHASE_MS), downloadMbps: mbps, idleMs: minRtt ?? undefined, loadedDownMs: median(rtts) ?? undefined });
  });
  idle = down.minRtt ?? idle;
  const up = await upload(loc.up, (mbps, rtts, elapsed) => {
    onProgress({
      phase: "upload",
      progress: 0.5 + 0.48 * Math.min(1, elapsed / PHASE_MS),
      downloadMbps: down.mbps,
      uploadMbps: mbps,
      idleMs: idle ?? undefined,
      loadedDownMs: median(down.rtts) ?? undefined,
      loadedUpMs: median(rtts) ?? undefined,
    });
  });
  const result: Result = {
    server: loc.server,
    downloadMbps: down.mbps,
    uploadMbps: up.mbps,
    idleMs: idle,
    loadedDownMs: median(down.rtts),
    loadedUpMs: median(up.rtts),
  };
  onProgress({ phase: "done", progress: 1, downloadMbps: result.downloadMbps, uploadMbps: result.uploadMbps, idleMs: idle ?? undefined, loadedDownMs: result.loadedDownMs ?? undefined, loadedUpMs: result.loadedUpMs ?? undefined });
  return result;
}

/** Simulated run for demos and screenshots (?demo in the URL). */
export async function runDemo(onProgress: (p: Progress) => void): Promise<Result> {
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  onProgress({ phase: "locate", progress: 0.02 });
  await sleep(400);
  const steps = 24;
  for (let i = 1; i <= steps; i++) {
    await sleep(120);
    const ramp = 1 - Math.exp((-i / steps) * 4);
    onProgress({ phase: "download", progress: 0.05 + (0.45 * i) / steps, downloadMbps: 286 * ramp * (0.95 + Math.random() * 0.08), idleMs: 12, loadedDownMs: 12 + 84 * ramp });
  }
  for (let i = 1; i <= steps; i++) {
    await sleep(120);
    const ramp = 1 - Math.exp((-i / steps) * 4);
    onProgress({ phase: "upload", progress: 0.5 + (0.48 * i) / steps, downloadMbps: 284, uploadMbps: 37.5 * ramp, idleMs: 12, loadedDownMs: 96, loadedUpMs: 12 + 200 * ramp });
  }
  const r: Result = { server: "Warsaw, PL (demo)", downloadMbps: 284, uploadMbps: 37.2, idleMs: 12, loadedDownMs: 96, loadedUpMs: 212 };
  onProgress({ phase: "done", progress: 1, downloadMbps: r.downloadMbps, uploadMbps: r.uploadMbps, idleMs: 12, loadedDownMs: 96, loadedUpMs: 212 });
  return r;
}
