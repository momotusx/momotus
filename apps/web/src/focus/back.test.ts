import { expect, test } from "bun:test";

// The stack's order without a browser: the newest entry closes first, a removed one never does, a held
// Back closes one thing, and other keys pass.
test("Back closes the newest overlay first, whatever the mount order", async () => {
  const listeners = new Map<string, ((e: KeyboardEvent) => void)[]>();
  const g = globalThis as unknown as { window?: unknown };
  const before = g.window;
  g.window = { addEventListener: (type: string, fn: (e: KeyboardEvent) => void) => listeners.set(type, [...(listeners.get(type) ?? []), fn]) };
  const { pushBack } = await import("./back");
  const closed: string[] = [];
  const stopped: string[] = [];
  const key = (k: string, repeat = false) =>
    ({ key: k, keyCode: k === "Escape" ? 27 : 0, repeat, target: null, preventDefault() {}, stopImmediatePropagation: () => stopped.push(k) }) as unknown as KeyboardEvent;
  const press = (e: KeyboardEvent) => {
    for (const l of listeners.get("keydown") ?? []) l(e);
  };
  const release = () => {
    for (const l of listeners.get("keyup") ?? []) l(key("Escape"));
  };
  const removePanel = pushBack(() => closed.push("panel"));
  const removeDialog = pushBack(() => closed.push("dialog"));
  press(key("Escape"));
  expect(closed).toEqual(["dialog"]);
  removeDialog();
  // Held down: the repeats are swallowed, the panel stays.
  press(key("Escape", true));
  press(key("Escape", true));
  expect(closed).toEqual(["dialog"]);
  expect(stopped).toEqual(["Escape", "Escape", "Escape"]);
  release();
  // Another key passes untouched.
  press(key("Enter"));
  expect(stopped).toEqual(["Escape", "Escape", "Escape"]);
  press(key("Escape"));
  expect(closed).toEqual(["dialog", "panel"]);
  removePanel();
  release();
  // Nothing open: Back is the app's, and a repeat with nothing closed is not swallowed.
  press(key("Escape"));
  press(key("Escape", true));
  expect(closed).toEqual(["dialog", "panel"]);
  expect(stopped).toEqual(["Escape", "Escape", "Escape", "Escape"]);
  g.window = before;
});
