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
//   in Versions, and that row's lease serving the same film;
// - a download of the torrent kept under <data>/native/downloads, which then
//   plays from the bridge's loopback server.
// MQ_REAL_PLUGINS=1 adds the phone's own default repositories (Manitux, then
// NiakVIO): both must install with their providers; the rows they list for a
// known film and whether a site lets that link play are logged only, since
// those sites answer a datacenter address as they please. With the wasm core at service/core (build-wasm.sh), the
// run starts with an update from the wasm-only app: the wasm core installs
// an addon in the data dir, then the native core must find it there.
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
const WASM_CORE = path.join(__dirname, '..', 'core', 'mq_wasm.js');
// The phone's default repositories (crates/mq-runtime/src/plugins.rs defaults) and films any of them lists.
const REAL_REPOS = [
  'https://raw.githubusercontent.com/manitux-app/nuvio-test-plugins/refs/heads/main/manifest.json',
  'https://raw.githubusercontent.com/niakw/NiakVIO/main/manifest.json',
];
const REAL_FILMS = [
  { id: 'tt0111161', title: 'The Shawshank Redemption', year: '1994' },
  { id: 'tt0468569', title: 'The Dark Knight', year: '2008' },
  { id: 'tt1375666', title: 'Inception', year: '2010' },
];
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
function addon(films, name) {
  const manifest = { id: name ? 'live.update' : 'live.native', version: '1.0.0', name: name || 'Live check', resources: ['stream'], types: ['movie'], idPrefixes: ['tt'], catalogs: [] };
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
  // A lease, or the view giving up on this version (a provider's dead link): then no url, and the caller moves on.
  const snap = await h.until('a lease for ' + row.key, () => (playback.now().url || playback.now().phase === 'failed') && playback.now(), LEASE_MS);
  if (!snap.url) {
    log('lease failed', { key: row.key, attempts: snap.attempts });
    await c.req('unsubscribe', { sub: playback.sub });
    throw new Error('no lease for ' + row.key);
  }
  log('lease', { key: row.key, leased: snap.key, attempts: (snap.attempts || []).length, phase: snap.phase, resumeMs: snap.resumeMs, start: snap.start, url: hidden(snap.url) });
  return { playback, snap };
}

// The public swarm: the first film whose torrent serves bytes passes.
async function publicTorrent(c, data) {
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
      result = { torrentLeaseIsTheTorrent: snap.key === row.key, torrentLeaseOnLoopback: /^http:\/\/127\.0\.0\.1:\d+\//.test(snap.url), torrentServesBytes: got.status === 206 && got.bytes === CHUNK };
      // MQ_PUBLIC_DOWNLOAD=1: the whole film kept and played from the copy (a Mac check; hundreds of MB).
      if (process.env.MQ_PUBLIC_DOWNLOAD === '1') {
        const params = Object.assign({ kind: 'movie', videoId: film.id, itemId: film.id, title: film.title, releaseInfo: film.year }, CAPS);
        Object.assign(result, await downloadAndPlay(c, data, film, params, versions, row, 'bytes=1000000-' + (1000000 + CHUNK - 1), (r) => r.bytes === CHUNK));
      }
      await c.req('unsubscribe', { sub: versions.sub });
      if (result.torrentServesBytes) break;
    } catch (e) {
      log('torrent failed', { film: film.title, error: String(e.message || e) });
    }
  }
  server.close();
  return result;
}

