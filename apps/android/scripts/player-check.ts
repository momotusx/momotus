/** Player check on the emulator (TV tree): open the Sintel title, press Play, and watch the
 * real trailer play through the loopback URL. Measures the time to the first frame, and checks
 * that progress reaches the core while playing and on pause, that a seek lands where it should,
 * that leaving the app pauses, and that a restart shows Continue Watching with progress.
 * Appends one JSON line per run to bench/m6/player.jsonl.
 *
 * Needs the debug app installed, the pinned emulator running (scripts/emu.sh), the local test
 * add-on up (bun scripts/sintel-addon.ts, adb reverse tcp:7780 tcp:7780) and installed in the app.
 * Run: bun scripts/player-check.ts */

import { appendFileSync } from "node:fs";
import { join } from "node:path";

const sdk = join(import.meta.dir, "sdk.sh");
const out = join(import.meta.dir, "../../../bench/m6/player.jsonl");

const adb = (...args: string[]) => Bun.spawnSync(["bash", sdk, "adb", ...args]).stdout.toString();
const key = (name: string) => adb("shell", "input", "keyevent", name);
const dump = () => {
  adb("shell", "uiautomator", "dump", "/sdcard/window.xml");
  return adb("exec-out", "cat", "/sdcard/window.xml");
};
const log = () => adb("logcat", "-d", "-s", "marquee:D");

async function until<T>(what: string, ms: number, probe: () => T | null | false | undefined): Promise<T> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const value = probe();
    if (value) return value;
    await Bun.sleep(400);
  }
  throw new Error(`timed out waiting for ${what}`);
}

const progressLines = (text: string) =>
  [...text.matchAll(/progress (\d+) of (\d+) ms/g)].map((m) => ({ at: Number(m[1]), of: Number(m[2]) }));

const result: Record<string, unknown> = { date: new Date().toISOString().slice(0, 10), form: "tv" };
const check = (name: string, ok: boolean, detail?: unknown) => {
  result[name] = detail === undefined ? ok : { ok, detail };
  console.log(ok ? "ok  " : "FAIL", name, detail ?? "");
  if (!ok) result.failed = [...((result.failed as string[]) ?? []), name];
};

adb("shell", "am", "force-stop", "app.marquee.android");
adb("logcat", "-c");
key("KEYCODE_TAB");
adb("shell", "am", "start", "-n", "app.marquee.android/.MainActivity", "--es", "form", "tv", "--es", "title", "movie:tt1727587");
await until("the title page with its version line", 60_000, () => /1080p, 15 MB/.test(dump()));
// A run that reached the end leaves the title marked watched, which keeps it out of Continue
// Watching: turn the mark off first (Resume, Versions, Library, Watched are the four buttons).
if (/text="Watched"/.test(dump())) {
  for (let i = 0; i < 3; i++) {
    key("KEYCODE_DPAD_RIGHT");
    await Bun.sleep(400);
  }
  key("KEYCODE_DPAD_CENTER");
  await until("the mark turned off", 20_000, () => /text="Mark as watched"/.test(dump()));
  for (let i = 0; i < 3; i++) {
    key("KEYCODE_DPAD_LEFT");
    await Bun.sleep(400);
  }
}
// On a loaded host the UI can stall for many seconds and swallow the press: look at the title
// page again and press again until playback starts.
let first: { ms: number; line: string } | undefined;
for (let attempt = 1; attempt <= 3 && !first; attempt++) {
  key("KEYCODE_DPAD_CENTER");
  first = await until("the first frame", 45_000, () => /first frame versions .* first frame (\d+) ms/.exec(log()))
    .then((m) => ({ ms: Number(m[1]), line: m[0] }))
    .catch(() => undefined);
  if (!first) console.log(`no first frame after press ${attempt}`);
}
if (!first) throw new Error("playback did not start");
result.firstFrameMs = first.ms;
console.log("first frame", first.line);

