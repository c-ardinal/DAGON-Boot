import { readFileSync } from 'node:fs';
import { expect, test, type Page, type WebSocketRoute } from '@playwright/test';

const SAMPLE = readFileSync(new URL('../../topology/sample.yaml', import.meta.url), 'utf8');
const PASS = '0123456789abcdef0123456789abcdef';
const KEY = '00'.repeat(32);

/** A dagon serve that does what the test says when it says: a build advances only on `send`. */
async function serve(page: Page, opts: { buildNodes: string[]; earlyEvents?: boolean }) {
  let ws: WebSocketRoute | undefined;
  let job = 0;
  const started: string[] = [];
  await page.routeWebSocket('**/v1/events', (w) => {
    ws = w;
    w.onMessage(() => undefined);
  });
  await page.route('**/v1/**', (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.slice('/v1/'.length);
    const json = (v: unknown, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(v) });
    if (route.request().method() === 'POST' && ['build', 'sign', 'release'].includes(path)) {
      started.push(path);
      job++;
      if (!opts.earlyEvents) return json({ job }, 202);
      // As the real one does: the job runs and talks before its 202 reaches the page.
      ws!.send(JSON.stringify({ type: 'job.state', job, state: 'running' }));
      ws!.send(JSON.stringify({ type: 'job.log', job, lines: ['hub: building slot a in hub'] }));
      return new Promise((r) => setTimeout(r, 200)).then(() => json({ job }, 202));
    }
    switch (path) {
      case 'hello':
        return json({ cli: '0.1.0', api: 1, formats: {}, workspace: 'p', features: ['build', 'sign', 'release', 'project', 'layout', 'kept'], key: true, key_id: 'ab', release_key: KEY, build_nodes: opts.buildNodes, project: 'p' });
      case 'project':
        return json({ open: { path: 'p', name: 'p' }, recent: [] });
      case 'topology':
        return route.fulfill({ status: 200, contentType: 'text/yaml', body: SAMPLE });
      case 'layout':
        return json({ format: 1 });
      case 'kept':
        return json({ kept: [] });
      default:
        return json({ error: 'none' }, 404);
    }
  });
  const send = (e: Record<string, unknown>) => ws!.send(JSON.stringify(e));
  return {
    started,
    state: (id: number, state: string) => send({ type: 'job.state', job: id, state }),
    log: (id: number, ...lines: string[]) => send({ type: 'job.log', job: id, lines }),
  };
}

/** The page has taken serve's answer: its job is listed, so events about it reach it. */
const taken = (page: Page) => expect(page.getByRole('tab', { name: '作業' })).toContainText('1');

const errors: string[] = [];
test.afterEach(() => {
  expect(errors.splice(0)).toEqual([]);
});

const lit = (page: Page) => page.locator('.node.st-busy').evaluateAll((els) => els.map((e) => e.getAttribute('data-key')));
const sentence = (page: Page) => page.locator('#band .sentence');

async function open(page: Page) {
  // The page opening faster than its WebAssembly loads throws nothing.
  page.on('pageerror', (e) => errors.push(e.message));
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.goto(`/#${PASS}`);
  await page.locator('.node').first().waitFor();
  await page.getByRole('radio', { name: '準備' }).click();
}

test('a build advances node by node: only the node being built has the light, and it goes when that node is done', async ({ page }) => {
  const s = await serve(page, { buildNodes: ['hub', 'seg', 'watch'] });
  await open(page);
  await expect(sentence(page)).toContainText('ビルドしてから');
  await page.getByRole('button', { name: 'ビルドする' }).click();
  await expect.poll(() => s.started).toEqual(['build']);
  await taken(page);
  await s.state(1, 'running');
  // Nobody named yet: nobody lit; the band says to wait, not to build.
  await expect(sentence(page)).toContainText('そのまま待って');
  expect(await lit(page)).toEqual([]);

  for (const [node, others] of [['hub', 2], ['seg', 2], ['watch', 2]] as const) {
    await s.log(1, `${node}: building slot a in ${node}`);
    await expect.poll(() => lit(page)).toEqual([`node:${node}`]);
    expect(others).toBe(2);
    // Long compiler output, with a delay: the same node keeps it, no other gets it.
    await s.log(1, ...Array.from({ length: 600 }, (_, i) => `   Compiling crate-${i}`));
    await page.waitForTimeout(300);
    expect(await lit(page)).toEqual([`node:${node}`]);
    await expect(page.locator('.chip', { hasText: 'ビルド' })).toHaveCount(0);
  }

  await s.state(1, 'succeeded');
  // watch is done: its light goes though signing has named no node.
  await expect.poll(() => lit(page)).toEqual([]);
  await expect.poll(() => s.started).toEqual(['build', 'sign']);
  await taken(page);
  await s.state(2, 'running');
  await s.log(2, 'seg: building the boot core for slot a');
  await expect.poll(() => lit(page)).toEqual(['node:seg']);
  await s.state(2, 'succeeded');
  await expect.poll(() => lit(page)).toEqual([]);
  await expect.poll(() => s.started).toEqual(['build', 'sign', 'release']);
  await taken(page);
  await s.state(3, 'running');
  expect(await lit(page)).toEqual([]);
  await s.state(3, 'succeeded');

  // Done: the band says the next true step, no longer the old one.
  await expect(sentence(page)).toContainText('書込に切り替えて');
  await expect(sentence(page)).not.toContainText('ビルドしてから');
  expect(await lit(page)).toEqual([]);
});

test('a job serve has already started when its number arrives still lights its node', async ({ page }) => {
  const s = await serve(page, { buildNodes: ['hub', 'seg', 'watch'], earlyEvents: true });
  await open(page);
  await page.getByRole('button', { name: 'ビルドする' }).click();
  await expect.poll(() => lit(page)).toEqual(['node:hub']);
  expect(s.started).toEqual(['build']);
});

test('an edit after a build asks for a build again', async ({ page }) => {
  const s = await serve(page, { buildNodes: ['hub', 'seg', 'watch'] });
  await open(page);
  await page.getByRole('button', { name: 'ビルドする' }).click();
  await expect.poll(() => s.started).toEqual(['build']);
  await taken(page);
  await s.state(1, 'succeeded');
  await expect.poll(() => s.started).toEqual(['build', 'sign']);
  await taken(page);
  await s.state(2, 'succeeded');
  await expect.poll(() => s.started).toEqual(['build', 'sign', 'release']);
  await taken(page);
  await s.state(3, 'succeeded');
  await expect(sentence(page)).toContainText('書込に切り替えて');
  await page.locator('#parts-rows [aria-label$="置く"]').first().dblclick();
  await expect(sentence(page)).not.toContainText('書込に切り替えて');
});

test('no [build.*] yet: the band leads to the skeleton', async ({ page }) => {
  await serve(page, { buildNodes: [] });
  await open(page);
  await expect(sentence(page)).toContainText('雛形を作る');
});
