// Tests read the Rust golden frames; @types/node is not a dependency.
declare module 'node:fs' {
  export function readFileSync(path: URL): Uint8Array;
}
