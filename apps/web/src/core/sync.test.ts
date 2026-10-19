/** One account start at a time (create, recover, a confirmed pairing), run
 * against the real module with the bridge mocked: each start owns the slot
 * from before its request goes, a second one is refused, and an answer that
 * no longer owns the slot is dropped. */

import { afterEach, expect, jest, mock, test } from "bun:test";
import type { SyncRecord } from "../lib/sync";
import { clientMock, fake } from "./fake-client";

const g = globalThis as unknown as Record<string, unknown>;
type Call = { name: string; fields: Record<string, unknown>; answer: (v: unknown) => void; fail: (e: unknown) => void };
const calls: Call[] = [];

/** Rounds and leaving answer at once; a start waits for the test to answer it. */
const QUIET = { record: null, fingerprint: "ABCD", again: false, refused: 0, unreadable: 0, newer: 0 };
mock.module("./client", clientMock);
fake.op = (name, fields) =>
  new Promise((answer, fail) => {
    calls.push({ name, fields, answer, fail });
    if (name === "syncNow") answer(QUIET);
    if (name === "signOut") answer({});
  });
mock.module("../form", () => ({ form: () => "desktop" }));
// What the module touches as it loads: the page's events and a storage that holds nothing.
g.document ??= { addEventListener: () => undefined, hidden: false };
g.window ??= { setTimeout };
g.indexedDB = {
  open: () => {
    const request: Record<string, unknown> = {};
    const store = { get: () => ({ result: undefined }) };
    const t = { objectStore: () => store, oncomplete: null as (() => void) | null };
    request.result = {
      close: () => undefined,
      transaction: () => (queueMicrotask(() => t.oncomplete?.()), t),
    };
    queueMicrotask(() => (request.onsuccess as () => void)());
    return request;
  },
};

const sync = await import("./sync");
await sync.vaultReady;

const account = (name: string) => ({ deviceToken: name }) as unknown as SyncRecord;
/** The account this page syncs, read from the round `join` starts. */
const syncing = () => calls.filter((c) => c.name === "syncNow").map((c) => (c.fields.record as { deviceToken: string }).deviceToken);
/** Waits until `ready` holds, yielding to the event loop between looks: a condition, not a clock. */
async function until(ready: () => boolean) {
  for (let i = 0; i < 10_000 && !ready(); i++) await new Promise((done) => setTimeout(done, 0));
  expect(ready()).toBe(true);
}
const asked = (name: string) => () => calls.some((c) => c.name === name);
const joinedRound = () => syncing().length === 1;

afterEach(async () => {
  // Leave the account here only, so the next test starts with none.
  if (sync.joined()) await sync.signOut(true, true);
  calls.length = 0;
});

test("a create in flight keeps a pairing from being confirmed, and joins with its own account", async () => {
  const creating = sync.createAccount("https://sync.example", "Desk", "");
  await until(asked("createSyncAccount"));
  expect(calls.map((c) => c.name)).toEqual(["createSyncAccount"]);
  // The pairing's Yes comes while the create waits for its answer: refused before any join.
  expect(sync.claimPairing()).toBeNull();
  expect(sync.mayPair()).toBe(false);
  calls[0]?.answer(account("created"));
  await creating;
  await until(joinedRound);
  expect(syncing()).toEqual(["created"]);
});

test("a confirmed pairing keeps a create from starting, and joins with the paired account", async () => {
  const claim = sync.claimPairing();
  expect(claim).not.toBeNull();
  // Start an account meanwhile: refused before its request goes, with its own words.
  const refused = await sync.createAccount("https://sync.example", "Desk", "").catch((e: { code?: string; message: string }) => e);
  expect((refused as { code?: string }).code).toBe("joining");
  expect(calls.filter((c) => c.name === "createSyncAccount")).toEqual([]);
  expect(sync.joinPaired(account("paired"), claim as symbol)).toBe(true);
  await until(joinedRound);
  expect(syncing()).toEqual(["paired"]);
});

test("an answer that no longer owns the slot is dropped", async () => {
  // A pairing claimed, then ended without a record; a create takes the slot.
  const old = sync.claimPairing() as symbol;
  sync.endPairing(old);
  const creating = sync.createAccount("https://sync.example", "Desk", "");
  await until(asked("createSyncAccount"));
  // The old pairing's record arrives late: it joins nothing.
  expect(sync.joinPaired(account("late"), old)).toBe(false);
  calls.find((c) => c.name === "createSyncAccount")?.answer(account("created"));
  await creating;
  await until(joinedRound);
  expect(syncing()).toEqual(["created"]);
});

test("a failed start frees the slot", async () => {
  const creating = sync.createAccount("https://sync.example", "Desk", "").catch(() => "failed");
  await until(asked("createSyncAccount"));
  calls[0]?.fail(new Error("http status 502"));
  expect(await creating).toBe("failed");
  const claim = sync.claimPairing();
  expect(claim).not.toBeNull();
  sync.endPairing(claim as symbol);
});

test("while an account is joined a round runs each minute the tab is visible, and none after it leaves", async () => {
  /** Settles promises only: the clock is fake here. */
  const flush = async () => {
    for (let i = 0; i < 50; i++) await Promise.resolve();
  };
  const rounds = () => calls.filter((c) => c.name === "syncNow").length;
  const page = g.document as { hidden: boolean };
  jest.useFakeTimers();
  try {
    const creating = sync.createAccount("https://sync.example", "Desk", "");
    await flush();
    calls.find((c) => c.name === "createSyncAccount")?.answer(account("timed"));
    await creating;
    await flush();
    const joined = rounds();
    jest.advanceTimersByTime(59_000);
    await flush();
    expect(rounds()).toBe(joined);
    jest.advanceTimersByTime(1_000);
    await flush();
    expect(rounds()).toBe(joined + 1);
    // Hidden, the minute passes without a round.
    page.hidden = true;
    jest.advanceTimersByTime(60_000);
    await flush();
    expect(rounds()).toBe(joined + 1);
    page.hidden = false;
    // Left: no more rounds, minute after minute.
    await sync.signOut(true, true);
    await flush();
    const left = rounds();
    jest.advanceTimersByTime(180_000);
    await flush();
    expect(rounds()).toBe(left);
    // Joined again: still one round a minute, not one per account this page ever held.
    const again = sync.createAccount("https://sync.example", "Desk", "");
    await flush();
    calls.filter((c) => c.name === "createSyncAccount").at(-1)?.answer(account("again"));
    await again;
    await flush();
    const rejoined = rounds();
    jest.advanceTimersByTime(60_000);
    await flush();
    expect(rounds()).toBe(rejoined + 1);
  } finally {
    page.hidden = false;
    jest.useRealTimers();
  }
});
