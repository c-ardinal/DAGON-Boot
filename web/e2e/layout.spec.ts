import { expect, test, type Page } from '@playwright/test';
import { boxes, canvasState, layoutFaults, openSample, settled } from './console.ts';

// The viewport matrix (ulthar config.json `viewport_matrix`) from 1024 wide up, the page's own 2560 theme edge (2559/2560) added.
const WIDTHS = [1024, 1099, 1100, 1280, 1366, 1440, 1920, 2020, 2400, 2559, 2560];
const HEIGHTS = [600, 700, 800, 1080, 1347, 1440];

/** The windows swept: each width at its low and tall ends, every height at a common width, and the matrix's own presets. */
const SIZES = [...WIDTHS.flatMap((w) => [[w, 700], [w, 1080]]), ...HEIGHTS.map((h) => [1920, h]), [1366, 768], [2560, 1440], [1024, 768]] as const;

/** The view toggle: in the header, or at a narrow bar in the menu's 構成 group. */
async function view(p: Page, radio: string, item: string) {
  if (await p.locator('header .seg').count()) return p.getByRole('radio', { name: radio, exact: true }).click();
  await p.getByRole('button', { name: 'メニューを開く' }).click();
  await p.locator('.panel').first().getByRole('button', { name: '構成', exact: true }).click();
  await p.locator('.panel').last().locator(`button:has(span:text-is("${item}"))`).click();
}

/** Each state puts the page into it and takes it out again. */
const STATES: [string, (p: Page) => Promise<void>, (p: Page) => Promise<void>][] = [
  ['準備', async () => undefined, async () => undefined],
  ['シート', (p) => p.locator('.node').first().dispatchEvent('click'), (p) => p.locator('.node').first().dispatchEvent('click')],
  ['引き出し', (p) => p.getByRole('tab', { name: '記録' }).click(), (p) => p.getByRole('tab', { name: '記録' }).click()],
  ['シートと引き出し', async (p) => { await p.locator('.node').first().dispatchEvent('click'); await p.getByRole('tab', { name: '記録' }).click(); }, async (p) => { await p.locator('.node').first().dispatchEvent('click'); await p.getByRole('tab', { name: '記録' }).click(); }],
  ['メニュー', async (p) => { await p.getByRole('button', { name: 'メニューを開く' }).click(); await p.getByRole('button', { name: 'つなぐ' }).click(); }, (p) => p.keyboard.press('Escape')],
  ['困ったとき', async (p) => { await p.getByRole('button', { name: 'メニューを開く' }).click(); await p.getByRole('button', { name: '困ったとき' }).click(); await p.getByRole('button', { name: 'キー操作' }).click(); }, (p) => p.keyboard.press('Escape')],
  ['表', (p) => view(p, '表', '表で見る'), (p) => view(p, 'グラフ', 'グラフで見る')],
  ['トースト', async (p) => { await p.getByRole('button', { name: 'メニューを開く' }).click(); await p.getByRole('button', { name: '困ったとき', exact: true }).click(); await p.locator('.panel').last().locator('button:has(span:text-is("この URL をコピーする"))').click(); await p.locator('.toast').waitFor(); }, async () => undefined],
  ['モーダル', async (p) => { await p.getByRole('button', { name: 'メニューを開く' }).click(); await p.locator('.panel').first().locator('button:has(span:text-is("最初からやり直す"))').click(); await p.locator('dialog[open]').waitFor(); }, async (p) => { await p.keyboard.press('Escape'); await p.locator('dialog[open]').waitFor({ state: 'detached' }); }],
];

