'use strict';
// A live session against the real wasm core (plans/webos.md, finish run F2):
// the service under this Node with MQ_DATA set and the core build-wasm.sh
// left in service/core, a page socket with the token, Home's catalogs from
// Cinemeta over the network, and one direct HTTP lease (the built-in
// Internet Archive source) whose URL is the source's final one, after its
// redirects, never 127.0.0.1. Not a *.test.js: it needs the network and a
// built core. Run: node test/live-core.js (WS_PATH as for run.sh).
const path = require('path');
const fs = require('fs');
const h = require('./harness.js');

const CORE = process.env.MQ_LIVE_CORE || path.join(__dirname, '..', 'core', 'mq_wasm.js');
// A public domain film the Archive holds (before its 1963 line).
const FILM = { id: 'tt0052077', title: 'Plan 9 from Outer Space', year: '1957' };
// What a webOS TV plays (apps/web/src/core/webos-caps.ts), and no headers.
const CAPS = {
  displayHeight: 1080,
  decoders: ['h264', 'h265', 'vp9', 'av1'],
  audioDecoders: ['aac', 'ac3', 'eac3', 'mp3'],
  containers: ['mkv', 'mp4', 'm4v', 'ts', 'm3u8'],
  hasFallbackEngine: false,
  directOnly: true,
};

const log = (what, value) => console.log(JSON.stringify({ step: what, value: value }));

function client(c) {
  let id = 0;
  const waiting = new Map();
  const last = new Map();
  c.ws.on('message', (raw) => {
    const m = JSON.parse(String(raw));
    if (m.sub !== undefined && m.updates) last.set(m.sub, m.updates[m.updates.length - 1]);
    else if (waiting.has(m.id)) waiting.get(m.id)(m);
  });
  const req = (op, fields) =>
    new Promise((resolve, reject) => {
      id += 1;
      waiting.set(id, (m) => (m.ok ? resolve(m.result) : reject(new Error(op + ': ' + JSON.stringify(m)))));
      c.send(JSON.stringify(Object.assign({ id: id, op: op }, fields || {})));
    });
  return {
    req: req,
    subscribe: (view, params) =>
      req('subscribe', { view: view, params: params }).then((r) => ({
        sub: r.sub,
        now: () => (last.has(r.sub) ? last.get(r.sub) : r.snapshot),
      })),
  };
}

// Home's pushes are typed updates; any tile with a Cinemeta poster counts.
function tiles(value) {
  const text = JSON.stringify(value || null);
  const posters = text.match(/https:\/\/[^"]*metahub[^"]*/g) || [];
  return posters.length;
}

async function main() {
  if (!fs.existsSync(CORE)) throw new Error('no core at ' + CORE + ': run apps/webos/scripts/build-wasm.sh');
  const started = Date.now();
  const svc = await h.startService({ env: { MQ_CORE: CORE } });
  // The core's own log lines (host.js prints them as "[level] text").
  let listed = false;
  svc.proc.stdout.on('data', (d) =>
    String(d)
      .split('\n')
      .filter((l) => l[0] === '[')
      .forEach((l) => {
        if (l.indexOf('archive list fetched') >= 0) listed = true;
        console.error(l.slice(0, 400));
      }),
  );
  log('service', { node: process.version, arch: process.arch, port: svc.port, probe: svc.start.probe });
  const ws = await h.authed(svc);
  const c = client(ws);
  const profiles = await c.req('defaultAddons');
  log('core answered', { ms: Date.now() - started, defaults: profiles.length });
  const ready = svc.find('ready')[0];
  log('core opened', ready && ready.open);
  await c.req('setPrefs', { patch: { archive: true, archiveNoticeSeen: true } });
  await c.req('installDefaults', { choices: [{ key: 'cinemeta', on: true }] });

  const home = await c.subscribe('home', {});
  const pushes = [];
  ws.ws.on('message', (raw) => {
    const m = JSON.parse(String(raw));
    if (m.sub === home.sub) pushes.push(m.updates);
  });
  const found = await h.until('Home catalogs from Cinemeta', () => tiles(pushes) + tiles(home.now()) >= 10, 90000);
  log('home', { tilesWithCinemetaPosters: tiles(pushes) + tiles(home.now()), pushes: pushes.length, ok: !!found });

  // The Archive's pass waits for its first list 10 s at most (archive.rs
  // FIRST_LIST); a slower list shows on the next open, as in the app.
  const params = Object.assign(
    { kind: 'movie', videoId: FILM.id, itemId: FILM.id, title: FILM.title, releaseInfo: FILM.year },
    CAPS,
  );
  const archiveRow = (v) =>
    [v.recommended]
      .concat(v.rows || [], (v.archive && v.archive.rows) || [])
      .filter((r) => r && String(r.key).indexOf('archive:') === 0)[0];
  let versions = await c.subscribe('versions', params);
  await h.until('the Archive pass', () => versions.now().archive && !versions.now().archive.pending, 60000);
  if (!archiveRow(versions.now())) {
    await h.until('the Archive list', () => listed, 120000);
    await c.req('unsubscribe', { sub: versions.sub });
    versions = await c.subscribe('versions', params);
    log('versions', 'opened again once the Archive list came');
  }
  const row = await h.until('an Internet Archive row', () => archiveRow(versions.now()), 60000);
  log('versions row', { key: row.key, via: row.via });

  const playback = await c.subscribe('playback', {
    versionsSub: versions.sub,
    key: row.key,
    kind: 'movie',
    itemId: FILM.id,
    videoId: FILM.id,
    name: FILM.title,
  });
  const snap = await h.until('a lease', () => playback.now().url && playback.now(), 120000);
  const url = snap.url;
  const host = url.replace(/^https?:\/\/([^/]+).*$/, '$1');
  log('lease', { phase: snap.phase, attempt: snap.attempt, manifest: snap.manifest, host: host, url: url.slice(0, 120) });
  const probe = await c.req('command', { sub: playback.sub, command: { type: 'probe', attempt: snap.attempt } });
  const stale = await c
    .req('command', { sub: playback.sub, command: { type: 'probe', attempt: snap.attempt + 100 } })
    .then((r) => ({ answered: r }), (e) => ({ refused: String(e.message).indexOf('switched') >= 0 }));
  log('probe', { now: probe, staleAttempt: stale });

  const checks = {
    homeFromCinemeta: !!found,
    leaseIsDirect: /^https:\/\//.test(url) && url.indexOf('127.0.0.1') < 0,
    leaseIsFinal: host !== 'archive.org' && /archive\.org$/.test(host),
    probeServed: probe.kind === null,
    staleProbeRefused: stale.refused === true,
  };
  log('checks', checks);
  await svc.kill();
  const ok = Object.keys(checks).every((k) => checks[k]);
  console.log((ok ? 'PASS' : 'FAIL') + ' live core on node ' + process.version + ' in ' + (Date.now() - started) + ' ms');
  process.exit(ok ? 0 : 1);
}

main().catch((e) => {
  console.log('FAIL ' + ((e && e.stack) || e));
  process.exit(1);
});
