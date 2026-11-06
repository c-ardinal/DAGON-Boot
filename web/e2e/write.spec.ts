import { fileURLToPath } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import { layoutFaults, openSample, settled } from './console.ts';

test.setTimeout(360_000);

const toWrite = async (page: Page, query: string, size = { width: 1920, height: 1080 }) => {
  await page.setViewportSize(size);
  await openSample(page, query);
  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  await page.locator('#band').getByRole('button', { name: '接続する' }).click();
  await page.locator('.st-alive').first().waitFor({ timeout: 60_000 });
  await expect(page.locator('.split .main')).toHaveAttribute('aria-disabled', 'false', { timeout: 30_000 });
  await expect(page.locator('.split .caret')).toHaveAttribute('aria-disabled', 'false', { timeout: 30_000 });
};
/** A card's check, by its order in the sample: hub, seg, watch. */
const check = (page: Page, n: 'hub' | 'seg' | 'watch') => page.locator('.check').nth(['hub', 'seg', 'watch'].indexOf(n));
const sentence = (page: Page) => page.locator('#band .sentence').innerText();
const finished = (page: Page) => expect(page.locator('.split .main')).toHaveAttribute('aria-disabled', 'false', { timeout: 240_000 });
const heights = (page: Page) => page.locator('.node').evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height));

test('the split button writes the checked nodes, again after done, and says when there is nothing to do', async ({ page }) => {
  await toWrite(page, 'sim');
  const before = await heights(page);
  // With a build there to write, the band asks for the press, not for a file.
  await expect(page.locator('#band .sentence')).toContainText('『書き込む』を押して');

  await check(page, 'watch').click();
  // Every height any card has while the write runs, the 親機's reboot included.
  await page.evaluate(() => {
    const seen = new Set<string>();
    (window as unknown as { seen: Set<string> }).seen = seen;
    const look = () => {
      seen.add(JSON.stringify([...document.querySelectorAll('.node')].map((e) => e.getBoundingClientRect().height)));
      requestAnimationFrame(look);
    };
    look();
  });
  await page.locator('.split .main').click();
  await expect(page.locator('.node.st-busy').first()).toBeVisible({ timeout: 30_000 });
  await finished(page);
  expect(await page.evaluate(() => [...(window as unknown as { seen: Set<string> }).seen])).toEqual([JSON.stringify(before)]);
  await expect(page.locator('.toast')).toBeVisible();
  // Left out by choice is no node that did not answer.
  await expect.poll(() => sentence(page)).toContain('チェックしたノード');
  expect(await sentence(page)).not.toContain('応答しなかった');

  // Every checked node is on it already: said, not silent.
  await page.locator('.toast').waitFor({ state: 'detached', timeout: 10_000 });
  await page.locator('.split .main').click();
  await expect(page.locator('.toast')).toContainText('すべて');
  await expect(page.locator('.node.st-busy')).toHaveCount(0);

  // The dropdown part does the same, and the focus comes back to the button.
  await page.locator('.toast').waitFor({ state: 'detached', timeout: 10_000 });
  await page.locator('.split .caret').click();
  await page.locator('.split .list .row').first().click();
  await expect(page.locator('.toast')).toContainText('すべて');
  await expect(page.locator('.split .main')).toBeFocused();

  // Nothing checked: said too.
  await page.locator('.toast').waitFor({ state: 'detached', timeout: 10_000 });
  for (const n of ['hub', 'seg'] as const) await check(page, n).click();
  await page.locator('.split .main').click();
  await expect(page.locator('.toast')).toContainText('ノードを選ぶ');

  // Another node checked: the press after done writes it, from the dropdown part.
  await check(page, 'watch').click();
  await page.locator('.toast').waitFor({ state: 'detached', timeout: 10_000 });
  await page.locator('.split .caret').click();
  await page.locator('.split .list .row').first().click();
  await expect(page.locator('.node.st-busy').first()).toBeVisible({ timeout: 30_000 });
  await finished(page);
  await expect(page.locator('dialog[open]')).toHaveCount(0);
});

