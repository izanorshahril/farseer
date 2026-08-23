// Level: node, launched by `npm run spawner`. Spawns one grandchild node, then idles.
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

const pidfile = process.env.JOBSPIKE_PIDFILE;
fs.appendFileSync(pidfile, `spawner ${process.pid}\n`);

const grandchild = spawn(process.execPath, [path.join(__dirname, 'grandchild.js')], {
  stdio: 'ignore',
  detached: false,
  env: process.env,
});

grandchild.on('error', (e) => fs.appendFileSync(pidfile, `spawn-error ${e.message}\n`));
setInterval(() => {}, 60000);
