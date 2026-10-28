import { expect, test } from "bun:test";
import sheet from "../../../shared/install-sheet.json";
import { addsLine, sheetFrom, sheetTitle } from "./install-sheet";

test("the Adds line follows every shared vector", () => {
  for (const v of sheet.vectors) expect(addsLine(v.provides, v.types), JSON.stringify(v)).toBe(v.line);
});

test("a name is plain text in the title", () => {
  for (const v of sheet.titleVectors) expect(sheetTitle(v.name)).toBe(v.title);
});

test("the title and From line fill in the addon's own words", () => {
  expect(sheetTitle("OpenSubtitles v3")).toBe("Install OpenSubtitles v3?");
  expect(sheetFrom("opensubtitles-v3.strem.io")).toBe("From opensubtitles-v3.strem.io");
});