test('a node whose boot loader alone differs is written, and its card says the boot loader', async ({ page }) => {
  const fixture = (name: string) => fileURLToPath(new URL(`../../crates/dagon-sim/fixtures/${name}`, import.meta.url));
  // What the project's latest build is: the apps at 1.1.0, then the same apps with the F401's boot loader at 1.1.0.
  let latest = 'sample-1.1.0.dgb';
  // The file itself, not the module that names its address (`?url`).
  await page.route((url) => url.pathname.endsWith('/sample-full-1.1.0.dgb') && !url.search, (route) => route.fulfill({ path: fixture(latest) }));
  await toWrite(page, 'sim');
  for (const n of ['hub', 'watch'] as const) await check(page, n).click();
  await page.locator('.split .main').click();
  await expect(page.locator('.node.st-busy').first()).toBeVisible({ timeout: 30_000 });
  latest = 'sample-core-1.1.0.dgb';
  await finished(page);
  await page.locator('.toast').waitFor({ state: 'detached', timeout: 10_000 });

  // The app is on 1.1.0 already: only the boot loader stands to be written.
  await page.locator('.split .main').click();
  await expect(page.locator('.node', { hasText: '→ ブートローダ 1.1.0' })).toBeVisible({ timeout: 60_000 });
  await finished(page);
  // The app was what the node runs: it was not written.
  await expect(page.locator('.node', { hasText: '→ 1.1.0' })).toHaveCount(0);
  await page.locator('.toast').waitFor({ state: 'detached', timeout: 10_000 });

  // Now both are the same: said, nothing written.
  await page.locator('.split .main').click();
  await expect(page.locator('.toast')).toContainText('すべて 1.1.0');
  await expect(page.locator('.node.st-busy')).toHaveCount(0);
});

for (const size of [{ width: 1920, height: 1080 }, { width: 1280, height: 720 }, { width: 1024, height: 600 }]) {
  test(`at ${size.width}x${size.height} a write that went back is told without a scrim, never over its node`, async ({ page }) => {
    await toWrite(page, 'sim&fail', size);
    await page.locator('.split .main').click();
    const notice = page.locator('[data-stop-notice]');
    await notice.waitFor({ timeout: 300_000 });
    await settled(page);
    await expect(page.locator('dialog[open]')).toHaveCount(0);
    const covered = () =>
      page.evaluate(() => {
        const n = document.querySelector('[data-stop-notice]')!.getBoundingClientRect();
        return [...document.querySelectorAll('.node.st-attention')].map((c) => c.getBoundingClientRect()).some((c) => c.left < n.right && n.left < c.right && c.top < n.bottom && n.top < c.bottom);
      });
    await expect(page.locator('.node.st-attention')).toHaveCount(1);
    expect(await covered()).toBe(false);
    expect(await layoutFaults(page)).toEqual([]);
    // The title has one mark: the words carry it.
    expect(await notice.locator('header > .icon').count()).toBe(0);
    await page.keyboard.press('Escape');
    await expect(notice).toHaveCount(0);
  });
}

test('the mascot does not move when a toast comes', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page, 'sim&agent=linked');
  await settled(page);
  const at = () => page.locator('.dagon').boundingBox();
  const before = await at();
  await page.getByRole('button', { name: 'メニューを開く' }).click();
  await page.getByRole('button', { name: '困ったとき', exact: true }).click();
  await page.locator('.panel').last().locator('button:has(> span:text-is("この URL をコピーする"))').click();
  await page.locator('.toast').waitFor();
  await settled(page);
  expect(await at()).toEqual(before);
  const [t, m] = [(await page.locator('.toast').boundingBox())!, (await at())!];
  expect(m.y + m.height <= t.y || m.x >= t.x + t.width, 'nor does the toast cover it').toBe(true);
});

test('a link starts at a whole port over the card edge, and its chip stands clear of the strands', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const faults = await page.evaluate(() => {
    const out: string[] = [];
    for (const p of document.querySelectorAll('.port.end')) {
      const r = p.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      if (top !== p) out.push(`port covered by ${top?.className}`);
    }
    for (const c of document.querySelectorAll('.link-chip')) {
      const r = c.getBoundingClientRect();
      for (const w of document.querySelectorAll<SVGPathElement>('.wire')) {
        const m = w.getScreenCTM()!;
        for (let s = 0; s < w.getTotalLength(); s += 4) {
          const q = w.getPointAtLength(s);
          const [x, y] = [m.a * q.x + m.e, m.d * q.y + m.f];
          if (x > r.left && x < r.right && y > r.top && y < r.bottom) return [...out, `chip ${c.textContent?.trim()} over a strand`];
        }
      }
    }
    return out;
  });
  expect(faults).toEqual([]);
  // Nothing beside the links' ports: a card with links shows no other port under it.
  expect(await page.locator('.port.out:not(.end)').count()).toBe(2);
});

test('the first view fills the stage: the graph takes most of the free height or its zoom limit', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const nodes = await page.locator('.node').evaluateAll((els) => els.map((e) => e.getBoundingClientRect()).map((r) => [r.top, r.bottom]));
  const span = Math.max(...nodes.map((n) => n[1])) - Math.min(...nodes.map((n) => n[0]));
  expect(span).toBeGreaterThan(500);
});
