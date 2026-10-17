import { describe, expect, test } from "bun:test";
import { type Candidate, pick } from "./spatial";

/** Two rows of three 100x150 tiles, 20 px gaps, second row scrolled so its
 * tiles are offset by 60 px. */
function grid(): Candidate<string>[] {
  const out: Candidate<string>[] = [];
  for (let i = 0; i < 3; i++) {
    out.push({ key: `a${i}`, group: "a", rect: { left: i * 120, top: 0, width: 100, height: 150 } });
    out.push({ key: `b${i}`, group: "b", rect: { left: 60 + i * 120, top: 200, width: 100, height: 150 } });
  }
  return out;
}

const at = (all: Candidate<string>[], key: string) => all.find((c) => c.key === key)!;

describe("pick", () => {
  test("moves along a row and stops at the edge", () => {
    const all = grid();
    const none = new Map<string, string>();
    expect(pick(at(all, "a0"), all, "right", none)).toBe("a1");
    expect(pick(at(all, "a1"), all, "left", none)).toBe("a0");
    expect(pick(at(all, "a2"), all, "right", none)).toBeNull();
    expect(pick(at(all, "a0"), all, "up", none)).toBeNull();
  });

  test("moves down to the best aligned tile", () => {
    const all = grid();
    expect(pick(at(all, "a1"), all, "down", new Map())).toBe("b0");
    expect(pick(at(all, "a2"), all, "down", new Map())).toBe("b1");
  });

  test("entering a row restores its remembered tile", () => {
    const all = grid();
    const memory = new Map([["b", "b2"]]);
    expect(pick(at(all, "a0"), all, "down", memory)).toBe("b2");
    // Memory for a tile that is no longer rendered is ignored.
    expect(pick(at(all, "a0"), all, "down", new Map([["b", "gone"]]))).toBe("b0");
  });
});
