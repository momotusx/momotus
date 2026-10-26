/** D-pad check on the emulator: open Home on the TV tree, move through the lanes, enter a title,
 * press Back, and require focus to come back on the same tile. Runs several times with different
 * moves and appends one JSON line per run to bench/m6/focus-restore.jsonl.
 *
 * Needs the debug app installed and the pinned emulator running (scripts/emu.sh), with Cinemeta
 * installed in the app. Run: bun scripts/focus-restore.ts [runs=20] */

import { appendFileSync } from "node:fs";
import { join } from "node:path";

const sdk = join(import.meta.dir, "sdk.sh");
const out = join(import.meta.dir, "../../../bench/m6/focus-restore.jsonl");
const runs = Number(process.argv[2] ?? 20);

const adb = (...args: string[]) => Bun.spawnSync(["bash", sdk, "adb", ...args]).stdout.toString();
const key = (name: string) => adb("shell", "input", "keyevent", name);

function dump(): string {
  adb("shell", "uiautomator", "dump", "/sdcard/window.xml");
  return adb("exec-out", "cat", "/sdcard/window.xml");
}

/** The title read by the node that has input focus (its own description, or its first child's), or null. */
function focused(xml: string): string | null {
  const at = xml.search(/<node [^>]* focused="true"/);
  if (at < 0) return null;
  return /content-desc="([^"]+)"/.exec(xml.slice(at, at + 2_000))?.[1] ?? null;
}

async function until<T>(what: string, ms: number, probe: () => T | null | false): Promise<T> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const value = probe();
    if (value) return value;
    await Bun.sleep(300);
  }
  throw new Error(`timed out waiting for ${what}`);
}

const results: { ok: boolean }[] = [];
let skipped = 0;
for (let run = 1; run <= runs; run++) {
  adb("shell", "am", "force-stop", "app.momotus.android");
  adb("logcat", "-c");
  // Touch mode is a display-wide state that any tap or swipe turns on, and in it focus requests do
  // not land; a TV is never in touch mode. A navigation key leaves it, and goes to the launcher
  // here (Tab does nothing there), before the app starts.
  key("KEYCODE_TAB");
  adb("shell", "am", "start", "-n", "app.momotus.android/app.marquee.android.MainActivity", "--es", "form", "tv");
  const first = await until("Home focus", 40_000, () => focused(dump()));

  // From the hero (or the first Continue Watching card) down into the lanes, then right.
  const downs = 1 + (run % 3);
  // Even runs go well past the visible tiles, so the lane has scrolled when Back restores focus.
  const rights = 1 + (run % 4) + (run % 2 === 0 ? 7 : 0);
  for (let i = 0; i < downs; i++) {
    key("KEYCODE_DPAD_DOWN");
    await Bun.sleep(450);
  }
  for (let i = 0; i < rights; i++) {
    key("KEYCODE_DPAD_RIGHT");
    await Bun.sleep(450);
  }
  await Bun.sleep(600);
  const before = await until("a focused tile", 5_000, () => focused(dump()));

  key("KEYCODE_DPAD_CENTER");
  // The title page needs Cinemeta, which the emulator can fail to reach in time when the host is
  // loaded. That is the network, not focus: the run is recorded as skipped and tried again.
  const page = await until("the title page", 15_000, () => /(Add to Library|In Library|could not load)/.exec(dump())?.[1]);
  if (page === "could not load") {
    const line = { date: new Date().toISOString().slice(0, 10), run, skipped: "the title did not load (network)" };
    appendFileSync(out, `${JSON.stringify(line)}\n`);
    console.log("skip", JSON.stringify(line));
    skipped++;
    if (skipped > 5) throw new Error("too many title loads failed, the network is not usable");
    run--;
    continue;
  }
  adb("logcat", "-c");
  key("KEYCODE_BACK");
  const after = await until("focus back on Home", 8_000, () => focused(dump())).catch(() => null);

  const log = adb("logcat", "-d", "-s", "marquee:D");
  const took = /focus on \S+ after (\d+) ms, restore=true/.exec(log)?.[1];
  const ok = after === before;
  results.push({ ok });
  const line = { date: new Date().toISOString().slice(0, 10), run, first, downs, rights, before, after, ok, restoreMs: took ? Number(took) : null };
  appendFileSync(out, `${JSON.stringify(line)}\n`);
  console.log(ok ? "pass" : "FAIL", JSON.stringify(line));
}

const passed = results.filter((r) => r.ok).length;
console.log(`${passed} of ${runs} runs restored focus on the exact tile`);
process.exit(passed === runs ? 0 : 1);
