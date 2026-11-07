'use strict';
// The core child's host (plans/webos.md 3.1 to 3.3): globals the glue needs on
// Node 8 to 20, and the mqHost calls the wasm imports (host.d.ts). CommonJS,
// Node 8.12 (webOS 6): no globalThis, TextDecoder or TextEncoder globals, no
// fs.promises, no queueMicrotask.
const fs = require('fs');
const http = require('http');
const https = require('https');
const dns = require('dns');
const os = require('os');
const path = require('path');
const tls = require('tls');
const zlib = require('zlib');
const { URL } = require('url');

// A Node without ICU has no fatal UTF-8 decoding. Re-encoding catches every
// invalid sequence, since Buffer replaces them with U+FFFD, which differs.
class StrictDecoder {
  constructor(label, options) {
    this.ignoreBOM = !!(options && options.ignoreBOM);
  }
  get encoding() {
    return 'utf-8';
  }
  get fatal() {
    return true;
  }
  decode(input) {
    let buf = Buffer.alloc(0);
    if (input instanceof ArrayBuffer) buf = Buffer.from(input);
    else if (input) buf = Buffer.from(input.buffer, input.byteOffset, input.byteLength);
    const text = buf.toString('utf8');
    if (!Buffer.from(text, 'utf8').equals(buf)) throw new TypeError('The encoded data was not valid utf-8');
    return !this.ignoreBOM && text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
  }
}

// Fatal decoding works only when the constructor takes `fatal` (Node without
// ICU throws ERR_NO_ICU there) and decoding invalid bytes throws.
function fatalDecoderWorks(TD) {
  let decoder;
  try {
    decoder = new TD('utf-8', { fatal: true });
  } catch (e) {
    return false;
  }
  try {
    decoder.decode(new Uint8Array([0xff]));
    return false;
  } catch (e) {
    return true;
  }
}

// The Mozilla bundle shipped beside this file (package.sh CA_BUNDLE) comes on
// top of the built-in roots; on a Node without `rootCertificates` it is the
// only list, and without a bundle the built-in default stays.
function caList() {
  let extra = [];
  try {
    extra = [fs.readFileSync(path.join(__dirname, 'cacert.pem'), 'utf8')];
  } catch (e) {}
  if (tls.rootCertificates) return tls.rootCertificates.concat(extra);
  return extra.length ? extra : undefined;
}

const TLS_CODES = /^(CERT_|ERR_TLS|ERR_SSL|UNABLE_TO|DEPTH_ZERO|SELF_SIGNED|HOSTNAME_MISMATCH)/;
function errorKind(e) {
  const code = String((e && e.code) || '');
  if (code === 'ECONNREFUSED' || code === 'EHOSTUNREACH' || code === 'ENETUNREACH') return 'refused';
  if (code === 'ENOTFOUND' || code === 'EAI_AGAIN') return 'dns';
  if (TLS_CODES.test(code)) return 'tls';
  if (code === 'ETIMEDOUT') return 'timeout';
  return 'reset';
}

