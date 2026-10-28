import type { Key } from "../i18n";

/** Maps a throughput test error code (WhyLag peer or iperf3) to a message. */
export function peerErrorKey(code: string | undefined): Key {
  if (code === "peerCode") return "perf.peer.needCode";
  if (code === "peerRefused") return "perf.peer.wrongCode";
  if (code === "iperfBusy") return "perf.peer.iperfBusy";
  if (code === "unresolved") return "perf.peer.unresolved";
  if (code?.startsWith("iperf")) return "perf.peer.iperfFailed";
  return "perf.peer.unreachable";
}
