import { expect, test } from "bun:test";
import { grouped, sampleVersions, statusWords } from "./versions";

test("sample versions follow the title and episode", () => {
  const v = sampleVersions("Game of Thrones", "tt0944947:1:1");
  expect(v.recommended?.releaseName).toStartWith("Game.of.Thrones.S01E01.2160p");
  expect(v.recommended?.reasons.length).toBeLessThanOrEqual(3);
  const keys = new Set([v.recommended?.key, ...v.rows.map((r) => r.key)]);
  expect(keys.size).toBe(v.rows.length + 1);
});

test("groups keep display order and drop empty groups", () => {
  const v = sampleVersions("Heat", "tt0113277");
  expect(grouped(v.rows).map((g) => g.group)).toEqual(["bestQuality", "smallerFaster", "otherLanguages", "needsDownload"]);
  expect(grouped(v.rows.filter((r) => r.group === "needsDownload")).length).toBe(1);
});

test("status words", () => {
  expect(statusWords({ state: "torrent", seeds: 214 }, "Torrentio")).toEqual({ tone: "warn", text: "Torrent, 214 seeds" });
  expect(statusWords({ state: "ready" }, "TorBox").text).toBe("Ready on TorBox");
});
