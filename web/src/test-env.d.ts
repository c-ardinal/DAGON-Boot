// Tests read the Rust golden frames; @types/node is not a dependency.
declare module 'node:fs' {
  export function readFileSync(path: URL): Uint8Array;
  export function existsSync(path: URL): boolean;
  export interface Dirent {
    name: string;
    isDirectory(): boolean;
  }
  export function readdirSync(path: URL, options: { withFileTypes: true }): Dirent[];
}
