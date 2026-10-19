import { expect, test } from "bun:test";
import { hiddenSummary, languageName, pairingProblem, plainAddress, reasonText, serverHost, statusText, subtitleNote } from "./words";

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

test("pairing keeps the core's own reasons and never shows an address", () => {
  expect(pairingProblem("pairing: the code is for another sync server")).toBe("The code is for another sync server.");
  expect(pairingProblem("the relay must use https unless it is on this network")).toBe("The sync server address must start with https.");
  for (const raw of ["network: error sending request (https://user:secret@relay.example/<redacted>)", "invalid url: https://user:secret@relay.example", "http status 502"]) {
    const words = pairingProblem(raw);
    expect(words.includes("relay.example") || words.includes("secret")).toBe(false);
  }
  expect(serverHost("https://user:secret@relay.example/path?q=1")).toBe("relay.example");
  expect(plainAddress("https://relay.example")).toBe(true);
  expect(plainAddress("https://user:secret@relay.example")).toBe(false);
  expect(plainAddress("not an address")).toBe(false);
});

test("a subtitle note names its source and what to do", () => {
  const say = (problem: Parameters<typeof subtitleNote>[0]["problem"]) => subtitleNote({ source: "OpenSubtitles v3", problem });
  expect(say("unreachable")).toBe("OpenSubtitles v3 did not answer. Try its subtitles again in a moment.");
  expect(say("blocked")).toBe("OpenSubtitles v3 is turning requests away. Marquee will try again in a few minutes.");
  expect(say("changed")).toBe("OpenSubtitles v3 changed its pages, so Marquee cannot read them. An update will fix this.");
  expect(say("trackFailed")).toBe("A subtitle file from OpenSubtitles v3 could not be read. Pick another one.");
});
