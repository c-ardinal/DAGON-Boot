# tools/agent-sdk

The program `dagon ai port` starts. It runs the `dagon-port` skill of the plugin in `plugin/` with the Claude Agent SDK, in this clone, until the conformance tests pass or the skill's limits are reached.

Install its packages once:

```sh
cd tools/agent-sdk
npm ci
```

`dagon ai port <MCU> <BOARD> --kind <S|V>` then starts it. The agent edits files and runs commands in this clone without asking; the plugin's guard hook keeps it from the conformance tests, the simulator and the shared crates. The API key is the one registered with `dagon ai key set`; dagon passes it to this program only.

`protocol.mjs` is what dagon and this program say to each other. `npm test` checks it.