// The fixture host's torrent, seek and resume, then its plugin repository.
async function fixtureRun(c, data) {
  const info = await getJson(DIRECT + '/info');
  const film = { id: 'tt0000417', title: 'Fixture Film', year: '2009', hash: info.infoHash, release: info.name, trackers: [FIXTURE + '/announce'] };
  const media = DIRECT + '/media/film.mp4';
  const head = await fetchRange(media, 'bytes=0-' + (CHUNK - 1));
  const tailStart = info.size - CHUNK - 4321;
  const tailRange = 'bytes=' + tailStart + '-' + (tailStart + CHUNK - 1);
  const tail = await fetchRange(media, tailRange);
  // A seek into the middle, after the head was read.
  const midStart = Math.floor(info.size / 2) + 777;
  const midRange = 'bytes=' + midStart + '-' + (midStart + CHUNK - 1);
  const mid = await fetchRange(media, midRange);
  log('fixture', { infoHash: info.infoHash, size: info.size, head: head.sha1, mid: mid.sha1, tail: tail.sha1 });

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
    const tMid = await fetchRange(first.snap.url, midRange);
    const tSeek = await fetchRange(first.snap.url, tailRange);
    log('torrent bytes', {
      start: [tStart.status, tStart.bytes, tStart.sha1 === head.sha1],
      middle: [tMid.status, tMid.bytes, tMid.sha1 === mid.sha1],
      end: [tSeek.status, tSeek.bytes, tSeek.sha1 === tail.sha1],
    });
    await c.req('command', { sub: first.playback.sub, command: { type: 'progress', positionMs: 120000, durationMs: 840000, paused: false } });
    await h.sleep(1500);
    await c.req('unsubscribe', { sub: first.playback.sub });
    const again = await play(c, versions, torrentRow, film);
    log('resume', { resumeMs: again.snap.resumeMs });
    await c.req('unsubscribe', { sub: again.playback.sub });
    return {
      // The lease is the torrent itself, not another version the view moved on to.
      torrentLeaseIsTheTorrent: first.snap.key === torrentRow.key && again.snap.key === torrentRow.key,
      torrentLeaseOnLoopback: onLoopback(first.snap.url),
      torrentServesTheFilm: tStart.status === 206 && tStart.sha1 === head.sha1,
      torrentSeekToTheMiddle: tMid.status === 206 && tMid.sha1 === mid.sha1,
      torrentSeekToTheEnd: tSeek.status === 206 && tSeek.sha1 === tail.sha1,
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
    pUrl = p.snap.key === pluginRow.key ? p.snap.url : '';
    pBytes = await fetchRange(pUrl, 'bytes=0-' + (CHUNK - 1));
    log('plugin bytes', { status: pBytes.status, bytes: pBytes.bytes, same: pBytes.sha1 === head.sha1 });
    await c.req('unsubscribe', { sub: p.playback.sub });
  }
  // Download: the torrent kept on the TV, then played from the copy.
  const download = await downloadAndPlay(c, data, film, params, versions, torrentRow, midRange, (got) => got.sha1 === mid.sha1, info.size).catch((e) => {
    log('download failed', String(e.message || e));
    return { downloadPlayed: false };
  });

  const counts = (await getJson(DIRECT + '/info')).counts;
  log('fixture asked', counts);
  server.close();
  return Object.assign(torrent, download, {
    pluginsAvailable: plugins.available === true,
    pluginRowListed: !!pluginRow,
    pluginRanItsFetch: Object.keys(counts).some((k) => k.indexOf('/lookup/') === 0),
    pluginLeaseServesTheFilm: !!pBytes && onLoopback(pUrl) && pBytes.status === 206 && pBytes.sha1 === head.sha1,
    // The swarm was asked: the fixture tracker heard the core.
    trackerAnnounced: (counts['/announce'] || 0) > 0,
  });
}

// `row` of `versions` downloaded as the title page does, kept under <data>/native (a torrent stays in its torrents dir,
// kept from the cache sweep; `size` bytes when known, else over 1 MB), then played from the copy: its row on the device, its lease on loopback, `range` passing `same`.
async function downloadAndPlay(c, data, film, params, versions, row, range, same, size) {
  // The network as the page reports it at load (core/client.ts networkChanged): no download runs before it.
  await c.req('lifecycle', { event: 'networkChanged', network: 'ethernet', metered: false, lowData: false });
  await c.req('setDownloadSettings', { wifiOnly: false });
  const added = await c.req('downloadAdd', { kind: 'movie', itemId: film.id, scope: 'video', videoId: film.id, params, pick: { versionsSub: versions.sub, key: row.key } });
  const board = await c.subscribe('downloads', {});
  await h.until('the download done', () => JSON.stringify(board.now()).indexOf('"state":"done"') >= 0, LEASE_MS).catch((e) => {
    const b = board.now() || {};
    log('download board', { active: b.active, storage: b.storage, rows: (b.downloading || []).map((r) => [r.state, r.reason, r.bytesDone, r.bytesTotal]) });
    throw e;
  });
  const files = walk(path.join(data, 'native')).filter((f) => fs.statSync(f).size > 1024 * 1024);
  log('download', { ids: added.ids, files: files.map((f) => [path.relative(data, f), fs.statSync(f).size]) });
  await c.req('unsubscribe', { sub: board.sub });
  const again = await c.subscribe('versions', params);
  const kept = await h.until('the downloaded row', () => rowsOf(again.now()).filter((r) => r.onDevice)[0], 120000);
  const p = await play(c, again, kept, film);
  const got = await fetchRange(p.snap.url, range);
  log('download plays', { key: kept.key, onDevice: kept.onDevice, status: got.status, bytes: got.bytes, same: same(got) });
  await c.req('unsubscribe', { sub: p.playback.sub });
  await c.req('unsubscribe', { sub: again.sub });
  return {
    downloadKeptUnderTheDataDir: files.some((f) => !size || fs.statSync(f).size === size),
    downloadPlaysFromLoopback: p.snap.key === kept.key && /^http:\/\/127\.0\.0\.1:\d+\//.test(p.snap.url) && got.status === 206 && same(got),
  };
}

