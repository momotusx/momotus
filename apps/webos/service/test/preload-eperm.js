'use strict';
// Preload for the service (node -r): the pid in EPERM_PID answers EPERM to
// every signal, as a process owned by another user does.
const kill = process.kill;
process.kill = function (pid, sig) {
  if (pid === Number(process.env.EPERM_PID)) {
    const e = new Error('kill EPERM');
    e.code = 'EPERM';
    throw e;
  }
  return kill.call(process, pid, sig);
};
