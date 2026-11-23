import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { openSample, settled } from './console.ts';

// WCAG 2.2 AA, as axe-core checks it, on every screen the Console shows. The criteria a
// machine cannot judge are in the audit record; the ones fixed by hand have their own tests below.
const WCAG = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22a', 'wcag22aa'];

async function violations(page: Page, where: string, into: string[]): Promise<void> {
  await page.waitForTimeout(400);
  const r = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  for (const v of r.violations) for (const n of v.nodes) into.push(`${where}: ${v.id} ${n.target.join(' ')}`);
}

const menu = async (p: Page, group: string, item: string) => {
  await p.getByRole('button', { name: 'メニューの表示' }).click();
  await p.locator('.panel').first().getByRole('button', { name: group, exact: true }).click();
  await p.locator('.panel').last().locator(`button:has(span:text-is("${item}"))`).click();
};

test.setTimeout(360_000);

test('axe finds no WCAG 2.2 AA violation on any screen: start, 準備 with its sheet, drawer, menu, help, table, dialog, toast, bubble, and 書込 before, during and after a write', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.setViewportSize({ width: 1920, height: 1080 });
  const found: string[] = [];
  await page.goto('/?sim');
  await page.waitForTimeout(700);
  await violations(page, '開始', found);
  await page.getByRole('button', { name: 'プロジェクト作成' }).click();
  await page.getByRole('radio', { name: 'サンプルから作成' }).click();
  await violations(page, '作成', found);
  await page.keyboard.press('Escape');
  await openSample(page, 'sim&agent=linked');
  await violations(page, '準備', found);
  await page.locator('.node').first().dispatchEvent('click');
  await violations(page, 'シート', found);
  await page.locator('.node').first().dispatchEvent('click');
  for (const t of ['問題', '作業', '出力', 'タイムライン', '会話', '記録']) {
    await page.getByRole('tab', { name: t }).click();
    await violations(page, `ドロワー ${t}`, found);
  }
  await page.getByRole('tab', { name: '記録' }).click();
  await page.getByRole('button', { name: 'メニューの表示' }).click();
  for (const g of ['プロジェクト', '構成', '接続操作', 'DAGON', 'トラブル対処']) {
    await page.locator('.panel').first().getByRole('button', { name: g, exact: true }).click();
    await violations(page, `メニュー ${g}`, found);
  }
  await page.keyboard.press('Escape');
  await menu(page, 'トラブル対処', 'キー操作');
  await violations(page, 'キー操作', found);
  await page.keyboard.press('Escape');
  await page.getByRole('radio', { name: '表', exact: true }).click();
  const tabs = page.locator('.table [role="tab"]');
  for (let i = 0; i < (await tabs.count()); i++) {
    await tabs.nth(i).click();
    await violations(page, `表 ${i}`, found);
  }
  await page.getByRole('radio', { name: 'グラフ', exact: true }).click();
  await page.locator('.dagon .mark').click();
  await violations(page, 'バブル', found);
  await page.locator('.dagon .mark').click();
  await menu(page, 'プロジェクト', 'リスタート');
  await violations(page, 'モーダル', found);
  await page.keyboard.press('Escape');
  await menu(page, 'トラブル対処', 'このURLのコピー');
  await page.locator('.toast').waitFor();
  await violations(page, 'トースト', found);

  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  await violations(page, '書込', found);
  await page.locator('header').getByRole('button', { name: '接続' }).click();
  await page.locator('.st-alive').first().waitFor({ timeout: 60_000 });
  await expect(page.locator('.split .caret')).toHaveAttribute('aria-disabled', 'false', { timeout: 30_000 });
  await violations(page, '書込 接続', found);
  await page.locator('.split .caret').click();
  await violations(page, '書込 バージョン', found);
  await page.keyboard.press('Escape');
  await page.locator('.check').nth(2).click();
  await page.locator('.split .main').click();
  await expect(page.locator('.node.st-busy').first()).toBeVisible({ timeout: 30_000 });
  await violations(page, '書込中', found);
  await expect(page.locator('.split .main')).toHaveAttribute('aria-disabled', 'false', { timeout: 240_000 });
  await violations(page, '書込後', found);
  expect([...new Set(found)]).toEqual([]);
});

