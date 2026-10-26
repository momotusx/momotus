import { expect, test } from "bun:test";
import shared from "../../../shared/find-addons-words.json";
import type { ListedAddon } from "../core/generated/ListedAddon";
import type { AddonList } from "../core/generated/AddonList";
import { action, cardAction, filterEmpty, initial, listLabel, matches, providesLine, repeatedNames, shows, spoken } from "./find-addons";
import { findAddons } from "./words";

const entry = (over: Partial<ListedAddon>): ListedAddon => ({
  key: "https://addon.example/manifest.json",
  id: "org.example",
  host: "addon.example",
  name: "Example",
  description: null,
  types: ["movie"],
  provides: ["subtitles"],
  configuration: "none",
  configureUrl: "https://addon.example/configure",
  installed: "absent",
  copyKey: null,
  ...over,
});

test("the words are the shared ones, every key and no more", () => {
  expect(findAddons as Record<string, string>).toEqual(shared);
});

test("each installed state gives its one action", () => {
  expect(action(entry({ installed: "on" }))).toEqual({ kind: "none", word: "Installed" });
  expect(action(entry({ installed: "copyOn" }))).toEqual({ kind: "none", word: "Set up" });
  expect(action(entry({ installed: "off" }))).toEqual({ kind: "turnOn", url: "https://addon.example/manifest.json", word: "Off" });
  const copy = entry({ installed: "copyOff", copyKey: "https://addon.example/abc/manifest.json" });
  expect(action(copy)).toEqual({ kind: "turnOn", url: "https://addon.example/abc/manifest.json", word: "Set up, off" });
});

test("setup decides between Install, Install and Configure, and Configure", () => {
  const page = "https://addon.example/configure";
  expect(action(entry({}))).toEqual({ kind: "install", configure: false });
  expect(action(entry({ configuration: "optional", configureUrl: page }))).toEqual({ kind: "install", configure: true });
  expect(action(entry({ configuration: "required", configureUrl: page }))).toEqual({ kind: "configure" });
  // An installed state wins over setup.
  expect(action(entry({ configuration: "required", installed: "on" })).kind).toBe("none");
});

test("an install refused as needing setup asks for Configure until the addon is installed another way", () => {
  expect(cardAction(entry({}), "needsSetup")).toEqual({ kind: "configure" });
  expect(cardAction(entry({}), "timeout")).toEqual({ kind: "install", configure: false });
  expect(cardAction(entry({ installed: "copyOn" }), "needsSetup")).toEqual({ kind: "none", word: "Set up" });
});

test("filters read what an addon provides", () => {
  const subs = entry({ provides: ["subtitles"] });
  expect(shows(subs, "all")).toBe(true);
  expect(shows(subs, "subtitles")).toBe(true);
  expect(shows(subs, "sources")).toBe(false);
  expect(shows(entry({ provides: ["catalog", "meta", "stream"] }), "sources")).toBe(true);
  expect(filterEmpty("subtitles")).toBe("No subtitle addons in this list.");
  expect(filterEmpty("sources")).toBe("No source addons in this list.");
});

test("the mark is the first letter or digit", () => {
  expect(initial("OpenSubtitles v3")).toBe("O");
  expect(initial("✨ 4K Streams")).toBe("4");
  expect(initial("שלום")).toBe("ש");
  expect(initial("!!!")).toBe("");
});

test("a card's spoken label names the action, then the host only when names repeat", () => {
  const subs = entry({ name: "OpenSubtitles v3" });
  expect(spoken(subs, action(subs), false, "Subtitles")).toBe("Install OpenSubtitles v3, subtitles");
  const trakt = entry({ name: "Trakt Tv", host: "trakt.example", provides: ["catalog"] });
  expect(spoken(trakt, action(trakt), true, "Catalogs")).toBe("Install Trakt Tv, from trakt.example, catalogs");
  const on = entry({ installed: "on" });
  expect(spoken(on, action(on), false, "Subtitles")).toBe("Example, subtitles, installed");
});

test("what a card provides reads as a sentence", () => {
  expect(providesLine(entry({ provides: ["subtitles"], types: ["movie", "series"] }))).toBe("Subtitles for movies and series");
  expect(providesLine(entry({ provides: ["catalog", "meta"], types: ["channel"] }))).toBe("Catalogs, Details for channels");
  expect(providesLine(entry({ provides: ["stream"], types: ["movie", "series", "other"] }))).toBe("Streams for movies, series and other");
  expect(providesLine(entry({ provides: ["stream"], types: [] }))).toBe("Streams");
});

test("only names shown twice in a list are marked", () => {
  const names = repeatedNames([entry({ name: "Trakt Tv" }), entry({ name: "Trakt Tv" }), entry({ name: "Kitsu" })]);
  expect([...names]).toEqual(["Trakt Tv"]);
});

const list = (id: string, name: string, host: string | null): AddonList => ({ id, name, host, state: { state: "ready" }, entries: [], more: false });

test("a publisher's list that shares a name with another chip says its host", () => {
  const official = list("official", "Official", null);
  const theirs = list("addon-1:official", "Official", "dir.example");
  const other = list("addon-1:dir", "Directory", "dir.example");
  const all = [official, list("community", "Community", null), theirs, other];
  expect(listLabel(official, all)).toBe("Official");
  expect(listLabel(theirs, all)).toBe("Official, dir.example");
  expect(listLabel(other, all)).toBe("Directory");
});

test("search finds every word, case and accents aside, in the name, host or description", () => {
  const e = entry({ name: "Türkçe Altyazı", host: "subs.example", description: "Subtitles for films" });
  expect(matches(e, "")).toBe(true);
  expect(matches(e, "turkce")).toBe(true);
  expect(matches(e, "TURKÇE films")).toBe(true);
  expect(matches(e, "subs.example")).toBe(true);
  expect(matches(e, "turkce series")).toBe(false);
});
