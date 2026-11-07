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
    for (const sel of ['.frame > header', 'header .modes', 'header .actions', '.pill', '.focal', '.dagon', '.dagon .mark', '.bubble', '.drawer .tabs', '#band']) {
      const e = document.querySelector(sel);
      if (!e) continue;
      const r = e.getBoundingClientRect();
      if (r.width) out[sel] = [r.left, r.top, r.width, r.height].map((v) => Math.round(v)).join(',');
    }
    return out;
  });
}

/** What the sweep measures on the page as it stands; empty when nothing is wrong. */
const DISABLED_REASONS = `globalThis.disabledReasons = () => {
  const out = [];
  for (const e of document.querySelectorAll('[aria-disabled="true"]')) {
    if (!e.getClientRects().length || e.closest('[inert], .visually-hidden')) continue;
    const own = (e.getAttribute('aria-label') ?? e.textContent ?? '').trim();
    const title = (e.getAttribute('title') ?? '').trim();
    const tag = e.tagName.toLowerCase() + '.' + String(e.getAttribute('class') ?? '').split(' ')[0] + (e.getAttribute('aria-label') ? '[' + e.getAttribute('aria-label') + ']' : '');
    if (!title || title === own) out.push(tag + ' disabled with no reason');
  }
  return out;
}`;

