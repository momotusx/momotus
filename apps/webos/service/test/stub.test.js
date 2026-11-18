'use strict';
// app/index.html's script run in a vm with a fake PalmServiceBridge.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const h = require('./harness.js');
const { assert, test } = h;

const html = fs.readFileSync(path.join(__dirname, '..', '..', 'app', 'index.html'), 'utf8');
const script = html.match(/<script>([\s\S]*)<\/script>/)[1];

function run(replies, bridgeThrows) {
  const box = { replaced: null, calls: [], focused: 0, bridges: [] };
  const els = {
    f: { className: '' },
    r: { disabled: false, onclick: null, focus: () => box.focused++ },
  };
  box.els = els;
  const sandbox = {
    document: { getElementById: (id) => els[id] },
    location: { href: 'file:///x/index.html', replace: (u) => (box.replaced = u) },
    PalmServiceBridge: function () {
      if (bridgeThrows) throw new Error('no bridge');
      this.call = (url) => box.calls.push(url);
      box.bridges.push(this);
    },
  };
  vm.runInNewContext(script, sandbox);
  for (const reply of replies) {
    const b = box.bridges[box.bridges.length - 1];
    if (b) b.onservicecallback(reply);
    if (els.f.className === 'on' && reply !== replies[replies.length - 1]) els.r.onclick();
  }
  return box;
}

test('a good reply redirects to the service origin with the token', () => {
  const b = run(['{"returnValue":true,"token":"abc"}']);
  assert.deepStrictEqual(b.calls, ['luna://app.momotus.webos.service/start']);
  assert.strictEqual(b.replaced, 'http://127.0.0.1:47801/#t=abc&stub=' + encodeURIComponent('file:///x/index.html'));
  assert.strictEqual(b.els.f.className, '');
});

test('a refusal, a garbage reply and a missing bridge all show the could-not-start line with Try again focused', () => {
  assert.ok(html.includes('Momotus could not start. Close the app and open it again.'));
  assert.ok(html.includes('>Try again</button>'));
  for (const b of [run(['{"returnValue":false,"errorText":"x"}']), run(['not json']), run(['null']), run([], true)]) {
    assert.strictEqual(b.els.f.className, 'on');
    assert.strictEqual(b.focused, 1);
    assert.strictEqual(b.replaced, null);
  }
});

test('Try again asks the service once more and goes on when it starts', () => {
  const b = run(['{"returnValue":false}', '{"returnValue":true,"token":"t2"}']);
  assert.strictEqual(b.calls.length, 2);
  assert.strictEqual(b.els.f.className, '');
  assert.ok(b.replaced.startsWith('http://127.0.0.1:47801/#t=t2&'));
});

h.run();
