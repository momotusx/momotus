'use strict';
// App updates on webOS (plans/auto-update.md section 2): the signed feed, this
// app's IPK checked (size, SHA-256, its control's Package) into a private dir
// under /tmp, and on the user's Install a detached update-install.js that asks
// appInstallService to install it and opens the app again. Node 8 to 20, no
// dependency: ECDSA P-256 through crypto.createVerify.
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const https = require('https');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { spawn } = require('child_process');
const { URL } = require('url');
const { caList } = require('./host.js');

const FEED = 'https://github.com/momotusx/momotus/releases/download/updates/latest.json';
const MAX_FEED = 64 * 1024;
const MAX_IPK = 200 * 1024 * 1024;
const MAX_HOPS = 5;
const TIMEOUT_MS = 30000;

// "1.2.3" to [1, 2, 3], or null.
function parts(v) {
  if (typeof v !== 'string' || !/^\d+\.\d+\.\d+$/.test(v)) return null;
  return v.split('.').map(Number);
}

function newer(a, b) {
  const x = parts(a);
  const y = parts(b);
  if (!x || !y) return false;
  for (let i = 0; i < 3; i += 1) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
}

// The feed's body when the envelope carries the key's signature, else null.
function verified(text, keyPem) {
  try {
    const outer = JSON.parse(text);
    if (typeof outer.signed !== 'string' || typeof outer.sig !== 'string') return null;
    const ok = crypto.createVerify('SHA256').update(Buffer.from(outer.signed, 'utf8')).verify(keyPem, outer.sig, 'base64');
    if (!ok) return null;
    const body = JSON.parse(outer.signed);
    return body && typeof body === 'object' && !Array.isArray(body) ? body : null;
  } catch (e) {
    return null;
  }
}

// The webos entry when it is well formed and newer than `own`, else null.
function pick(body, own) {
  const e = body && body.webos;
  if (!e || typeof e !== 'object') return null;
  if (!parts(e.version) || typeof e.url !== 'string' || e.url.indexOf('https://') !== 0) return null;
  if (typeof e.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(e.sha256)) return null;
  if (typeof e.size !== 'number' || !(e.size > 0) || e.size > MAX_IPK) return null;
  return newer(e.version, own) ? { version: e.version, url: e.url, sha256: e.sha256, size: e.size } : null;
}

// One GET, redirects followed (GitHub answers an asset with a 302), the body
// streamed to `sink` and stopped past `limit` bytes. Resolves the byte count.
function get(url, limit, sink, hops) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const lib = u.protocol === 'https:' ? https : u.protocol === 'http:' ? http : null;
    if (!lib) return reject(new Error('not http: ' + url));
    // Node 8 takes no (url, options) pair: the URL goes in as options.
    const options = {
      protocol: u.protocol,
      hostname: u.hostname,
      port: u.port || undefined,
      path: u.pathname + u.search,
      headers: { 'User-Agent': 'Momotus' },
      ca: u.protocol === 'https:' ? caList() : undefined,
      timeout: TIMEOUT_MS,
    };
    const req = lib.get(options, (res) => {
      const code = res.statusCode;
      if (code >= 300 && code < 400 && res.headers.location) {
        res.resume();
        if ((hops || 0) >= MAX_HOPS) return reject(new Error('too many redirects'));
        const next = new URL(res.headers.location, url).toString();
        if (u.protocol === 'https:' && next.indexOf('https://') !== 0) return reject(new Error('a redirect left https'));
        return resolve(get(next, limit, sink, (hops || 0) + 1));
      }
      if (code !== 200) {
        res.resume();
        return reject(new Error('HTTP ' + code));
      }
      let total = 0;
      res.on('data', (chunk) => {
        total += chunk.length;
        if (total > limit) {
          req.abort();
          return reject(new Error('larger than ' + limit + ' bytes'));
        }
        sink(chunk);
      });
      res.on('end', () => resolve(total));
      res.on('error', reject);
    });
    req.on('timeout', () => req.abort());
    req.on('error', reject);
  });
}

// The `Package` field of an IPK's control file: an ar archive holding
// control.tar.gz, whose ./control is a "Key: value" list.
function ipkPackage(file) {
  const buf = fs.readFileSync(file);
  if (buf.toString('latin1', 0, 8) !== '!<arch>\n') return null;
  let at = 8;
  while (at + 60 <= buf.length) {
    const name = buf.toString('latin1', at, at + 16).trim().replace(/\/$/, '');
    const size = parseInt(buf.toString('latin1', at + 48, at + 58).trim(), 10);
    const body = at + 60;
    if (!(size >= 0) || body + size > buf.length) return null;
    if (name === 'control.tar.gz') return controlPackage(zlib.gunzipSync(buf.slice(body, body + size)));
    at = body + size + (size % 2);
  }
  return null;
}