// A title that was played before starts at its saved position: go back to the start first, so
// every run measures the same thing and a seek never crosses the finished mark.
for (let i = 0; i < 6; i++) {
  key("KEYCODE_MEDIA_REWIND");
  await Bun.sleep(300);
}
await Bun.sleep(1_500);
const restart = progressLines(log()).length;

// Playing: a report about every 5 s, and the position moves on.
await Bun.sleep(12_000);
const playing = progressLines(log()).slice(restart);
check("progress while playing", playing.length >= 2 && playing.at(-1)!.at > 5_000 && playing[0]!.of > 0, playing.slice(0, 3));
const durationMs = playing[0]?.of ?? 0;

// Pause, then seek forward 10 s with the media key while paused: the report after the seek settles
// is where it landed, so it is 10 s past the report that the pause made.
key("KEYCODE_MEDIA_PAUSE");
await Bun.sleep(1_500);
const paused = progressLines(log()).at(-1)!;
key("KEYCODE_MEDIA_FAST_FORWARD");
await Bun.sleep(2_500);
const seeked = progressLines(log()).at(-1)!;
check("a 10 s seek is reported where it landed", seeked.at - paused.at >= 9_500 && seeked.at - paused.at <= 10_500, {
  before: paused.at,
  after: seeked.at,
});
const reports = progressLines(log()).length;
await Bun.sleep(7_000);
check("no reports while paused", progressLines(log()).length === reports);

// Leaving the app: coming back finds the same player (the first key brings the controls back).
key("KEYCODE_HOME");
await Bun.sleep(2_000);
adb("shell", "am", "start", "-n", "app.marquee.android/.MainActivity");
await Bun.sleep(3_000);
key("KEYCODE_DPAD_DOWN");
await Bun.sleep(1_500);
check("the app is back on the player after Home", /content-desc="Back 10 seconds"/.test(dump()));

// Back leaves the player for the title page, which offers to resume where it stopped.
// (A press that the first screen after Home ignores is pressed again, at most every 8 s, so a slow
// title page is never left by a second press.)
let backs = 0;
let lastBack = 0;
const resumeText = await until("Resume on the title page", 90_000, () => {
  const screen = dump();
  const resume = /Resume( from 0:\d\d| · \d+ min left)/.exec(screen)?.[0];
  if (resume) return resume;
  if (/content-desc="Back 10 seconds"/.test(screen) && backs < 4 && Date.now() - lastBack > 8_000) {
    backs++;
    lastBack = Date.now();
    key("KEYCODE_BACK");
  }
  return undefined;
});
check("the title page offers to resume", true, { text: resumeText, backPresses: backs });

// Resume from the middle: Play again from the saved position, which the stream server must serve
// from a seeked start. The first frame is measured the same way as the first run.
adb("logcat", "-c");
let middle: { ms: number; line: string } | undefined;
for (let attempt = 1; attempt <= 3 && !middle; attempt++) {
  key("KEYCODE_DPAD_CENTER");
  middle = await until("the first frame of the resumed run", 45_000, () => /first frame versions .* first frame (\d+) ms/.exec(log()))
    .then((m) => ({ ms: Number(m[1]), line: m[0] }))
    .catch(() => undefined);
  if (!middle) console.log(`no resumed first frame after press ${attempt}`);
}
check("a resume from the middle reaches its first frame", !!middle, middle?.line);
if (middle) result.resumeFirstFrameMs = middle.ms;
await Bun.sleep(3_000);
key("KEYCODE_BACK");
await until("the title page again", 30_000, () => /Resume( from 0:\d\d| · \d+ min left)/.test(dump()));

// Kill and relaunch: Continue Watching leads Home.
adb("shell", "am", "force-stop", "app.marquee.android");
key("KEYCODE_TAB");
adb("shell", "am", "start", "-n", "app.marquee.android/.MainActivity", "--es", "form", "tv");
const home = await until("Continue Watching on Home", 60_000, () => /Continue Watching/.test(dump()) && dump());
check("Continue Watching shows after a restart", /Sintel/.test(home), { durationMs });

appendFileSync(out, `${JSON.stringify(result)}\n`);
process.exit(result.failed ? 1 : 0);
