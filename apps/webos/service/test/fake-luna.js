'use strict';
// Stands in for webos-service (MQ_LUNA): `start` fires once on its own with the
// sender from FAKE_SENDER, and the activity calls print JSON lines the tests
// read from the service's stdout.
const out = (o) => console.log(JSON.stringify(o));

module.exports = function Service() {
  this.register = (name, handler) => {
    setImmediate(() =>
      handler({
        sender: process.env.FAKE_SENDER || 'app.momotus.webos',
        respond: (response) => out({ luna: name, response }),
      }),
    );
  };
  this.activityManager = {
    create: (name, cb) => {
      out({ activity: 'create' });
      setImmediate(() => cb({ id: 1 }));
    },
    complete: (activity, cb) => {
      out({ activity: 'complete' });
      cb();
    },
  };
};
