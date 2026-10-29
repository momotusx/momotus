// The plugin's world (plans/plugins-and-marketplace.md 6.2 and 6.4): the web
// APIs a Nuvio plugin reaches for, over the natives in `N`, which the plugin
// never sees. Evaluated once per call, before the plugin, as a function of
// (N, config).
(N, config) => {
  "use strict";
  const g = globalThis;
  const define = (name, value) =>
    Object.defineProperty(g, name, { value, writable: true, configurable: true, enumerable: false });
  const apply = Reflect.apply;

  // QuickJS-NG walks a generic array-like in C loops that never read the
  // interrupt (`Array.prototype.reverse.call({length: 2 ** 53 - 1})` ran
  // forever, 2026-10-28), so every Array.prototype method refuses a length
  // no 24 MiB heap could hold.
  const proto = Array.prototype;
  for (const name of Object.getOwnPropertyNames(proto)) {
    const method = proto[name];
    if (typeof method !== "function" || name === "constructor") continue;
    const guarded = {
      [name](...args) {
        if (this != null && +this.length > config.maxArray) throw new RangeError("array too long");
        return apply(method, this, args);
      },
    }[name];
    Object.defineProperty(proto, name, { value: guarded, writable: true, configurable: true, enumerable: false });
  }

  define("global", g);
  define("window", g);
  define("self", g);
  define("SCRAPER_ID", config.pluginId);
  define("SCRAPER_SETTINGS", {});
  // Plugins that need the app's TMDB key get none (6.2).
  define("TMDB_API_KEY", "");
  const module = { exports: {} };
  define("module", module);
  define("exports", module.exports);

  // console: one line per call to the host, within the call's log budget.
  const words = (args) =>
    args
      .map((a) => {
        if (typeof a === "string") return a;
        if (a instanceof Error) return `${a.name}: ${a.message}`;
        try {
          const json = JSON.stringify(a);
          return json === undefined ? String(a) : json;
        } catch {
          try {
            return String(a);
          } catch {
            return "?";
          }
        }
      })
      .join(" ");
  const log = (level) => (...args) => N.log(level, words(args));
  define("console", {
    debug: log(0),
    log: log(1),
    info: log(1),
    warn: log(2),
    error: log(3),
    trace: log(0),
  });

  // Timers: host sleeps, each at most 10 s, inside the call's deadline.
  let lastTimer = 0;
  const timers = new Set();
  const report = (e) => console.error(e);
  define("setTimeout", (fn, ms, ...args) => {
    const id = ++lastTimer;
    timers.add(id);
    N.sleep(+ms || 0).then(() => {
      if (timers.delete(id) && typeof fn === "function") {
        try {
          fn(...args);
        } catch (e) {
          report(e);
        }
      }
    });
    return id;
  });
  define("setInterval", (fn, ms, ...args) => {
    const id = ++lastTimer;
    timers.add(id);
    const tick = () =>
      N.sleep(Math.max(+ms || 0, 10)).then(() => {
        if (!timers.has(id)) return;
        try {
          if (typeof fn === "function") fn(...args);
        } catch (e) {
          report(e);
        }
        tick();
      });
    tick();
    return id;
  });
  define("clearTimeout", (id) => void timers.delete(id));
  define("clearInterval", (id) => void timers.delete(id));
  define("queueMicrotask", (fn) => void Promise.resolve().then(fn).catch(report));

  // Bytes.
  const bytesOf = (data) => {
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    throw new TypeError("expected an ArrayBuffer or a view of one");
  };
  class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(text = "") {
      return N.encode(String(text));
    }
  }
  class TextDecoder {
    #fatal;
    constructor(label = "utf-8", options = {}) {
      if (!["utf-8", "utf8", "unicode-1-1-utf-8"].includes(String(label).trim().toLowerCase()))
        throw new RangeError(`unsupported encoding: ${label}`);
      this.#fatal = !!(options && options.fatal);
    }
    get encoding() {
      return "utf-8";
    }
    get fatal() {
      return this.#fatal;
    }
    decode(data) {
      return data === undefined ? "" : N.decode(bytesOf(data), this.#fatal);
    }
  }
  define("TextEncoder", TextEncoder);
  define("TextDecoder", TextDecoder);
  define("atob", (data) => N.atob(String(data)));
  define("btoa", (data) => N.btoa(String(data)));

  const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  define("crypto", {
    getRandomValues(array) {
      if (!ArrayBuffer.isView(array) || array instanceof DataView || array instanceof Float32Array || array instanceof Float64Array)
        throw new TypeError("expected an integer typed array");
      if (array.byteLength > 65536) throw new RangeError("at most 65536 bytes");
      bytesOf(array).set(N.random(array.byteLength));
      return array;
    },
    randomUUID() {
      const b = N.random(16);
      b[6] = (b[6] & 0x0f) | 0x40;
      b[8] = (b[8] & 0x3f) | 0x80;
      const h = hex(b);
      return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
    },
  });

  // URL and URLSearchParams over the `url` crate's WHATWG parser.
  const BIND = Symbol("bind");
  const RESET = Symbol("reset");
  const SET = Symbol("set");
  class URLSearchParams {
    #list = [];
    #changed = null;
    constructor(init) {
      if (init == null) return;
      if (init instanceof URLSearchParams) this.#list = init.#list.map(([k, v]) => [k, v]);
      else if (typeof init === "object" && typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {
          const [k, v, ...rest] = pair;
          if (rest.length || pair.length !== 2) throw new TypeError("each pair needs a name and a value");
          this.#list.push([String(k), String(v)]);
        }
      } else if (typeof init === "object") {
        for (const k of Object.keys(init)) this.#list.push([k, String(init[k])]);
      } else {
        const text = String(init);
        this.#list = N.formParse(text.startsWith("?") ? text.slice(1) : text);
      }
    }
    static [BIND](params, changed) {
      params.#changed = changed;
    }
    static [RESET](params, search) {
      params.#list = N.formParse(search.startsWith("?") ? search.slice(1) : search);
    }
    #update() {
      if (this.#changed) this.#changed(this.toString());
    }
    get size() {
      return this.#list.length;
    }
    append(k, v) {
      this.#list.push([String(k), String(v)]);
      this.#update();
    }
    delete(k, v) {
      k = String(k);
      this.#list = this.#list.filter(([a, b]) => a !== k || (v !== undefined && b !== String(v)));
      this.#update();
    }
    get(k) {
      const hit = this.#list.find(([a]) => a === String(k));
      return hit ? hit[1] : null;
    }
    getAll(k) {
      return this.#list.filter(([a]) => a === String(k)).map(([, b]) => b);
    }
    has(k, v) {
      return this.#list.some(([a, b]) => a === String(k) && (v === undefined || b === String(v)));
    }
    set(k, v) {
      k = String(k);
      const at = this.#list.findIndex(([a]) => a === k);
      if (at < 0) this.#list.push([k, String(v)]);
      else {
        this.#list[at][1] = String(v);
        this.#list = this.#list.filter(([a], i) => a !== k || i === at);
      }
      this.#update();
    }
    sort() {
      this.#list.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
      this.#update();
    }
    forEach(fn, that) {
      for (const [k, v] of this.#list) fn.call(that, v, k, this);
    }
    *entries() {
      for (const [k, v] of this.#list) yield [k, v];
    }
    *keys() {
      for (const [k] of this.#list) yield k;
    }
    *values() {
      for (const [, v] of this.#list) yield v;
    }
    [Symbol.iterator]() {
      return this.entries();
    }
    toString() {
      return N.formSerialize(this.#list);
    }
  }
  class URL {
    #href;
    #params = null;
    constructor(input, base) {
      // An explicit undefined is not a missing argument to the host function.
      this.#href = base === undefined ? N.urlParse(String(input)) : N.urlParse(String(input), String(base));
    }
    static canParse(input, base) {
      try {
        new URL(input, base);
        return true;
      } catch {
        return false;
      }
    }
    static parse(input, base) {
      try {
        return new URL(input, base);
      } catch {
        return null;
      }
    }
    static [SET](url, part, value) {
      url.#href = N.urlSet(url.#href, part, String(value));
      if (url.#params) URLSearchParams[RESET](url.#params, url.search);
    }
    get searchParams() {
      if (!this.#params) {
        this.#params = new URLSearchParams(this.search);
        URLSearchParams[BIND](this.#params, (query) => {
          this.#href = N.urlSet(this.#href, "search", query);
        });
      }
      return this.#params;
    }
    toString() {
      return this.#href;
    }
    toJSON() {
      return this.#href;
    }
  }
  for (const part of ["href", "protocol", "username", "password", "host", "hostname", "port", "pathname", "search", "hash"]) {
    Object.defineProperty(URL.prototype, part, {
      get() {
        return N.urlGet(this.toString(), part);
      },
      set(value) {
        URL[SET](this, part, value);
      },
      configurable: true,
    });
  }
  Object.defineProperty(URL.prototype, "origin", {
    get() {
      return N.urlGet(this.toString(), "origin");
    },
    configurable: true,
  });
  define("URL", URL);
  define("URLSearchParams", URLSearchParams);

  // AbortController and AbortSignal do nothing (6.4): the call's own deadline
  // ends every request.
  class AbortSignal {
    aborted = false;
    reason = undefined;
    onabort = null;
    addEventListener() {}
    removeEventListener() {}
    throwIfAborted() {
      if (this.aborted) throw this.reason;
    }
    static timeout() {
      return new AbortSignal();
    }
    static abort(reason) {
      const s = new AbortSignal();
      s.aborted = true;
      s.reason = reason;
      return s;
    }
    static any() {
      return new AbortSignal();
    }
  }
  class AbortController {
    signal = new AbortSignal();
    abort(reason) {
      this.signal.aborted = true;
      this.signal.reason = reason === undefined ? new Error("aborted") : reason;
    }
  }
  define("AbortSignal", AbortSignal);
  define("AbortController", AbortController);

  // fetch over the host (6.4).
  class Headers {
    #map = new Map();
    constructor(init) {
      if (init == null) return;
      if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
      else if (typeof init[Symbol.iterator] === "function") for (const [k, v] of init) this.append(k, v);
      else for (const k of Object.keys(init)) this.append(k, init[k]);
    }
    append(k, v) {
      k = String(k).toLowerCase();
      v = String(v);
      const was = this.#map.get(k);
      this.#map.set(k, was === undefined ? v : `${was}, ${v}`);
    }
    set(k, v) {
      this.#map.set(String(k).toLowerCase(), String(v));
    }
    get(k) {
      const v = this.#map.get(String(k).toLowerCase());
      return v === undefined ? null : v;
    }
    has(k) {
      return this.#map.has(String(k).toLowerCase());
    }
    delete(k) {
      this.#map.delete(String(k).toLowerCase());
    }
    forEach(fn, that) {
      for (const [k, v] of this.#map) fn.call(that, v, k, this);
    }
    *entries() {
      yield* this.#map.entries();
    }
    *keys() {
      yield* this.#map.keys();
    }
    *values() {
      yield* this.#map.values();
    }
    [Symbol.iterator]() {
      return this.entries();
    }
  }
  class Response {
    #body;
    #used = false;
    constructor(status, statusText, url, headers, body) {
      this.status = status;
      this.statusText = statusText;
      this.ok = status >= 200 && status < 300;
      this.url = url;
      this.redirected = false;
      this.type = "basic";
      this.headers = new Headers(headers);
      this.#body = body;
    }
    get bodyUsed() {
      return this.#used;
    }
    #take() {
      if (this.#used) return Promise.reject(new TypeError("body already read"));
      this.#used = true;
      return Promise.resolve(this.#body);
    }
    arrayBuffer() {
      return this.#take().then((b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength));
    }
    bytes() {
      return this.#take().then((b) => b.slice());
    }
    text() {
      return this.#take().then((b) => N.decode(b, false));
    }
    // As Nuvio's: an empty or broken body reads as null instead of throwing,
    // which plugins written for it rely on.
    json() {
      return this.text().then((t) => {
        try {
          return t === "" ? null : JSON.parse(t);
        } catch {
          return null;
        }
      });
    }
    clone() {
      if (this.#used) throw new TypeError("body already read");
      return new Response(this.status, this.statusText, this.url, this.headers, this.#body.slice());
    }
  }
  const METHODS = ["GET", "POST", "HEAD"];
  const bodyOf = (body, headers) => {
    if (body == null) return null;
    if (typeof body === "string") return N.encode(body);
    if (body instanceof URLSearchParams) {
      if (!headers.has("content-type")) headers.set("content-type", "application/x-www-form-urlencoded;charset=UTF-8");
      return N.encode(body.toString());
    }
    if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return bytesOf(body).slice();
    return N.encode(String(body));
  };
  define("Headers", Headers);
  define("Response", Response);
  define("fetch", (input, init) => {
    try {
      init = init || {};
      const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : String((input && input.url) || input);
      if (url.length > config.maxUrl) throw new TypeError("URL too long");
      const method = String(init.method || "GET").toUpperCase();
      if (!METHODS.includes(method)) throw new TypeError(`method not allowed: ${method}`);
      const headers = new Headers(init.headers);
      const body = method === "GET" || method === "HEAD" ? null : bodyOf(init.body, headers);
      return N.fetch(method, url, [...headers], body, init.redirect !== "manual").then(
        ([status, statusText, final, list, bytes]) => new Response(status, statusText, final, list, bytes),
      );
    } catch (e) {
      return Promise.reject(e);
    }
  });

  // cheerio (Nuvio's subset, with cheerio's own meaning for children and
  // parent) over the capped tree.
  class Selection {
    constructor(doc, ids) {
      this._doc = doc;
      this._ids = ids;
      this.length = ids.length;
    }
    #wrap(ids) {
      return new Selection(this._doc, ids);
    }
    #each(fn) {
      return this._ids.map((id) => this.#wrap([id])).map(fn);
    }
    each(fn) {
      this.#each((el, i) => fn.call(el, i, el));
      return this;
    }
    map(fn) {
      const out = [];
      this.#each((el, i) => {
        const v = fn.call(el, i, el);
        if (Array.isArray(v)) out.push(...v);
        else if (v != null) out.push(v);
      });
      return { length: out.length, get: (i) => (i === undefined ? out : out[i < 0 ? out.length + i : i]), toArray: () => out };
    }
    filter(test) {
      if (typeof test === "function") return this.#wrap(this._ids.filter((id, i) => test.call(this.#wrap([id]), i, this.#wrap([id]))));
      return this.#wrap(N.matching(this._doc, this._ids, String(test)));
    }
    is(test) {
      return this.filter(test).length > 0;
    }
    find(selector) {
      return this.#wrap(this._ids.length ? N.select(this._doc, String(selector), this._ids) : []);
    }
    children(selector) {
      const ids = this._ids.flatMap((id) => N.children(this._doc, id));
      return this.#wrap(selector === undefined ? ids : N.matching(this._doc, ids, String(selector)));
    }
    parent(selector) {
      const ids = [...new Set(this._ids.map((id) => N.parent(this._doc, id)).filter((id) => id > 0))];
      return this.#wrap(selector === undefined ? ids : N.matching(this._doc, ids, String(selector)));
    }
    closest(selector) {
      return this.#wrap([...new Set(this._ids.map((id) => N.closest(this._doc, id, String(selector))).filter((id) => id > 0))]);
    }
    next() {
      return this.#wrap(this._ids.map((id) => N.sibling(this._doc, id, 1)).filter((id) => id > 0));
    }
    prev() {
      return this.#wrap(this._ids.map((id) => N.sibling(this._doc, id, -1)).filter((id) => id > 0));
    }
    first() {
      return this.#wrap(this._ids.slice(0, 1));
    }
    last() {
      return this.#wrap(this._ids.slice(-1));
    }
    eq(i) {
      i = Math.trunc(+i || 0);
      const id = this._ids[i < 0 ? this._ids.length + i : i];
      return this.#wrap(id === undefined ? [] : [id]);
    }
    slice(start, end) {
      return this.#wrap(this._ids.slice(start, end));
    }
    get(i) {
      if (i === undefined) return this.toArray();
      const id = this._ids[i < 0 ? this._ids.length + i : i];
      return id === undefined ? undefined : this.#wrap([id]);
    }
    toArray() {
      return this.#each((el) => el);
    }
    text() {
      return N.text(this._doc, this._ids);
    }
    html() {
      return this._ids.length ? N.html(this._doc, this._ids[0], false) : null;
    }
    attr(name) {
      if (!this._ids.length) return undefined;
      if (name === undefined) return N.attrs(this._doc, this._ids[0]);
      return N.attr(this._doc, this._ids[0], String(name));
    }
    val() {
      return this.attr("value");
    }
    hasClass(name) {
      return this._ids.some((id) => (N.attr(this._doc, id, "class") || "").split(/\s+/).includes(String(name)));
    }
    data(name) {
      return name === undefined ? undefined : this.attr(`data-${name}`);
    }
    [Symbol.iterator]() {
      return this.toArray()[Symbol.iterator]();
    }
  }
  const cheerio = {
    load(html) {
      const doc = N.load(String(html ?? ""));
      const $ = (selector, context) => {
        if (selector instanceof Selection) return selector;
        if (typeof selector !== "string") return new Selection(doc, []);
        if (context === undefined) return new Selection(doc, N.select(doc, selector, null));
        return $(context).find(selector);
      };
      $.html = (selection) =>
        selection instanceof Selection ? (selection._ids.length ? N.html(doc, selection._ids[0], true) : "") : N.html(doc, 0, false);
      $.text = (selection) => (selection instanceof Selection ? selection.text() : N.text(doc, [0]));
      $.root = () => new Selection(doc, [0]);
      return $;
    },
  };

  let cryptoJs;
  const loadCryptoJs = () => (cryptoJs ??= N.cryptoJs());
  Object.defineProperty(g, "CryptoJS", { get: loadCryptoJs, configurable: true, enumerable: false });
  define("cheerio", cheerio);
  define("require", (name) => {
    switch (String(name)) {
      case "cheerio":
      case "cheerio-without-node-native":
      case "react-native-cheerio":
        return cheerio;
      case "crypto-js":
        return loadCryptoJs();
      default:
        throw new Error(`Module '${name}' is not available`);
    }
  });
};
