import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';

// Every place in the source that can open linking, talking, keys or the CLI sheet.
// e2e/demo.spec.ts presses each of them in a demo; a new one fails here until it is closed there and listed.
const SOURCES = import.meta.glob('./**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const OPENER = /\bopenCli\b|\bopenLink\(|<LinkDialog|<CliPanel \{cli\} \{agent\}|cliPanel = '|dagonOpen\+\+|data-control="(?:ask|link|apply)"|control: '(?:ask|link|apply)'|\{ id: 'talk'|oncli=|AGENT\.menu\w+|<Dagon /;

const LISTED: Record<string, number> = {
  './App.svelte': 17,
  './screens/Dagon.svelte': 3,
  './screens/StopNotice.svelte': 1,
};

describe('the demo closes every opener', () => {
  test('the openers in the code are the ones the demo test presses', () => {
    const found = Object.fromEntries(
      Object.entries(SOURCES)
        .map(([file, text]) => [file, text.split('\n').filter((l) => OPENER.test(l)).length] as const)
        .filter(([, n]) => n > 0),
    );
    expect(found).toEqual(LISTED);
  });

  test('one condition guards them in the page', () => {
    const app = SOURCES['./App.svelte'];
    expect(app).toMatch(/const agentLinked = \$derived\(!simulated && agent\.linked\)/);
    expect(app.match(/\bagent\.linked\b/g)).toHaveLength(1);
    expect(app).toMatch(/async function openLink[^\n]*\n\s*if \(simulated\) return;/);
    expect(app).toMatch(/function openCli\(\) \{\n\s*if \(simulated\) return;/);
    expect(app).toMatch(/oncli=\{simulated \? undefined : openCli\}/);
  });
});