function install(core) {
  if (typeof globalThis === 'undefined') global.globalThis = global;
  if (typeof TextDecoder === 'undefined') {
    global.TextDecoder = require('util').TextDecoder;
    global.TextEncoder = require('util').TextEncoder;
  }
  if (!fatalDecoderWorks(TextDecoder)) global.TextDecoder = StrictDecoder;
  if (typeof globalThis.performance === 'undefined') {
    globalThis.performance = require('perf_hooks').performance;
  }
  if (!globalThis.crypto || !globalThis.crypto.getRandomValues) {
    const nodeCrypto = require('crypto');
    globalThis.crypto = {
      getRandomValues(a) {
        nodeCrypto.randomFillSync(a);
        return a;
      },
    };
  }
  const timers = new Map();
  const open = new Set();
  const inflight = new Map();
  const ca = caList();
  const agents = {
    'http:': new http.Agent({ keepAlive: true }),
    'https:': new https.Agent({ keepAlive: true }),
  };
  let dead = false;
  // The core child never outlives its parent (it would keep writing the
  // unlocked database), and any uncaught error is a death: Node 8 and 12 only
  // warn on an unhandled rejection and keep a dead core running.
  const fatal = (what) => (e) => globalThis.mqHost.died(what + String((e && e.stack) || e || ''));
  process.on('disconnect', fatal('parent gone: '));
  process.on('unhandledRejection', fatal('unhandled rejection: '));
  process.on('uncaughtException', fatal('uncaught: '));

  // One hop to one address; `done(error, response)` once.
  function hop(req, url, addr, done) {
    const secure = url.protocol === 'https:';
    const headers = {};
    req.headers.forEach((h) => (headers[h[0]] = h[1]));
    headers.Host = url.host;
    if (!Object.keys(headers).some((k) => k.toLowerCase() === 'accept-encoding')) headers['Accept-Encoding'] = 'gzip';
    const options = {
      host: addr,
      port: url.port || (secure ? 443 : 80),
      method: req.method,
      path: url.pathname + url.search,
      headers,
      agent: agents[url.protocol],
    };
    if (secure) {
      // An IP literal as servername is not allowed in the handshake.
      options.servername = /^[\d.]+$|:/.test(url.hostname) ? undefined : url.hostname;
      if (ca) options.ca = ca;
    }
    const r = (secure ? https : http).request(options, (res) => {
      const gz = /gzip/i.test(res.headers['content-encoding'] || '');
      const parts = [];
      let size = 0;
      let finished = false;
      const finish = (truncated, error) => {
        if (finished) return;
        finished = true;
        res.destroy();
        if (error) return done({ error });
        const out = [];
        for (let i = 0; i < res.rawHeaders.length; i += 2) {
          const k = res.rawHeaders[i].toLowerCase();
          if (gz && (k === 'content-encoding' || k === 'content-length')) continue;
          out.push([res.rawHeaders[i], res.rawHeaders[i + 1]]);
        }
        done(null, { status: res.statusCode, headers: out, body: Buffer.concat(parts).toString('base64'), truncated });
      };
      const body = gz ? res.pipe(zlib.createGunzip()) : res;
      body.on('data', (c) => {
        if (finished) return;
        const room = req.prefixBytes === undefined ? req.maxBytes - size : req.prefixBytes - size;
        if (c.length > room) {
          if (req.prefixBytes === undefined) return finish(false, 'cap');
          parts.push(c.slice(0, room));
          return finish(true);
        }
        parts.push(c);
        size += c.length;
        if (req.prefixBytes !== undefined && size >= req.prefixBytes) finish(false);
      });
      body.on('end', () => finish(false));
      body.on('error', () => finish(false, 'reset'));
      res.on('error', () => finish(false, 'reset'));
      res.on('aborted', () => finish(false, 'reset'));
    });
    r.on('error', (e) => done({ error: errorKind(e), connect: true }));
    if (req.body) r.write(Buffer.from(req.body, 'base64'));
    r.end();
    return r;
  }

  globalThis.mqHost = {
    fsOpen(path, create) {
      if (dead) return -1;
      try {
        const fd = fs.openSync(path, create ? 'a+' : 'r+');
        // 'a+' appends on every write on some systems; reopen read-write.
        fs.closeSync(fd);
        const rw = fs.openSync(path, 'r+');
        open.add(rw);
        return rw;
      } catch (e) {
        return -1;
      }
    },
    fsExists: (path) => !dead && fs.existsSync(path),
    fsRead(fd, buf, offset) {
      if (dead) return -1;
      try {
        let got = 0;
        while (got < buf.length) {
          const n = fs.readSync(fd, buf, got, buf.length - got, offset + got);
          if (n === 0) break;
          got += n;
        }
        return got;
      } catch (e) {
        return -1;
      }
    },
    fsWrite(fd, buf, offset) {
      if (dead) return false;
      try {
        let put = 0;
        while (put < buf.length) put += fs.writeSync(fd, buf, put, buf.length - put, offset + put);
        return true;
      } catch (e) {
        return false;
      }
    },
    fsSync(fd) {
      if (dead) return false;
      try {
        fs.fsyncSync(fd);
        return true;
      } catch (e) {
        return false;
      }
    },
    fsTruncate(fd, size) {
      if (dead) return false;
      try {
        fs.ftruncateSync(fd, size);
        return true;
      } catch (e) {
        return false;
      }
    },
    fsSize(fd) {
      if (dead) return -1;
      try {
        return fs.fstatSync(fd).size;
      } catch (e) {
        return -1;
      }
    },
    fsClose(fd) {
      if (dead) return;
      open.delete(fd);
      try {
        fs.closeSync(fd);
      } catch (e) {}
    },
    fsUnlink(path) {
      if (dead) return false;
      try {
        fs.unlinkSync(path);
        return true;
      } catch (e) {
        return false;
      }
    },
    setTimer(ms, id) {
      timers.set(id, setTimeout(() => {
        timers.delete(id);
        core().fire(id);
      }, ms));
    },
    clearTimer(id) {
      clearTimeout(timers.get(id));
      timers.delete(id);
    },
    resolve(id, host) {
      // dns.lookup, never dns.resolve*: the jail has no /etc/resolv.conf.
      dns.lookup(host, { all: true }, (err, list) => {
        const v4 = (list || []).filter((a) => a.family === 4);
        const v6 = (list || []).filter((a) => a.family !== 4);
        const addresses = JSON.stringify(v4.concat(v6).map((a) => a.address));
        setImmediate(() => core().resolved(id, addresses));
      });
    },
    interfaces: () => JSON.stringify(os.networkInterfaces()),
    fetch(id, request) {
      let req;
      let url;
      try {
        req = JSON.parse(request);
        url = new URL(req.url);
        if (!Array.isArray(req.addresses) || !Array.isArray(req.headers)) throw new Error('bad request');
      } catch (e) {
        // A bad request is a failed fetch, never an exception into the wasm,
        // and the answer never runs inside this call.
        return void setImmediate(() => core().fetched(id, JSON.stringify({ error: 'reset' })));
      }
      const entry = { aborted: false, answered: false, req: null, timer: null };
      inflight.set(id, entry);
      let next = 0;
      // One deferred path for every answer: the core is never re-entered from
      // inside the import call that started the fetch (host.d.ts). An abort
      // before delivery drops the answer.
      const answer = (error, response) => {
        if (entry.answered || entry.aborted) return;
        entry.answered = true;
        clearTimeout(entry.timer);
        setImmediate(() => {
          if (inflight.get(id) !== entry) return;
          inflight.delete(id);
          core().fetched(id, JSON.stringify(error ? { error: error.error } : response));
        });
      };
      const attempt = () => {
        if (entry.answered || entry.aborted) return;
        // The next approved address, only when the connection itself failed.
        try {
          entry.req = hop(req, url, req.addresses[next++], (error, response) => {
            if (error && error.connect && next < req.addresses.length) return attempt();
            answer(error, response);
          });
        } catch (e) {
          answer({ error: 'reset' });
        }
      };
      entry.timer = setTimeout(() => {
        if (entry.req) entry.req.abort();
        answer({ error: 'timeout' });
      }, req.timeoutMs);
      if (!req.addresses.length) return answer({ error: 'dns' });
      attempt();
    },
    abort(id) {
      const entry = inflight.get(id);
      if (!entry) return;
      entry.aborted = true;
      clearTimeout(entry.timer);
      if (entry.req) entry.req.abort();
      inflight.delete(id);
    },
    log(level, line) {
      console.log('[' + level + '] ' + line);
    },
    died(message) {
      // Once only: a panic reports here, then its trap arrives as an
      // uncaught exception. Close every descriptor before the child goes, and
      // from here every file call fails (plans/webos.md 1).
      if (dead) return;
      dead = true;
      for (const fd of open) {
        try {
          fs.closeSync(fd);
        } catch (e) {}
      }
      open.clear();
      if (onDied) onDied(message);
      else process.exit(3);
    },
  };
}

let onDied = null;

module.exports = {
  install,
  StrictDecoder,
  setOnDied(f) {
    onDied = f;
  },
};
