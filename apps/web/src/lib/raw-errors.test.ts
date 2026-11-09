/** The owner's rule on every app: a screen shows a failure's words (apps/shared/error-words.json), never
 * an exception's own text. Each app's sources are read for the ways that text reaches a screen; a log
 * line is fine, and the few places that keep the raw text for the log or the core are named. */

import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const APPS = new URL("../../../", import.meta.url).pathname;

const files = (dir: string, ext: string): string[] =>
  readdirSync(join(APPS, dir), { recursive: true, encoding: "utf8" })
    .filter((f) => f.endsWith(ext) && !/(^|\/)(build|test|generated)\//.test(f) && !/\.test\.tsx?$/.test(f))
    .map((f) => join(dir, f));

/** Every line of `sources` matching `raw`, but log lines and the files in `kept`. */
function hits(sources: string[], raw: RegExp, log: RegExp, kept: string[]): string[] {
  return sources
    .filter((f) => !kept.some((k) => f.endsWith(k)))
    .flatMap((f) =>
      readFileSync(join(APPS, f), "utf8")
        .split("\n")
        .flatMap((line, i) => (raw.test(line) && !log.test(line) ? [`${f}:${i + 1}: ${line.trim()}`] : [])),
    );
}

test("no web screen shows an error's own text", () => {
  // words.ts keeps the core's `said` sentence; client.ts keeps the raw text for Details; the sync
  // helpers read the core's own `pairing:` sentences; failover.ts hands the player's error to the core;
  // reports.ts hands an uncaught error to the core's error reports, whose uploader scrubs it.
  const kept = ["lib/words.ts", "core/client.ts", "core/sync.ts", "screens/sync.tsx", "lib/failover.ts", "lib/reports.ts"];
  expect(hits(files("web/src", ".ts").concat(files("web/src", ".tsx")), /\b(e|err|error)\.message\b|String\((e|err|error)\)/, /console\./, kept)).toEqual([]);
});

test("no Android screen shows an error's own text", () => {
  const sources = ["app", "core", "design", "player", "ui-phone", "ui-tv"].flatMap((m) => files(`android/${m}/src/main`, ".kt"));
  // ErrorCopy keeps the core's `said` sentence; SyncAccount reads the core's own `pairing:` sentences;
  // the others keep the raw text for the log (CoreLog.describe's own definition among them).
  const kept = ["ErrorCopy.kt", "client/Seams.kt", "sync/SyncAccount.kt", "ProfileGate.kt", "session/CoreSession.kt", "secrets/DeviceId.kt"];
  expect(hits(sources, /\b(e|it|error|t)\.message\b|CoreLog\.describe\(/, /CoreLog\.[dwe]\(|Log\.[dwei]\(/, kept)).toEqual([]);
});

test("no Apple screen shows an error's own text", () => {
  // CoreError.swift turns a failure into words; the player's errors go to the core (which logs them);
  // the sign-in sheets keep the system's text inside a failure that is shown in words.
  const kept = ["Core/CoreError.swift", "Core/PlaybackModels.swift", "Core/CoreClient.swift", "Player/AetherSession.swift", "Account/SignInSheets.swift"];
  expect(hits(files("apple/Marquee", ".swift"), /localizedDescription/, /coreLog\.|DebugHooks\./, kept)).toEqual([]);
});