// What runs the core: the binary's ELF header (`file`), and on Linux the bridge process's own exe and command line
// (under binfmt, exe is qemu-arm and the command line names the ARM binary).
function whatRuns(bin, pidFile) {
  const run = (cmd, args) => {
    try {
      return require('child_process').execFileSync(cmd, args, { encoding: 'utf8' }).trim();
    } catch (e) {
      return null;
    }
  };
  const pid = fs.existsSync(pidFile) ? fs.readFileSync(pidFile, 'utf8').trim() : null;
  const proc = (name, read) => {
    try {
      return read('/proc/' + pid + '/' + name);
    } catch (e) {
      return null;
    }
  };
  return {
    file: run('file', ['-b', bin]),
    pid,
    exe: pid && proc('exe', fs.readlinkSync),
    cmdline: pid && proc('cmdline', (p) => fs.readFileSync(p, 'utf8').split('\0').filter(Boolean).join(' ')),
    host: run('uname', ['-m']),
  };
}

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir).reduce((all, name) => {
    const p = path.join(dir, name);
    return all.concat(fs.statSync(p).isDirectory() ? walk(p) : [p]);
  }, []);
}

// The phone's default repositories, as a user adds them. Gating is what our code does alone: each installs with its
// providers. The rows they list for a known film and whether a row's link plays are the sites' to decide (from a
// datacenter address they refuse, or list nothing), so both are logged, never gating; the fixture's plugin gates the
// plugin engine end to end.
async function realPlugins(c) {
  const result = { realReposInstall: true, realPluginRows: false };
  for (const repo of REAL_REPOS) {
    const added = await c.req('addPluginRepo', { url: repo }).catch((e) => ({ error: String(e.message || e) }));
    const info = (added.repos || []).filter((r) => r.url === repo)[0];
    log('real repo', { repo, error: added.error || (info && info.problem) || null, providers: info ? info.providers.length : 0 });
    if (!info || !info.providers.length) result.realReposInstall = false;
  }
  for (const film of REAL_FILMS) {
    const versions = await c.subscribe('versions', Object.assign({ kind: 'movie', videoId: film.id, itemId: film.id, title: film.title, releaseInfo: film.year }, CAPS));
    await h.until('the plugin pass', () => versions.now().done && !(versions.now().plugins || {}).pending, 120000).catch(() => null);
    const rows = ((versions.now().plugins || {}).rows || []).filter((r) => r.origin === 'plugin');
    log('real plugin rows', { film: film.title, rows: rows.map((r) => [r.sourceName, r.quality, r.host]) });
    if (rows.length) result.realPluginRows = true;
    // Information only: the first row's link through the bridge, as the player would ask for it.
    if (rows.length && !result.leaseTried) {
      result.leaseTried = true;
      const p = await play(c, versions, rows[0], film).catch((e) => ({ error: String(e.message || e) }));
      const got = p.snap && (await fetchRange(p.snap.url, 'bytes=0-' + (CHUNK - 1)).catch((e) => ({ status: 0, bytes: 0, error: String(e.message) })));
      log('real plugin lease (information)', p.snap ? { key: p.snap.key === rows[0].key, manifest: p.snap.manifest, status: got.status, bytes: got.bytes, type: got.type } : { error: p.error });
      if (p.playback) await c.req('unsubscribe', { sub: p.playback.sub });
    }
    await c.req('unsubscribe', { sub: versions.sub });
  }
  delete result.leaseTried;
  return result;
}

