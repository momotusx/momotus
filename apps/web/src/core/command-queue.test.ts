import { expect, test } from "bun:test";
import { commandQueue } from "./command-queue";

const tick = () => new Promise<void>((r) => setTimeout(r, 0));

test("commands held while the view opens go first, in order, before a newer one", async () => {
  const sent: string[] = [];
  const q = commandQueue<string>();
  void q.send("older");
  // The view opens in one microtask and a newer command arrives in the next.
  const opened = Promise.resolve().then(() => q.open(async (c) => void sent.push(c)));
  const newer = Promise.resolve().then(() => q.send("newer"));
  await Promise.all([opened, newer]);
  expect(sent).toEqual(["older", "newer"]);
});

test("a command sent while the held ones go out lands after them", async () => {
  const sent: string[] = [];
  const q = commandQueue<string>();
  void q.send("a");
  void q.send("b");
  q.open(async (c) => {
    sent.push(c);
    if (c === "a") void q.send("during");
  });
  await tick();
  expect(sent).toEqual(["a", "b", "during"]);
});

test("once open, a command goes at once, before the caller's next line", () => {
  const sent: string[] = [];
  const q = commandQueue<string>();
  q.open(async (c) => void sent.push(c));
  void q.send("last report");
  expect(sent).toEqual(["last report"]);
});

test("a view that never opens drops held and later commands", async () => {
  const sent: string[] = [];
  const q = commandQueue<string>();
  const held = q.send("held");
  q.drop();
  await held;
  await q.send("later");
  q.open(async (c) => void sent.push(c));
  expect(sent).toEqual([]);
});
