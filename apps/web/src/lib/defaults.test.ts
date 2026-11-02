import { expect, test } from "bun:test";
import type { DefaultAddonInfo } from "../core/generated/DefaultAddonInfo";
import { readFileSync } from "node:fs";
import { afterContinue, cardOffer, catalogMissing, choicesFor, defaultsWords, grouped } from "./defaults";

const row = (key: string, group: string, installed: DefaultAddonInfo["installed"] = "absent"): DefaultAddonInfo => ({
  key,
  name: key.toUpperCase(),
  line: `${key} line`,
  group,
  installed,
  logo: null,
});
const nine = [
  row("cinemeta", "catalogs"),
  row("torrentio", "streams"),
  row("turkcealtyazi", "subtitles"),
  row("subsense", "subtitles"),
  row("opensubtitles", "subtitles"),
  row("trakt", "lists"),
  row("rottentomatoes", "lists"),
  row("torrentcatalogs", "lists"),
  row("kitsu", "catalogs"),
];

test("groups come in the order they first appear, each in the core's order", () => {
  const g = grouped(nine);
  expect(g.map((x) => x.title)).toEqual(["Catalogs and details", "Streams", "Subtitles", "Lists"]);
  expect(g[0]?.addons.map((a) => a.key)).toEqual(["cinemeta", "kitsu"]);
  expect(g[2]?.addons.map((a) => a.key)).toEqual(["turkcealtyazi", "subsense", "opensubtitles"]);
});

test("Continue names every switch on a fresh profile, all on unless turned off", () => {
  const choices = choicesFor(nine, new Set(["torrentio"]));
  expect(choices).toHaveLength(9);
  expect(choices.filter((c) => !c.on).map((c) => c.key)).toEqual(["torrentio"]);
});

test("after an import, a default already there is not named, so it keeps its own switch", () => {
  const imported = nine.map((a) => (a.key === "cinemeta" ? { ...a, installed: "off" as const } : a));
  const choices = choicesFor(imported, new Set());
  expect(choices.map((c) => c.key)).not.toContain("cinemeta");
  expect(choices).toHaveLength(8);
});

test("the catalog note shows only when no catalog would be on", () => {
  expect(catalogMissing(nine, new Set())).toBe(false);
  expect(catalogMissing(nine, new Set(["cinemeta"]))).toBe(false); // Kitsu is a catalog too
  expect(catalogMissing(nine, new Set(["cinemeta", "kitsu"]))).toBe(true);
  const present = nine.map((a) => (a.key === "cinemeta" ? { ...a, installed: "on" as const } : a));
  expect(catalogMissing(present, new Set(["kitsu"]))).toBe(false);
});

test("the Home card offers the missing ones once per list version, never on a kids profile", () => {
  const defaults = { version: 1, addons: nine.map((a) => (a.key === "cinemeta" ? { ...a, installed: "on" as const } : a)) };
  expect(cardOffer(defaults, 0, false)).toBe(8);
  expect(cardOffer(defaults, 1, false)).toBeNull();
  expect(cardOffer(defaults, 0, true)).toBeNull();
  expect(cardOffer({ version: 1, addons: nine.map((a) => ({ ...a, installed: "off" as const })) }, 0, false)).toBeNull();
  expect(cardOffer(null, 0, false)).toBeNull();
  expect(cardOffer(defaults, undefined, false)).toBeNull();
});

test("the Home card says how many, never which", () => {
  expect(defaultsWords.cardLine(1)).toBe("Momotus recommends 1 addon you do not have yet.");
  expect(defaultsWords.cardLine(8)).toBe("Momotus recommends 8 addons you do not have yet.");
});

test("Browse first and Review go Home, the doors go to the setup check", () => {
  expect(afterContinue("home")).toBe("/");
  expect(afterContinue(undefined)).toBe("/welcome?step=3");
});

test("an empty Home and Discover point to the recommended addons, not the old standard catalog", () => {
  for (const line of [defaultsWords.emptyHome, defaultsWords.emptyDiscover]) {
    expect(line).toContain("recommended addons");
    expect(line).not.toContain("standard catalog");
  }
  for (const screen of ["home.tsx", "discover.tsx"]) {
    const source = readFileSync(new URL(`../screens/${screen}`, import.meta.url), "utf8");
    expect(source).not.toContain("standard catalog");
  }
});
