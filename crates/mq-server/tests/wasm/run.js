'use strict';
// Host for mq-server's wasm detect run (wasm-test.sh), CommonJS for Node 8.12
// to 20: the service's own host.js against a local server. A ranged file, a
// server that ignores Range, an HLS playlist (and a player without HLS), a
// redirect chain whose last URL is the lease's, a refusal, and a subtitle
// served through `answer`.
// Usage: node run.js <glue.js>
const http = require('http');
const path = require('path');
const host = require('../../../../apps/webos/service/host.js');

let core;
host.install(() => core);
host.setOnDied((message) => {
  console.log('FAIL died: ' + message);
  process.exit(1);
});
core = require(path.resolve(process.argv[2]));

const FILE = Buffer.alloc(1 << 20, 7);
const PLAYLIST = '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXT-X-ENDLIST\n';
const SRT = '1\n00:00:01,000 --> 00:00:02,000\nHello\n';
let ignoredClosedEarly = false;

function route(req, res) {
  const range = /bytes=(\d+)-(\d+)/.exec(req.headers.range || '');
  switch (req.url) {
    case '/film.mp4': {
      if (!range) {
        res.writeHead(200, { 'Content-Type': 'video/mp4', 'Content-Length': FILE.length });
        return res.end(FILE);
      }
      const start = Number(range[1]);
      const end = Math.min(Number(range[2]), FILE.length - 1);
      res.writeHead(206, {
        'Content-Type': 'video/mp4',
        'Content-Range': 'bytes ' + start + '-' + end + '/' + FILE.length,
        'Content-Length': end - start + 1,
      });
      return res.end(FILE.slice(start, end + 1));
    }
    case '/whole.mp4':
      // Ignores Range: the whole file every time, written slowly enough that
      // the host's early close shows.
      res.writeHead(200, { 'Content-Type': 'video/mp4', 'Content-Length': FILE.length });
      res.on('close', () => {
        if (!res.finished) ignoredClosedEarly = true;
      });
      res.write(FILE.slice(0, 256 << 10));
      return setTimeout(() => res.end(FILE.slice(256 << 10)), 500);
    case '/live.m3u8':
      res.writeHead(200, { 'Content-Type': 'application/vnd.apple.mpegurl' });
      return res.end(PLAYLIST);
    case '/r1':
      res.writeHead(302, { Location: '/r2' });
      return res.end();
    case '/r2':
      res.writeHead(301, { Location: '/film.mp4' });
      return res.end();
    case '/gone.mp4':
      res.writeHead(404, { 'Content-Type': 'text/plain' });
      return res.end('gone');
    case '/en.srt':
      res.writeHead(200, { 'Content-Type': 'application/x-subrip' });
      return res.end(SRT);
    default:
      res.writeHead(404);
      return res.end();
  }
}

const server = http.createServer(route).listen(0, '127.0.0.1', async () => {
  const base = 'http://127.0.0.1:' + server.address().port;
  const addon = base + '/manifest.json';
  const open = (p, hls, sub) => core.open_lease(base + p, addon, hls, sub ? base + sub : '').then(JSON.parse);
  const cases = [
    ['a ranged file is leased under its own URL', () => open('/film.mp4', true), (r) => r.url === base + '/film.mp4' && r.format === null],
    [
      'a server that ignores Range is read only to the sniff size',
      // The server sees the dropped connection a moment after the answer.
      () => open('/whole.mp4', true).then((r) => new Promise((ok) => setTimeout(() => ok(r), 300))),
      (r) => r.url === base + '/whole.mp4' && r.format === null && ignoredClosedEarly,
    ],
    ['an HLS playlist is leased as listed', () => open('/live.m3u8', true), (r) => r.url === base + '/live.m3u8' && r.format === 'm3u8'],
    ['HLS for a player without it fails with the format words', () => open('/live.m3u8', false), (r) => /format/.test(r.error)],
    ['a redirect chain leases its final URL', () => open('/r1', true), (r) => r.url === base + '/film.mp4'],
    ['a 404 fails with its words', () => open('/gone.mp4', true), (r) => r.error === 'The link no longer exists'],
    [
      'a subtitle is served as WebVTT, never cached',
      () => open('/film.mp4', true, '/en.srt'),
      (r) =>
        r.subtitle.status === 200 &&
        /^WEBVTT/.test(r.subtitle.body) &&
        /Hello/.test(r.subtitle.body) &&
        r.subtitle.headers.some((h) => h[0] === 'cache-control' && h[1] === 'no-store'),
    ],
  ];
  let failed = 0;
  for (const [name, run, check] of cases) {
    let result;
    try {
      result = await run();
    } catch (e) {
      result = { thrown: String(e) };
    }
    let ok = false;
    try {
      ok = check(result);
    } catch (e) {}
    if (!ok) failed += 1;
    console.log((ok ? 'ok   ' : 'FAIL ') + name + (ok ? '' : ': ' + JSON.stringify(result)));
  }
  console.log(process.version + ': ' + (cases.length - failed) + ' passed, ' + failed + ' failed');
  server.close();
  process.exit(failed ? 1 : 0);
});
