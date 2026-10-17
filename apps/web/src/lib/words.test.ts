import { expect, test } from "bun:test";
import { hiddenSummary, languageName, reasonText, statusText } from "./words";

test("reasons read as plain sentences", () => {
  expect(reasonText("readyNow", { kind: "ready", service: "Real-Debrid" })).toBe("Ready now, cached on Real-Debrid");
  expect(reasonText("readyNow", { kind: "direct" })).toBe("Ready now");
  expect(reasonText("smallerFaster", { kind: "direct" })).toBe("Smaller and faster to start");
});

test("status always has a word", () => {
  expect(statusText({ kind: "torrent", seeds: 45 })).toEqual({ tone: "warn", text: "Torrent, 45 seeds" });
  expect(statusText({ kind: "torrent", seeds: null }).text).toBe("Torrent");
  expect(statusText({ kind: "ready", service: null }).text).toBe("Ready");
});

test("hidden summary counts each reason", () => {
  expect(hiddenSummary({ count: 20, byReason: [["wrongTitle", 19], ["wrongEpisode", 1]] })).toBe(
    "19 are for other titles, 1 is another episode",
  );
});

test("language codes become names", () => {
  expect(languageName("fr")).toBe("French");
  expect(languageName("eng")).toBe("English");
  expect(languageName("xx-nope")).toBe("XX-NOPE");
});
