import { expect, test } from "bun:test";
import { press, summary, tap, tapAction, onFirstLine } from "./language-picker";

const run = (list: string[], taps: string[], audio = false) => {
  let state = { list, added: new Set<string>() };
  for (const code of taps) state = tap(state.list, code, state.added, audio);
  return state.list;
};

test("subtitles: one visit ranks in tap order, before the older picks", () => {
  expect(run([], ["tr", "en"])).toEqual(["tr", "en"]);
  expect(run(["fr"], ["tr", "en"])).toEqual(["tr", "en", "fr"]);
});

test("subtitles: a tap on this visit's pick undoes it; an older one moves first; the older first comes out", () => {
  expect(run(["a", "b"], ["c", "d", "d"])).toEqual(["c", "a", "b"]);
  expect(run(["fr", "tr", "en"], ["tr"])).toEqual(["tr", "fr", "en"]);
  expect(run(["fr", "tr"], ["fr"])).toEqual(["tr"]);
  expect(run(["fr"], ["fr"])).toEqual([]);
  // After a promote, a new pick follows the visit's last addition.
  expect(run(["fr", "tr"], ["de", "tr", "en"])).toEqual(["tr", "de", "en", "fr"]);
});

test("audio keeps its order: a new pick goes last, a tap removes", () => {
  expect(run(["en"], ["tr"], true)).toEqual(["en", "tr"]);
  expect(run(["en", "tr"], ["tr"], true)).toEqual(["en"]);
  expect(tapAction(["en", "tr"], "tr", new Set(), true)).toBe("remove");
});

test("each state names its action", () => {
  expect(tapAction(["fr", "tr"], "en", new Set(), false)).toBe("add");
  expect(tapAction(["fr", "tr"], "tr", new Set(), false)).toBe("default");
  expect(tapAction(["fr", "tr"], "tr", new Set(["tr"]), false)).toBe("remove");
  expect(tapAction(["fr", "tr"], "fr", new Set(), false)).toBe("remove");
});

test("the row's summary and the TV keyboard", () => {
  expect(summary([], "None, subtitles start off")).toBe("None, subtitles start off");
  expect(summary(["Türkçe"], "x")).toBe("Türkçe first");
  expect(summary(["Türkçe", "English", "Français"], "x")).toBe("Türkçe first, 2 more");
  expect(press("tu", "r")).toBe("tur");
  expect(press("tur", "delete")).toBe("tu");
  expect(press("", "delete")).toBe("");
  expect(press("", "space")).toBe("");
  expect(press("pt", "space")).toBe("pt ");
  expect(press("pt ", "space")).toBe("pt ");
});

test("Second line: the first subtitle language's primary language is the first line's, in any spelling", () => {
  expect(onFirstLine("en", "eng")).toBe(true);
  expect(onFirstLine("en", "en")).toBe(true);
  expect(onFirstLine("pt", "pt-BR")).toBe(true);
  expect(onFirstLine("pt-BR", "por")).toBe(true);
  expect(onFirstLine("tr", "eng")).toBe(false);
  expect(onFirstLine("en", undefined)).toBe(false);
});