test('a control the state stops is disabled for assistive technology and says why, and comes back with its own state (4.1.2)', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  // The stand-in has no serve: テンプレート生成 and ビルド are stopped by the lock table.
  const gen = page.locator('[data-control="gen"]');
  await expect(gen).toHaveAttribute('aria-disabled', 'true');
  expect((await gen.getAttribute('title'))?.length).toBeGreaterThan(0);
  // A write stops the edit tools and the undo; when it is over they are theirs again.
  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  await page.locator('header').getByRole('button', { name: '接続' }).click();
  await page.locator('.st-alive').first().waitFor({ timeout: 60_000 });
  const hub = page.locator('header [data-control="connect"]');
  await expect(hub).toHaveAttribute('aria-disabled', 'false', { timeout: 30_000 });
  await expect(page.locator('.split .main')).toHaveAttribute('aria-disabled', 'false', { timeout: 30_000 });
  await page.locator('.check').nth(2).click();
  await page.locator('.split .main').click();
  await expect(page.locator('.node.st-busy').first()).toBeVisible({ timeout: 30_000 });
  await expect(hub).toHaveAttribute('aria-disabled', 'true');
  await expect(page.locator('.split .main')).toHaveAttribute('aria-disabled', 'false', { timeout: 240_000 });
  await expect(hub).toHaveAttribute('aria-disabled', 'false');
  expect((await hub.getAttribute('title')) ?? '').not.toContain('書込中');
  // Nothing still stands as stopped whose control the page no longer stops.
  const stale = await page.evaluate(() => {
    const locks = (document.querySelector('.frame')?.getAttribute('data-locks') ?? '').split(' ');
    return [...document.querySelectorAll<HTMLElement>('[data-lock-own-disabled]')]
      .filter((e) => !locks.includes(e.closest<HTMLElement>('[data-control]')?.dataset.control ?? ''))
      .map((e) => e.outerHTML.slice(0, 80));
  });
  expect(stale).toEqual([]);
});

test('F and / act only with the focus on the canvas (2.1.4)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page, 'sim&agent=linked');
  await page.getByRole('button', { name: 'キャンバスの拡大' }).click();
  const zoom = () => page.locator('.pill').innerText();
  const zoomed = await zoom();
  await page.getByRole('button', { name: 'メニューの表示' }).focus();
  await page.keyboard.press('f');
  await page.keyboard.press('/');
  expect(await zoom()).toBe(zoomed);
  await expect(page.locator('.bubble')).toHaveCount(0);
  await page.locator('#canvas').focus();
  await page.keyboard.press('f');
  await expect.poll(zoom).not.toBe(zoomed);
  await page.keyboard.press('/');
  await expect(page.locator('.bubble')).toBeVisible();
});

test('the drawer height without a drag: a full-size button switches it between the tallest and the lowest (2.5.7, 2.5.8)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page);
  await page.getByRole('tab', { name: '記録' }).click();
  await page.waitForTimeout(400);
  const height = async () => (await page.locator('.drawer').boundingBox())!.height;
  const before = await height();
  const raise = page.getByRole('button', { name: 'パネルの最大化' });
  const box = (await raise.locator('.layer').boundingBox())!;
  expect(Math.min(box.width, box.height)).toBeGreaterThanOrEqual(24);
  await raise.click();
  await expect.poll(height).toBeGreaterThan(before);
  await page.getByRole('button', { name: 'パネルの最小化' }).click();
  await expect.poll(height).toBeLessThan(before + 1);
});

test('text holds 4.5:1 where axe cannot see its ground: placeholders, and the card words over the water (1.4.3)', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const ratio = (fg: string, bg: string) =>
    page.evaluate(
      ([f, b]) => {
        const lum = (c: string) => {
          // rgb() in 0–255, color(srgb …) (what color-mix() computes to) in 0–1.
          const scale = c.startsWith('color(') ? 1 : 255;
          const [r, g, bl] = c.match(/[\d.]+/g)!.slice(0, 3).map((v) => {
            const x = Number(v) / scale;
            return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
          });
          return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
        };
        const [x, y] = [lum(f), lum(b)];
        return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
      },
      [fg, bg],
    );
  const ink = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const placeholder = await page.locator('input[placeholder]').first().evaluate((e) => getComputedStyle(e, '::placeholder').color);
  expect(await ratio(placeholder, ink)).toBeGreaterThanOrEqual(4.5);

  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  await page.locator('header').getByRole('button', { name: '接続' }).click();
  await page.locator('.st-alive').first().waitFor({ timeout: 60_000 });
  await expect(page.locator('.split .main')).toHaveAttribute('aria-disabled', 'false', { timeout: 30_000 });
  await page.locator('.check').nth(2).click();
  await page.locator('.split .main').click();
  const card = page.locator('.node.filling').first();
  await card.waitFor({ timeout: 30_000 });
  const water = await card.locator('.level').evaluate((e) => getComputedStyle(e).backgroundColor);
  for (const sel of ['.role', '.meta']) {
    const fg = await card.locator(sel).evaluate((e) => getComputedStyle(e).color);
    expect(await ratio(fg, water), sel).toBeGreaterThanOrEqual(4.5);
  }
});