test('sweep: no overflow, overlap or clipped text at any window, in 準備 (sheet, drawer, menu, help, table, toast, modal) and 書込 (versions)', async ({ page, context }) => {
  test.setTimeout(1_500_000);
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const faults: string[] = [];
  for (const [w, h] of SIZES) {
    await page.setViewportSize({ width: w, height: h });
    // The bar re-lays out once for the new width (its edit tools come or go); the rest is what stands after that.
    await page.waitForTimeout(260);
    const rest = await boxes(page);
    for (const [name, into, out] of STATES) {
      await into(page);
      await page.waitForTimeout(260);
      for (const f of await layoutFaults(page)) faults.push(`${w}x${h} ${name}: ${f}`);
      // Nothing that stands on the page vibrates: its box is the same a few frames later.
      const a = await boxes(page);
      await page.waitForTimeout(120);
      const b = await boxes(page);
      for (const k of Object.keys(a)) if (a[k] !== b[k]) faults.push(`${w}x${h} ${name}: ${k} moved ${a[k]} -> ${b[k]}`);
      // A sheet, menu, help or toast opening moves none of the standing parts (the DAGON mark, the zoom, the focal number, the header). The open drawer is the one thing they give way to, by structure, so it never covers them.
      // The open drawer is what the zoom and the focal number give way to, by structure; everything else moves for nothing.
      const still = name.includes('引き出し') ? ['.frame > header', 'header .modes', 'header .actions', '.dagon', '.dagon .mark'] : Object.keys(rest);
      for (const k of still) if (a[k] !== undefined && rest[k] !== undefined && a[k] !== rest[k]) faults.push(`${w}x${h} ${name}: ${k} moved for it ${rest[k]} -> ${a[k]}`);
      await out(page);
    }
  }
  // The mode toggle and the mark stand still across modes at every width.
  for (const [w, h] of SIZES) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(260);
    const prep = await boxes(page);
    await page.getByRole('radio', { name: '書込' }).click();
    await page.waitForTimeout(300);
    const write = await boxes(page);
    for (const k of ['header .modes', '.frame > header', '.dagon .mark']) if (prep[k] !== undefined && prep[k] !== write[k]) faults.push(`${w}x${h} 準備→書込: ${k} moved ${prep[k]} -> ${write[k]}`);
    await page.getByRole('radio', { name: '準備' }).click();
    await page.waitForTimeout(300);
  }
  await page.setViewportSize({ width: 1920, height: 1080 });
  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  await page.locator('header').getByRole('button', { name: '接続する' }).click();
  await page.locator('.st-alive').first().waitFor({ timeout: 30_000 });
  await page.getByRole('button', { name: 'バージョンを選んで書き込む' }).waitFor();
  await expect(page.locator('.split .caret')).toHaveAttribute('aria-disabled', 'false', { timeout: 15_000 });
  for (const [w, h] of SIZES) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(120);
    for (const f of await layoutFaults(page)) faults.push(`${w}x${h} 書込: ${f}`);
    await page.getByRole('button', { name: 'バージョンを選んで書き込む' }).click();
    await page.waitForTimeout(200);
    for (const f of await layoutFaults(page)) faults.push(`${w}x${h} 書込 バージョン: ${f}`);
    await page.keyboard.press('Escape');
  }
  expect(faults).toEqual([]);
});

test('scale and pan do not change when a sheet, drawer, menu or toast appears', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const before = await canvasState(page);
  for (const [name, into, out] of STATES.filter(([n]) => n !== '表' && n !== 'トースト' && n !== 'モーダル')) {
    await into(page);
    await page.waitForTimeout(300);
    expect(await canvasState(page), name).toEqual(before);
    await out(page);
    await page.waitForTimeout(300);
    expect(await canvasState(page), `${name} gone`).toEqual(before);
  }
});

test('closing the parts window and changing mode leave the view alone; the view survives the table', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const before = await canvasState(page);
  await page.getByRole('button', { name: '部品を閉じる' }).click();
  await settled(page);
  expect(await canvasState(page)).toEqual(before);
  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  expect(await canvasState(page)).toEqual(before);
  await page.getByRole('radio', { name: '準備' }).click();
  await settled(page);
  await page.getByRole('radio', { name: '表', exact: true }).click();
  await page.getByRole('radio', { name: 'グラフ' }).click();
  await settled(page);
  expect(await canvasState(page)).toEqual(before);
});

test('the window is the only thing that sizes the box, and the page never scrolls', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openSample(page);
  const { transform } = await canvasState(page);
  await page.setViewportSize({ width: 1100, height: 700 });
  await settled(page);
  expect((await canvasState(page)).transform).toBe(transform);
  const scrolled = await page.evaluate(() => {
    scrollTo(0, 500);
    document.querySelector('.box')!.scrollTop = 200;
    return [scrollY, document.querySelector('.box')!.scrollTop];
  });
  expect(scrolled).toEqual([0, 0]);
});

