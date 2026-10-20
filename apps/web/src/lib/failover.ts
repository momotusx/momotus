import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";

/** What failed, as `failOver` reports it; the core words it for the attempt list. */
export type FailureKind = "http" | "network" | "timeout" | "unreadable" | "decode" | "audio" | "other";

/** A player failure: what failed, the HTTP status when there was one, and the
 * player's own words, which the core keeps only for `other`. */
export type PlayerFailure = { kind: FailureKind; status?: number; error: string };

export type FailOverCommand = { type: "failOver"; error: string; kind: FailureKind; status?: number; attempt?: number };

// MediaError's codes (the global does not exist outside a browser).
const MEDIA_ERR_NETWORK = 2;
const MEDIA_ERR_DECODE = 3;
const MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

/** Sorts a `<video>` element's error into what failed. Only a decoder that
 * could not decode this file is `decode`, which the core can keep against
 * the version (plans/core-protocol.md); a source the element could not
 * open at all is `unreadable`. */
export function mediaFailure(error: { code: number; message: string } | null): PlayerFailure {
  const said = error?.message || "player error";
  switch (error?.code) {
    case MEDIA_ERR_DECODE:
      return { kind: "decode", error: said };
    case MEDIA_ERR_SRC_NOT_SUPPORTED:
      return { kind: "unreadable", error: said };
    case MEDIA_ERR_NETWORK:
      return { kind: "network", error: said };
    default:
      return { kind: "other", error: said };
  }
}

/** A `<video>` error before the file's metadata loaded. The element calls a
 * failed fetch (a loopback 404, an upstream 5xx, a refused answer) a format
 * error too, so the source is asked once: an error answer or none is the
 * source's failure (`http`, `network`, `timeout`); bytes that came back and
 * still did not open are `unreadable` (`mediaFailure`). Never `decode`
 * before the metadata: no decoder has seen the file yet. */
export async function earlyFailure(
  url: string,
  error: { code: number; message: string } | null,
  get: (url: string, init: RequestInit) => Promise<Response> = fetch,
): Promise<PlayerFailure> {
  try {
    const answer = await get(url, { headers: { Range: "bytes=0-0" }, signal: AbortSignal.timeout(5000) });
    void answer.body?.cancel().catch(() => undefined);
    if (!answer.ok) return { kind: "http", status: answer.status, error: answer.statusText || `HTTP ${answer.status}` };
  } catch (e) {
    const timedOut = e instanceof DOMException && e.name === "TimeoutError";
    return { kind: timedOut ? "timeout" : "network", error: String(e) };
  }
  return mediaFailure(error);
}

/** Remembers the core's attempt number of the source the engine loaded, so a
 * failover names that source even after the snapshot moved on: the core then
 * ignores a late error from a source it already gave up on. With no attempt
 * known, the command names none and the core acts on the current source. */
export function loadedSource(current: () => PlaybackSnapshot | null) {
  let attempt: number | null = null;
  return {
    /** The engine loads the current snapshot's url. */
    load() {
      attempt = current()?.attempt ?? null;
    },
    /** The failOver command for a failure of the loaded source. */
    failOver(failure: PlayerFailure): FailOverCommand {
      const command: FailOverCommand = { type: "failOver", error: failure.error, kind: failure.kind };
      if (failure.status != null) command.status = failure.status;
      if (attempt != null) command.attempt = attempt;
      return command;
    },
  };
}

/** A start position the `<video>` element has not applied yet: until a new
 * source's metadata arrives the element says 0, so the playhead is the start
 * (a failure before then resumes the next source there, and no progress
 * report says 0). */
export function pendingStart() {
  let pending: number | null = null;
  return {
    /** A new source starts at `sec` once its metadata is in. */
    set(sec: number) {
      pending = sec > 0 ? sec : null;
    },
    /** The start to apply now that the metadata is in, once. */
    take(): number | null {
      const at = pending;
      pending = null;
      return at;
    },
    /** A viewer's seek before the metadata: it becomes the start, so the
     * metadata does not undo it. False when no start is pending. */
    retarget(sec: number): boolean {
      if (pending == null) return false;
      pending = Math.max(0, sec);
      return true;
    },
    /** The playhead, given the element's `currentTime`. */
    position(current: number): number {
      return pending ?? current;
    },
  };
}

/** Where each source starts. The first at the saved position; after a
 * failover, where the failed one stopped: the core passes through a moment
 * with no source, so the next one cannot tell it from a first start. */
export function sourceStart() {
  let stoppedAt: number | null = null;
  return {
    /** The playing source went away at `at` seconds. */
    stop(at: number) {
      stoppedAt = at;
    },
    /** Where the next source starts, given the saved position in seconds. */
    next(savedSec: number): number {
      const at = stoppedAt ?? savedSec;
      stoppedAt = null;
      return at;
    },
  };
}
