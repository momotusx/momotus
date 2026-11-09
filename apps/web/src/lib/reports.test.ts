/** Error reports from the page (plans/error-reports.md 6.5, 10): only our own uncaught errors, never a
 * failed op, frames reduced to file and line, and the Settings words every app shares. */

import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { BridgeError } from "../core/bridge";
import { browserWords, jsReport, ownFrame, reportsWanted } from "./reports";
import words from "../../../shared/report-words.json";

const ORIGIN = "http://127.0.0.1:5173";

test("a frame of ours is reduced to file and line; others are not ours", () => {
  expect(ownFrame(`    at render (${ORIGIN}/assets/index-B3xQ9kLm.js:1:2345)`, ORIGIN)).toBe("index-B3xQ9kLm.js:1:2345");
  expect(ownFrame(`render@${ORIGIN}/src/screens/home.tsx?t=1:20:5`, ORIGIN)).toBe("home.tsx:20:5");
  expect(ownFrame("    at x (chrome-extension://abc/content.js:1:1)", ORIGIN)).toBeNull();
  expect(ownFrame("    at x (https://other.example/a.js:1:1)", ORIGIN)).toBeNull();
});

test("an uncaught error of ours becomes a js report with raw text and our frames only", () => {
  const e = new TypeError("x is undefined");
  e.stack = `TypeError: x is undefined\n    at a (${ORIGIN}/assets/index-B3xQ9kLm.js:1:2)\n    at b (chrome-extension://abc/c.js:3:4)`;
  const r = jsReport(e, ORIGIN, 7);
  expect(r).toEqual({ kind: "js", at: 7, message: "TypeError: x is undefined", stack: ["index-B3xQ9kLm.js:1:2"] });
});

test("failed ops, extensions and frameless errors are never reported", () => {
  // A failed op thrown from our own code: still not a report (section 2).
  const failedOp = new BridgeError("network");
  failedOp.stack = `Error: network\n    at op (${ORIGIN}/assets/index-B3xQ9kLm.js:1:2)`;
  expect(jsReport(failedOp, ORIGIN)).toBeNull();
  const extension = new Error("from an extension");
  extension.stack = "Error: from an extension\n    at x (chrome-extension://abc/c.js:1:1)";
  expect(jsReport(extension, ORIGIN)).toBeNull();
  expect(jsReport("Script error.", ORIGIN)).toBeNull();
});

test("the browser is named by family and major version only", () => {
  const chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36";
  expect(browserWords(chrome)).toEqual({ os: "macOS", model: "Chrome 132" });
  expect(browserWords("Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0")).toEqual({ os: "Linux", model: "Firefox 133" });
  expect(browserWords("curl/8")).toEqual({ os: "web", model: "Browser" });
});

test("on by default in a release build; development and webOS send nothing unless asked", () => {
  expect(reportsWanted({ DEV: false }, false)).toBe(true);
  expect(reportsWanted({ DEV: true }, false)).toBe(false);
  expect(reportsWanted({ DEV: true, VITE_MARQUEE_REPORTS: "1" }, false)).toBe(true);
  expect(reportsWanted({ DEV: false }, true)).toBe(false);
});

test("the shared words are the ones the plan gives", () => {
  expect(words.label).toBe("Error reports");
  expect([words.on, words.off]).toEqual(["On", "Off"]);
  expect(words.note).toContain("Turning this off deletes reports not sent yet.");
  expect(words.note).not.toMatch(/—/);
  expect(words.privacy).toBe("Privacy");
  expect(words.privacyPath).toBe("/privacy");
});

test("Settings shows the switch outside webOS, and About the privacy page", () => {
  const settings = readFileSync(new URL("../screens/settings.tsx", import.meta.url), "utf8");
  expect(settings).toContain("<ReportsRow />");
  expect(settings).toMatch(/<Show when=\{!isWebOS\(\)\}>\s*<ReportsRow \/>/);
  expect(settings).toContain("<PrivacyLink url={url()} />");
});
