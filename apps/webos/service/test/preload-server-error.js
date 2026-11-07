'use strict';
// Preload for the service (node -r): the http server emits two errors after
// it is listening, as a failing accept would.
const http = require('http');
const create = http.createServer;
http.createServer = function () {
  const server = create.apply(this, arguments);
  setTimeout(() => {
    server.emit('error', new Error('accept failed 1'));
    server.emit('error', new Error('accept failed 2'));
  }, 400);
  return server;
};