// An update from the wasm-only app: the wasm core installs the addon at `addonUrl` into `dir`'s data.
async function wasmSession(dir, addonUrl) {
  const svc = await h.startService({ dir, env: { MQ_NATIVE: '', MQ_CORE: WASM_CORE } });
  await svc.waitFor('ready');
  const c = client(await h.authed(svc));
  await c.req('installAddon', { url: addonUrl });
  const addons = await c.subscribe('addons', {});
  const listed = JSON.stringify(addons.now()).indexOf('Update check') >= 0;
  log('wasm session', { core: svc.find('ready')[0].core, addonListed: listed });
  await svc.kill();
  return listed;
}

async function main() {
  if (!fs.existsSync(BIN)) throw new Error('no native core at ' + BIN + ': run apps/webos/scripts/build-native.sh');
  const started = Date.now();
  const dir = h.tmpDir();
  const marker = await addon([], 'Update check');
  const markerUrl = 'http://127.0.0.1:' + marker.address().port + '/manifest.json';
  const updated = fs.existsSync(WASM_CORE) ? await wasmSession(dir, markerUrl) : null;
  // qemu-user runs an ARM binary several times slower than the TV.
  const svc = await h.startService({ dir, env: { MQ_NATIVE: BIN, MQ_NATIVE_CAP_MS: '600000', MQ_NATIVE_QUIET_MS: '120000' } });
  svc.proc.stdout.on('data', (d) =>
    String(d)
      .split('\n')
      .filter((l) => l.indexOf('nativeLog') >= 0 || l.indexOf('retire') >= 0)
      .forEach((l) => console.error(l.slice(0, 400))),
  );
  await svc.waitFor('ready', 1);
  const ready = svc.find('ready')[0];
  log('core ready', { core: ready.core, ms: Date.now() - started, node: process.version, arch: process.arch, fixture: FIXTURE || null });
  log('native binary', whatRuns(BIN, path.join(svc.data, 'core.pid')));
  const page = await h.get(svc, '/');
  const ws = await h.authed(svc, { query: '?core=native' });
  const c = client(ws);
  let update = {};
  if (updated !== null) {
    const addons = await c.subscribe('addons', {});
    update = { wasmLibraryWritten: updated, nativeFindsTheWasmLibrary: JSON.stringify(addons.now()).indexOf('Update check') >= 0, adoptedOnce: svc.find('adopted').length === 1 };
    log('update from wasm', Object.assign({ adopted: svc.find('adopted')[0] || null }, update));
    await c.req('unsubscribe', { sub: addons.sub });
  }
  await c.req('installDefaults', { choices: [{ key: 'cinemeta', on: true }] });
  const home = await c.subscribe('home', {});
  const pushes = [];
  ws.ws.on('message', (raw) => {
    const m = JSON.parse(String(raw));
    if (m.sub === home.sub) pushes.push(m.updates);
  });
  const found = await h.until('Home catalogs from Cinemeta', () => tiles(pushes) + tiles(home.now()) >= 10, 120000).catch(() => false);
  log('home', { tilesWithCinemetaPosters: tiles(pushes) + tiles(home.now()), ok: !!found });

  const media = FIXTURE ? await fixtureRun(c, svc.data) : await publicTorrent(c, svc.data);
  const real = process.env.MQ_REAL_PLUGINS === '1' ? await realPlugins(c) : {};
  marker.close();
  const checks = Object.assign(
    {
      nativeCore: ready.core === 'native',
      pageMarked: page.body.indexOf('<meta name="mq-core" content="native">') >= 0,
      homeFromCinemeta: !!found,
      stayedNative: svc.find('ready').length === 1 && svc.find('native').length === 0,
    },
    update,
    media,
    { realReposInstall: real.realReposInstall !== false },
  );
  if (real.realPluginRows !== undefined) log('real plugin rows listed (information)', real.realPluginRows);
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
