import { expect, test, type Page } from '@playwright/test';
import { canvasState, openSample, settled } from './console.ts';

/** Presses one row of the ☰ menu, or returns it unpressed when asked to look only. */
async function row(page: Page, group: string | null, label: string) {
  await page.getByRole('button', { name: 'メニューを開く' }).click();
  if (group) await page.locator('.panel').first().getByRole('button', { name: group, exact: true }).click();
  return page.locator('.panel').last().locator(`button:has(span:text-is("${label}"))`);
}

const press = async (page: Page, group: string | null, label: string) => (await row(page, group, label)).click();

async function offWithReason(page: Page, group: string, label: string) {
  const r = await row(page, group, label);
  await expect(r).toHaveAttribute('aria-disabled', 'true');
  expect((await r.getAttribute('title'))?.length, `${label} says why`).toBeGreaterThan(0);
  await r.click({ force: true });
  await expect(page.locator('#menu'), 'an unavailable item leaves the menu open').toBeVisible();
  await page.keyboard.press('Escape');
}

const connect = async (page: Page) => {
  await page.locator('header .hub').click();
  await expect(page.locator('header .hub')).toHaveText('切断する', { timeout: 30_000 });
  await page.locator('.st-alive').first().waitFor({ timeout: 30_000 });
};

const sentence = (page: Page) => page.locator('#band .sentence').innerText();

/** Drags card `n` out of the tree. */
async function dragCard(page: Page, n = 1) {
  const c = (await page.locator('.node').nth(n).boundingBox())!;
  await page.mouse.move(c.x + 60, c.y + 12);
  await page.mouse.down();
  await page.mouse.move(c.x + 160, c.y + 90, { steps: 4 });
  await page.mouse.up();
  await page.waitForTimeout(100);
}

const start = async (page: Page, query = 'sim') => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page, query);
};

