import { expect, test } from "bun:test";
import shared from "../../../shared/find-addons-words.json";
import type { ListedAddon } from "../core/generated/ListedAddon";
import { action, filterEmpty, initial, shows, spoken } from "./find-addons";
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
  configureUrl: null,
  installed: "absent",
  copyKey: null,
  ...over,
});

test("the words are the shared ones, every key and no more", () => {
  expect(findAddons as Record<string, string>).toEqual(shared);
});

test("each installed state gives its one action", () => {
  expect(action(entry({ installed: "on" }), false)).toEqual({ kind: "none", word: "Installed" });
  expect(action(entry({ installed: "copyOn" }), false)).toEqual({ kind: "none", word: "Set up" });
  expect(action(entry({ installed: "off" }), false)).toEqual({ kind: "turnOn", url: "https://addon.example/manifest.json", word: "Off" });
  const copy = entry({ installed: "copyOff", copyKey: "https://addon.example/abc/manifest.json" });
  expect(action(copy, false)).toEqual({ kind: "turnOn", url: "https://addon.example/abc/manifest.json", word: "Set up, off" });
});

test("setup decides between Install, Install and Configure, and Configure", () => {
  const page = "https://addon.example/configure";
  expect(action(entry({}), false)).toEqual({ kind: "install", configure: false });
  expect(action(entry({ configuration: "optional", configureUrl: page }), false)).toEqual({ kind: "install", configure: true });
  expect(action(entry({ configuration: "optional", configureUrl: page }), true)).toEqual({ kind: "install", configure: false });
  expect(action(entry({ configuration: "required", configureUrl: page }), false)).toEqual({ kind: "configure" });
  // An installed state wins over setup.
  expect(action(entry({ configuration: "required", installed: "on" }), false).kind).toBe("none");
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
  expect(spoken(subs, action(subs, false), false, "Subtitles")).toBe("Install OpenSubtitles v3, subtitles");
  const trakt = entry({ name: "Trakt Tv", host: "trakt.example", provides: ["catalog"] });
  expect(spoken(trakt, action(trakt, false), true, "Catalogs")).toBe("Install Trakt Tv, from trakt.example, catalogs");
  const on = entry({ installed: "on" });
  expect(spoken(on, action(on, false), false, "Subtitles")).toBe("Example, subtitles, installed");
});
