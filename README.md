<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/product-symbol/lockup-horizontal-dark-2400w.png">
    <img src="assets/product-symbol/lockup-horizontal-light-2400w.png" alt="DAGON Boot" width="480">
  </picture>
</p>

# DAGON Boot (DIY Assured-update Gateway for Own Nodes)

[日本語版 / Japanese: README.ja.md](README.ja.md)

**Make hardware that works, easily, and keep growing it after it ships.**

DAGON Boot is for developers, from first-timers to experienced firmware
engineers, who build something out of a few microcontrollers and want to
keep improving it once it is on a shelf, the wall or the wrist. You choose
the boards (or just their MCUs), draw how they connect, and write your
application in Rust. DAGON takes care of the rest:

- **A project that builds.** `dagon new` and `dagon gen` give every node a
  Rust crate with its start-up, its wiring and an empty `app.rs` for your
  code. Flash layout, image headers, linker addresses and the boot loader
  are decided for you.
- **Safe updates for the whole group.** One hub on your PC's USB sends each
  node its new version. Every image is signed; a node runs a new version on
  trial and goes back to the previous one by itself when the new one never
  gets confirmed. A node that no longer answers can be rewritten through the
  hub.
- **A browser tool and a command line that do the same.** DAGON Console
  (`dagon serve`) shows the nodes and links as a graph, checks them as you
  edit, builds, signs and updates. Every step is also a `dagon` command.
- **Help from Claude.** A new project comes with Claude Code instructions
  and skills for the common questions: a failing build, a board that is not
  in the list, a problem in the topology.

## Documentation

The documentation is published at <https://c-ardinal.github.io/DAGON-Boot/doc/>,
with DAGON Console on a simulator at <https://c-ardinal.github.io/DAGON-Boot/demo/>.
Its pages are the files under [`site/src/content/docs`](site/src/content/docs):

- [Try DAGON](site/src/content/docs/getting-started/index.md), [Install](site/src/content/docs/getting-started/install.md)
  and [From a new project to an update](site/src/content/docs/getting-started/first-project.md)
- How-to guides: [your own board](site/src/content/docs/how-to/own-board.md),
  [swapping a link](site/src/content/docs/how-to/swap-link.md),
  [adding DAGON to existing firmware](site/src/content/docs/how-to/existing-firmware.md),
  [Claude help in your project](site/src/content/docs/how-to/claude-help.md)
- Explanation: [words used here](site/src/content/docs/explanation/words.md),
  [versions and releases](site/src/content/docs/explanation/versioning.md),
  [lifecycle events and extensions](site/src/content/docs/explanation/lifecycle-events.md),
  [the repository](site/src/content/docs/explanation/repository.md),
  [limitations](site/src/content/docs/explanation/limitations.md)
- Reference: [boards and MCUs](site/src/content/docs/reference/boards.md), [tools by MCU](site/src/content/docs/reference/tools.md),
  [serve HTTP API](site/src/content/docs/reference/serve-api.md), [events](site/src/content/docs/reference/events.md),
  [extension manifest](site/src/content/docs/reference/extension-manifest.md),
  [licenses](site/src/content/docs/reference/licenses.md)

## License

MIT or Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
