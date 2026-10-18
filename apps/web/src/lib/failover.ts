import type { PlaybackSnapshot } from "../core/generated/PlaybackSnapshot";

export type FailOverCommand = { type: "failOver"; error: string; attempt?: number };

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
    /** The failOver command for an error of the loaded source. */
    failOver(error: string): FailOverCommand {
      return attempt == null ? { type: "failOver", error } : { type: "failOver", error, attempt };
    },
  };
}
