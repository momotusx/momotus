import { expect, test } from "bun:test";
import type { PluginRepoInfo } from "../core/generated/PluginRepoInfo";
import type { VersionRow } from "../core/generated/VersionRow";
import type { ListedRepo } from "../core/generated/ListedRepo";
import { addsProviders, directoryLine, matchesRepo, pluginsSummary } from "./plugins";
import { rowWords } from "./words";

test("the sheet names five providers and counts the rest", () => {
  expect(addsProviders(["One"])).toBe("Adds 1 provider: One.");
  expect(addsProviders(["A", "B"])).toBe("Adds 2 providers: A and B.");
  expect(addsProviders(["A", "B", "C", "D", "E", "F", "G", "H"])).toBe("Adds 8 providers: A, B, C, D, E and 3 more.");
});

test("a plugin row says what any direct link says, never the plugin's name or its host (QA r1 Q5 row 6)", () => {
  const row = {
    quality: "1080p",
    sizeBytes: null,
    status: { kind: "direct" },
    line: { kind: "quick" },
    onDevice: null,
    origin: "plugin",
    sourceName: "Dizibox",
    host: "cdn.example.com",
  } as unknown as VersionRow;
  expect(rowWords(row)).toBe("Direct link");
});

test("a listed repository says its providers, kinds and languages, and is found by any of them", () => {
  const r = { name: "Türk Plugins", providers: ["Dizibox", "HDFilmCehennemi"], languages: ["tr"], types: ["movie", "series"] } as unknown as ListedRepo;
  expect(directoryLine(r)).toBe("2 providers for films and series. Languages: tr.");
  expect(directoryLine({ ...r, providers: ["One"], types: [], languages: [] })).toBe("1 provider.");
  expect(matchesRepo(r, "turk")).toBe(true);
  expect(matchesRepo(r, "dizibox TR")).toBe(true);
  expect(matchesRepo(r, "anime")).toBe(false);
  expect(matchesRepo(r, "  ")).toBe(true);
});

test("the Settings line counts providers on in repositories that are on", () => {
  const repo = (enabled: boolean, on: boolean[]) =>
    ({ enabled, providers: on.map((enabled) => ({ enabled })) }) as unknown as PluginRepoInfo;
  expect(pluginsSummary([])).toBe("Add Nuvio repositories for direct links.");
  expect(pluginsSummary([repo(true, [true, false, true]), repo(false, [true])])).toBe("2 repositories, 2 providers on");
  expect(pluginsSummary([repo(true, [true])])).toBe("1 repository, 1 provider on");
});
