'use strict';
// mq-http's webOS transport (examples/host_e2e.rs) over the service's real
// host (apps/webos/service/host.js) against local fixture servers: hops,
// gzip and the cap after it, credentials on a cross-origin redirect,
// private addresses, DNS, a timeout, an abort, prefixes and statuses.
// CommonJS for Node 8.12 to 20. Usage: node run.js <glue.js>
const http = require('http');
const path = require('path');
const zlib = require('zlib');
const host = require(path.join(__dirname, '../../../../apps/webos/service/host.js'));

let core = null;
host.setOnDied((message) => {
  console.log('died: ' + message);
  process.exit(3);
});
host.install(() => core);
core = require(path.resolve(process.argv[2]));

const hits = {};
const closed = {};

function listen(handler) {
  return new Promise((resolve) => {
    const server = http.createServer(handler);
    server.listen(0, '127.0.0.1', () => resolve(server));
  });
}

function main(other) {
  return (req, res) => {
    const route = req.url.split('?')[0];
    hits[route] = (hits[route] || 0) + 1;
    const send = (status, headers, body) => {
      res.writeHead(status, headers);
      res.end(body);
    };
    switch (route) {
      case '/ok':
        return send(200, { 'Content-Type': 'text/plain' }, 'hello');
      case '/r1':
        return send(302, { Location: '/ok' }, '');
      case '/loop':
        return send(302, { Location: '/loop' }, '');
      case '/gz':
        return send(200, { 'Content-Encoding': 'gzip' }, zlib.gzipSync(Buffer.alloc(100, 'a')));
      case '/echo':
        return send(200, { 'Content-Type': 'application/json' }, JSON.stringify(req.headers));
      case '/same':
        return send(302, { Location: '/echo' }, '');
      case '/cross':
        return send(302, { Location: 'http://127.0.0.1:' + other + '/echo' }, '');
      case '/big':
        return send(200, {}, Buffer.alloc(10000, 'b'));
      case '/forbidden':
        return send(403, {}, Buffer.alloc(5000, 'f'));
      case '/busy':
        return send(429, { 'Retry-After': '7' }, 'wait');
      case '/slow':
        req.socket.on('close', () => (closed.slow = true));
        return undefined;
      case '/hang':
        return undefined;
      default:
        return send(404, {}, '');
    }
  };
}

function echo(req, res) {
  res.writeHead(200, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(req.headers));
}

function ask(question) {
  return core.get(JSON.stringify(question)).then((text) => JSON.parse(text));
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function run() {
  const second = await listen(echo);
  const other = second.address().port;
  const first = await listen(main(other));
  const port = first.address().port;
  const base = 'http://127.0.0.1:' + port;
  const creds = 'http://user:pw@127.0.0.1:' + port;
  const local = (route, extra) => Object.assign({ url: base + route, allowPrivate: true }, extra || {});
  const keys = [['X-Api-Key', 'secret'], ['Referer', 'https://site.example/']];
  const basic = 'Basic ' + Buffer.from('user:pw').toString('base64');
  let started = 0;
  const cases = [
    ['a plain answer', local('/ok'), (r) => r.status === 200 && r.body === 'hello'],
    ['a redirect is followed', local('/r1'), (r) => r.body === 'hello' && /\/ok$/.test(r.url)],
    ['a loop stops at the hop cap', local('/loop'), (r) => /too many redirects/.test(r.text)],
    ['gzip is decoded', local('/gz'), (r) => r.len === 100],
    ['the cap holds after gzip', local('/gz', { maxBytes: 50 }), (r) => r.error === 'too large'],
    [
      'credentials reach their own origin',
      { url: creds + '/same', allowPrivate: true, headers: keys },
      (r) => {
        const h = JSON.parse(r.body);
        return h.authorization === basic && h['x-api-key'] === 'secret' && h['accept-encoding'] === 'gzip';
      },
    ],
    [
      'credentials stay off another origin',
      { url: creds + '/cross', allowPrivate: true, headers: keys },
      (r) => {
        const h = JSON.parse(r.body);
        return !h.authorization && !h['x-api-key'] && h.referer === keys[1][1] && h.host === '127.0.0.1:' + other;
      },
    ],
    [
      'a private address is refused unscoped',
      { url: base + '/ok' },
      (r) => r.error === 'blocked address' && hits['/ok'] === 2,
    ],
    ['a private address is refused scoped', { url: base + '/ok', scoped: true }, (r) => r.error === 'blocked address'],
    ['a name that does not resolve', { url: 'http://nothing.invalid/' }, (r) => r.unresolved === true],
    [
      'a timeout',
      local('/hang', { timeoutMs: 300 }),
      (r) => r.error === 'timeout' && Date.now() - started < 2000,
    ],
    ['a dropped request is aborted', local('/slow', { race: base + '/ok' }), (r) => r.body === 'hello'],
    ['a prefix of an ignored range', local('/big', { prefix: 100 }), (r) => r.status === 200 && r.len === 100 && r.truncated],
    ['a prefix of an error body', local('/forbidden', { prefix: 100 }), (r) => r.status === 403 && r.len === 100],
    ['a status is an error', local('/busy'), (r) => r.error === 'http status'],
  ];
  let failed = 0;
  for (const [name, question, check] of cases) {
    started = Date.now();
    const result = await ask(question);
    let ok = false;
    try {
      ok = check(result);
    } catch (e) {}
    if (!ok) failed++;
    console.log((ok ? 'ok   ' : 'FAIL ') + name + (ok ? '' : ': ' + JSON.stringify(result)));
  }
  // The host closed the slow request once the race dropped it.
  await wait(200);
  const aborted = closed.slow === true;
  if (!aborted) failed++;
  console.log((aborted ? 'ok   ' : 'FAIL ') + 'the host closed the dropped connection');
  console.log(process.version + ': ' + (cases.length + 1 - failed) + ' passed, ' + failed + ' failed');
  first.close();
  second.close();
  process.exit(failed ? 1 : 0);
}

run().catch((e) => {
  console.log('error: ' + ((e && e.stack) || e));
  process.exit(1);
});
