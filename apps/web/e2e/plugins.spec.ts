/** Nuvio plugin repositories through the web screens (plans/plugins-and-marketplace.md 7.7, 7.8, R7), on
 * a core of its own whose default repository is our inert fixture (crates/mq-plugins/tests/fixtures/
 * plugin-repo) on a made-up public name, answered by the recorder (e2e/replay.ts `pluginFixture`): a
 * pasted link is read and added only from the sheet's Add; a provider switches off and on; the
 * repository is the recommended list's plugin row; with no addon row the provider's link is Versions'
 * pick, never with the provider's name or host. Nothing asks a real repository. */

import { expect, type Page, test } from "./fixture";
import type { ChildProcess } from "node:child_process";
import type { DefaultAddons } from "../src/core/generated/DefaultAddons";
import { Client, cleanup, PLUGINS_PORT, PORTS, SHOTS, startBridge } from "./core";
import { LISTED_REPO, PLUGIN_HOST } from "./replay";
import { BBB_ID } from "./source";

const REPO = `https://${PLUGIN_HOST}/repo/manifest.json`;
let core: { proc: ChildProcess; data: string } | null = null;
let client: Client;

test.beforeAll(async () => {
  core = await startBridge(PLUGINS_PORT, { MQ_DEV_PLUGINS: REPO });
  client = new Client(`ws://127.0.0.1:${PLUGINS_PORT}/ws`);
});

test.afterAll(() => {
  client?.close();
  if (core) cleanup(core.proc, core.data);
});

/** A desktop page whose socket reaches this spec's core. */
async function page(browser: import("@playwright/test").Browser): Promise<Page> {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${PLUGINS_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
  });
  return context.newPage();
}

