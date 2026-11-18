'use strict';
// A live session against the real native core (plans/webos.md v5): the
// service under this Node with the armv7 mq-bridge (build-native.sh; on an
// x86 runner through qemu-user's binfmt), a page socket with the token,
// Home's catalogs from Cinemeta over the network, then a public domain film
// as a torrent from a one-file addon on loopback: the lease is the bridge's
// own loopback URL and serves the film's first bytes. Not a *.test.js: it
// needs the network, the swarm and a built core.
// Run: MQ_LIVE_NATIVE=<binary> node test/live-native.js (WS_PATH as for run.sh).
const http = require('http');
const path = require('path');
const fs = require('fs');
const h = require('./harness.js');

const BIN = process.env.MQ_LIVE_NATIVE || path.join(__dirname, '..', 'bin', 'mq-bridge');
// Blender's open films, with long-lived public swarms; the first that serves bytes passes.
const FILMS = [
  { id: 'tt1254207', title: 'Big Buck Bunny', year: '2008', hash: 'dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c', release: 'Big.Buck.Bunny.2008.480p.x264.AAC.mp4' },
  { id: 'tt1727587', title: 'Sintel', year: '2010', hash: '08ada5a7a6183aae1e09d831df6748d566095a10', release: 'Sintel.2010.720p.x264.AAC.mp4' },
];
const TRACKERS = ['udp://tracker.opentrackr.org:1337/announce', 'udp://explodie.org:6969', 'udp://tracker.torrent.eu.org:451/announce', 'udp://open.stealth.si:80/announce'];
// What a webOS TV plays with the native core (apps/web/src/core/webos-caps.ts): no directOnly.
const CAPS = {
  displayHeight: 1080,
  decoders: ['h264', 'h265', 'vp9'],
  audioDecoders: ['aac', 'ac3', 'eac3', 'mp3'],
  containers: ['mkv', 'mp4', 'ts', 'm3u8'],
  hasFallbackEngine: false,
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

const tiles = (value) => (JSON.stringify(value || null).match(/https:\/\/[^"]*metahub[^"]*/g) || []).length;

// A Stremio addon with one torrent stream per film.
function addon() {
  const manifest = { id: 'live.native', version: '1.0.0', name: 'Live check', resources: ['stream'], types: ['movie'], idPrefixes: ['tt'], catalogs: [] };
  const server = http.createServer((req, res) => {
    const film = FILMS.filter((f) => req.url === '/stream/movie/' + f.id + '.json')[0];
    const body = req.url === '/manifest.json' ? manifest : film ? { streams: [{ name: 'Live check', title: film.release, infoHash: film.hash, sources: TRACKERS.map((t) => 'tracker:' + t).concat('dht:' + film.hash) }] } : null;
    res.writeHead(body ? 200 : 404, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' });
    res.end(body ? JSON.stringify(body) : '');
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// The first bytes of the lease, as the TV's player asks for them.
function firstBytes(url) {
  return new Promise((resolve, reject) => {
    const req = http.get(Object.assign(require('url').parse(url), { headers: { Range: 'bytes=0-65535' } }), (res) => {
      let n = 0;
      res.on('data', (d) => (n += d.length));
      res.on('end', () => resolve({ status: res.statusCode, bytes: n, type: res.headers['content-type'] }));
    });
    req.on('error', reject);
    req.setTimeout(240000, () => req.destroy(new Error('no bytes in 240 s')));
  });
}

async function torrent(c, film) {
  const params = Object.assign({ kind: 'movie', videoId: film.id, itemId: film.id, title: film.title, releaseInfo: film.year }, CAPS);
  const versions = await c.subscribe('versions', params);
  const pick = (v) => [v.recommended].concat(v.rows || []).filter((r) => r && r.kind === 'torrent')[0];
  const row = await h.until('a torrent row for ' + film.title, () => pick(versions.now()), 120000);
  log('versions row', { film: film.title, key: row.key, status: row.status, quality: row.quality });
  const playback = await c.subscribe('playback', { versionsSub: versions.sub, key: row.key, kind: 'movie', itemId: film.id, videoId: film.id, name: film.title });
  const snap = await h.until('a lease for ' + film.title, () => playback.now().url && playback.now(), 240000);
  log('lease', { phase: snap.phase, url: snap.url.replace(/\/[^/]{20,}\//, '/<token>/').slice(0, 140) });
  const got = await firstBytes(snap.url);
  log('bytes', got);
  await c.req('unsubscribe', { sub: playback.sub });
  await c.req('unsubscribe', { sub: versions.sub });
  return { url: snap.url, got };
}

async function main() {
  if (!fs.existsSync(BIN)) throw new Error('no native core at ' + BIN + ': run apps/webos/scripts/build-native.sh');
  const started = Date.now();
  // qemu-user runs an ARM binary several times slower than the TV.
  const svc = await h.startService({ env: { MQ_NATIVE: BIN, MQ_NATIVE_READY_MS: '120000' } });
  svc.proc.stdout.on('data', (d) =>
    String(d)
      .split('\n')
      .filter((l) => l.indexOf('nativeLog') >= 0 || l.indexOf('retire') >= 0)
      .forEach((l) => console.error(l.slice(0, 400))),
  );
  await svc.waitFor('ready');
  const ready = svc.find('ready')[0];
  log('core ready', { core: ready.core, ms: Date.now() - started, node: process.version, arch: process.arch });
  const page = await h.get(svc, '/');
  const ws = await h.authed(svc, { query: '?core=native' });
  const c = client(ws);
  await c.req('installDefaults', { choices: [{ key: 'cinemeta', on: true }] });
  const home = await c.subscribe('home', {});
  const pushes = [];
  ws.ws.on('message', (raw) => {
    const m = JSON.parse(String(raw));
    if (m.sub === home.sub) pushes.push(m.updates);
  });
  const found = await h.until('Home catalogs from Cinemeta', () => tiles(pushes) + tiles(home.now()) >= 10, 120000).catch(() => false);
  log('home', { tilesWithCinemetaPosters: tiles(pushes) + tiles(home.now()), ok: !!found });

  const server = await addon();
  await c.req('installAddon', { url: 'http://127.0.0.1:' + server.address().port + '/manifest.json' });
  let played = null;
  for (const film of FILMS) {
    played = await torrent(c, film).catch((e) => {
      log('torrent failed', { film: film.title, error: String(e.message || e) });
      return null;
    });
    if (played && played.got.bytes > 0) break;
  }
  const checks = {
    nativeCore: ready.core === 'native',
    pageMarked: page.body.indexOf('<meta name="mq-core" content="native">') >= 0,
    homeFromCinemeta: !!found,
    torrentLeaseOnLoopback: !!played && /^http:\/\/127\.0\.0\.1:\d+\//.test(played.url),
    torrentServesBytes: !!played && (played.got.status === 206 || played.got.status === 200) && played.got.bytes > 0,
  };
  log('checks', checks);
  server.close();
  await svc.kill();
  const ok = Object.keys(checks).every((k) => checks[k]);
  console.log((ok ? 'PASS' : 'FAIL') + ' live native core on node ' + process.version + ' in ' + (Date.now() - started) + ' ms');
  process.exit(ok ? 0 : 1);
}

main().catch((e) => {
  console.log('FAIL ' + ((e && e.stack) || e));
  process.exit(1);
});
