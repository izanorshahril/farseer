// Deepest level. Idles so orphan survival is observable.
const fs = require('fs');
fs.appendFileSync(process.env.JOBSPIKE_PIDFILE, `grandchild ${process.pid}\n`);
setInterval(() => {}, 60000);
