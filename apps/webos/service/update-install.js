'use strict';
// Installs an update IPK on its own (plans/auto-update.md section 2): the
// service starts it detached and is then killed by the install it asked for.
//   node update-install.js <ipk> <app id> <log file>
// `luna-send-pub -i` never exits by itself, so its replies are read line by
// line, as Homebrew Channel does: statusValue 30 is installed, a returnValue
// false or an errorCode is a failure. Then the app is opened again. Each step
// goes to the log file, which the next start can read. MQ_LUNA_SEND names
// another luna-send-pub (tests).
const fs = require('fs');
const path = require('path');
const { spawn, execFile } = require('child_process');

const [ipk, app, logFile] = process.argv.slice(2);
const LUNA = process.env.MQ_LUNA_SEND || 'luna-send-pub';
const LIMIT_MS = Number(process.env.MQ_INSTALL_LIMIT_MS) || 10 * 60 * 1000;
const INSTALLED = 30;

function log(entry) {
  try {
    fs.appendFileSync(logFile, JSON.stringify(Object.assign({ at: new Date().toISOString() }, entry)) + '\n');
  } catch (e) {}
}

function done(code) {
  try {
    fs.unlinkSync(ipk);
    fs.rmdirSync(path.dirname(ipk));
  } catch (e) {}
  process.exit(code);
}

function launch() {
  const args = ['-n', '1', 'luna://com.webos.applicationManager/launch', JSON.stringify({ id: app })];
  execFile(LUNA, args, { timeout: 30000 }, (e, out) => {
    log({ launch: e ? String(e.message) : String(out).trim() });
    done(e ? 1 : 0);
  });
}

// ares-install's request id; the package itself comes from the IPK.
const request = JSON.stringify({ id: 'com.ares.defaultName', ipkUrl: ipk, subscribe: true });
const child = spawn(LUNA, ['-i', 'luna://com.webos.appInstallService/dev/install', request]);
let over = false;
let rest = '';
const timer = setTimeout(() => finish(false, 'timed out'), LIMIT_MS);

function finish(ok, why) {
  if (over) return;
  over = true;
  clearTimeout(timer);
  child.kill();
  log({ install: ok ? 'installed' : 'failed', why });
  if (ok) launch();
  else done(1);
}

function reply(line) {
  let m;
  try {
    m = JSON.parse(line);
  } catch (e) {
    return;
  }
  const details = (m && m.details) || {};
  if (m.returnValue === false) return finish(false, m.errorText || 'refused');
  if (details.errorCode !== undefined) return finish(false, details.reason || String(details.errorCode));
  if (m.statusValue === INSTALLED || details.state === 'installed') finish(true);
}

child.stdout.on('data', (data) => {
  rest += data;
  for (let i = rest.indexOf('\n'); i >= 0; i = rest.indexOf('\n')) {
    const line = rest.slice(0, i).trim();
    rest = rest.slice(i + 1);
    if (line) reply(line);
  }
});
child.on('error', (e) => finish(false, String(e.message)));
child.on('exit', () => finish(false, 'luna-send-pub ended before the install did'));
log({ start: ipk, app });
