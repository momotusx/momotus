'use strict';
// Gate (d) of build-wasm.sh: node ping-core.js <mq_wasm.js>. Loads the core
// over the service's host globals, as the core child does, and prints its
// `ping` answer with the load time.
const path = require('path');
const host = require(path.join(__dirname, '../service/host.js'));

host.install(() => core);
host.setOnDied((message) => {
  console.log('died: ' + message);
  process.exit(3);
});
const t0 = Date.now();
const core = require(path.resolve(process.argv[2]));
const answer = JSON.parse(core.ping());
if (answer.pong !== true) process.exit(1);
console.log(JSON.stringify({ pong: true, loadMs: Date.now() - t0 }));
process.exit(0);
