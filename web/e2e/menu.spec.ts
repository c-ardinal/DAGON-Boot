import { expect, test, type Page } from '@playwright/test';
import { canvasState, openSample, settled } from './console.ts';

/** Presses one row of the ☰ menu, or returns it unpressed when asked to look only. */
async function row(page: Page, group: string | null, label: string) {
  await page.getByRole('button', { name: 'メニューの表示' }).click();
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
  await expect(page.locator('header .hub')).toHaveText('切断', { timeout: 30_000 });
  await page.locator('.st-alive').first().waitFor({ timeout: 30_000 });
};

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
  'プロジェクト/プロジェクト作成': {
    check: async (p) => {
      await press(p, 'プロジェクト', 'プロジェクト作成');
      await expect(p.locator('.node')).toHaveCount(0);
    },
  },
  'プロジェクト/プロジェクトのオープン': {
    check: async (p) => {
      await press(p, 'プロジェクト', 'プロジェクトのオープン');
      await expect(p.locator('#start-h')).toBeVisible();
    },
  },
  // The stand-in's 構成 has no project file to save to; the served page's item is in the same row.
  'プロジェクト/保存': { check: (p) => offWithReason(p, 'プロジェクト', '保存') },
  '構成/取消': {
    check: async (p) => {
      await offWithReason(p, '構成', '取消');
      const n = await p.locator('.node').count();
      await p.locator('.node').first().click();
      await p.locator('.node').first().click();
      await p.locator('#parts-rows [role="button"], #parts-rows [aria-label$="配置"]').first().dblclick();
      await expect(p.locator('.node')).toHaveCount(n + 1);
      await press(p, '構成', '取消');
      await expect(p.locator('.node')).toHaveCount(n);
    },
  },
  '構成/再実行': {
    check: async (p) => {
      await offWithReason(p, '構成', '再実行');
      const n = await p.locator('.node').count();
      await p.locator('#parts-rows [role="button"], #parts-rows [aria-label$="配置"]').first().dblclick();
      await press(p, '構成', '取消');
      await press(p, '構成', '再実行');
      await expect(p.locator('.node')).toHaveCount(n + 1);
    },
  },
  '構成/部品の表示': {
    check: async (p) => {
      await p.getByRole('button', { name: '部品の非表示' }).click();
      await expect(p.locator('.float.parts')).toHaveCount(0);
      await press(p, '構成', '部品の表示');
      await expect(p.locator('.float.parts')).toBeVisible();
    },
  },
  '構成/自動整列': {
    check: async (p) => {
      await offWithReason(p, '構成', '自動整列');
      const before = (await p.locator('.node').nth(1).boundingBox())!;
      await dragCard(p);
      expect((await p.locator('.node').nth(1).boundingBox())!.x).not.toBe(before.x);
      await press(p, '構成', '自動整列');
      await settled(p);
      expect((await p.locator('.node').nth(1).boundingBox())!.x).toBeCloseTo(before.x, 0);
    },
  },
  '構成/キャンバス全体の表示': {
    check: async (p) => {
      const first = await canvasState(p);
      const box = (await p.locator('.box').boundingBox())!;
      await p.mouse.move(box.x + box.width - 500, box.y + 200);
      await p.mouse.wheel(40, 40);
      expect((await canvasState(p)).transform).not.toBe(first.transform);
      await press(p, '構成', 'キャンバス全体の表示');
      await settled(p);
      expect(await canvasState(p)).toEqual(first);
    },
  },
  '接続操作/接続': {
    check: async (p) => {
      await press(p, '接続操作', '接続');
      await expect(p.locator('header .hub')).toHaveText('切断', { timeout: 30_000 });
    },
  },
  '接続操作/パッケージファイルの選択と書込': {
    check: async (p) => {
      await offWithReason(p, '接続操作', 'パッケージファイルの選択と書込');
      await p.getByRole('radio', { name: '書込' }).click();
      await connect(p);
      const chooser = p.waitForEvent('filechooser', { timeout: 5000 });
      await press(p, '接続操作', 'パッケージファイルの選択と書込');
      await chooser;
    },
  },
  '接続操作/前のバージョンへの復元': {
    check: async (p) => {
      await offWithReason(p, '接続操作', '前のバージョンへの復元');
      await p.getByRole('radio', { name: '書込' }).click();
      await settled(p);
      await connect(p);
      await offWithReason(p, '接続操作', '前のバージョンへの復元');
      // The sample's nodes start with one version each: nothing to go back to, and the item says so.
      await p.locator('.node').nth(1).dispatchEvent('click');
      await expect(await row(p, '接続操作', '前のバージョンへの復元')).toHaveAttribute('title', '復元可能な前のバージョンはありません。');
      await p.keyboard.press('Escape');
      await offWithReason(p, '接続操作', '前のバージョンへの復元');
    },
  },
  'DAGON/DAGONとの連携': {
    check: async (p) => {
      await press(p, 'DAGON', 'DAGONとの連携');
      await expect(p.locator('.why')).toBeVisible();
    },
  },
  'DAGON/CLIの接続とAPIキーの表示': {
    check: async (p) => {
      await press(p, 'DAGON', 'CLIの接続とAPIキーの表示');
      await expect(p.locator('.float.sheet')).toBeVisible();
    },
  },
  'DAGON/DAGONとの対話': {
    query: 'sim&agent=linked',
    check: async (p) => {
      await press(p, 'DAGON', 'DAGONとの対話');
      await expect(p.locator('.bubble')).toBeVisible();
    },
  },
  // Pressing it with a proposal is agent.test's and the bubble's own path; here: no proposal says so.
  'DAGON/DAGONの案の反映': { query: 'sim&agent=linked', check: (p) => offWithReason(p, 'DAGON', 'DAGONの案の反映') },
  'DAGON/DAGONとの連携解除': {
    query: 'sim&agent=linked',
    check: async (p) => {
      await expect(p.locator('.dagon')).toBeVisible();
      await press(p, 'DAGON', 'DAGONとの連携解除');
      await p.locator('dialog[open]').getByRole('button', { name: '連携解除' }).click();
      await expect(p.locator('.dagon')).toHaveCount(0);
    },
  },
  'トラブル対処/トラブル対処': {
    check: async (p) => {
      await press(p, 'トラブル対処', 'トラブル対処');
      await expect(p.locator('#help')).toBeVisible();
    },
  },
  'トラブル対処/キー操作': {
    check: async (p) => {
      await press(p, 'トラブル対処', 'キー操作');
      await expect(p.locator('#help h2')).toHaveText('キー操作');
    },
  },
  'トラブル対処/記録のコピー': {
    check: async (p) => {
      await p.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      await offWithReason(p, 'トラブル対処', '記録のコピー');
      await press(p, 'DAGON', 'DAGONとの連携');
      await expect(p.locator('.why')).toBeVisible();
      // Stopped by the lock table: it said why where it was pressed, and the menu stays.
      await p.keyboard.press('Escape');
      await press(p, 'トラブル対処', '記録のコピー');
      await expect(p.locator('.toast')).toBeVisible();
      expect((await p.evaluate(() => navigator.clipboard.readText())).length).toBeGreaterThan(0);
    },
  },
  'トラブル対処/このURLのコピー': {
    check: async (p) => {
      await p.context().grantPermissions(['clipboard-read', 'clipboard-write']);
      await press(p, 'トラブル対処', 'このURLのコピー');
      await expect(p.locator('.toast')).toBeVisible();
      expect(await p.evaluate(() => navigator.clipboard.readText())).toBe(await p.evaluate(() => location.href));
    },
  },
  'プロジェクト/リスタート': {
    check: async (p) => {
      await press(p, 'プロジェクト', 'リスタート');
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
    await page.getByRole('button', { name: 'メニューの表示' }).click();
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