test('pan, wheel zoom and the explicit fit are what move the view', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const start = await canvasState(page);
  const box = await page.locator('.box').boundingBox();
  await page.mouse.move(box!.x + box!.width - 500, box!.y + 40);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width - 440, box!.y + 90);
  await page.mouse.up();
  const panned = await canvasState(page);
  expect(panned.transform).not.toBe(start.transform);
  expect(panned.transform).toMatch(/scale\(/);
  await page.mouse.wheel(0, 40);
  expect((await canvasState(page)).transform).not.toBe(panned.transform);
  await page.getByRole('button', { name: 'キャンバス全体を表示する' }).click();
  await settled(page);
  expect((await canvasState(page)).transform).toBe(start.transform);
});

test('version choice and write are one split button; no separate dropdown', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  expect(await page.locator('header select').count()).toBe(0);
  const [main, caret] = [await page.locator('.split .main').boundingBox(), await page.locator('.split .caret').boundingBox()];
  expect(caret!.x - (main!.x + main!.width)).toBeLessThanOrEqual(1);
  expect(caret!.y).toBe(main!.y);
});

test('undo, redo, the view toggle and the connection are in the header; the edit tools in 準備 only', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const bar = (await page.locator('.frame > header').boundingBox())!;
  const inBar = (b: { y: number; height: number }) => b.y >= bar.y && b.y + b.height <= bar.y + bar.height;
  const undo = (await page.getByRole('button', { name: '元に戻す' }).boundingBox())!;
  const redo = (await page.getByRole('button', { name: 'やり直す' }).boundingBox())!;
  const seg = (await page.getByRole('radio', { name: '表', exact: true }).boundingBox())!;
  const hub = (await page.locator('header .hub').boundingBox())!;
  for (const b of [undo, redo, seg, hub]) expect(inBar(b)).toBe(true);
  expect(redo.x).toBeGreaterThan(undo.x);
  expect(seg.x).toBeGreaterThan(redo.x);
  for (const name of ['元に戻す', 'やり直す']) expect((await page.getByRole('button', { name }).innerText()).trim()).toBe('');
  await page.getByRole('radio', { name: '書込' }).click();
  await settled(page);
  expect(await page.getByRole('button', { name: '元に戻す' }).count()).toBe(0);
  expect(await page.locator('header .seg').count()).toBe(0);
  await expect(page.locator('header').getByRole('button', { name: '接続する' })).toBeVisible();
  // At a narrow bar the edit tools give way to ☰, where they always are.
  await page.setViewportSize({ width: 1024, height: 768 });
  await page.getByRole('radio', { name: '準備' }).click();
  await settled(page);
  expect(await page.locator('header .edit').count()).toBe(0);
  await page.getByRole('button', { name: 'メニューを開く' }).click();
  await page.getByRole('button', { name: '構成', exact: true }).click();
  await expect(page.locator('.panel').last().locator('button:has(span:text-is("元に戻す"))')).toBeVisible();
  await expect(page.locator('.panel').last().locator('button:has(span:text-is("表で見る"))')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 1920, height: 1080 });
  await settled(page);
  await page.getByRole('button', { name: 'メニューを開く' }).click();
  await page.getByRole('button', { name: '構成', exact: true }).click();
  await expect(page.locator('.panel').last().locator('button:has(span:text-is("表で見る"))')).toHaveCount(0);
});

