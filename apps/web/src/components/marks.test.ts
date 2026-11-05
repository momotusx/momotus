import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { MARKS } from "./marks";

test("the marks are design/marks' files, unaltered", () => {
  for (const name of ["imdb", "tmdb", "trakt", "google", "facebook"] as const) {
    const file = readFileSync(join(import.meta.dir, "../../../../design/marks", `${name}.svg`), "utf8").trim();
    expect(MARKS[name] as string).toBe(file);
  }
});
