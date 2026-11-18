'use strict';
// Preload for the service (node -r): the pid in UNKILLABLE_PID shrugs off
// SIGTERM and SIGKILL, as a process stuck in the kernel does; signal 0 still
// says whether it lives.
const kill = process.kill;
process.kill = function (pid, sig) {
  if (pid === Number(process.env.UNKILLABLE_PID) && sig !== 0) return true;
  return kill.call(process, pid, sig);
};
