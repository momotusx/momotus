/** A download op the core refused says so in one line (plans/offline-downloads.md 3.3), never silence:
 * the line shows over the page until the next op goes through or a few seconds pass. */

import { createSignal } from "solid-js";

const [problem, setProblem] = createSignal<string | null>(null);
export { problem as downloadProblem };

let clear: ReturnType<typeof setTimeout> | undefined;

export const downloadWordsFailed = {
  add: "Couldn't start this download. Try again.",
  change: "Couldn't change this download. Try again.",
};

/** Runs `op`; a refusal shows `words` and is logged with its reason. */
export async function reported(op: Promise<unknown>, words: string): Promise<void> {
  try {
    await op;
    setProblem(null);
  } catch (e) {
    // Only the code: the core's text can name the title (Incognito downloads go through here too).
    console.warn("download op failed", (e as { code?: unknown } | null)?.code ?? "");
    setProblem(words);
    clearTimeout(clear);
    clear = setTimeout(() => setProblem(null), 6000);
  }
}
