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

import shared from "../../../shared/stream-choice-words.json";
import { blockedWords, choiceDefaults, choiceSummary, choiceWords, screenClass, conditionsHeader, conditionsLine, counted, learnedNote, lineWords, type ReasonLine } from "./words";

const GiB = 1024 ** 3;

test("the web renders the shared stream choice words as they are", () => {
  expect(choiceWords).toEqual(shared);
  for (const [kind, w] of Object.entries(shared.lines)) {
    const line = { kind, seeders: 3, year: 1999, service: "TorBox", sizeBytes: GiB, limitBytes: 5 * GiB, needMbps: 30, haveMbps: 12, samples: 0, quality: "cam" } as unknown as ReasonLine;
    expect(lineWords(line)).toMatchObject({ tone: w.tone, text: w.text });
  }
});

test("every line has its tone, its plain text and its numbers in the details", () => {
  expect(lineWords({ kind: "copy", quality: "telesync" })).toEqual({ tone: "bad", text: "Recorded in a cinema, low quality", details: "Telesync copy" });
  expect(lineWords({ kind: "fetch", service: "Real-Debrid" })).toEqual({ tone: "bad", text: "Not ready yet, has to download first", details: "Real-Debrid does not have it yet" });
  expect(lineWords({ kind: "veryFew", seeders: 1 }).details).toBe("1 seeder");
  expect(lineWords({ kind: "veryFew", seeders: 3 })).toEqual({ tone: "bad", text: "May not start", details: "3 seeders" });
  expect(lineWords({ kind: "otherYear", year: 2004 })).toEqual({ tone: "warn", text: "May be a different film or show", details: "The release says 2004" });
  expect(lineWords({ kind: "washedOut" })).toEqual({ tone: "warn", text: "Colors may look washed out on this screen", details: "HDR file, screen without HDR" });
  expect(lineWords({ kind: "few", seeders: 31 })).toEqual({ tone: "warn", text: "May start slowly", details: "31 seeders" });
  expect(lineWords({ kind: "ready", service: "TorBox", sizeBytes: 7.4 * GiB })).toEqual({ tone: "ok", text: "Starts right away", details: "Ready on TorBox, 7.4 GB" });
  expect(lineWords({ kind: "quick", seeders: 1204, sizeBytes: 2.2 * GiB })).toEqual({ tone: "ok", text: "Starts quickly", details: "1,204 seeders, 2.2 GB" });
  expect(lineWords({ kind: "should", seeders: 89, sizeBytes: 4 * GiB, service: "Real-Debrid" })).toEqual({
    tone: "ok",
    text: "Should start quickly",
    details: "89 seeders, 4 GB, may be ready on Real-Debrid",
  });
});

test("a detail with an unknown number is left out, never shown with a gap", () => {
  expect(lineWords({ kind: "should", seeders: null, sizeBytes: null, service: null }).details).toBe("");
  expect(lineWords({ kind: "quick", seeders: null, sizeBytes: 3 * GiB }).details).toBe("3 GB");
  expect(lineWords({ kind: "ready", service: null, sizeBytes: 3 * GiB }).details).toBe("3 GB");
  expect(lineWords({ kind: "fetch", service: null }).details).toBe("");
});

test("a large file says whether the size was the user's choice", () => {
  const large: ReasonLine = { kind: "large", sizeBytes: 7.4 * GiB, limitBytes: 5 * GiB };
  expect(lineWords(large)).toEqual({ tone: "warn", text: "Large file", details: "7.4 GB; Marquee prefers files up to 5 GB" });
  expect(lineWords(large, true).text).toBe("Larger than the size you chose");
});

test("may pause says where its speed comes from", () => {
  expect(lineWords({ kind: "mayPause", needMbps: 31.6, haveMbps: 17.5, samples: 0 }).details).toBe("Needs about 32 Mbit/s; this connection gives about 18 (estimated)");
  expect(lineWords({ kind: "mayPause", needMbps: 79, haveMbps: 25, samples: 1 }).details).toBe("Needs about 79 Mbit/s; this connection gives about 25 (measured from 1 playback)");
  expect(lineWords({ kind: "mayPause", needMbps: 79, haveMbps: 25, samples: 6 }).details).toContain("(measured from 6 playbacks)");
});

test("the blocked state counts the versions it found", () => {
  expect(blockedWords("needsAccount", 1).text).toStartWith("Marquee found 1 version. This version of the app");
  expect(blockedWords("nothingPlaysHere", 42)).toEqual({ text: "Found 42 versions, but none plays on this device.", action: "Show them" });
  expect(counted("versions", 2000)).toBe("2,000 versions");
});

test("the device line reads as one sentence, leaving out what is not known", () => {
  const tv = { screenClass: 2160, hdr: true, network: "wifi", speedMbps: 42.2, speedMeasured: true, freeBytes: 61 * GiB };
  expect(conditionsLine(tv)).toBe("4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free");
  expect(conditionsHeader("tv", tv)).toBe("This TV: 4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free");
  expect(conditionsLine({ screenClass: 1080, hdr: false, network: "cellular", speedMbps: 15, speedMeasured: false, freeBytes: null })).toBe("1080p, mobile data, speed not measured yet");
  expect(conditionsHeader("desktop", { screenClass: 480, hdr: false, network: "weird", speedMbps: null, speedMeasured: false, freeBytes: null })).toBe(
    "This computer: SD, this network, speed not measured yet",
  );
});

test("a learned note names the device and the new limit", () => {
  expect(learnedNote("size", true, "tv", { limit: 10 * GiB })).toBe("On this TV you often picked bigger files, so Marquee now picks files up to 10 GB.");
  expect(learnedNote("height", false, "phone", { height: 720 })).toBe("On this phone you often picked lower resolutions, so Marquee now picks up to 720p.");
  expect(learnedNote("hdr", true, "desktop")).toBe("On this computer you often picked HDR versions, so Marquee now prefers HDR.");
  expect(learnedNote("size", true, "tv")).toBeNull();
});

test("the onboarding patches are the plan's, and Skip is Start fast", () => {
  expect(shared.question.fast.patch).toEqual({ sizeLimitBytes: 5 * GiB, minSeeders: 50 });
  expect(shared.question.best.patch).toEqual({ sizeLimitBytes: null, minSeeders: 20 });
  expect(shared.question.skip).toEqual({ label: "Skip", means: "fast" });
  expect(choiceDefaults).toEqual({ ...shared.question.fast.patch, hdr: true, preferReady: true, learn: true });
});

test("the Settings row sums up the size and the seeders as the chips say them", () => {
  expect(choiceSummary({})).toBe("Up to 5 GB, shared by at least 50 people");
  expect(choiceSummary({ sizeLimitBytes: null, minSeeders: 20 })).toBe("Any size, shared by at least 20 people");
  expect(choiceSummary({ sizeLimitBytes: 2.5 * GiB, minSeeders: 35 })).toBe("Up to 2.5 GB, shared by at least 35 people");
});

test("a display's lines fall into the core's screen classes", () => {
  expect([2160, 2000, 1440, 1179, 1000, 720, 700, 576].map(screenClass)).toEqual([2160, 2160, 1080, 1080, 1080, 720, 720, 480]);
});