test("a repository added from the sheet runs in Versions under From plugins", async ({ browser }) => {
  // The default repository is listed with a switch, not yet given (no addon list yet).
  const before = await client.req<DefaultAddons>("defaultAddons");
  expect(before.addons.filter((a) => a.group === "plugins").map((a) => [a.name, a.installed])).toEqual([["Test plugins", "absent"]]);
  await client.req("installDefaults", { choices: [{ key: "cinemeta", on: true }] });

  const p = await page(browser);
  await p.goto("/plugins");
  await expect(p.locator("main h1")).toHaveText("Plugins");
  await expect(p.getByText("No repositories yet.")).toBeVisible();
  await p.locator('[data-f="plugins:url"]').fill(REPO);
  await p.locator('[data-f="plugins:add"]').click();
  const sheet = p.getByRole("dialog");
  await expect(sheet.getByRole("heading")).toHaveText("Add Momotus test plugins?");
  await expect(sheet).toContainText(`From ${PLUGIN_HOST}.`);
  await expect(sheet).toContainText("Adds 1 provider: Test film.");
  await expect(sheet).toContainText("can contact any public website");
  await p.screenshot({ path: `${SHOTS}/plugins-sheet-desktop.png` });
  // Nothing is added until Add.
  expect((await client.req<{ repos: unknown[] }>("plugins")).repos).toHaveLength(0);
  // Add answers once the sheet is armed, so the press that opened it never adds.
  await expect(sheet).toHaveAttribute("data-armed", "");
  await sheet.locator('[data-f="sheet:install"]').click();
  await expect(p.locator("ol.addons > li")).toHaveCount(1);
  const provider = p.getByRole("button", { name: "Test film: On" });
  await expect(provider).toHaveAttribute("aria-pressed", "true");
  await provider.click();
  await expect(p.getByRole("button", { name: "Test film: Off" })).toHaveAttribute("aria-pressed", "false");
  await p.getByRole("button", { name: "Test film: Off" }).click();
  await expect(p.getByRole("button", { name: "Test film: On" })).toBeVisible();
  await p.screenshot({ path: `${SHOTS}/plugins-desktop.png` });

  // The same link is the default: the recommended list shows it on.
  const after = await client.req<DefaultAddons>("defaultAddons");
  expect(after.addons.find((a) => a.group === "plugins")?.installed).toBe("on");

  await p.goto(`/title/movie/${BBB_ID}/versions/${BBB_ID}`);
  // No addon lists it: the plugin's row is the pick, which Play plays, listed once (the owner, 2026-11-02).
  const pick = p.locator('section.pick[aria-label="Recommended version"]');
  await expect(pick.locator('[data-f="versions:play"]')).toBeVisible();
  // Never the provider's name or the host (QA r1 Q5 row 6).
  await expect(pick).not.toContainText(PLUGIN_HOST);
  await expect(pick).not.toContainText("Test film");
  await expect(p.locator('[data-group="plugins"]')).toHaveCount(0);
  await expect(p.locator(".vstate")).toHaveText("1 version");
  await expect(p.locator(".pick-wait")).toHaveCount(0);
  await p.screenshot({ path: `${SHOTS}/plugins-versions-desktop.png` });

  // Play opens the player on that link, never on "No source has this yet" (owner, 2026-11-02).
  await p.goto(`/play/movie/${BBB_ID}/${BBB_ID}`);
  await expect(p.locator('[data-f="player:toggle"]')).toBeVisible({ timeout: 30_000 });
  await expect.poll(() => p.evaluate(() => document.querySelector("video")?.src ?? ""), { timeout: 30_000 }).toMatch(/^http:\/\/127\.0\.0\.1:\d+\//);
  await expect(p.getByText("No source has this yet")).toHaveCount(0);
  await p.context().close();
});

test("Find addons lists the directory's repositories, searched on the device, added from the sheet", async ({ browser }) => {
  const p = await page(browser);
  await p.goto("/find-addons");
  const section = p.locator(".fa-plugins");
  await expect(section.getByRole("heading", { name: "Nuvio plugins" })).toBeVisible();
  // Only the manifest on raw.githubusercontent.com: the other link on the page is refused.
  const card = section.locator(".fa-pcard");
  await expect(card).toHaveCount(1);
  await expect(card).toContainText("Momotus test plugins");
  await expect(card).toContainText("From nuvioplugins.com");
  await expect(card).toContainText("1 provider for films and series. Languages: en.");
  await section.locator('[data-f="fa-plugins:search"]').fill("nothing like it");
  await expect(section.getByText("No repository matches.")).toBeVisible();
  await section.locator('[data-f="fa-plugins:search"]').fill("test FILM");
  await expect(card).toHaveCount(1);
  await card.getByRole("button", { name: "Add Momotus test plugins" }).click();
  const sheet = p.getByRole("dialog");
  await expect(sheet.getByRole("heading")).toHaveText("Add Momotus test plugins?");
  await expect(sheet).toContainText("From raw.githubusercontent.com.");
  await expect(sheet).toHaveAttribute("data-armed", "");
  await p.screenshot({ path: `${SHOTS}/plugins-directory-sheet-desktop.png` });
  await sheet.locator('[data-f="sheet:install"]').click();
  await expect(card).toContainText("Installed");
  const repos = (await client.req<{ repos: { url: string }[] }>("plugins")).repos.map((r) => r.url);
  expect(repos).toContain(LISTED_REPO);
  await p.screenshot({ path: `${SHOTS}/plugins-directory-desktop.png` });
  await p.context().close();
});

test("tv: the remote goes down from each repository's switches through its providers to the next one (QA r1 Q6 row 53)", async ({ browser }) => {
  // Two repositories by now (the two tests above), one provider each.
  expect((await client.req<{ repos: unknown[] }>("plugins")).repos).toHaveLength(2);
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const upstream = new WebSocket(`ws://127.0.0.1:${PLUGINS_PORT}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => ws.send(String(e.data)));
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (upstream.readyState === WebSocket.OPEN ? upstream.send(String(m)) : early.push(String(m))));
  });
  const p = await context.newPage();
  await p.goto("/plugins?form=tv");
  const active = () => p.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f);
  await expect(p.locator('[data-f="plugin:1:0"]')).toBeVisible();
  await p.locator('[data-f="plugin-repo:0:enabled"]').focus();
  const walk: (string | undefined)[] = [];
  for (let i = 0; i < 3; i++) {
    await p.keyboard.press("ArrowDown");
    walk.push(await active());
  }
  expect(walk[0]).toBe("plugin:0:0");
  expect(walk[1]).toMatch(/^plugin-repo:1:(enabled|remove)$/);
  expect(walk[2]).toBe("plugin:1:0");
  // Up comes back the same way, and Remove reaches the providers too.
  await p.keyboard.press("ArrowUp");
  await p.keyboard.press("ArrowUp");
  expect(await active()).toBe("plugin:0:0");
  await p.locator('[data-f="plugin-repo:0:remove"]').focus();
  await p.keyboard.press("ArrowDown");
  expect(await active()).toBe("plugin:0:0");
  await p.screenshot({ path: `${SHOTS}/plugins-tv.png` });
  await context.close();
});
