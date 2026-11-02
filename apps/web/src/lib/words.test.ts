import { expect, test } from "bun:test";
import { addonHost, listFailed, addonsSummary, audioTrackName, nextVersion, qualityCell, sheetDetails, languageName, pairingProblem, plainAddress, problemWords, restartLine, serverHost, startAudio, subtitleNote, subtitleNoteLines, lowerFirst, kindBest, kindChips } from "./words";
import shared from "../../../shared/stream-choice-words.json";
import type { EmptyAction } from "../core/generated/EmptyAction";
import type { EmptyKind } from "../core/generated/EmptyKind";
import type { Learned } from "../core/generated/Learned";
import type { ReasonLine } from "../core/generated/ReasonLine";
import type { VersionRow } from "../core/generated/VersionRow";
import type { VersionsConditions } from "../core/generated/VersionsConditions";
import {
  anyLearned,
  bestFor,
  rowWords,
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

test("a subtitle note says what to do and never names its source", () => {
  const say = (problem: Parameters<typeof subtitleNote>[0]["problem"]) => subtitleNote({ source: "OpenSubtitles v3", problem });
  expect(say("unreachable")).toBe("A subtitle source did not answer. Try its subtitles again in a moment.");
  expect(say("blocked")).toBe("A subtitle source is turning requests away. Momotus will try again in a few minutes.");
  expect(say("changed")).toBe("A subtitle source changed its pages, so Momotus cannot read them. An update will fix this.");
  expect(say("trackFailed")).toBe("This subtitle could not be read.");
  // A problem from a newer core: the general line.
  expect(say("somethingNew" as never)).toBe("Some subtitles did not load.");
  // Two sources with one problem read alike, so the list says it once (the owner's rule: no addon names).
  const lines = subtitleNoteLines([
    { source: "OpenSubtitles v3", problem: "unreachable" },
    { source: "SubSense", problem: "unreachable" },
  ]);
  expect(lines).toEqual(["A subtitle source did not answer. Try its subtitles again in a moment."]);
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
  expect(lineWords({ kind: "veryFew", seeders: 1 }).details).toBe("Shared by 1 person");
  expect(lineWords({ kind: "veryFew", seeders: 3 })).toEqual({ tone: "bad", text: "May not start", details: "Shared by 3 people" });
  expect(lineWords({ kind: "otherYear", year: 2004 })).toEqual({ tone: "warn", text: "May be a different film or show", details: "The release says 2004" });
  expect(lineWords({ kind: "washedOut" })).toEqual({ tone: "warn", text: "Colors may look washed out on this screen", details: "HDR file, screen without HDR" });
  expect(lineWords({ kind: "stalled" })).toEqual({ tone: "bad", text: "Did not start here last time", details: "Almost nothing arrived when this device last tried it" });
  expect(lineWords({ kind: "noPicture" })).toEqual({ tone: "bad", text: "No picture on this device", details: "Its HEVC video is stored in a form this device cannot show" });
  expect(lineWords({ kind: "didNotPlay" })).toEqual({ tone: "bad", text: "Did not play on this device", details: "This device could not decode it when it last tried" });
  expect(lineWords({ kind: "few", seeders: 31 })).toEqual({ tone: "warn", text: "May start slowly", details: "Shared by 31 people" });
  expect(lineWords({ kind: "ready", service: "TorBox", sizeBytes: 7.4 * GiB })).toEqual({ tone: "ok", text: "Starts right away", details: "Ready now, 7.4 GB" });
  expect(lineWords({ kind: "quick", seeders: 1204, sizeBytes: 2.2 * GiB })).toEqual({ tone: "ok", text: "Starts quickly", details: "Shared by 1,204 people, 2.2 GB" });
  expect(lineWords({ kind: "should", seeders: 89, sizeBytes: 4 * GiB, service: "Real-Debrid" })).toEqual({
    tone: "ok",
    text: "Should start quickly",
    details: "Shared by 89 people, 4 GB, may be ready in your account",
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
  expect(lineWords(large)).toEqual({ tone: "warn", text: "Large file", details: "7.4 GB; Momotus prefers files up to 5 GB" });
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
  expect(lineWords(buffer)).toEqual({ tone: "ok", text: "Plays without storing the whole file", details: "Shared by 214 people, 40 GB, room for about 7 min ahead in 2.5 GB" });
  expect(lineWords({ ...buffer, aheadMin: null }).details).toBe("Shared by 214 people, 40 GB, keeps only a short stretch ahead on mobile data");
  expect(lineWords({ ...buffer, seeders: null, aheadMin: 10 }).details).toBe("40 GB, room for about 10 min ahead in 2.5 GB");
});

test("every zero-playable state reads its shared line with its actions in the table's order", () => {
  const steps = (kind: EmptyKind, action: EmptyAction, onVersions = false) => emptyWords({ kind, action }, onVersions).steps.map((s) => s.text);
  expect(emptyWords({ kind: "noSource", action: "settingsAddons" }).text).toBe("Add a source in Settings");
  expect(steps("noSource", "settingsAddons")).toEqual(["Settings"]);
  expect(steps("unreachable", "playFromDevice")).toEqual(["Play from this device", "Try again"]);
  expect(steps("unreachable", "tryAgain")).toEqual(["Try again"]);
  // An account is set up in Settings only: the state is said quietly, with no step.
  expect(steps("needsAccount", "none")).toEqual([]);
  expect(emptyWords({ kind: "needsAccount", action: "none" }).text).toBe("None of these plays on this device");
  expect(steps("nothingPlaysHere", "chooseVersion")).toEqual(["Choose a version"]);
  // Choose a version is the player's and the title's, never Versions' own.
  expect(steps("nothingPlaysHere", "chooseVersion", true)).toEqual([]);
  for (const kind of ["kidsNothing", "noResults"] as const) {
    expect(steps(kind, "none")).toEqual([]);
    expect(emptyWords({ kind, action: "none" }).text).toBe(shared.empty[kind].text);
  }
  // An older bridge's kind (noRoom, gone) or a newer one reads as noResults's quiet line, never a crash.
  for (const kind of ["noRoom", "somethingNew"] as unknown as EmptyKind[]) {
    expect(emptyWords({ kind, action: "none" })).toEqual({ text: shared.empty.noResults.text, steps: [] });
  }
  expect(counted("versions", 2000)).toBe("2,000 versions");
});

test("the device line reads as one sentence, leaving out what is not known", () => {
  const measured = { mbps: 42.2, samples: 3 };
  const prior = { mbps: 15, samples: 0 };
  expect(conditionsLine({ screenClass: 2160, hdr: true, network: "wifi", speed: measured })).toBe("4K HDR, about 42 Mbit/s on Wi-Fi");
  expect(conditionsLine({ screenClass: 1080, hdr: false, network: "cellular", speed: prior })).toBe("1080p, mobile data");
  const v: VersionsConditions = {
    screenClass: 480, hdr: false, network: "other", http: prior, p2p: measured, freeBytes: null,
    runtimeMin: 52, sizeLimitBytes: 5 * GiB, minSeeders: 50, engine: true, account: false,
  };
  // No account and the engine: the torrent speed decides; with an account, or without the engine, the HTTP one does.
  expect(speedFor(v, false)).toBe(measured);
  expect(speedFor(v, true)).toBe(prior);
  expect(speedFor({ ...v, engine: false }, false)).toBe(prior);
});

test("a learned note names the device and the preferences now in effect", () => {
  expect(learnedNote("size", true, "tv", { sizeLimitBytes: 10 * GiB, maxHeight: null })).toBe("On this TV you often picked bigger files, so Momotus now picks files up to 10 GB.");
  expect(learnedNote("height", false, "phone", { sizeLimitBytes: null, maxHeight: 720 })).toBe("On this phone you often picked lower resolutions, so Momotus now picks up to 720p.");
  expect(learnedNote("hdr", true, "desktop", { sizeLimitBytes: null, maxHeight: null })).toBe("On this computer you often picked HDR versions, so Momotus now prefers HDR.");
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
  expect(addonHost({ builtIn: true, host: "5a0d1888fa64-turkcealtyaziorg.baby-beamup.club" })).toBe("Built into Momotus, runs on this device");
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
  expect(problemWords("http status 404").text).toBe("It has nothing at this address.");
  expect(problemWords("http status 403").text).toBe("It refused access.");
  expect(problemWords("http status 429").text).toBe("It is busy; try again soon.");
  expect(problemWords("http status 502").text).toBe("It had an error on its side.");
  expect(problemWords("http status 400").text).toBe("It answered with an error.");
  // The owner's rule: the line never carries the status; Details keeps the core's text.
  for (const n of [400, 401, 403, 404, 410, 429, 451, 500, 503]) {
    const w = problemWords(`http status ${n}`);
    expect(w.text).not.toMatch(/\d/);
    expect(w.raw).toBe(`http status ${n}`);
  }
  expect(problemWords("decode: expected value at line 1").text).toBe("It sent something Momotus cannot read.");
  // The core's own sentences stay as they are.
  expect(problemWords("This list took too long to load")).toEqual({ text: "This list took too long to load", raw: null });
  // Discover says the core's sentence once, never "This list could not load. This list ...".
  expect(listFailed("This list took too long to load")).toBe("This list took too long to load");
  expect(listFailed("http status 500")).toBe("This list could not load. It had an error on its side.");
});

test("a row's one line starts with its size, then where it comes from, a warning or why it is dimmed", () => {
  const row = (line: ReasonLine, status: VersionRow["status"], sizeBytes: number | null = 1.5 * GiB, onDevice: string | null = null) =>
    ({ line, status, sizeBytes, onDevice }) as VersionRow;
  expect(rowWords(row({ kind: "quick", seeders: 1204, sizeBytes: GiB }, { kind: "torrent", seeds: 1204 }))).toBe("1.5 GB, shared by 1,204 people");
  expect(rowWords(row({ kind: "ready", service: "TorBox", sizeBytes: GiB }, { kind: "ready", service: "TorBox" }))).toBe("1.5 GB, ready now");
  expect(rowWords(row({ kind: "quick", seeders: 1, sizeBytes: GiB }, { kind: "torrent", seeds: 1 }))).toBe("1.5 GB, shared by 1 person");
  // A warning never drops the size.
  expect(rowWords(row({ kind: "few", seeders: 31 }, { kind: "torrent", seeds: 31 }, 12 * GiB))).toBe("12 GB, may start slowly");
  expect(rowWords(row({ kind: "few", seeders: 31 }, { kind: "torrent", seeds: 31 }), "Won't play here")).toBe("1.5 GB, won't play here");
  expect(rowWords(row({ kind: "quick", seeders: null, sizeBytes: null }, { kind: "direct" }, null, "On this device"))).toBe("On this device");
  expect(rowWords(row({ kind: "quick", seeders: null, sizeBytes: null }, { kind: "torrent", seeds: null }, null))).toBe("Starts quickly");
});

test("the card's kicker names the form: this TV, this phone, this computer", () => {
  expect(bestFor("tv")).toBe("Best for this TV");
  expect(bestFor("phone")).toBe("Best for this phone");
  expect(bestFor("desktop")).toBe("Best for this computer");
});

test("a quality the release does not state is no badge: nothing, never Unknown", () => {
  expect(qualityCell("Quality not stated")).toBe("");
  expect(qualityCell("Unknown")).toBe("");
  expect(qualityCell("1080p")).toBe("1080p");
  // A source the core names in its place is a badge.
  expect(qualityCell("WEB-DL")).toBe("WEB-DL");
});

test("the sheet says nothing twice: it drops each part the face says, and keeps the rest", () => {
  const row = (line: ReasonLine, onDevice: string | null = null, status: VersionRow["status"] = { kind: "torrent", seeds: 1204 }, sizeBytes = 2 * GiB) =>
    ({ line, status, sizeBytes, onDevice }) as VersionRow;
  // "2 GB, shared by 1,204 people" already said "Shared by 1,204 people, 2 GB".
  expect(sheetDetails(row({ kind: "quick", seeders: 1204, sizeBytes: 2 * GiB }))).toBeNull();
  // What only the details say stays: the account, the window ahead.
  expect(sheetDetails(row({ kind: "should", seeders: 1204, sizeBytes: 2 * GiB, service: "TorBox" }))).toBe("May be ready in your account");
  expect(sheetDetails(row({ kind: "buffer", seeders: 1204, sizeBytes: 2 * GiB, aheadMin: 7, budgetBytes: 2.5 * GiB }))).toBe("Room for about 7 min ahead in 2.5 GB");
  // A ready file: "2 GB, ready now" says both parts.
  expect(sheetDetails(row({ kind: "ready", service: null, sizeBytes: 2 * GiB }, null, { kind: "ready", service: null }))).toBeNull();
  // "On this device" names no people and no limit: they stay, the size goes.
  expect(sheetDetails(row({ kind: "quick", seeders: 1204, sizeBytes: 2 * GiB }, "On this device"))).toBe("Shared by 1,204 people");
  expect(sheetDetails(row({ kind: "large", sizeBytes: 7.4 * GiB, limitBytes: 5 * GiB, chosen: false }, "On this device", { kind: "direct" }, 7.4 * GiB))).toBe("7.4 GB; Momotus prefers files up to 5 GB");
  // "117 MB, may not start" says no people: they follow.
  expect(sheetDetails(row({ kind: "veryFew", seeders: 2 }))).toBe("Shared by 2 people");
  // A part is said only whole: "14 GB" and "1.4 GB" do not say "4 GB".
  expect(sheetDetails(row({ kind: "quick", seeders: 1204, sizeBytes: 4 * GiB }, null, { kind: "torrent", seeds: 1204 }, 14 * GiB))).toBe("4 GB");
  expect(sheetDetails(row({ kind: "quick", seeders: 1204, sizeBytes: 4 * GiB }, null, { kind: "torrent", seeds: 1204 }, 1.4 * GiB))).toBe("4 GB");
  // A dimmed row's line says its size and why it is dimmed: the people follow.
  expect(sheetDetails(row({ kind: "quick", seeders: 1204, sizeBytes: 2 * GiB }), "Won't play here")).toBe("Shared by 1,204 people");
});

test("after the colon a word in capitals stays in capitals", () => {
  expect(lowerFirst("CAM copy")).toBe("CAM copy");
  expect(lowerFirst("HDR file, screen without HDR")).toBe("HDR file, screen without HDR");
  expect(lowerFirst("Shared by 3 people")).toBe("shared by 3 people");
  expect(lowerFirst(lineWords({ kind: "copy", quality: "CAM" } as ReasonLine).details)).toBe("CAM copy");
});

test("Settings' Addons line counts and never names an addon", () => {
  const a = (enabled: boolean, provides: string[]) => ({ name: "Torrentio", enabled, provides });
  expect(addonsSummary([])).toBe("None installed yet.");
  expect(addonsSummary([a(true, ["catalog"])])).toBe("1 installed. None of them finds streams yet.");
  expect(addonsSummary([a(true, ["stream"]), a(false, ["stream"])])).toBe("2 installed. 1 finds streams.");
  expect(addonsSummary([a(true, ["stream"]), a(true, ["stream", "meta"]), a(true, ["subtitles"])])).toBe("3 installed. 2 find streams.");
  expect(addonsSummary([a(true, ["stream"])])).not.toContain("Torrentio");
});

test("the start audio is the first wanted language the file lists", () => {
  // A French release's tracks: French first, the original English second, one untagged.
  const tracks = [
    { id: "1", language: "fre" },
    { id: "2", language: "en" },
    { id: "3", language: "" },
  ];
  expect(startAudio(tracks, ["en"])).toBe("2");
  expect(startAudio(tracks, ["de", "fr"])).toBe("1");
  expect(startAudio(tracks, ["ja"])).toBeNull();
  expect(startAudio(tracks, [])).toBeNull();
  expect(startAudio([{ id: "a", language: "pt-BR" }], ["pt"])).toBe("a");
});

test("the kind chips are the core's kinds after All, each with its count, the debrid one named by its service", () => {
  const chips = kindChips([
    { kind: "torrent", service: null, count: 37 },
    { kind: "direct", service: null, count: 1 },
    { kind: "debrid", service: null, count: 1204 },
  ]);
  expect(chips.map((c) => [c.value, c.label, c.note])).toEqual([
    ["all", "All", undefined],
    ["torrent", "Torrents", "37"],
    ["direct", "Direct links", "1"],
    ["debrid", "Debrid", "1,204"],
  ]);
  expect(chips[1]?.a11y).toBe("Torrents, 37 versions");
  expect(chips[2]?.a11y).toBe("Direct links, 1 version");
  const named = [{ kind: "debrid" as const, service: "Real-Debrid", count: 2 }];
  expect(kindChips(named)[1]?.label).toBe("Real-Debrid");
  expect(kindBest("debrid", named)).toBe("Best on Real-Debrid");
  expect(kindBest("debrid", [])).toBe("Best debrid link");
  expect(kindBest("torrent", named)).toBe("Best torrent");
  expect(kindBest("direct", named)).toBe("Best direct link");
});

test("an audio track that names no language reads Audio alone, by its place among several", () => {
  // The shared player words (stream-choice-words.json), never "Original": a dub is not.
  expect(audioTrackName({ lang: null, label: "" }, 0, 1)).toBe(shared.player.audio);
  expect(audioTrackName({ lang: null, label: "" }, 0, 1)).toBe("Audio");
  expect(audioTrackName({ lang: null, label: "" }, 1, 3)).toBe("Audio 2");
  expect(audioTrackName({ lang: "en", label: "" }, 0, 3)).toBe("English");
  expect(audioTrackName({ lang: null, label: "Commentary" }, 1, 3)).toBe("Commentary");
});

test("a kids session gets no Settings to add a source: no dead end to a page it cannot open", () => {
  expect(emptyWords({ kind: "noSource", action: "settingsAddons" }, false, true).steps).toEqual([]);
  expect(emptyWords({ kind: "noSource", action: "settingsAddons" }, false, false).steps.map((s) => s.step)).toEqual(["settingsAddons"]);
});
