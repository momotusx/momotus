/** Multi-device sync through the web screens, for real: a local sync server
 * (crates/mq-sync-server) and three cores of their own, one per device, each
 * page reaching its core through a routed WebSocket. The shared core of the
 * other specs never joins an account. The kit read and shown here belongs to
 * this run's throwaway server and is gone when the run ends.
 *
 * Desk (desktop) starts the account; Phone pairs by a pasted code and makes a
 * kit; Living room TV joins with that kit; Desk reports Phone lost, keeping
 * the TV, then removes the TV; both learn they were removed. */

import { type Browser, chromium, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { encode } from "uqr";
import { Client, PORTS, startBridge, startRelay, SYNC_PORTS } from "./core";

const SERVER = `http://127.0.0.1:${SYNC_PORTS.relay}`;
const procs: { proc: ChildProcess; data: string }[] = [];
let desk: Page;
let phone: Page;
let tv: Page;
let kit = "";
const errors: string[] = [];
/** Desk's fake camera films this file (Chromium reads it when the camera opens). */
let camera = "";
let deskBrowser: Browser;

/** A Y4M video of `text` as a QR code, black on white, for the fake camera. */
function film(text: string) {
  const [width, height] = [640, 480];
  const qr = encode(text, { ecc: "M", border: 4 });
  const px = Math.floor(440 / qr.size);
  const [x0, y0] = [(width - qr.size * px) / 2, (height - qr.size * px) / 2].map(Math.floor) as [number, number];
  const y = Buffer.alloc(width * height, 255);
  for (let r = 0; r < qr.size * px; r++)
    for (let c = 0; c < qr.size * px; c++) if (qr.data[Math.floor(r / px)]?.[Math.floor(c / px)]) y[(y0 + r) * width + x0 + c] = 0;
  const chroma = Buffer.alloc((width / 2) * (height / 2) * 2, 128);
  const frame = Buffer.concat([Buffer.from("FRAME\n"), y, chroma]);
  writeFileSync(camera, Buffer.concat([Buffer.from(`YUV4MPEG2 W${width} H${height} F10:1 Ip A1:1 C420jpeg\n`), frame, frame]));
}

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
};

/** A browser context whose page talks to the core on `port`. */
async function device(browser: Browser, port: number, form: keyof typeof FORMS): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.grantPermissions(["clipboard-read", "clipboard-write", "camera"]);
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => ws.send(String(e.data)));
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => (core.readyState === WebSocket.OPEN ? core.send(String(m)) : early.push(String(m))));
    ws.onClose(() => core.close());
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  await page.goto(`/settings?form=${form}`);
  return page;
}

const synced = (page: Page) => expect(page.locator(".sync-state .st").first()).toContainText("Synced at", { timeout: 30_000 });
const member = (page: Page, name: string) => page.locator(".sync-member").filter({ has: page.getByRole("heading", { name }) });
const names = (page: Page) => page.locator(".sync-members h3").evaluateAll((hs) => hs.map((h) => h.firstChild?.textContent ?? ""));
const shot = async (page: Page, name: string) => {
  await page.waitForTimeout(500);
  await page.screenshot({ path: `e2e/shots/${name}.jpg`, type: "jpeg", quality: 80 });
};
/** In-app navigation: a reload would drop the tab's sync record, as it does for a user. */
const go = async (page: Page, path: string) => {
  await page.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);
  await expect(page).toHaveURL(new RegExp(`${path.replace(/[/?]/g, "\\$&")}$`));
};

