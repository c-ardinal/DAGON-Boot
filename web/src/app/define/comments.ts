// Hand-written comments of the project's YAML across a save. The page
// writes the 構成 back in its own layout (toYaml), which drops comments; the
// comments of the opened text are kept by the content line they sit above or
// end, and put back on the same line of the new text. A line that changed
// takes its comments with it; every other line keeps them.

interface Anchored {
  /** Whole-line comments above the line, as written (without indentation). */
  above: string[];
  /** The comment at the end of the line, if any. */
  inline: string | null;
}

/** `# ...` at the end of a line, outside quotes: YAML needs a space before it. */
function splitInline(line: string): [string, string | null] {
  let quote: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === '#' && i > 0 && /\s/.test(line[i - 1])) {
      return [line.slice(0, i).trimEnd(), line.slice(i)];
    }
  }
  return [line, null];
}

const isComment = (line: string) => line.trimStart().startsWith('#');

/**
 * Each content line by its text and how many times that text came before it,
 * so repeated lines ("max_attempts: 3") keep their own comments.
 */
function keyed(lines: string[], each: (key: string, content: string, line: string) => void): void {
  const seen = new Map<string, number>();
  for (const line of lines) {
    const content = splitInline(line)[0].trim();
    const n = seen.get(content) ?? 0;
    seen.set(content, n + 1);
    each(`${content}\u0000${n}`, content, line);
  }
}

/** The comments of `source`, by content line, and those after the last line. */
function commentsOf(source: string): { at: Map<string, Anchored>; tail: string[] } {
  const at = new Map<string, Anchored>();
  let above: string[] = [];
  const content: string[] = [];
  const pending: string[][] = [];
  for (const line of source.split(/\r?\n/)) {
    if (isComment(line)) above.push(line.trim());
    else if (line.trim() !== '') {
      content.push(line);
      pending.push(above);
      above = [];
    }
  }
  let i = 0;
  keyed(content, (key, _, line) => {
    const inline = splitInline(line)[1];
    const a = pending[i++];
    if (a.length || inline) at.set(key, { above: a, inline });
  });
  return { at, tail: above };
}

/**
 * `generated` with the comments of `source` put back on the lines that did
 * not change. Lines are compared by content (indentation aside).
 */
export function keepComments(source: string, generated: string): string {
  const { at, tail } = commentsOf(source);
  if (!at.size && !tail.length) return generated;
  const lines = generated.replace(/\n$/, '').split('\n');
  const out: string[] = [];
  const blank = (l: string) => l.trim() === '';
  let k = 0;
  const keys: (string | null)[] = [];
  keyed(lines.filter((l) => !blank(l)), (key) => keys.push(key));
  for (const line of lines) {
    if (blank(line)) {
      out.push(line);
      continue;
    }
    const c = at.get(keys[k++]!);
    const pad = /^\s*/.exec(line)![0];
    if (c) out.push(...c.above.map((a) => pad + a));
    out.push(c?.inline ? `${line}  ${c.inline}` : line);
  }
  return `${[...out, ...tail].join('\n')}\n`;
}
