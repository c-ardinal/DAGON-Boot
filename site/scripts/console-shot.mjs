// Retakes the README's picture of DAGON Console (assets/console/console-writing.png):
// the Console demo build, a write in progress on the simulator. Needs Node 22+ and
// Chrome or Edge. The labels below are the Console's own (web/src/app/copy.ts); when
// the Console's screens change, adjust them and retake.
//
//   npm --prefix web run build:demo -- --outDir ../site/dist/demo --emptyOutDir
//   node site/scripts/console-shot.mjs [--browser <path to chrome or msedge>]
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '../..');
const DEMO = join(ROOT, 'site/dist/demo');
const OUT = join(ROOT, 'assets/console/console-writing.png');
const BROWSERS = [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
];
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.json': 'application/json', '.woff2': 'font/woff2', '.png': 'image/png', '.svg': 'image/svg+xml' };
const DEBUG_PORT = 9341;
/** From the start of the first 子機's write to about the middle of its transfer (dagon-sim's link times). */
const MID_WRITE_MS = 6000;

const arg = process.argv.indexOf('--browser');
const browser = arg > 0 ? process.argv[arg + 1] : BROWSERS.find((b) => existsSync(b));
if (!browser) throw new Error('no browser found; pass --browser <path>');
if (!existsSync(join(DEMO, 'index.html'))) throw new Error('no demo build; see the top of this file');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const server = createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const file = join(DEMO, path.endsWith('/') ? `${path}index.html` : path);
  if (!file.startsWith(DEMO) || !existsSync(file) || statSync(file).isDirectory()) return void res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/`;

const profile = mkdtempSync(join(tmpdir(), 'console-shot-'));
const chrome = spawn(browser, ['--headless=new', `--remote-debugging-port=${DEBUG_PORT}`, `--user-data-dir=${profile}`, '--window-size=1440,900', 'about:blank'], { stdio: 'ignore' });

async function target() {
  for (let i = 0; i < 50; i++) {
    try {
      const tabs = await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json`)).json();
      const tab = tabs.find((t) => t.type === 'page');
      if (tab) return tab.webSocketDebuggerUrl;
    } catch {
      // The browser is still starting.
    }
    await sleep(200);
  }
  throw new Error('the browser did not open its debugging port');
}

const ws = new WebSocket(await target());
await new Promise((r, x) => ((ws.onopen = r), (ws.onerror = x)));
let id = 0;
const waiting = new Map();
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (waiting.has(d.id)) waiting.get(d.id)(d), waiting.delete(d.id);
};
const call = (method, params = {}) => new Promise((r) => (waiting.set(++id, r), ws.send(JSON.stringify({ id, method, params }))));
const js = async (expression) => (await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;
const press = (label, exact = true) =>
  js(`(() => { const b = [...document.querySelectorAll('button')].filter((b) => ${exact ? 'b.textContent.trim()' : 'b.textContent'}${exact ? ' === ' : '.includes('}${JSON.stringify(label)}${exact ? '' : ')'}).at(-1); if (b) b.click(); return !!b; })()`);

async function step(label, exact = true) {
  for (let i = 0; i < 100; i++) {
    if (await press(label, exact)) return sleep(400);
    await sleep(100);
  }
  throw new Error(`no button ${label}: the Console's labels changed; adjust this script`);
}

try {
  await call('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await call('Page.navigate', { url });
  await sleep(2000);
  await step('プロジェクトを作る', false);
  await step('見本から作る');
  await step('名前と場所を決める', false);
  await sleep(1500);
  await step('書込');
  await sleep(600);
  await step('接続する');
  await sleep(5000);
  await step('書き込む');
  // Wait for the first card being written, then to about the middle of its transfer.
  for (let i = 0; i < 100 && !(await js(`!!document.querySelector('.node.st-busy')`)); i++) await sleep(100);
  await sleep(MID_WRITE_MS);
  if (!(await js(`!!document.querySelector('.node.st-busy')`))) throw new Error('no node is being written; adjust MID_WRITE_MS');
  const shot = await call('Page.captureScreenshot', { format: 'png' });
  mkdirSync(join(ROOT, 'assets/console'), { recursive: true });
  writeFileSync(OUT, Buffer.from(shot.result.data, 'base64'));
  console.log(`wrote ${OUT}`);
} finally {
  ws.close();
  chrome.kill();
  server.close();
  await sleep(1000);
  // The browser may still hold its profile for a moment; a folder left in the temporary place is harmless.
  try {
    rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 });
  } catch {
    console.warn(`could not remove ${profile}`);
  }
}