export async function layoutFaults(page: Page): Promise<string[]> {
  await page.evaluate(DISABLED_REASONS);
  return page.evaluate(() => {
    const faults: string[] = [];
    const name = (e: Element) => `${e.tagName.toLowerCase()}.${String(e.getAttribute('class') ?? '').split(' ')[0]}${e.getAttribute('aria-label') ? `[${e.getAttribute('aria-label')}]` : ''}`;
    const [w, h] = [innerWidth, innerHeight];
    const root = document.documentElement;
    if (root.scrollWidth > w || root.scrollHeight > h) faults.push(`page scrolls ${root.scrollWidth}x${root.scrollHeight} in ${w}x${h}`);
    if (scrollX || scrollY) faults.push('page scrolled');

    // Everything fixed to the window stays inside it.
    for (const e of document.querySelectorAll('.frame > header, #band, .drawer, .tools, .float, .focal, .pill, .toast, .notice, .panel, #help, .split, .edit-area.table, .dagon, .bubble, dialog[open]')) {
      const r = e.getBoundingClientRect();
      if (r.width === 0) continue;
      if (r.left < -0.5 || r.top < -0.5 || r.right > w + 0.5 || r.bottom > h + 0.5) faults.push(`${name(e)} outside the window: ${Math.round(r.left)},${Math.round(r.top)},${Math.round(r.right)},${Math.round(r.bottom)}`);
    }
    // The overlay's peers and the bars do not overlap.
    const peers = [...document.querySelectorAll('.tools, .float, .focal, .pill, .toast, .notice, .edit-area.table, .drawer, #band, .dagon .mark, .bubble, header .modes, header .right > *')].filter((e) => e.getBoundingClientRect().width > 0);
    for (let i = 0; i < peers.length; i++) {
      for (let j = i + 1; j < peers.length; j++) {
        if (peers[i].contains(peers[j]) || peers[j].contains(peers[i])) continue;
        // The mark keeps its place when the drawer opens (only a drag moves it), standing over the drawer's foot.
        if (peers[i].matches('.dagon .mark') && peers[j].matches('.drawer') || peers[j].matches('.dagon .mark') && peers[i].matches('.drawer')) continue;
        const [a, b] = [peers[i].getBoundingClientRect(), peers[j].getBoundingClientRect()];
        if (Math.min(a.right, b.right) - Math.max(a.left, b.left) > 0.5 && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 0.5) faults.push(`${name(peers[i])} overlaps ${name(peers[j])}`);
      }
    }
    // Header controls inside the bar, on one row.
    const bar = document.querySelector('.frame > header')!.getBoundingClientRect();
    for (const e of document.querySelectorAll('.frame > header .modes, .frame > header .split > button, .frame > header .actions .set > button, .frame > header .button-icon')) {
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
    // Rendered spacing and type: what the browser lays out, not what the source says (a rem on a 14 px root would pass the source and fail here).
    // The type scale, and the 2560 theme's own t-mono (18) and t-title (44).
    const scale = [160, 28, 20, 16, 14, 12, 13, 18, 44];
    const onStep = (v: string) => { const n = parseFloat(v); return !n || Math.abs(n) % 4 === 0; };
    for (const e of document.querySelectorAll<HTMLElement>('.frame > header *, .sheet *, .float.parts *, .edit-area.table *, .bubble *, .drawer *, .pill *, .toast *, .focal *, .panel *, #help *, dialog[open] *, #band *')) {
      if (e.closest('.visually-hidden, svg, .box') || e.tagName === 'OPTION') continue;
      const c = getComputedStyle(e);
      for (const k of ['paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'marginTop', 'marginRight', 'marginBottom', 'marginLeft', 'rowGap', 'columnGap'] as const) {
        if (!onStep(c[k])) faults.push(`${name(e)} rendered ${k} ${c[k]}`);
      }
      if (e.textContent?.trim() && !e.children.length && !scale.includes(Math.round(parseFloat(c.fontSize) * 100) / 100)) faults.push(`${name(e)} rendered font-size ${c.fontSize}`);
    }
    // The table fits its window: no column is lost to a sideways scroll.
    for (const e of document.querySelectorAll<HTMLElement>('.table-view .scroll')) {
      if (e.scrollWidth > e.clientWidth + 1) faults.push(`table scrolls sideways ${e.scrollWidth} in ${e.clientWidth}`);
    }
    faults.push(...disabledReasons());
    // A label never breaks: a button's words stay on one line.
    for (const b of document.querySelectorAll<HTMLElement>('.more, #band button, .toast button')) {
      if (b.textContent?.trim() && b.offsetHeight > 0 && getComputedStyle(b).whiteSpace !== 'nowrap') faults.push(`${name(b)} can wrap: ${b.textContent.trim().slice(0, 16)}`);
    }
    return faults;
  });
}

/** Every visible disabled control carries a reason on hover: a title that is not merely its name. */
export async function disabledFaults(page: Page): Promise<string[]> {
  await page.evaluate(DISABLED_REASONS);
  return page.evaluate(() => disabledReasons());
}

declare global {
  function disabledReasons(): string[];
}

/**
 * Every focusable element, focused in turn as the keyboard would reach it: its focus ring (outline or
 * shadow) lies inside every ancestor that clips (overflow, contain: paint, clip-path) and inside the window.
 * Scroll positions the focusing moved are put back.
 */
export async function focusFaults(page: Page): Promise<string[]> {
  // One Tab first: focus moved by script then shows the ring (:focus-visible) as a keyboard user sees it.
  await page.keyboard.press('Tab');
  return page.evaluate(() => {
    const faults: string[] = [];
    const name = (e: Element) => `${e.tagName.toLowerCase()}.${String(e.getAttribute('class') ?? '').split(' ')[0]}${e.getAttribute('aria-label') ? `[${e.getAttribute('aria-label')}]` : ''}`;
    const clips = (c: CSSStyleDeclaration) => c.overflowX !== 'visible' || c.overflowY !== 'visible' || c.contain.includes('paint') || c.clipPath !== 'none';
    // The outline past the border, and any outer (not inset) shadow: x/y offset + blur + spread.
    const ringExtent = (c: CSSStyleDeclaration) => {
      let r = c.outlineStyle !== 'none' ? parseFloat(c.outlineWidth) + parseFloat(c.outlineOffset) : 0;
      for (const part of c.boxShadow.split(/,(?![^(]*\))/)) {
        if (part.includes('inset')) continue;
        const m = /(-?[\d.]+)px (-?[\d.]+)px (-?[\d.]+)px (-?[\d.]+)px/.exec(part);
        if (m) r = Math.max(r, Math.max(Math.abs(+m[1]), Math.abs(+m[2])) + +m[3] + +m[4]);
      }
      return r;
    };
    const had = document.activeElement as HTMLElement | null;
    const scrolled = new Map<Element, [number, number]>();
    const all = document.querySelectorAll<HTMLElement>('a[href], button, input, select, textarea, summary, [tabindex]:not([tabindex="-1"]), [contenteditable]');
    for (const el of all) {
      if (el.closest('.visually-hidden, [inert], [aria-hidden="true"]') || (el as HTMLButtonElement).disabled || !el.getClientRects().length) continue;
      el.focus();
      if (document.activeElement !== el || !el.matches(':focus-visible')) continue;
      // Nothing sticky in its scroll window lies over it (WCAG 2.4.11): the field scrolled in under a head is unseen.
      const port = (() => { for (let a = el.parentElement; a; a = a.parentElement) { const o = getComputedStyle(a).overflowY; if (o === 'auto' || o === 'scroll') return a; } return null; })();
      // An element riding a sticky strip itself (the sheet's ×) stands over the head, not under it.
      const rides = (() => { for (let a = el.parentElement; a && a !== port; a = a.parentElement) if (getComputedStyle(a).position === 'sticky') return true; return false; })();
      if (port && !rides) {
        const r0 = el.getBoundingClientRect();
        for (const st of port.querySelectorAll<HTMLElement>('*')) {
          if (getComputedStyle(st).position !== 'sticky' || st.contains(el)) continue;
          const b = st.getBoundingClientRect();
          if (Math.min(r0.right, b.right) - Math.max(r0.left, b.left) > 0.5 && Math.min(r0.bottom, b.bottom) - Math.max(r0.top, b.top) > 0.5) faults.push(`${name(el)} hidden under ${name(st)}`);
        }
      }
      const ring = ringExtent(getComputedStyle(el));
      if (ring <= 0) continue;
      const r = el.getBoundingClientRect();
      const box = { left: r.left - ring, top: r.top - ring, right: r.right + ring, bottom: r.bottom + ring };
      const within = (b: { left: number; top: number; right: number; bottom: number }, c: { left: number; top: number; right: number; bottom: number }) => b.left >= c.left - 0.5 && b.top >= c.top - 0.5 && b.right <= c.right + 0.5 && b.bottom <= c.bottom + 0.5;
      // Only an element that is itself in view: one panned or scrolled out of a window is the window's matter, not the ring's.
      const cut = (c: { left: number; top: number; right: number; bottom: number }) => within(r, c) && !within(box, c);
      if (cut({ left: 0, top: 0, right: innerWidth, bottom: innerHeight })) faults.push(`${name(el)} focus ring outside the window`);
      for (let a = el.parentElement; a; a = a.parentElement) {
        if (!clips(getComputedStyle(a))) continue;
        if (!scrolled.has(a)) scrolled.set(a, [a.scrollLeft, a.scrollTop]);
        const b = a.getBoundingClientRect();
        if (cut({ left: b.left + a.clientLeft, top: b.top + a.clientTop, right: b.left + a.clientLeft + a.clientWidth, bottom: b.top + a.clientTop + a.clientHeight })) faults.push(`${name(el)} focus ring clipped by ${name(a)}`);
      }
    }
    for (const [a, [l, t]] of scrolled) {
      a.scrollLeft = l;
      a.scrollTop = t;
    }
    had?.focus();
    return [...new Set(faults)];
  });
}
