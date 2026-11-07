import { expect, test, type Page } from '@playwright/test';
import { openSample } from './console.ts';

/** `?demo`: what `dagon demo` and the demo page are; `agent=linked`: the worst case, an agent that says it is linked. */
const DEMO = 'sim&demo&agent=linked';
const MARK = '.dagon .mark';
const BUBBLE = 'デモでは会話できません。dagon serve で連携すると相談できます。';

async function group(p: Page, name: string) {
  await p.getByRole('button', { name: 'メニューを開く' }).click();
  await p.locator('.panel').first().getByRole('button', { name, exact: true }).click();
  return p.locator('.panel').last();
}
const closedItem = async (p: Page, label: string) => {
  const r = (await group(p, 'DAGON')).locator(`button:has(span:text-is("${label}"))`);
  await expect(r).toHaveAttribute('aria-disabled', 'true');
  expect((await r.getAttribute('title'))?.length, `${label} says why`).toBeGreaterThan(0);
  await r.click({ force: true });
  await p.keyboard.press('Escape');
};
const absentItem = async (p: Page, label: string) => {
  await expect((await group(p, 'DAGON')).locator(`button:has(span:text-is("${label}"))`)).toHaveCount(0);
  await p.keyboard.press('Escape');
};
const nothingOpened = async (p: Page) => {
  await expect(p.locator('dialog[open]')).toHaveCount(0);
  await expect(p.locator('.toast')).toHaveCount(0);
  await expect(p.locator('aside.sheet')).toHaveCount(0);
};

/** Every opener of linking, talking, keys and the CLI sheet (src/demoOpeners.test.ts fails when the code has one this list does not). */
const OPENERS: Record<string, (p: Page) => Promise<void>> = {
  '☰ DAGON と連携する': (p) => closedItem(p, 'DAGON と連携する'),
  '☰ CLI の接続と API キーを見る': (p) => closedItem(p, 'CLI の接続と API キーを見る'),
  '☰ DAGON と話す': (p) => absentItem(p, 'DAGON と話す'),
  '☰ DAGON の案を反映する': (p) => absentItem(p, 'DAGON の案を反映する'),
  '☰ DAGON との連携を外す': (p) => absentItem(p, 'DAGON との連携を外す'),
  'the status band\'s console icon': (p) => expect(p.locator('.dock button.mark')).toHaveCount(0),
  'the drawer\'s 会話 tab': (p) => expect(p.getByRole('tab', { name: '会話' })).toHaveCount(0),
  'the bubble\'s questions and field': async (p) => {
    await p.locator(MARK).click();
    await expect(p.locator('.bubble')).toContainText(BUBBLE);
    await expect(p.locator('.bubble input, .bubble button, .bubble form')).toHaveCount(0);
    await p.locator(MARK).click();
    await expect(p.locator('.bubble')).toHaveCount(0);
  },
  'the / key': async (p) => {
    await p.keyboard.press('/');
    await expect(p.locator('.bubble')).toContainText(BUBBLE);
    await expect(p.locator('.bubble input')).toHaveCount(0);
    await p.keyboard.press('Escape');
  },
};

test.setTimeout(120_000);

test('in the demo every opener is closed, greyed with its reason or absent, and no press answers with an error', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page, DEMO);
  for (const [name, closed] of Object.entries(OPENERS)) {
    await test.step(name, async () => {
      await closed(page);
      await nothingOpened(page);
    });
  }
});

test('in the demo the mark stays, floating and dragged', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page, DEMO);
  const name = await page.locator(`${MARK} img`).evaluate((e) => getComputedStyle(e).animationName);
  expect(name).not.toBe('none');
  const at = (await page.locator(MARK).boundingBox())!;
  await page.mouse.move(at.x + at.width / 2, at.y + at.height / 2);
  await page.mouse.down();
  await page.mouse.move(600, 300, { steps: 5 });
  await page.mouse.up();
  expect((await page.locator(MARK).boundingBox())!.x).not.toBe(at.x);
});

test('dragging the mark with its bubble open moves only the mark: no layout read per move, one storage write on the drop', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page, DEMO);
  await page.locator(MARK).click();
  await expect(page.getByText(BUBBLE)).toBeVisible();
  await page.evaluate(() => {
    const w = window as unknown as { count: { rects: number; writes: number } };
    w.count = { rects: 0, writes: 0 };
    const rect = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function () {
      w.count.rects++;
      return rect.call(this);
    };
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (k, v) {
      w.count.writes++;
      return set.call(this, k, v);
    };
  });
  const count = () => page.evaluate(() => ({ ...(window as unknown as { count: { rects: number; writes: number } }).count }));
  const at = (await page.locator(MARK).boundingBox())!;
  await page.mouse.move(at.x + at.width / 2, at.y + at.height / 2);
  await page.mouse.down();
  await page.mouse.move(600, 300, { steps: 30 });
  // Before the fix one move cost ~7000 bubble placements (~48k layout reads, ~25 s). The stage settling may still place it once (a few boxes), never per move.
  const during = await count();
  expect(during.writes).toBe(0);
  expect(during.rects).toBeLessThan(30);
  await page.mouse.up();
  expect((await count()).writes).toBe(1);
  const moved = (await page.locator(MARK).boundingBox())!;
  expect(Math.abs(moved.x + moved.width / 2 - 600)).toBeLessThan(2);
  await expect(page.getByText(BUBBLE)).toBeVisible();
});

test('outside the demo the same openers are there', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page, 'sim&agent=linked');
  await page.locator(MARK).click();
  await expect(page.locator('.bubble input')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('tab', { name: '会話' })).toHaveCount(1);
  await expect(page.locator('.dock button.mark')).toHaveCount(1);
  const dagon = await group(page, 'DAGON');
  await expect(dagon.locator('button:has(span:text-is("DAGON と話す"))')).toHaveCount(1);
});
