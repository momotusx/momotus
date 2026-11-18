'use strict';
// A live session against the real native core (plans/webos.md v5): the
// service under this Node with the armv7 mq-bridge (build-native.sh; on an
// x86 runner through qemu-user's binfmt), a page socket with the token and
// Home's catalogs from Cinemeta over the network. Then, with MQ_FIXTURE
// (the fixture-host.js base URL, which CI serves from a network namespace on
// a public-looking address, and an aria2 seeder beside it):
// - a torrent of the e2e test film from a one-file addon on loopback: the
//   lease is the bridge's own loopback URL, its first bytes and bytes near
//   the end (a seek) match the film, and the position played comes back as
//   the next playback's resume point;
// - the plugin e2e's repository added as a user adds one, its provider's row
//   in Versions, and that row's lease serving the same film.
// Without MQ_FIXTURE the torrent is a public domain film from the public
// swarm (Blender's open films), the check for a Mac with no namespace.
// Not a *.test.js: it needs the network and a built core.
// Run: [MQ_FIXTURE=http://203.0.113.2:8000] MQ_LIVE_NATIVE=<binary> node test/live-native.js
// (WS_PATH as for run.sh).
const crypto = require('crypto');
const http = require('http');
const path = require('path');
const fs = require('fs');
const parse = require('url').parse;
const h = require('./harness.js');

const BIN = process.env.MQ_LIVE_NATIVE || path.join(__dirname, '..', 'bin', 'mq-bridge');
const FIXTURE = process.env.MQ_FIXTURE || '';
// Where this script itself reaches the fixture host, when not at FIXTURE (a Mac run of a development core
// whose MQ_DEV_REPLAY sends the core there instead).
const DIRECT = process.env.MQ_FIXTURE_DIRECT || FIXTURE;
const PUBLIC = [
  { id: 'tt1254207', title: 'Big Buck Bunny', year: '2008', hash: 'dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c', release: 'Big.Buck.Bunny.2008.480p.x264.AAC.mp4' },
  { id: 'tt1727587', title: 'Sintel', year: '2010', hash: '08ada5a7a6183aae1e09d831df6748d566095a10', release: 'Sintel.2010.720p.x264.AAC.mp4' },
];
const TRACKERS = ['udp://tracker.opentrackr.org:1337/announce', 'udp://explodie.org:6969', 'udp://tracker.torrent.eu.org:451/announce', 'udp://open.stealth.si:80/announce'];
// What a webOS TV plays with the native core (apps/web/src/core/webos-caps.ts): no directOnly.
const CAPS = {
  displayHeight: 1080,
  decoders: ['h264', 'h265', 'vp9'],
  tenBitHevc: true,
  audioDecoders: ['aac', 'ac3', 'eac3', 'mp3'],
  containers: ['mkv', 'mp4', 'ts', 'm3u8'],
  hasFallbackEngine: false,
};
const CHUNK = 65536;
// How long a lease may take (a torrent under qemu-user is slow to start).
const LEASE_MS = Number(process.env.MQ_LEASE_MS) || 240000;

const log = (what, value) => console.log(JSON.stringify({ step: what, value: value }));
const sha1 = (b) => crypto.createHash('sha1').update(b).digest('hex');

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