function controlPackage(tar) {
  for (let at = 0; at + 512 <= tar.length; ) {
    const name = tar.toString('latin1', at, at + 100).replace(/\0.*$/, '');
    if (!name) return null;
    const size = parseInt(tar.toString('latin1', at + 124, at + 136).replace(/\0.*$/, '').trim(), 8) || 0;
    if (name.replace(/^\.\//, '') === 'control') {
      const m = /^Package:\s*(\S+)\s*$/m.exec(tar.toString('utf8', at + 512, at + 512 + size));
      return m ? m[1] : null;
    }
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return null;
}

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

// opts: app, version (this build's), dataDir, onState(state), and for tests
// feed, keyPem, get, installer (argv of the install child), tmp.
function create(opts) {
  const fetch = opts.get || get;
  const feed = opts.feed || FEED;
  let state = { kind: 'idle' };
  let busy = false;
  let staged = null; // { version, file, sha256 }

  function set(next) {
    state = next;
    opts.onState(state);
  }

  // One check and download; `asked` (Check for updates) shows its answer. The
  // page sends the unasked ones (at start, then daily) only while nothing plays.
  function check(asked) {
    if (busy || state.kind === 'ready' || state.kind === 'installing') return Promise.resolve(state);
    busy = true;
    if (asked) set({ kind: 'checking' });
    const chunks = [];
    return fetch(feed, MAX_FEED, (c) => chunks.push(c))
      .then(() => verified(Buffer.concat(chunks).toString('utf8'), opts.keyPem))
      .then((body) => {
        if (!body) throw new Error('the feed signature does not hold');
        const found = pick(body, opts.version);
        if (!found) return set(asked ? { kind: 'current' } : { kind: 'idle' });
        return download(found);
      })
      .catch((e) => {
        console.log(JSON.stringify({ update: String((e && e.message) || e) }));
        set(asked ? { kind: 'failed' } : { kind: 'idle' });
      })
      .then(() => {
        busy = false;
        return state;
      });
  }

  function download(found) {
    set({ kind: 'downloading', version: found.version, percent: 0 });
    const dir = fs.mkdtempSync(path.join(opts.tmp || os.tmpdir(), 'momotus-update-'));
    const file = path.join(dir, 'momotus.ipk');
    const fd = fs.openSync(file, 'w', 0o600);
    const hash = crypto.createHash('sha256');
    let got = 0;
    let said = 0;
    return fetch(found.url, found.size, (chunk) => {
      fs.writeSync(fd, chunk, 0, chunk.length);
      hash.update(chunk);
      got += chunk.length;
      const percent = Math.floor((got * 100) / found.size);
      if (percent !== said) {
        said = percent;
        set({ kind: 'downloading', version: found.version, percent });
      }
    })
      .then((total) => {
        fs.closeSync(fd);
        if (total !== found.size || hash.digest('hex') !== found.sha256) throw new Error('the IPK differs from the feed');
        if (ipkPackage(file) !== opts.app) throw new Error('the IPK is not ' + opts.app);
        staged = { version: found.version, file, sha256: found.sha256 };
        set({ kind: 'ready', version: found.version });
      })
      .catch((e) => {
        try {
          fs.closeSync(fd);
        } catch (x) {}
        fs.unlinkSync(file);
        fs.rmdirSync(dir);
        throw e;
      });
  }

  // The user's Install: the file checked once more, then a detached child that
  // outlives this service (installing kills it) does the rest.
  function install() {
    if (!staged || state.kind !== 'ready') return state;
    if (sha256(staged.file) !== staged.sha256) {
      staged = null;
      set({ kind: 'failed' });
      return state;
    }
    const argv = opts.installer || [process.execPath, path.join(__dirname, 'update-install.js')];
    const child = spawn(argv[0], argv.slice(1).concat([staged.file, opts.app, path.join(opts.dataDir, 'update.log')]), {
      detached: true,
      stdio: 'ignore',
    });
    child.unref();
    set({ kind: 'installing', version: staged.version });
    return state;
  }

  // A page message {"host":"update","id","op"}: `state`, `check` (asked or
  // not) or `install`; the answer carries the state.
  function handle(text) {
    let req;
    try {
      req = JSON.parse(text);
    } catch (e) {
      req = {};
    }
    const id = typeof req.id === 'number' ? req.id : null;
    const reply = (s) => JSON.stringify({ host: 'update', id, state: s });
    if (req.op === 'check') return check(!!req.asked).then(reply);
    if (req.op === 'install') return Promise.resolve(reply(install()));
    return Promise.resolve(reply(state));
  }

  return {
    handle,
    check,
    get state() {
      return state;
    },
  };
}

module.exports = { create, verified, pick, newer, ipkPackage, get, FEED, PREFIX: '{"host":"update"' };
