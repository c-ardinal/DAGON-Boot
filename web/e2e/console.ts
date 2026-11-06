import type { Page } from '@playwright/test';

/** Opens the development simulator's Console on the sample 構成, in 準備. */
export async function openSample(page: Page, query = 'sim'): Promise<void> {
  await page.goto(`/?${query}`);
  await page.getByRole('button', { name: 'プロジェクトを作る' }).click();
  await page.getByRole('radio', { name: '見本から作る' }).click();
  await page.getByRole('button', { name: '名前と場所を決める' }).click();
  await page.locator('.node').first().waitFor();
  await settled(page);
}

/** Waits until entrance motion and the first view are over. */
export async function settled(page: Page): Promise<void> {
  await page.waitForTimeout(700);
}

/** The canvas's zoom and pan as drawn, and its box. */
export async function canvasState(page: Page): Promise<{ transform: string; box: number[] }> {
  return page.evaluate(() => {
    const c = document.querySelector<HTMLElement>('.canvas')!;
    const b = document.querySelector('.box')!.getBoundingClientRect();
    return { transform: c.style.transform, box: [b.left, b.top, b.width, b.height] };
  });
}

/** The boxes of the parts that stand on the page (never move for a sheet, drawer, menu or toast), by name. */
export async function boxes(page: Page): Promise<Record<string, string>> {
  return page.evaluate(() => {
    const out: Record<string, string> = {};
    for (const sel of ['.frame > header', 'header .modes', 'header .actions', '.pill', '.focal', '.dagon', '.drawer .tabs', '#band']) {
      const e = document.querySelector(sel);
      if (!e) continue;
      const r = e.getBoundingClientRect();
      if (r.width) out[sel] = [r.left, r.top, r.width, r.height].map((v) => Math.round(v)).join(',');
    }
    return out;
  });
}

/** What the sweep measures on the page as it stands; empty when nothing is wrong. */
export async function layoutFaults(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const faults: string[] = [];
    const name = (e: Element) => `${e.tagName.toLowerCase()}.${String(e.getAttribute('class') ?? '').split(' ')[0]}${e.getAttribute('aria-label') ? `[${e.getAttribute('aria-label')}]` : ''}`;
    const [w, h] = [innerWidth, innerHeight];
    const root = document.documentElement;
    if (root.scrollWidth > w || root.scrollHeight > h) faults.push(`page scrolls ${root.scrollWidth}x${root.scrollHeight} in ${w}x${h}`);
    if (scrollX || scrollY) faults.push('page scrolled');

    // Everything fixed to the window stays inside it.
    for (const e of document.querySelectorAll('.frame > header, #band, .drawer, .tools, .float, .focal, .pill, .toast, .notice, .panel, #help, .split, .edit-area.table, dialog[open]')) {
      const r = e.getBoundingClientRect();
      if (r.width === 0) continue;
      if (r.left < -0.5 || r.top < -0.5 || r.right > w + 0.5 || r.bottom > h + 0.5) faults.push(`${name(e)} outside the window: ${Math.round(r.left)},${Math.round(r.top)},${Math.round(r.right)},${Math.round(r.bottom)}`);
    }
    // The overlay's peers and the bars do not overlap.
    const peers = [...document.querySelectorAll('.tools, .float, .focal, .pill, .toast, .notice, .edit-area.table, .drawer, #band')].filter((e) => e.getBoundingClientRect().width > 0);
    for (let i = 0; i < peers.length; i++) {
      for (let j = i + 1; j < peers.length; j++) {
        if (peers[i].contains(peers[j]) || peers[j].contains(peers[i])) continue;
        const [a, b] = [peers[i].getBoundingClientRect(), peers[j].getBoundingClientRect()];
        if (Math.min(a.right, b.right) - Math.max(a.left, b.left) > 0.5 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 0.5) faults.push(`${name(peers[i])} overlaps ${name(peers[j])}`);
      }
    }
    // Header controls inside the bar, on one row.
    const bar = document.querySelector('.frame > header')!.getBoundingClientRect();
    for (const e of document.querySelectorAll('.frame > header .modes, .frame > header .split > button, .frame > header .actions > button, .frame > header .button-icon')) {
      const r = e.getBoundingClientRect();
      if (r.top < bar.top - 0.5 || r.bottom > bar.bottom + 0.5) faults.push(`${name(e)} outside the header bar`);
    }
    const [left, right] = [document.querySelector('.frame > header .left')!.getBoundingClientRect(), document.querySelector('.frame > header .right')!.getBoundingClientRect()];
    if (left.right > right.left + 0.5) faults.push('header sides overlap');
    // Text with no way to read it: a truncated name carries its full text in a title.
    for (const e of document.querySelectorAll('header *, #band *, .drawer [role="tab"], .tools *, .pill *, .toast *, .notice *, .focal *, .panel *, .list *, dialog[open] *')) {
      const el = e as HTMLElement;
      if (el.closest('.visually-hidden') || !el.textContent?.trim() || el.children.length) continue;
      if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflowX !== 'visible' && !el.closest('[title]')) faults.push(`${name(el)} clipped: ${el.textContent.trim().slice(0, 24)}`);
      const c = el.getBoundingClientRect();
      const box = el.closest('button, .pill, .toast, .notice')?.getBoundingClientRect();
      if (box && c.width && (c.left < box.left - 0.5 || c.right > box.right + 0.5 || c.top < box.top - 0.5 || c.bottom > box.bottom + 0.5)) faults.push(`${name(el)} over its frame: ${el.textContent.trim().slice(0, 24)}`);
    }
    // A label never breaks: a button's words stay on one line.
    for (const b of document.querySelectorAll<HTMLElement>('.more, #band button, .toast button')) {
      if (b.textContent?.trim() && b.offsetHeight > 0 && getComputedStyle(b).whiteSpace !== 'nowrap') faults.push(`${name(b)} can wrap: ${b.textContent.trim().slice(0, 16)}`);
    }
    return faults;
  });
}