test.describe.serial("sync across three devices", () => {
  test.beforeAll(async ({ browser }) => {
    procs.push(await startRelay(SYNC_PORTS.relay));
    for (const port of [SYNC_PORTS.a, SYNC_PORTS.b, SYNC_PORTS.c]) procs.push(await startBridge(port));
    // Something of Desk's to see on Phone: the local sample addon.
    const c = new Client(`ws://127.0.0.1:${SYNC_PORTS.a}/ws`);
    await c.req("installAddon", { url: `http://127.0.0.1:${PORTS.source}/manifest.json` });
    c.close();
    camera = join(mkdtempSync(join(tmpdir(), "mq-camera-")), "camera.y4m");
    film("nothing to see yet");
    deskBrowser = await chromium.launch({
      args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", `--use-file-for-fake-video-capture=${camera}`],
    });
    desk = await device(deskBrowser, SYNC_PORTS.a, "desktop");
    phone = await device(browser, SYNC_PORTS.b, "phone");
    tv = await device(browser, SYNC_PORTS.c, "tv");
  });

  test.afterAll(async () => {
    for (const p of [desk, phone, tv]) await p?.context().close();
    await deskBrowser?.close();
    if (camera) rmSync(join(camera, ".."), { recursive: true, force: true });
    for (const { proc, data } of procs) {
      proc.kill();
      rmSync(data, { recursive: true, force: true });
    }
  });

  test("Desk starts an account from Settings", async () => {
    await desk.locator('[data-f="settings:sync"]').click();
    await expect(desk.locator('[data-f="settings:sync"]')).toHaveCount(0);
    await desk.locator('[data-f="sync:start"]').click();
    await desk.locator('[data-f="sync-form:server"]').fill(`${SERVER.replace("http://", "http://me:pw@")}`);
    await desk.locator('[data-f="sync-form:go"]').click();
    await expect(desk.locator(".sync-form .st.bad")).toHaveText("Enter the address without a user name or password.");
    await desk.locator('[data-f="sync-form:server"]').fill(SERVER);
    await desk.locator('[data-f="sync-form:name"]').fill("Desk");
    await desk.locator('[data-f="sync-form:go"]').click();
    await expect(desk).toHaveURL(/\/sync$/);
    await synced(desk);
    await expect(member(desk, "Desk")).toContainText("This device");
    await expect(member(desk, "Desk")).toContainText(", the first device");
    // No kit yet: the page asks for one.
    await expect(desk.locator(".sync-callout")).toContainText("This account has no recovery kit");
    await expect(desk.locator(".sync-fp")).toHaveText(/^[0-9A-Z]{4} [0-9A-Z]{4} [0-9A-Z]{4}$/);
  });

  test("Phone pairs: Desk scans its code with the camera", async () => {
    await go(phone, "/pair");
    await phone.locator('[data-f="pair:server"]').fill(SERVER);
    await phone.locator('[data-f="pair:name"]').fill("Phone");
    await phone.locator('[data-f="pair:start"]').click();
    await expect(phone.locator(".qrbox svg")).toBeVisible();
    await phone.locator('[data-f="pair:copy"]').click();
    await expect(phone.locator('[data-f="pair:copy"]')).toHaveText("Copied");
    const code = await phone.evaluate(() => navigator.clipboard.readText());
    expect(code).toMatch(/^marquee:pair\?v=2&/);
    await shot(phone, "sync-pair-phone");

    await go(desk, "/sync/add");
    await expect(desk.locator(".sync-callout")).toContainText("Approve only a code on a screen in front of you.");
    await desk.locator('[data-f="sync-add:code"]').fill("hello");
    await desk.locator('[data-f="sync-add:approve"]').click();
    await expect(desk.locator("main .st.bad")).toHaveText("Not a pairing code.");
    // The camera films the code the phone shows; the page approves what it reads.
    film(code);
    await desk.locator('[data-f="sync-add:scan"]').click();
    const digits = (await desk.locator(".confirm .code").textContent()) ?? "";
    expect(digits).toMatch(/^\d{6}$/);
    await shot(desk, "sync-add-desktop");

    // Both screens show the same six digits; only the new device confirms.
    await expect(phone.locator(".confirm .code")).toHaveText(digits);
    await phone.locator('[data-f="pair:yes"]').click();
    await expect(phone.locator(".summary h2")).toHaveText("This device joined the account");
    await phone.locator('[data-f="pair:done"]').click();
    await expect(phone).toHaveURL(/\/sync$/);
    await synced(phone);
    await desk.locator('[data-f="sync-add:done"]').click();
    await expect(member(desk, "Phone")).toContainText("through Desk");
  });

  test("what Desk had arrives on Phone", async () => {
    await go(phone, "/addons");
    await expect(phone.locator(".addon h2", { hasText: "Sample Source" })).toBeVisible({ timeout: 30_000 });
    await go(phone, "/settings");
    await expect(phone.locator('[data-f="settings:sync"]')).toContainText(/On\. Synced at/);
  });

  test("Phone makes a recovery kit, shown once", async () => {
    await go(phone, "/sync");
    await phone.locator('[data-f="sync:kit"]').click();
    await expect(phone.locator(".lede")).toContainText("Anyone who has it can join your account");
    await phone.locator('[data-f="sync-kit:make"]').click();
    kit = (await phone.locator(".sync-kit-text").textContent()) ?? "";
    expect(kit).toMatch(/^([0-9A-Z]{4}-){7}[0-9A-Z]{4}$/);
    await expect(phone.locator(".sync-kit .sync-callout")).toContainText("It is not shown again");
    await phone.locator('[data-f="sync-kit:copy"]').click();
    await expect(phone.locator('[data-f="sync-kit:copy"]')).toHaveText("Copied");
    expect(await phone.evaluate(() => navigator.clipboard.readText())).toBe(kit);
    // A throwaway kit of this run's own server.
    await shot(phone, "sync-kit-phone");
    await phone.locator('[data-f="sync-kit:saved"]').click();
    await expect(phone).toHaveURL(/\/sync$/);
    // Nothing on the page keeps it, and the kit now counts.
    expect(await phone.content()).not.toContain(kit);
    await expect(member(phone, "Recovery kit")).toContainText("on Phone");
    await expect(phone.locator(".sync-callout")).toHaveCount(0);
    await shot(phone, "sync-phone");
  });

  test("the TV joins with the kit, typed with the remote", async () => {
    await go(tv, "/sync");
    await expect.poll(() => tv.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)).toBeTruthy();
    for (let i = 0; i < 4 && (await tv.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)) !== "sync:recover"; i++) await tv.keyboard.press("ArrowDown");
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/sync\/recover$/);
    await tv.locator('[data-f="sync-form:server"]').fill(SERVER);
    await shot(tv, "sync-recover-tv");
    await tv.locator('[data-f="sync-form:kit"]').fill("AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA");
    await tv.locator('[data-f="sync-form:go"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv.locator(".sync-form .st.bad")).toHaveText("The sync server does not know this kit. Check the letters and try again.");
    await tv.locator('[data-f="sync-form:kit"]').fill(kit.toLowerCase());
    await tv.locator('[data-f="sync-form:name"]').fill("Living room TV");
    await tv.locator('[data-f="sync-form:go"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/sync$/);
    await synced(tv);
    await expect(tv.locator(".sync-callout")).toContainText("You joined with a recovery kit. Make a new one now");
    await expect(member(tv, "Living room TV")).toContainText("through Recovery kit");
    // On a TV, adding devices and making kits say where to do them.
    await expect(tv.locator('[data-f="sync:add"]')).toContainText("a TV has no camera");
    await expect(tv.locator('[data-f="sync:kit"]')).toContainText("on a TV, anyone in the room");
    await expect.poll(() => tv.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f)).toBe("sync:now");
    await shot(tv, "sync-tv");
  });

  test("on the TV every action is reachable with the remote", async () => {
    const focused = () => tv.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
    const seen = new Set<string>();
    for (let i = 0; i < 16; i++) {
      seen.add(await focused());
      await tv.keyboard.press("ArrowDown");
    }
    expect([...seen].join(" ")).toMatch(/sync:[0-9a-f]+:remove/);
    for (const k of ["sync:add", "sync:kit", "sync:leave"]) expect(seen.has(k)).toBe(true);
    // Report lost sits right of Remove; it opens with the remote, its focus
    // starts on Cancel, and Back leaves it untouched.
    const row = member(tv, "Phone");
    await row.locator('[data-f$=":remove"]').focus();
    await tv.keyboard.press("ArrowRight");
    await expect.poll(focused).toBe(await row.locator('[data-f$=":lost"]').getAttribute("data-f"));
    await tv.keyboard.press("Enter");
    await expect(tv).toHaveURL(/\/sync\/lost\//);
    await expect.poll(focused).toBe("sync-lost:cancel");
    await tv.keyboard.press("Escape");
    await expect(tv).toHaveURL(/\/sync$/);
  });

  test("Desk reports Phone lost and keeps the TV", async () => {
    await go(desk, "/settings");
    await go(desk, "/sync");
    await synced(desk);
    await expect(member(desk, "Living room TV")).toBeVisible();
    await member(desk, "Phone").locator('[data-f$=":lost"]').click();
    await expect(desk.locator("h1")).toHaveText("Report lost: Phone");
    const tvRow = desk.locator(".sync-check", { hasText: "Living room TV" });
    await expect(tvRow).toHaveAttribute("aria-checked", "true");
    await expect(tvRow).toContainText("through Recovery kit");
    await expect(desk.locator(".sync-callout")).toHaveText("1 recovery kit made on these devices stops working. Make a new one afterwards.");
    await expect(desk.locator('[data-f="sync-lost:go"]')).toHaveText("Remove 2 devices");
    await tvRow.click();
    await expect(tvRow).toHaveAttribute("aria-checked", "false");
    await expect(desk.locator('[data-f="sync-lost:go"]')).toHaveText("Remove 1 device");
    // The kit Phone made goes whatever is kept.
    await expect(desk.locator(".sync-callout")).toContainText("1 recovery kit");
    await shot(desk, "sync-lost-desktop");
    await desk.locator('[data-f="sync-lost:go"]').click();
    await expect(desk).toHaveURL(/\/sync$/);
    await expect.poll(() => names(desk)).toEqual(["Desk", "Living room TV"]);
  });

  test("Phone learns it was removed; the TV's edit reaches Desk in one Sync now", async () => {
    await phone.locator('[data-f="sync:now"]').click();
    await expect(phone.locator("main > .st.bad")).toHaveText("This device was removed from your sync account. Your library stays here as local data.");
    await expect(phone.locator('[data-f="sync:pair"]')).toBeVisible();
    await shot(phone, "sync-removed-phone");

    // An edit on the TV (made through its core, so the page stays put and
    // nothing else catches up first) before it learns of the device change.
    // One Sync now takes the new key, stores the record, and only then the
    // next round sends the edit.
    const c = new Client(`ws://127.0.0.1:${SYNC_PORTS.c}/ws`);
    await c.req("setPrefs", { patch: { autoplayNext: false } });
    c.close();
    await expect(tv.locator('[data-f="sync:now"]')).toBeFocused();
    await tv.keyboard.press("Enter");
    await synced(tv);
    await desk.locator('[data-f="sync:now"]').click();
    await synced(desk);
    await expect.poll(async () => (await tv.locator(".sync-fp").textContent()) === (await desk.locator(".sync-fp").textContent())).toBe(true);
    await shot(desk, "sync-desktop");
    await go(desk, "/settings");
    await expect(desk.locator('[data-f="pref-next:false"]')).toHaveAttribute("aria-checked", "true");
    await go(desk, "/sync");
  });

  test("Phone, found again, pairs with a pasted code", async () => {
    await phone.locator('[data-f="sync:pair"]').click();
    // The server is remembered: the code shows at once.
    await phone.locator('[data-f="pair:copy"]').click();
    const code = await phone.evaluate(() => navigator.clipboard.readText());
    await go(desk, "/sync/add");
    await desk.locator('[data-f="sync-add:code"]').fill(code);
    await desk.locator('[data-f="sync-add:approve"]').click();
    const digits = (await desk.locator(".confirm .code").textContent()) ?? "";
    await expect(phone.locator(".confirm .code")).toHaveText(digits);
    await phone.locator('[data-f="pair:yes"]').click();
    await phone.locator('[data-f="pair:done"]').click();
    await synced(phone);
    await desk.locator('[data-f="sync-add:done"]').click();
    await expect.poll(() => names(desk)).toEqual(["Desk", "Living room TV", "Phone"]);
    // A new member: Phone shows the same 12 characters as Desk once both synced.
    await desk.locator('[data-f="sync:now"]').click();
    await synced(desk);
    await phone.locator('[data-f="sync:now"]').click();
    await synced(phone);
    await expect.poll(async () => (await phone.locator(".sync-fp").textContent()) === (await desk.locator(".sync-fp").textContent())).toBe(true);
  });

  test("Desk removes the TV, then stops syncing", async () => {
    const row = member(desk, "Living room TV");
    await row.locator('[data-f$=":remove"]').click();
    await expect(row.locator(".sync-ask")).toContainText("Remove Living room TV? It stops syncing");
    await row.locator('[data-f$=":confirm"]').click();
    await expect.poll(() => names(desk)).toEqual(["Desk", "Phone"]);
    await tv.locator('[data-f="sync:now"]').focus();
    await tv.keyboard.press("Enter");
    await expect(tv.locator("main > .st.bad")).toContainText("This device was removed from your sync account.");

    await desk.locator('[data-f="sync:leave"]').click();
    await expect(desk.locator('[data-f="sync:start"]')).toBeVisible();
    await go(desk, "/settings");
    await expect(desk.locator('[data-f="settings:sync"]')).toContainText("Keep your library, progress and addons the same on every device.");
    expect(errors).toEqual([]);
  });
});
