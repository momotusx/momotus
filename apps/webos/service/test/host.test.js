'use strict';
// host.js on its own: `died` runs once and every file call fails after it, and
// the strict UTF-8 decoder stands in where the built-in one is not fatal.
const fs = require('fs');
const path = require('path');
const h = require('./harness.js');
const host = require('../host.js');
const { assert, test } = h;

test('died runs once, closes descriptors and fails every file call after it', async () => {
  const file = path.join(h.tmpDir(), 'f');
  const deaths = [];
  host.setOnDied((m) => deaths.push(m));
  host.install(() => null);
  const mq = global.mqHost;
  const fd = mq.fsOpen(file, true);
  assert.ok(fd >= 0);
  assert.strictEqual(mq.fsWrite(fd, Buffer.from('abc'), 0), true);
  mq.died('first');
  mq.died('second');
  assert.deepStrictEqual(deaths, ['first']);
  assert.strictEqual(mq.fsOpen(file, false), -1);
  assert.strictEqual(mq.fsWrite(fd, Buffer.from('x'), 0), false);
  assert.strictEqual(mq.fsSync(fd), false);
  assert.strictEqual(mq.fsRead(fd, Buffer.alloc(3), 0), -1);
  assert.strictEqual(mq.fsSize(fd), -1);
  assert.strictEqual(mq.fsUnlink(file), false);
  assert.strictEqual(fs.readFileSync(file, 'utf8'), 'abc');
  assert.throws(() => fs.fstatSync(fd), 'the descriptor was closed by died');
});

test('the strict decoder rejects invalid UTF-8 and strips a BOM', () => {
  const D = host.StrictDecoder;
  const d = new D('utf-8', { fatal: true });
  assert.strictEqual(d.decode(new Uint8Array(Buffer.from('héllo \u{1f600}'))), 'héllo \u{1f600}');
  assert.strictEqual(d.decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61])), 'a');
  assert.strictEqual(new D('utf-8', { ignoreBOM: true }).decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61])), '﻿a');
  assert.strictEqual(d.decode(new Uint8Array([0xef, 0xbf, 0xbd]).buffer), '�');
  [[0xff], [0xc0, 0x80], [0xed, 0xa0, 0x80], [0xe2, 0x82], [0xf8, 0x88, 0x80, 0x80, 0x80]].forEach((bytes) => {
    assert.throws(() => d.decode(new Uint8Array(bytes)), TypeError);
  });
});

test('a no-ICU decoder (constructor throws on fatal) is replaced by the strict one', () => {
  const real = global.TextDecoder;
  class NoIcu {
    constructor(label, options) {
      if (options && options.fatal) {
        const e = new TypeError('"fatal" option is not supported on Node.js compiled without ICU');
        e.code = 'ERR_NO_ICU';
        throw e;
      }
    }
    decode(b) {
      return Buffer.from(b).toString('utf8');
    }
  }
  // A decoder that constructs but does not throw on invalid input is replaced too.
  class Lax {
    decode(b) {
      return Buffer.from(b).toString('utf8');
    }
  }
  try {
    [NoIcu, Lax].forEach((D) => {
      global.TextDecoder = D;
      host.setOnDied(() => {});
      host.install(() => null);
      assert.strictEqual(global.TextDecoder, host.StrictDecoder);
    });
    global.TextDecoder = real;
    host.install(() => null);
    assert.strictEqual(global.TextDecoder, real, 'a decoder with working fatal is kept');
  } finally {
    global.TextDecoder = real;
  }
});

h.run();