test('a sheet closes by its ×, by Escape and by the canvas background; deleting asks first', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  await page.locator('.node').first().dispatchEvent('click');
  await expect(page.locator('.sheet')).toBeVisible();
  await page.getByRole('button', { name: 'シートを閉じる' }).click();
  await expect(page.locator('.sheet')).toHaveCount(0);
  await page.locator('.node').first().dispatchEvent('click');
  await page.keyboard.press('Escape');
  await expect(page.locator('.sheet')).toHaveCount(0);
  await page.locator('.node').first().dispatchEvent('click');
  const box = (await page.locator('.box').boundingBox())!;
  await page.mouse.click(box.x + 40, box.y + box.height - 200);
  await expect(page.locator('.sheet')).toHaveCount(0);
  const nodes = await page.locator('.node').count();
  await page.locator('.node').nth(1).dispatchEvent('click');
  await page.getByRole('button', { name: 'このノードを消す' }).click();
  await expect(page.locator('dialog[open]')).toBeVisible();
  expect(await page.locator('.node').count()).toBe(nodes);
  await page.locator('dialog[open]').getByRole('button', { name: 'やめる' }).click();
  expect(await page.locator('.node').count()).toBe(nodes);
  await page.getByRole('button', { name: 'このノードを消す' }).click();
  await page.locator('dialog[open]').getByRole('button', { name: '消す' }).click();
  await expect(page.locator('.node')).toHaveCount(nodes - 1);
});

test('the zoom readout stays inside the pill, every part on its axis, at the bottom-right', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const pill = (await page.locator('.pill').boundingBox())!;
  const axis = pill.x + pill.width / 2;
  for (const part of await page.locator('.pill > *').all()) {
    const b = (await part.boundingBox())!;
    expect(Math.abs(b.x + b.width / 2 - axis), 'on the axis').toBeLessThan(1);
    expect(b.y).toBeGreaterThanOrEqual(pill.y);
    expect(b.y + b.height).toBeLessThanOrEqual(pill.y + pill.height);
  }
  // The readout sits on the straight part: clear of both round ends.
  const pct = (await page.locator('.pill .pct').boundingBox())!;
  expect(pct.y - pill.y).toBeGreaterThanOrEqual(pill.width / 2);
  expect(pill.y + pill.height - (pct.y + pct.height)).toBeGreaterThanOrEqual(pill.width / 2);
  const stage = (await page.locator('.stage').boundingBox())!;
  expect(pill.x + pill.width).toBeGreaterThan(stage.x + stage.width - 40);
  const dock = (await page.locator('.dock').boundingBox())!;
  expect(pill.y + pill.height).toBeLessThanOrEqual(dock.y);
});

test('the menu is a popover at ☰ that moves nothing; groups, then their items', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const stage = await page.locator('.stage').boundingBox();
  const button = (await page.getByRole('button', { name: 'メニューを開く' }).boundingBox())!;
  await page.getByRole('button', { name: 'メニューを開く' }).click();
  await page.waitForTimeout(300);
  expect(await page.locator('.stage').boundingBox()).toEqual(stage);
  const panel = (await page.locator('.panel').first().boundingBox())!;
  expect(panel.y).toBeGreaterThanOrEqual(button.y + button.height - 1);
  expect(panel.x + panel.width).toBeCloseTo(button.x + button.width, 0);
  for (const g of ['プロジェクト', '構成', 'つなぐ', 'DAGON', '困ったとき']) await expect(page.getByRole('button', { name: g, exact: true })).toBeVisible();
  await page.getByRole('button', { name: '構成', exact: true }).click();
  await expect(page.getByRole('button', { name: 'キャンバス全体を表示する' }).last()).toBeVisible();
  await page.mouse.click(600, 600);
  await expect(page.locator('#menu')).toHaveCount(0);
});

test('the band says one sentence in readable type', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const s = page.locator('#band .sentence');
  await expect(s).toBeVisible();
  expect((await s.innerText()).trim().length).toBeGreaterThan(0);
  const { size, colour, paper } = await s.evaluate((e) => {
    const c = getComputedStyle(e);
    const probe = document.createElement('i');
    probe.style.color = 'var(--c-paper)';
    document.body.append(probe);
    const paper = getComputedStyle(probe).color;
    probe.remove();
    return { size: parseFloat(c.fontSize), colour: c.color, paper };
  });
  expect(size).toBeGreaterThanOrEqual(14);
  expect(colour).toBe(paper);
});

