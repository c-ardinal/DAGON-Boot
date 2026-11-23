// A control the lock table stops is dimmed (base.css) and refuses its press (App's capture
// handlers). This also says so to assistive technology and on hover (WCAG 4.1.2): every
// interactive part of a stopped [data-control] carries aria-disabled="true" and the reason as
// its title while the lock holds, and gets its own values back when it ends. The components
// keep writing their own values meanwhile; what they write is kept as the value to give back.

const INTERACTIVE = 'button, input, select, textarea, a[href], [role="button"], [role="tab"], [role="checkbox"], [role="radio"], [tabindex]:not([tabindex="-1"])';
/** Each attribute the lock stands over, and where the element's own value waits ('\u0000' when it had none). */
const KEPT = { 'aria-disabled': 'lockOwnDisabled', title: 'lockOwnTitle' } as const;
type Attr = keyof typeof KEPT;
const NONE = '\u0000';

/** `reason(control)`: why the control is stopped now, or null when it can be used. */
export function lockAria(root: HTMLElement, reason: (control: string) => string | null): { sync: () => void; stop: () => void } {
  /** Writes of ours the observer has still to report, per element and attribute: those are not the component's. */
  const ours = new WeakMap<Element, Partial<Record<Attr, number>>>();
  const write = (el: HTMLElement, a: Attr, value: string | null) => {
    if (el.getAttribute(a) === value) return;
    const n = ours.get(el) ?? {};
    n[a] = (n[a] ?? 0) + 1;
    ours.set(el, n);
    if (value === null) el.removeAttribute(a);
    else el.setAttribute(a, value);
  };
  const stand = (el: HTMLElement, a: Attr, value: string) => {
    if (el.dataset[KEPT[a]] === undefined) el.dataset[KEPT[a]] = el.getAttribute(a) ?? NONE;
    write(el, a, value);
  };
  const giveBack = (el: HTMLElement, a: Attr) => {
    const own = el.dataset[KEPT[a]];
    if (own === undefined) return;
    delete el.dataset[KEPT[a]];
    write(el, a, own === NONE ? null : own);
  };
  /** Elements under a lock now, with its reason. */
  let stopped = new Map<HTMLElement, string>();

  function sync() {
    // What the components wrote since the last report counts first: a value written in the same flush as the lock lifted is theirs to keep.
    take(observer.takeRecords());
    const now = new Map<HTMLElement, string>();
    for (const c of root.querySelectorAll<HTMLElement>('[data-control]')) {
      const why = reason(c.dataset.control ?? '');
      if (why === null) continue;
      for (const el of c.matches(INTERACTIVE) ? [c] : c.querySelectorAll<HTMLElement>(INTERACTIVE)) now.set(el, why);
    }
    for (const [el, why] of now) {
      stand(el, 'aria-disabled', 'true');
      stand(el, 'title', why);
    }
    for (const el of stopped.keys()) {
      if (now.has(el) || !el.isConnected) continue;
      giveBack(el, 'aria-disabled');
      giveBack(el, 'title');
    }
    stopped = now;
  }

  /** Reads the observer's records; true when the page changed in a way the locks must look at again. */
  function take(records: MutationRecord[]): boolean {
    let changed = false;
    for (const r of records) {
      // Parts come and go, and a control can turn into another (the write button is 書込 or 更新 by state).
      if (r.type === 'childList' || r.attributeName === 'data-control') {
        changed = true;
        continue;
      }
      const el = r.target as HTMLElement;
      const a = r.attributeName as Attr;
      const n = ours.get(el);
      if (n?.[a]) {
        n[a]--;
        continue;
      }
      // The component changed its own value under the lock: keep it for later, and stand over it again.
      if (el.dataset[KEPT[a]] !== undefined) {
        el.dataset[KEPT[a]] = el.getAttribute(a) ?? NONE;
        changed = true;
      }
    }
    return changed;
  }
  const observer = new MutationObserver((records) => {
    if (take(records)) sync();
  });
  observer.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: [...Object.keys(KEPT), 'data-control'] });
  sync();
  return { sync, stop: () => observer.disconnect() };
}
