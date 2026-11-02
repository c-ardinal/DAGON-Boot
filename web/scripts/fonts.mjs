// Fetches subset woff2 files from Google Fonts into public/fonts/.
// Run after changing screen text under src/: `npm run fonts`. Output is
// committed so the page never loads fonts from a third party at run time.
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';

const root = new URL('..', import.meta.url);
const out = new URL('public/fonts/', root);

const ascii = Array.from({ length: 95 }, (_, i) => String.fromCharCode(32 + i)).join('');
const digits = '0123456789';
const latin = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
const sources = (await readdir(new URL('src/', root), { recursive: true }))
  .filter((p) => /\.(ts|svelte)$/.test(p) && !p.endsWith('.test.ts'));
let screenText = '';
for (const p of sources) screenText += await readFile(new URL(`src/${p.replaceAll('\\', '/')}`, root), 'utf8');
const japanese = [...new Set(screenText)].filter((c) => c.codePointAt(0) > 0x7e).sort().join('');

const fonts = [
  { file: 'doto-700', family: 'Doto', axis: 'wght@700', text: `${digits}%/ ` },
  { file: 'dotgothic16-400', family: 'DotGothic16', axis: '', text: `${digits}${latin}./ ` },
  { file: 'jetbrains-mono-500', family: 'JetBrains Mono', axis: 'wght@500', text: `${ascii}·…→←` },
  ...[400, 500, 700].map((w) => ({
    file: `zen-kaku-gothic-new-${w}`,
    family: 'Zen Kaku Gothic New',
    axis: `wght@${w}`,
    text: ascii + japanese,
  })),
];

// A current Chrome UA makes the API answer with woff2.
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36';

await mkdir(out, { recursive: true });
for (const f of fonts) {
  const family = f.family.replaceAll(' ', '+') + (f.axis ? `:${f.axis}` : '');
  const css = await get(`https://fonts.googleapis.com/css2?family=${family}&text=${encodeURIComponent(f.text)}`);
  const urls = [...(await css.text()).matchAll(/url\((https:[^)]+)\)/g)].map((m) => m[1]);
  if (urls.length !== 1) throw new Error(`${f.file}: expected one face, got ${urls.length}`);
  const bytes = new Uint8Array(await (await get(urls[0])).arrayBuffer());
  if (String.fromCharCode(...bytes.subarray(0, 4)) !== 'wOF2') throw new Error(`${f.file}: not woff2`);
  await writeFile(new URL(`${f.file}.woff2`, out), bytes);
  console.log(`${f.file}.woff2 ${bytes.length} B`);
}

async function get(url) {
  const r = await fetch(url, { headers: { 'User-Agent': UA } });
  if (!r.ok) throw new Error(`${r.status} ${url}`);
  return r;
}