/** What pressing each item must do, on a page in the state the item needs. */
const ITEMS: Record<string, { query?: string; check: (p: Page) => Promise<void> }> = {
  'プロジェクト/プロジェクトを作る': {
    check: async (p) => {
      await press(p, 'プロジェクト', 'プロジェクトを作る');
      await expect(p.locator('.node')).toHaveCount(0);
    },
  },
  'プロジェクト/プロジェクトを開く': {
    check: async (p) => {
      await press(p, 'プロジェクト', 'プロジェクトを開く');
      await expect(p.locator('#start-h')).toBeVisible();
    },
  },
  // The stand-in's 構成 has no project file to save to; the served page's item is in the same row.
  'プロジェクト/保存する': { check: (p) => offWithReason(p, 'プロジェクト', '保存する') },
  '構成/元に戻す': {
    check: async (p) => {
      await offWithReason(p, '構成', '元に戻す');
      const n = await p.locator('.node').count();
      await p.locator('.node').first().click();
      await p.locator('.node').first().click();
      await p.locator('#parts-rows [role="button"], #parts-rows [aria-label$="置く"]').first().dblclick();
      await expect(p.locator('.node')).toHaveCount(n + 1);
      await press(p, '構成', '元に戻す');
      await expect(p.locator('.node')).toHaveCount(n);
    },
  },
  '構成/やり直す': {
    check: async (p) => {
      await offWithReason(p, '構成', 'やり直す');
      const n = await p.locator('.node').count();
      await p.locator('#parts-rows [role="button"], #parts-rows [aria-label$="置く"]').first().dblclick();
      await press(p, '構成', '元に戻す');
      await press(p, '構成', 'やり直す');
      await expect(p.locator('.node')).toHaveCount(n + 1);
    },
  },
  '構成/部品を開く': {
    check: async (p) => {
      await p.getByRole('button', { name: '部品を閉じる' }).click();
      await expect(p.locator('.float.parts')).toHaveCount(0);
      await press(p, '構成', '部品を開く');
      await expect(p.locator('.float.parts')).toBeVisible();
    },
  },
  '構成/自動で並べる': {
    check: async (p) => {
      await offWithReason(p, '構成', '自動で並べる');
      const before = (await p.locator('.node').nth(1).boundingBox())!;
      await dragCard(p);
      expect((await p.locator('.node').nth(1).boundingBox())!.x).not.toBe(before.x);
      await press(p, '構成', '自動で並べる');
      await settled(p);
      expect((await p.locator('.node').nth(1).boundingBox())!.x).toBeCloseTo(before.x, 0);
    },
  },
  '構成/キャンバス全体を表示する': {
    check: async (p) => {
      const first = await canvasState(p);
      const box = (await p.locator('.box').boundingBox())!;
      await p.mouse.move(box.x + box.width - 500, box.y + 200);
      await p.mouse.wheel(40, 40);
      expect((await canvasState(p)).transform).not.toBe(first.transform);
      await press(p, '構成', 'キャンバス全体を表示する');
      await settled(p);
      expect(await canvasState(p)).toEqual(first);
    },
  },
  'つなぐ/接続する': {
    check: async (p) => {
      await press(p, 'つなぐ', '接続する');
      await expect(p.locator('header .hub')).toHaveText('切断する', { timeout: 30_000 });
    },
  },
  'つなぐ/パッケージファイルを選んで書き込む': {
    check: async (p) => {
      await offWithReason(p, 'つなぐ', 'パッケージファイルを選んで書き込む');
      await p.getByRole('radio', { name: '書込' }).click();
      await connect(p);
      const chooser = p.waitForEvent('filechooser', { timeout: 5000 });
      await press(p, 'つなぐ', 'パッケージファイルを選んで書き込む');
      await chooser;
    },
  },
  'つなぐ/前のバージョンに戻す': {
    check: async (p) => {
      await offWithReason(p, 'つなぐ', '前のバージョンに戻す');
      await p.getByRole('radio', { name: '書込' }).click();
      await settled(p);
      await connect(p);
      await offWithReason(p, 'つなぐ', '前のバージョンに戻す');
      await p.locator('.node').nth(1).dispatchEvent('click');
      const before = await sentence(p);
      await press(p, 'つなぐ', '前のバージョンに戻す');
      // Going back asks first, like every change that cannot be undone.
      await p.locator('dialog[open]').getByRole('button', { name: '戻す' }).click();
      await expect.poll(() => sentence(p), { timeout: 15_000 }).not.toBe(before);
    },
  },
  'DAGON/DAGON と連携する': {
    check: async (p) => {
      await press(p, 'DAGON', 'DAGON と連携する');
      await expect(p.locator('.why')).toBeVisible();
    },
  },
  'DAGON/CLI の接続と API キーを見る': {
    check: async (p) => {
      await press(p, 'DAGON', 'CLI の接続と API キーを見る');
      await expect(p.locator('.float.sheet')).toBeVisible();
    },
  },
  'DAGON/DAGON と話す': {
    query: 'sim&agent=linked',
    check: async (p) => {
      await press(p, 'DAGON', 'DAGON と話す');
      await expect(p.locator('.bubble')).toBeVisible();
    },
  },
  // Pressing it with a proposal is agent.test's and the bubble's own path; here: no proposal says so.
  'DAGON/DAGON の案を反映する': { query: 'sim&agent=linked', check: (p) => offWithReason(p, 'DAGON', 'DAGON の案を反映する') },
  'DAGON/DAGON との連携を外す': {
    query: 'sim&agent=linked',
    check: async (p) => {
      await expect(p.locator('.dagon')).toBeVisible();
      await press(p, 'DAGON', 'DAGON との連携を外す');
      await p.locator('dialog[open]').getByRole('button', { name: '連携を外す' }).click();
      await expect(p.locator('.dagon')).toHaveCount(0);
    },
  },
  '困ったとき/困ったとき': {
    check: async (p) => {
      await press(p, '困ったとき', '困ったとき');
      await expect(p.locator('#help')).toBeVisible();
    },
  },
  '困ったとき/キー操作': {
    check: async (p) => {
      await press(p, '困ったとき', 'キー操作');
      await expect(p.locator('#help h2')).toHaveText('キー操作');
    },
  },
  '困ったとき/記録をコピーする': {
    check: async (p) => {
      await p.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      await offWithReason(p, '困ったとき', '記録をコピーする');
      await press(p, 'DAGON', 'DAGON と連携する');
      await expect(p.locator('.why')).toBeVisible();
      // Stopped by the lock table: it said why where it was pressed, and the menu stays.
      await p.keyboard.press('Escape');
      await press(p, '困ったとき', '記録をコピーする');
      await expect(p.locator('.toast')).toBeVisible();
      expect((await p.evaluate(() => navigator.clipboard.readText())).length).toBeGreaterThan(0);
    },
  },
  '困ったとき/この URL をコピーする': {
    check: async (p) => {
      await p.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      await press(p, '困ったとき', 'この URL をコピーする');
      await expect(p.locator('.toast')).toBeVisible();
      expect(await p.evaluate(() => navigator.clipboard.readText())).toBe(await p.evaluate(() => location.href));
    },
  },
  '/最初からやり直す': {
    check: async (p) => {
      await press(p, null, '最初からやり直す');
      await expect(p.locator('dialog[open]')).toBeVisible();
    },
  },
};

test.setTimeout(45_000);

for (const [name, { query, check }] of Object.entries(ITEMS)) {
  test(`menu item ${name}`, async ({ page }) => {
    await start(page, query);
    await check(page);
  });
}

test('the table above covers every item the menu shows, linked or not', async ({ page }) => {
  const shown = new Set<string>();
  for (const query of ['sim', 'sim&agent=linked']) {
    await start(page, query);
    await page.getByRole('button', { name: 'メニューを開く' }).click();
    const first = page.locator('.panel').first();
    for (const r of await first.locator('button:not([aria-expanded]) .words > span:first-child').all()) shown.add(`/${(await r.innerText()).trim()}`);
    for (const g of await first.locator('button[aria-expanded]').all()) {
      const group = (await g.innerText()).trim();
      await g.click();
      for (const r of await page.locator('.panel').last().locator('button .words > span:first-child').all()) shown.add(`${group}/${(await r.innerText()).trim()}`);
    }
  }
  expect([...shown].sort()).toEqual(Object.keys(ITEMS).sort());
});
