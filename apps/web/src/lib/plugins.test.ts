import { expect, test } from "bun:test";
import type { PluginRepoInfo } from "../core/generated/PluginRepoInfo";
import type { VersionRow } from "../core/generated/VersionRow";
import { addsProviders, pluginRowLine, pluginsSummary } from "./plugins";

test("the sheet names five providers and counts the rest", () => {
  expect(addsProviders(["One"])).toBe("Adds 1 provider: One.");
  expect(addsProviders(["A", "B"])).toBe("Adds 2 providers: A and B.");
  expect(addsProviders(["A", "B", "C", "D", "E", "F", "G", "H"])).toBe("Adds 8 providers: A, B, C, D, E and 3 more.");
});

test("a plugin row says what it is and where it plays from", () => {
  const row = { quality: "1080p", sourceName: "Dizibox", host: "cdn.example.com" } as VersionRow;
  expect(pluginRowLine(row)).toBe("Dizibox, plays from cdn.example.com");
});

test("the Settings line counts providers on in repositories that are on", () => {
  const repo = (enabled: boolean, on: boolean[]) =>
    ({ enabled, providers: on.map((enabled) => ({ enabled })) }) as unknown as PluginRepoInfo;
  expect(pluginsSummary([])).toBe("Add Nuvio repositories for direct links.");
  expect(pluginsSummary([repo(true, [true, false, true]), repo(false, [true])])).toBe("2 repositories, 2 providers on");
  expect(pluginsSummary([repo(true, [true])])).toBe("1 repository, 1 provider on");
});
