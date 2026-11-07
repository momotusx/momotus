'use strict';
// app/index.html's script run in a vm with a fake PalmServiceBridge.
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const h = require('./harness.js');
const { assert, test } = h;

const html = fs.readFileSync(path.join(__dirname, '..', '..', 'app', 'index.html'), 'utf8');
const script = html.match(/<script>([\s\S]*)<\/script>/)[1];

function run(reply, bridgeThrows) {
  const box = { text: '', replaced: null, call: null };
  const sandbox = {
    document: { getElementById: () => ({ set textContent(v) { box.text = v; } }) },
    location: { href: 'file:///x/index.html', replace: (u) => (box.replaced = u) },
    PalmServiceBridge: function () {
      if (bridgeThrows) throw new Error('no bridge');
      this.call = (url) => (box.call = url);
      box.bridge = this;
    },
  };
  vm.runInNewContext(script, sandbox);
  if (box.bridge) box.bridge.onservicecallback(reply);
  return box;
}

test('a good reply redirects to the service origin with the token', () => {
  const b = run('{"returnValue":true,"token":"abc"}');
  assert.strictEqual(b.call, 'luna://app.momotus.webos.service/start');
  assert.strictEqual(b.replaced, 'http://127.0.0.1:47801/#t=abc&stub=' + encodeURIComponent('file:///x/index.html'));
  assert.strictEqual(b.text, '');
});

test('a refusal, a garbage reply and a missing bridge all show the could-not-start line', () => {
  const line = 'Momotus could not start. Close the app and open it again.';
  assert.strictEqual(run('{"returnValue":false,"errorText":"x"}').text, line);
  assert.strictEqual(run('not json').text, line);
  assert.strictEqual(run('null').text, line);
  assert.strictEqual(run('', true).text, line);
});

h.run();
