import { expect, test } from "bun:test";
import { extends_ } from "./discover-hold";

const ids = (...xs: string[]) => xs.map((id) => ({ id }));

test("more pages extend the list; a late list that reorders it does not", () => {
  expect(extends_(ids("a", "b"), ids("a", "b", "c"))).toBe(true);
  expect(extends_([], ids("a"))).toBe(true);
  expect(extends_(ids("a", "b"), ids("c", "a", "b"))).toBe(false);
  expect(extends_(ids("a", "b"), ids("a"))).toBe(false);
});
