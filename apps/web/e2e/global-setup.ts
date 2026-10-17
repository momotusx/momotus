/** One fresh core per run: real addons installed through the protocol, then
 * two titles watched part way so Continue Watching has real progress. */

import { BBB_ID, ensureMedia, startSource } from "./source";
import { Client, cleanup, GOT, PORTS, startBridge, until, watch } from "./core";

export default async function globalSetup() {
  await ensureMedia();
  const source = await startSource(PORTS.source);
  const { proc, data } = await startBridge();
  const c = new Client();
  try {
    for (const url of [
      "https://v3-cinemeta.strem.io/manifest.json",
      "https://opensubtitles-v3.strem.io/manifest.json",
      "https://torrentio.strem.fun/manifest.json",
      `http://127.0.0.1:${PORTS.source}/manifest.json`,
    ]) {
      // Public addons sometimes miss the core's 10 s fetch limit; try thrice.
      for (let attempt = 1; ; attempt++) {
        try {
          await c.req("installAddon", { url });
          break;
        } catch (e) {
          if (attempt === 3) throw e;
        }
      }
    }

    // Game of Thrones S2:E3, 21 of 57 minutes (the pick is a Torrentio torrent).
    await watch(
      c,
      { kind: "series", itemId: GOT.id, videoId: GOT.resume, title: "Game of Thrones", season: 2, episode: 3 },
      21 * 60_000,
      57 * 60_000,
    );
    // Big Buck Bunny watched last, so it leads Continue Watching.
    await watch(c, { kind: "movie", itemId: BBB_ID, videoId: BBB_ID, title: "Big Buck Bunny", via: "Sample Source" }, 150_000, 596_000);

    // Both titles in the library, through the title view as a user would.
    for (const [kind, id] of [
      ["series", GOT.id],
      ["movie", BBB_ID],
    ] as const) {
      type Header = { state: { state: string } };
      const title = await c.subscribe<{ header?: Header }>("title", { kind, id });
      await until(`details for ${id}`, () => (c.header<Header>(title.sub) ?? title.now().header)?.state.state === "ready");
      await c.command(title.sub, { type: "setInLibrary", value: true });
      await c.req("unsubscribe", { sub: title.sub });
    }
  } catch (e) {
    c.close();
    cleanup(proc, data);
    source.close();
    throw e;
  }
  c.close();
  return async () => {
    cleanup(proc, data);
    source.close();
  };
}
