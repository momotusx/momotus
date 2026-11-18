'use strict';
// The live check's own public host (plans/webos.md v5): the native core
// refuses loopback and LAN peers and links (the C27 address policy), so CI
// runs this in a network namespace on a documentation address
// (203.0.113.2), which the core counts as public. It serves:
// - a torrent of the e2e test film (apps/web/e2e/fixtures/hevc-160x90.mp4
//   with a 2 MiB free box after it, so it spans 33 pieces) and its tracker,
//   which names one peer: the aria2 seeder beside it on SEED_PORT;
// - the plugin e2e's repository (crates/mq-plugins/tests/fixtures/
//   plugin-repo), its lookups and its film, with Range.
// A repository link must be https: with FIXTURE_CERT and FIXTURE_KEY (a
// certificate for <host>, which the core trusts through SSL_CERT_FILE) it
// also listens with TLS on <port>+1, and the repository and its provider are
// named there. It writes the film (under its torrent name) and film.torrent
// into <dir>, for the seeder, and prints one JSON line with the info hash.
// Run: node fixture-host.js <host> <port> <dir>.
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const https = require('https');
const path = require('path');

const [host, portText, dir] = process.argv.slice(2);
const port = Number(portText);
const SEED_PORT = Number(process.env.SEED_PORT) || 6881;
const ROOT = path.join(__dirname, '..', '..', '..', '..');
const TLS = process.env.FIXTURE_CERT && { cert: fs.readFileSync(process.env.FIXTURE_CERT), key: fs.readFileSync(process.env.FIXTURE_KEY) };
// FIXTURE_BASE and FIXTURE_TLS_BASE: the names the core asks by, when they are not where this listens (a
// replayed Mac run, which serves both over plain http).
const BASE = process.env.FIXTURE_BASE || 'http://' + host + ':' + port;
const TLS_BASE = process.env.FIXTURE_TLS_BASE || (TLS ? 'https://' + host + ':' + (port + 1) : BASE);
const NAME = 'Fixture.Film.2009.1080p.WEB.x265.AAC.mp4';
const PIECE = 64 * 1024;

// A free box (size, 'free', zeros): players skip it, the file grows.
const fixture = fs.readFileSync(path.join(ROOT, 'apps/web/e2e/fixtures/hevc-160x90.mp4'));
const pad = Buffer.alloc(2 * 1024 * 1024);
pad.writeUInt32BE(pad.length, 0);
pad.write('free', 4, 'latin1');
const film = Buffer.concat([fixture, pad]);

function bencode(v) {
  if (Buffer.isBuffer(v)) return Buffer.concat([Buffer.from(v.length + ':'), v]);
  if (typeof v === 'string') return bencode(Buffer.from(v));
  if (typeof v === 'number') return Buffer.from('i' + v + 'e');
  const keys = Object.keys(v).sort();
  return Buffer.concat([Buffer.from('d')].concat(keys.map((k) => Buffer.concat([bencode(k), bencode(v[k])]))).concat(Buffer.from('e')));
}

const hashes = [];
for (let at = 0; at < film.length; at += PIECE) hashes.push(crypto.createHash('sha1').update(film.slice(at, at + PIECE)).digest());
const info = { length: film.length, name: NAME, 'piece length': PIECE, pieces: Buffer.concat(hashes) };
const infoHash = crypto.createHash('sha1').update(bencode(info)).digest('hex');
fs.writeFileSync(path.join(dir, 'film.torrent'), bencode({ announce: BASE + '/announce', info }));
// The film is the torrent's name in the seeder's dir.
fs.writeFileSync(path.join(dir, NAME), film);

const peer = Buffer.alloc(6);
host.split('.').forEach((o, i) => peer.writeUInt8(Number(o), i));
peer.writeUInt16BE(SEED_PORT, 4);

const repo = path.join(ROOT, 'crates/mq-plugins/tests/fixtures/plugin-repo');
const script = fs
  .readFileSync(path.join(repo, 'test-film.js'), 'utf8')
  .split('{{BASE}}')
  .join(TLS_BASE)
  .split('/media/test-film.webm')
  .join('/media/film.mp4');

function ranged(req, res, body, type) {
  const m = /^bytes=(\d+)-(\d*)$/.exec(req.headers.range || '');
  if (!m) {
    res.writeHead(200, { 'Content-Type': type, 'Content-Length': body.length, 'Accept-Ranges': 'bytes' });
    return res.end(body);
  }
  const start = Number(m[1]);
  const end = Math.min(m[2] ? Number(m[2]) : body.length - 1, body.length - 1);
  if (start > end) {
    res.writeHead(416, { 'Content-Range': 'bytes */' + body.length });
    return res.end();
  }
  res.writeHead(206, { 'Content-Type': type, 'Content-Length': end - start + 1, 'Content-Range': 'bytes ' + start + '-' + end + '/' + body.length, 'Accept-Ranges': 'bytes' });
  res.end(body.slice(start, end + 1));
}

const counts = {};
function answer(req, res) {
    // A development core with MQ_DEV_REPLAY pointed here asks as /<scheme>/<host><path>.
    const url = req.url.split('?')[0].replace(/^\/https?\/[^/]+(?=\/)/, '');
    counts[url] = (counts[url] || 0) + 1;
    const json = (v) => {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(v));
    };
    if (url === '/info') return json({ infoHash, size: film.length, name: NAME, base: BASE, repo: TLS_BASE + '/repo/manifest.json', counts });
    if (url === '/announce') {
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      return res.end(bencode({ interval: 1800, peers: peer }));
    }
    if (url === '/repo/manifest.json') return ranged(req, res, fs.readFileSync(path.join(repo, 'manifest.json')), 'application/json');
    if (url === '/repo/test-film.js') return ranged(req, res, Buffer.from(script), 'text/javascript');
    if (url.indexOf('/lookup/') === 0) return json({ title: 'Fixture Film' });
    if (url === '/media/film.mp4') return ranged(req, res, film, 'video/mp4');
    res.writeHead(404);
    res.end();
}
http.createServer(answer).listen(port, host, () => console.log(JSON.stringify({ fixtureHost: BASE, tls: TLS_BASE, infoHash, size: film.length, pieces: hashes.length })));
if (TLS) https.createServer(TLS, answer).listen(port + 1, host);
