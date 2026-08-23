// Stands in for a dev server running inside a workspace.
// Holds exactly the handles a real one holds: a recursive directory watcher,
// an open write stream to a log inside the tree, and a listening socket.
const fs = require('fs');
const http = require('http');
const path = require('path');

const root = process.env.WSSPIKE_WATCH_DIR || __dirname;
const log = fs.createWriteStream(path.join(root, 'dev.log'), { flags: 'a' });

// Recursive watch is the handle that actually blocks deletes on Windows.
const watcher = fs.watch(root, { recursive: true }, (ev, name) => {
  log.write(`${ev} ${name}\n`);
});

const server = http.createServer((_, res) => res.end('ok'));
server.listen(0, '127.0.0.1', () => {
  fs.writeFileSync(path.join(root, 'READY'), String(process.pid));
});

// Keep a plain read handle open on a tracked file too.
const held = fs.openSync(path.join(root, 'src', 'index.js'), 'r');

setInterval(() => log.write(`tick ${Date.now()}\n`), 500);
process.on('exit', () => {
  try {
    watcher.close();
    fs.closeSync(held);
  } catch {}
});
