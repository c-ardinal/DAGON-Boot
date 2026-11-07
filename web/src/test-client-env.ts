// A test environment that runs in Node but has modules compiled as the browser gets them,
// so runes ($effect, flushSync) behave as on the page. No DOM: only logic is tested here.
import type { Environment } from 'vitest/runtime';

export default <Environment>{
  name: 'client-node',
  viteEnvironment: 'client',
  setup() {
    // The client modules name the global scope `self`, as browsers and workers do.
    const g = globalThis as { self?: unknown };
    const had = 'self' in g;
    g.self ??= globalThis;
    return {
      teardown() {
        if (!had) delete g.self;
      },
    };
  },
};
