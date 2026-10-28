import type { Key } from "../i18n";

/** Maps a peer test error code to a message for the user. */
export function peerErrorKey(code: string | undefined): Key {
  if (code === "peerCode") return "perf.peer.needCode";
  if (code === "peerRefused") return "perf.peer.wrongCode";
  return "perf.peer.unreachable";
}