test('the longest sentence wraps and the band grows over the canvas, moving nothing and covering nothing', async ({ page }) => {
  for (const [width, height] of [[1024, 600], [1920, 1080]]) {
    await page.setViewportSize({ width, height });
    await openSample(page);
    const before = await canvasState(page);
    await page.locator('#band .sentence').evaluate((e) => {
      e.textContent = '終わるまで電源と USB を抜かず、このページを開いたままにしてください。途中で止まっても、ノードは前のバージョンのまま動きます。'.repeat(3);
    });
    await page.waitForTimeout(120);
    expect(await canvasState(page)).toEqual(before);
    expect(await layoutFaults(page)).toEqual([]);
  }
});

test('dragging a card pulls its cable behind it, ends on the ports, and the view stays', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const view = await canvasState(page);
  const card = page.locator('.node').nth(1);
  const start = (await card.boundingBox())!;
  const wires = page.locator('.bundle .wire');
  await page.mouse.move(start.x + 60, start.y + 12);
  await page.mouse.down();
  for (let i = 1; i <= 8; i++) await page.mouse.move(start.x + 60 + 30 * i, start.y + 12 + 12 * i);
  const during = await wires.first().getAttribute('d');
  await page.mouse.up();
  await page.waitForTimeout(1600);
  const rest = await wires.first().getAttribute('d');
  await page.waitForTimeout(300);
  expect(await wires.first().getAttribute('d'), 'at rest it stops changing').toBe(rest);
  expect(rest).not.toBe(during);
  const moved = (await card.boundingBox())!;
  expect(moved.x).toBeGreaterThan(start.x + 100);
  expect(await canvasState(page)).toEqual(view);
  // The cable ends at the card's top, at its port.
  const port = await page.evaluate(() => {
    const c = document.querySelectorAll('.node')[1].getBoundingClientRect();
    const paths = [...document.querySelectorAll<SVGPathElement>('.bundle .wire')].map((p) => {
      const m = p.getScreenCTM()!;
      const e = p.getPointAtLength(p.getTotalLength());
      return { x: m.a * e.x + m.e, y: m.d * e.y + m.f };
    });
    return { paths, top: c.top, mid: c.left + c.width / 2 };
  });
  expect(port.paths.some((p) => Math.abs(p.y - port.top) < 2 && Math.abs(p.x - port.mid) < 24)).toBe(true);
});

test('reduced motion shows the rest shape at once', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  const card = page.locator('.node').nth(1);
  const start = (await card.boundingBox())!;
  await page.mouse.move(start.x + 60, start.y + 12);
  await page.mouse.down();
  await page.mouse.move(start.x + 200, start.y + 80);
  await page.waitForTimeout(32);
  const held = await page.locator('.bundle .wire').first().getAttribute('d');
  await page.waitForTimeout(300);
  expect(await page.locator('.bundle .wire').first().getAttribute('d')).toBe(held);
  await page.mouse.up();
});

test('no animation frames are asked for once nothing moves', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  await page.waitForTimeout(1500);
  const asked = await page.evaluate(async () => {
    let n = 0;
    const real = requestAnimationFrame;
    window.requestAnimationFrame = (cb) => (n++, real(cb));
    await new Promise((r) => setTimeout(r, 500));
    window.requestAnimationFrame = real;
    return n;
  });
  expect(asked).toBe(0);
});

test('the mode toggle and zoom are pills, the header keeps its clearance', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  for (const sel of ['header .modes', '.pill']) {
    const r = await page.locator(sel).evaluate((e) => parseFloat(getComputedStyle(e).borderTopLeftRadius));
    expect(r, sel).toBeGreaterThan(100);
  }
  const bar = (await page.locator('header').boundingBox())!;
  const modes = (await page.locator('header .modes').boundingBox())!;
  expect(modes.y - bar.y).toBeGreaterThanOrEqual(8);
  expect(bar.y + bar.height - (modes.y + modes.height)).toBeGreaterThanOrEqual(8);
});

test('a toast stands left of the zoom pill, inside the canvas', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openSample(page);
  await page.getByRole('button', { name: 'メニューを開く' }).click();
  await page.getByRole('button', { name: '困ったとき', exact: true }).click();
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.getByRole('button', { name: 'この URL をコピーする' }).click();
  const toast = (await page.locator('.toast').boundingBox())!;
  const pill = (await page.locator('.pill').boundingBox())!;
  expect(toast.x + toast.width).toBeLessThanOrEqual(pill.x);
});
