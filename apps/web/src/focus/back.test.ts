import { expect, test } from "bun:test";

// The stack's order without a browser: the newest entry closes first, a removed one never does.
test("Back closes the newest overlay first, whatever the mount order", async () => {
  const listeners: ((e: KeyboardEvent) => void)[] = [];
  const g = globalThis as unknown as { window?: unknown };
  const before = g.window;
  g.window = { addEventListener: (_: string, fn: (e: KeyboardEvent) => void) => listeners.push(fn) };
  const { pushBack } = await import("./back");
  const closed: string[] = [];
  const removePanel = pushBack(() => closed.push("panel"));
  const removeDialog = pushBack(() => closed.push("dialog"));
  const key = { key: "Escape", keyCode: 27, target: null, preventDefault() {}, stopImmediatePropagation() {} } as unknown as KeyboardEvent;
  for (const l of listeners) l(key);
  expect(closed).toEqual(["dialog"]);
  removeDialog();
  for (const l of listeners) l(key);
  expect(closed).toEqual(["dialog", "panel"]);
  removePanel();
  for (const l of listeners) l(key);
  expect(closed).toEqual(["dialog", "panel"]);
  g.window = before;
});