// A Stremio addon on loopback with one torrent stream per film.
function addon(films) {
  const manifest = { id: 'live.native', version: '1.0.0', name: 'Live check', resources: ['stream'], types: ['movie'], idPrefixes: ['tt'], catalogs: [] };
  const server = http.createServer((req, res) => {
    const film = films.filter((f) => req.url === '/stream/movie/' + f.id + '.json')[0];
    const stream = film && { name: 'Live check', title: film.release, infoHash: film.hash, sources: film.trackers.map((t) => 'tracker:' + t).concat('dht:' + film.hash) };
    const body = req.url === '/manifest.json' ? manifest : stream ? { streams: [stream] } : null;
    res.writeHead(body ? 200 : 404, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' });
    res.end(body ? JSON.stringify(body) : '');
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// `range` of `url` as the TV's player asks for it: status, bytes and their SHA-1.
function fetchRange(url, range) {
  return new Promise((resolve, reject) => {
    const req = http.get(Object.assign(parse(url), { headers: range ? { Range: range } : {} }), (res) => {
      const parts = [];
      res.on('data', (d) => parts.push(d));
      res.on('end', () => {
        const body = Buffer.concat(parts);
        resolve({ status: res.statusCode, bytes: body.length, sha1: sha1(body), type: res.headers['content-type'], body });
      });
    });
    req.on('error', reject);
    req.setTimeout(240000, () => req.destroy(new Error('no answer in 240 s')));
  });
}

const getJson = (url) => fetchRange(url).then((r) => JSON.parse(r.body.toString()));
const hidden = (url) => url.replace(/\/[^/]{20,}\//, '/<token>/').slice(0, 140);
// Every row the view lists: the pick, the addon rows, and the plugin and Archive sections.
const rowsOf = (v) => [v.recommended].concat(v.rows || [], (v.plugins && v.plugins.rows) || [], (v.archive && v.archive.rows) || []).filter(Boolean);

// One playback of `row` from `versions`; returns its subscription and first snapshot with a lease.
async function play(c, versions, row, film) {
  const playback = await c.subscribe('playback', { versionsSub: versions.sub, key: row.key, kind: 'movie', itemId: film.id, videoId: film.id, name: film.title });
  const snap = await h.until('a lease for ' + row.key, () => playback.now().url && playback.now(), LEASE_MS);
  log('lease', { key: row.key, phase: snap.phase, resumeMs: snap.resumeMs, start: snap.start, url: hidden(snap.url) });
  return { playback, snap };
}

// The public swarm: the first film whose torrent serves bytes passes.
async function publicTorrent(c) {
  const films = PUBLIC.map((f) => Object.assign({ trackers: TRACKERS }, f));
  const server = await addon(films);
  await c.req('installAddon', { url: 'http://127.0.0.1:' + server.address().port + '/manifest.json' });
  let result = { torrentLeaseOnLoopback: false, torrentServesBytes: false };
  for (const film of films) {
    try {
      const versions = await c.subscribe('versions', Object.assign({ kind: 'movie', videoId: film.id, itemId: film.id, title: film.title, releaseInfo: film.year }, CAPS));
      const row = await h.until('a torrent row', () => rowsOf(versions.now()).filter((r) => r.kind === 'torrent')[0], 120000);
      const { playback, snap } = await play(c, versions, row, film);
      const got = await fetchRange(snap.url, 'bytes=0-' + (CHUNK - 1));
      log('bytes', { status: got.status, bytes: got.bytes, type: got.type });
      await c.req('unsubscribe', { sub: playback.sub });
      await c.req('unsubscribe', { sub: versions.sub });
      result = { torrentLeaseOnLoopback: /^http:\/\/127\.0\.0\.1:\d+\//.test(snap.url), torrentServesBytes: got.status === 206 && got.bytes === CHUNK };
      if (result.torrentServesBytes) break;
    } catch (e) {
      log('torrent failed', { film: film.title, error: String(e.message || e) });
    }
  }
  server.close();
  return result;
}

// The fixture host's torrent, seek and resume, then its plugin repository.
async function fixtureRun(c) {
  const info = await getJson(DIRECT + '/info');
  const film = { id: 'tt0000417', title: 'Fixture Film', year: '2009', hash: info.infoHash, release: info.name, trackers: [FIXTURE + '/announce'] };
  const media = DIRECT + '/media/film.mp4';
  const head = await fetchRange(media, 'bytes=0-' + (CHUNK - 1));
  const tailStart = info.size - CHUNK - 4321;
  const tailRange = 'bytes=' + tailStart + '-' + (tailStart + CHUNK - 1);
  const tail = await fetchRange(media, tailRange);
  log('fixture', { infoHash: info.infoHash, size: info.size, head: head.sha1, tail: tail.sha1 });

  const server = await addon([film]);
  await c.req('installAddon', { url: 'http://127.0.0.1:' + server.address().port + '/manifest.json' });
  const plugins = await c.req('plugins');
  log('plugins', { available: plugins.available, repos: (plugins.repos || []).length });
  const added = await c.req('addPluginRepo', { url: info.repo });
  log('plugin repo', { repos: (added.repos || []).length });

  const params = Object.assign({ kind: 'movie', videoId: film.id, itemId: film.id, title: film.title, releaseInfo: film.year }, CAPS);
  let versions = await c.subscribe('versions', params);
  const torrentRow = await h.until('the fixture torrent row', () => rowsOf(versions.now()).filter((r) => r.kind === 'torrent')[0], 120000);
  const pluginRow = await h.until('the plugin row', () => rowsOf(versions.now()).filter((r) => r.origin === 'plugin')[0], 120000).catch(() => null);
  log('versions', { torrent: torrentRow.key, torrentStatus: torrentRow.status, plugin: pluginRow && pluginRow.key, rows: rowsOf(versions.now()).map((r) => r.kind + '/' + r.origin) });

  // Torrent: first bytes, a seek near the end, the position as the next start.
  const onLoopback = (u) => /^http:\/\/127\.0\.0\.1:\d+\//.test(u);
  const torrent = await (async () => {
    const first = await play(c, versions, torrentRow, film);
    const tStart = await fetchRange(first.snap.url, 'bytes=0-' + (CHUNK - 1));
    const tSeek = await fetchRange(first.snap.url, tailRange);
    log('torrent bytes', { start: [tStart.status, tStart.bytes, tStart.sha1 === head.sha1], seek: [tSeek.status, tSeek.bytes, tSeek.sha1 === tail.sha1] });
    await c.req('command', { sub: first.playback.sub, command: { type: 'progress', positionMs: 120000, durationMs: 600000, paused: false } });
    await h.sleep(1500);
    await c.req('unsubscribe', { sub: first.playback.sub });
    const again = await play(c, versions, torrentRow, film);
    log('resume', { resumeMs: again.snap.resumeMs });
    await c.req('unsubscribe', { sub: again.playback.sub });
    return {
      torrentLeaseOnLoopback: onLoopback(first.snap.url),
      torrentServesTheFilm: tStart.status === 206 && tStart.sha1 === head.sha1,
      torrentSeekServesTheFilm: tSeek.status === 206 && tSeek.sha1 === tail.sha1,
      torrentResumes: again.snap.resumeMs === 120000,
    };
  })().catch((e) => {
    log('torrent failed', String(e.message || e));
    return { torrentPlayed: false };
  });

  // Plugin: its row's lease serves the same film.
  let pBytes = null;
  let pUrl = '';
  if (pluginRow) {
    await c.req('unsubscribe', { sub: versions.sub });
    versions = await c.subscribe('versions', params);
    await h.until('the plugin row again', () => rowsOf(versions.now()).filter((r) => r.key === pluginRow.key)[0], 120000);
    const p = await play(c, versions, pluginRow, film);
    pUrl = p.snap.url;
    pBytes = await fetchRange(pUrl, 'bytes=0-' + (CHUNK - 1));
    log('plugin bytes', { status: pBytes.status, bytes: pBytes.bytes, same: pBytes.sha1 === head.sha1 });
    await c.req('unsubscribe', { sub: p.playback.sub });
  }
  const counts = (await getJson(DIRECT + '/info')).counts;
  log('fixture asked', counts);
  server.close();
  return Object.assign(torrent, {
    pluginsAvailable: plugins.available === true,
    pluginRowListed: !!pluginRow,
    pluginRanItsFetch: Object.keys(counts).some((k) => k.indexOf('/lookup/') === 0),
    pluginLeaseServesTheFilm: !!pBytes && onLoopback(pUrl) && pBytes.status === 206 && pBytes.sha1 === head.sha1,
  });
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
  await svc.waitFor('ready', 1);
  const ready = svc.find('ready')[0];
  log('core ready', { core: ready.core, ms: Date.now() - started, node: process.version, arch: process.arch, fixture: FIXTURE || null });
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

  const media = FIXTURE ? await fixtureRun(c) : await publicTorrent(c);
  const checks = Object.assign(
    {
      nativeCore: ready.core === 'native',
      pageMarked: page.body.indexOf('<meta name="mq-core" content="native">') >= 0,
      homeFromCinemeta: !!found,
      stayedNative: svc.find('ready').length === 1 && svc.find('native').length === 0,
    },
    media,
  );
  log('checks', checks);
  await svc.kill();
  const ok = Object.keys(checks).every((k) => checks[k]);
  console.log((ok ? 'PASS' : 'FAIL') + ' live native core on node ' + process.version + ' in ' + (Date.now() - started) + ' ms');
  process.exit(ok ? 0 : 1);
}

main().catch((e) => {
  console.log('FAIL ' + ((e && e.stack) || e));
  process.exit(1);
});
