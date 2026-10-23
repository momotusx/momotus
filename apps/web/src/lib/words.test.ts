import { expect, test } from "bun:test";
import { addonHost, nextVersion, languageName, pairingProblem, plainAddress, problemWords, restartLine, serverHost, statusText, subtitleNote } from "./words";
import shared from "../../../shared/stream-choice-words.json";
import type { EmptyAction } from "../core/generated/EmptyAction";
import type { EmptyKind } from "../core/generated/EmptyKind";
import type { Learned } from "../core/generated/Learned";
import type { ReasonLine } from "../core/generated/ReasonLine";
import type { VersionsConditions } from "../core/generated/VersionsConditions";
import {
  anyLearned,
  emptyWords,
  choiceSummary,
  choiceWords,
  conditionsLine,
  counted,
  learnedNote,
  learnedRows,
  lineWords,
  screenClass,
  speedFor,
} from "./words";

test("status always has a word", () => {
  expect(statusText({ kind: "ready", service: "TorBox" }).text).toBe("Ready");
  expect(statusText({ kind: "needsDownload", service: "TorBox" }).text).toBe("Needs download");
  expect(statusText({ kind: "torrent", seeds: 45 })).toEqual({ tone: "warn", text: "Torrent, 45 seeds" });
  expect(statusText({ kind: "torrent", seeds: null }).text).toBe("Torrent");
  expect(statusText({ kind: "ready", service: null }).text).toBe("Ready");
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

const GiB = 1024 ** 3;

test("the web renders the shared stream choice words as they are", () => {
  expect(choiceWords).toEqual(shared);
  for (const [kind, w] of Object.entries(shared.lines)) {
    const line = { kind, seeders: 3, year: 1999, service: "TorBox", sizeBytes: GiB, limitBytes: 5 * GiB, chosen: false, needMbps: 30, haveMbps: 12, samples: 0, quality: "cam", aheadMin: 7, budgetBytes: 2.5 * GiB } as unknown as ReasonLine;
    expect(lineWords(line)).toMatchObject({ tone: w.tone, text: w.text });
    // The viewer's streaming service is never named, as no addon is.
    expect(lineWords(line).details).not.toContain("TorBox");
  }
});

test("every line has its tone, its plain text and its numbers in the details", () => {
  expect(lineWords({ kind: "copy", quality: "telesync" })).toEqual({ tone: "bad", text: "Recorded in a cinema, low quality", details: "Telesync copy" });
  expect(lineWords({ kind: "fetch", service: "Real-Debrid" })).toEqual({ tone: "bad", text: "Not ready yet, has to download first", details: "Your account does not have it yet" });
  expect(lineWords({ kind: "veryFew", seeders: 1 }).details).toBe("1 seeder");
  expect(lineWords({ kind: "veryFew", seeders: 3 })).toEqual({ tone: "bad", text: "May not start", details: "3 seeders" });
  expect(lineWords({ kind: "otherYear", year: 2004 })).toEqual({ tone: "warn", text: "May be a different film or show", details: "The release says 2004" });
  expect(lineWords({ kind: "washedOut" })).toEqual({ tone: "warn", text: "Colors may look washed out on this screen", details: "HDR file, screen without HDR" });
  expect(lineWords({ kind: "stalled" })).toEqual({ tone: "bad", text: "Did not start here last time", details: "Almost nothing arrived when this device last tried it" });
  expect(lineWords({ kind: "noPicture" })).toEqual({ tone: "bad", text: "No picture on this device", details: "Its HEVC video is stored in a form this device cannot show" });
  expect(lineWords({ kind: "didNotPlay" })).toEqual({ tone: "bad", text: "Did not play on this device", details: "This device could not decode it when it last tried" });
  expect(lineWords({ kind: "few", seeders: 31 })).toEqual({ tone: "warn", text: "May start slowly", details: "31 seeders" });
  expect(lineWords({ kind: "ready", service: "TorBox", sizeBytes: 7.4 * GiB })).toEqual({ tone: "ok", text: "Starts right away", details: "Ready now, 7.4 GB" });
  expect(lineWords({ kind: "quick", seeders: 1204, sizeBytes: 2.2 * GiB })).toEqual({ tone: "ok", text: "Starts quickly", details: "1,204 seeders, 2.2 GB" });
  expect(lineWords({ kind: "should", seeders: 89, sizeBytes: 4 * GiB, service: "Real-Debrid" })).toEqual({
    tone: "ok",
    text: "Should start quickly",
    details: "89 seeders, 4 GB, may be ready in your account",
  });
});

test("a detail with an unknown number is left out, never shown with a gap", () => {
  expect(lineWords({ kind: "should", seeders: null, sizeBytes: null, service: null }).details).toBe("");
  expect(lineWords({ kind: "quick", seeders: null, sizeBytes: 3 * GiB }).details).toBe("3 GB");
  expect(lineWords({ kind: "ready", service: null, sizeBytes: 3 * GiB }).details).toBe("Ready now, 3 GB");
  expect(lineWords({ kind: "ready", service: null, sizeBytes: null }).details).toBe("Ready now");
  expect(lineWords({ kind: "fetch", service: null }).details).toBe("");
});

test("a large file says whether the size was the user's choice", () => {
  const large: ReasonLine = { kind: "large", sizeBytes: 7.4 * GiB, limitBytes: 5 * GiB, chosen: false };
  expect(lineWords(large)).toEqual({ tone: "warn", text: "Large file", details: "7.4 GB; Marquee prefers files up to 5 GB" });
  expect(lineWords({ ...large, chosen: true }).text).toBe("Larger than the size you chose");
});

test("may pause says where its speed comes from", () => {
  // An unmeasured speed is never said: the part is left out.
  expect(lineWords({ kind: "mayPause", needMbps: 31.6, haveMbps: 17.5, samples: 0, smallSwarm: false }).details).toBe("");
  expect(lineWords({ kind: "mayPause", needMbps: 79, haveMbps: 25, samples: 1, smallSwarm: false }).details).toBe("Needs about 79 Mbit/s; this connection gives about 25 (measured from 1 playback)");
  expect(lineWords({ kind: "mayPause", needMbps: 79, haveMbps: 25, samples: 6, smallSwarm: false }).details).toContain("(measured from 6 playbacks)");
});

test("may pause on a small swarm says few people share it", () => {
  const thin = lineWords({ kind: "mayPause", needMbps: 2.1, haveMbps: 1.7, samples: 0, smallSwarm: true });
  expect(thin.text).toBe("May pause: only a few people are sharing it");
  expect(thin.details).toBe("");
  expect(lineWords({ kind: "mayPause", needMbps: 2.1, haveMbps: 1.7, samples: 2, smallSwarm: true }).details).toBe("Needs about 2 Mbit/s; this connection gives about 2 (measured from 2 playbacks)");
  expect(lineWords({ kind: "mayPause", needMbps: 2.1, haveMbps: 1.7, samples: 0, smallSwarm: false }).text).toBe("May pause on this connection");
});

test("a window says its minutes ahead, or on mobile data that it keeps a short stretch", () => {
  const buffer: ReasonLine = { kind: "buffer", seeders: 214, sizeBytes: 40 * GiB, aheadMin: 7, budgetBytes: 2.5 * GiB };
  expect(lineWords(buffer)).toEqual({ tone: "ok", text: "Plays without storing the whole file", details: "214 seeders, 40 GB, room for about 7 min ahead in 2.5 GB" });
  expect(lineWords({ ...buffer, aheadMin: null }).details).toBe("214 seeders, 40 GB, keeps only a short stretch ahead on mobile data");
  expect(lineWords({ ...buffer, seeders: null, aheadMin: 10 }).details).toBe("40 GB, room for about 10 min ahead in 2.5 GB");
});

test("every zero-playable state reads its shared line with its actions in the table's order", () => {
  const steps = (kind: EmptyKind, action: EmptyAction, onVersions = false) => emptyWords({ kind, action }, onVersions).steps.map((s) => s.text);
  expect(emptyWords({ kind: "noSource", action: "settingsAddons" }).text).toBe("Add a source in Settings");
  expect(steps("noSource", "settingsAddons")).toEqual(["Settings"]);
  expect(steps("unreachable", "playFromDevice")).toEqual(["Play from this device", "Try again"]);
  expect(steps("unreachable", "tryAgain")).toEqual(["Try again"]);
  expect(steps("needsAccount", "settingsAccounts")).toEqual(["Settings"]);
  expect(steps("nothingPlaysHere", "chooseVersion")).toEqual(["Choose a version"]);
  // Choose a version is the player's and the title's, never Versions' own.
  expect(steps("nothingPlaysHere", "chooseVersion", true)).toEqual([]);
  for (const kind of ["noRoom", "kidsNothing", "noResults"] as const) {
    expect(steps(kind, "none")).toEqual([]);
    expect(emptyWords({ kind, action: "none" }).text).toBe(shared.empty[kind].text);
  }
  expect(counted("versions", 2000)).toBe("2,000 versions");
});

test("the device line reads as one sentence, leaving out what is not known", () => {
  const measured = { mbps: 42.2, samples: 3 };
  const prior = { mbps: 15, samples: 0 };
  expect(conditionsLine({ screenClass: 2160, hdr: true, network: "wifi", speed: measured, freeBytes: 61 * GiB })).toBe("4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free");
  expect(conditionsLine({ screenClass: 1080, hdr: false, network: "cellular", speed: prior, freeBytes: null })).toBe("1080p, mobile data");
  const v: VersionsConditions = {
    screenClass: 480, hdr: false, network: "other", http: prior, p2p: measured, freeBytes: null,
    runtimeMin: 52, sizeLimitBytes: 5 * GiB, minSeeders: 50, engine: true, account: false, window: true,
  };
  // No account and the engine: the torrent speed decides; with an account, or without the engine, the HTTP one does.
  expect(speedFor(v, false)).toBe(measured);
  expect(speedFor(v, true)).toBe(prior);
  expect(speedFor({ ...v, engine: false }, false)).toBe(prior);
});

test("a learned note names the device and the preferences now in effect", () => {
  expect(learnedNote("size", true, "tv", { sizeLimitBytes: 10 * GiB, maxHeight: null })).toBe("On this TV you often picked bigger files, so Marquee now picks files up to 10 GB.");
  expect(learnedNote("height", false, "phone", { sizeLimitBytes: null, maxHeight: 720 })).toBe("On this phone you often picked lower resolutions, so Marquee now picks up to 720p.");
  expect(learnedNote("hdr", true, "desktop", { sizeLimitBytes: null, maxHeight: null })).toBe("On this computer you often picked HDR versions, so Marquee now prefers HDR.");
  // No number to name: no note rather than a gap.
  expect(learnedNote("size", true, "tv", { sizeLimitBytes: null, maxHeight: null })).toBeNull();
});

const zero: Learned = { ready: 0, height: 0, hdr: 0, size: 0, picks: 0, epoch: 0, atMs: 0, muted: [] };
const learned = (uhd: Partial<Learned>, fhd: Partial<Learned> = {}) => ({ uhd: { ...zero, ...uhd }, fhd: { ...zero, ...fhd }, hd: zero, sd: zero });

test("Settings lists what was learned at 3 or more, unless undone or forgotten", () => {
  const p = { learned: learned({ size: 3, height: -4, hdr: 2, picks: 9 }, { ready: 5, hdr: 3, muted: ["ready"] }), learnedEpoch: 0 };
  // Largest screens first, then size, height, ready, HDR.
  expect(learnedRows(p)).toEqual([
    { screenClass: 2160, choice: "size", up: true, text: "On 4K screens you often picked bigger files." },
    { screenClass: 2160, choice: "height", up: false, text: "On 4K screens you often picked lower resolutions." },
    { screenClass: 1080, choice: "hdr", up: true, text: "On 1080p screens you often picked HDR versions." },
  ]);
  // A class stamped before the last Forget reads as zero.
  expect(learnedRows({ ...p, learnedEpoch: 1 })).toEqual([]);
  expect(anyLearned(p)).toBe(true);
  expect(anyLearned({ ...p, learnedEpoch: 1 })).toBe(false);
});

test("the onboarding patches are the plan's, and Skip is Start fast", () => {
  expect(shared.question.fast.patch).toEqual({ sizeLimitBytes: 5 * GiB, minSeeders: 50 });
  expect(shared.question.best.patch).toEqual({ sizeLimitBytes: null, minSeeders: 20 });
  expect(shared.question.skip).toEqual({ label: "Skip", means: "fast" });
});

test("the Settings row sums up the size and the seeders as the chips say them", () => {
  expect(choiceSummary({ sizeLimitBytes: 5 * GiB, minSeeders: 50 })).toBe("Up to 5 GB, shared by at least 50 people");
  expect(choiceSummary({ sizeLimitBytes: null, minSeeders: 20 })).toBe("Any size, shared by at least 20 people");
  expect(choiceSummary({ sizeLimitBytes: 2.5 * GiB, minSeeders: 35 })).toBe("Up to 2.5 GB, shared by at least 35 people");
});

test("a display's lines fall into the core's screen classes", () => {
  expect([2160, 2000, 1440, 1179, 1000, 720, 700, 576].map(screenClass)).toEqual([2160, 2160, 1080, 1080, 1080, 720, 720, 480]);
});

test("a built-in source says where it runs instead of a host", () => {
  expect(addonHost({ builtIn: true, host: "5a0d1888fa64-turkcealtyaziorg.baby-beamup.club" })).toBe("Built into Marquee, runs on this device");
  expect(addonHost({ builtIn: false, host: "v3-cinemeta.strem.io" })).toBe("v3-cinemeta.strem.io");
});

test("the next version, tried or offered, by its label", () => {
  expect(nextVersion.trying("1080p WEB")).toBe("Trying the next version: 1080p WEB");
  expect(nextVersion.play("4K Remux")).toBe("Play 4K Remux");
  // A move on during Try again names the version it tries, as Android does.
  const trying = { phase: "resolving", trying: { label: "720p WEB" } };
  expect(restartLine(trying, true)).toBe("Trying the next version: 720p WEB");
  expect(restartLine({ phase: "resolving", trying: null }, true)).toBe("Trying again");
  expect(restartLine({ phase: "failed", trying: null }, true)).toBe("Trying again");
  expect(restartLine({ phase: "resolving", trying: null }, false)).toBeNull();
  expect(nextVersion.notOpened).toBe("This version could not be opened");
  expect(nextVersion.finding).toBe("Finding this version");
  // The headline never says nothing played while it offers another version.
  expect(nextVersion.failed(true)).toBe("This version could not be played");
  expect(nextVersion.failed(false)).toBe("None of the sources could be played");
});


test("raw reasons read as sentences, the raw text kept", () => {
  const net = "network: error sending request for url (https://x.example/catalog.json): Connection refused (os error 61)";
  expect(problemWords(net)).toEqual({ text: "It could not be reached. Check the connection, then try again.", raw: net });
  expect(problemWords("timed out after 10s").text).toBe("It did not answer in time. Check the connection, then try again.");
  expect(problemWords("timed out").text).toBe("It did not answer in time. Check the connection, then try again.");
  expect(problemWords("http status 404").text).toBe("It has nothing at this address (404).");
  expect(problemWords("http status 403").text).toBe("It refused access (403).");
  expect(problemWords("http status 429").text).toBe("It is busy; try again soon (429).");
  expect(problemWords("http status 502").text).toBe("It had an error on its side (502).");
  expect(problemWords("http status 400").text).toBe("It answered with an error (400).");
  expect(problemWords("decode: expected value at line 1").text).toBe("It sent something Marquee cannot read.");
  expect(problemWords("addon returned no catalog items").text).toBe("It sent an empty list.");
  // The core's own sentences stay as they are.
  expect(problemWords("Cinemeta did not answer in time")).toEqual({ text: "Cinemeta did not answer in time", raw: null });
});
