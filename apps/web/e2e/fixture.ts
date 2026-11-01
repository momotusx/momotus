/** Every spec's `test` (plans/batch-10-incognito.md 7.4, C P2-5): the specs share one core, and an Incognito scope a
 * spec left alive would put the next spec's page behind the chooser. Before each test the shared core's `ping` must
 * report no live scope within 2 s, or that test fails and names the one that ran before it. */

import { test as base, expect } from "@playwright/test";
import { Client } from "./core";

export * from "@playwright/test";

let before = "the run's first test";

async function liveScopes(): Promise<number> {
  const c = new Client();
  try {
    await c.ready;
    return (await c.req<{ liveScopes: number }>("ping")).liveScopes;
  } finally {
    c.close();
  }
}

export const test = base.extend<{ noLiveIncognito: void }>({
  noLiveIncognito: [
    async ({}, use, info) => {
      const until = Date.now() + 2000;
      let live = await liveScopes();
      while (live > 0 && Date.now() < until) {
        await new Promise((done) => setTimeout(done, 100));
        live = await liveScopes();
      }
      expect(live, `an Incognito scope is still alive after: ${before}`).toBe(0);
      before = info.titlePath.slice(1).join(" > ");
      await use();
    },
    { auto: true },
  ],
});
