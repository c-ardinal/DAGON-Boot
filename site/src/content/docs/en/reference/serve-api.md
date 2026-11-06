---
title: Serve HTTP API
description: Every route of dagon serve, from its router.
editUrl: false
---

:::note
Generated from `crates/dagon-cli/src/serve/api.rs`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

Every route is under `dagon serve`, on the loopback address it prints. Every
`/v1/` request needs the passphrase from the address serve printed, as
`Authorization: Bearer <passphrase>` (401 without it), and passes serve's
Host and Origin checks (403 otherwise).

| Method | Path | `dagon demo` | What it does |
|---|---|---|---|
| `GET` | `/v1/hello` |  | Versions, work area, abilities; whether keys exist (never the keys). |
| `GET` | `/v1/agent` |  | The agents and their ways (whether each can be used here; never a key, path or command), the consent, what linking sends. |
| `POST` | `/v1/agent/link` | refused | Records the user's consent to send to that agent that way; anything short of an explicit yes for a defined agent and way records nothing. |
| `DELETE` | `/v1/agent/link` | refused | Withdraws the consent (done also when none was). |
| `PUT` | `/v1/agent/key/{agent}/{way}` | refused | Registers the way's key in the OS credential store. The answer is GET /v1/agent's body (whether a key is registered, never the key). |
| `DELETE` | `/v1/agent/key/{agent}/{way}` | refused | Removes the way's key (fine when none was registered). |
| `POST` | `/v1/agent/ask` | refused | One question to DAGON, answered in this request. |
| `GET` | `/v1/definitions` |  | The definitions in use as `Registry::to_json` gives them, for the page's dagon-wasm (`load_definitions`). |
| `GET` | `/v1/topology` |  | The project's 構成 YAML as it is on disk (404 before the first save). |
| `PUT` | `/v1/topology` |  | Writes the topology YAML after the same check `dagon topo check` runs. |
| `GET` | `/v1/nodes/{id}/build` |  | The node's `[build.<id>]` table of dagon.toml (404 when it has none). |
| `PUT` | `/v1/nodes/{id}/build` |  | Replaces `[build.<id>]` in dagon.toml, keeping the rest of the file. |
| `POST` | `/v1/build` |  | Builds the applications of `nodes` (all when left out) as a job; the answer lists each built slot and its size. |
| `POST` | `/v1/sign` |  | Signs the built applications of `nodes` with the CLI's key. |
| `POST` | `/v1/release` |  | Builds a release from every signed image, each of the version its node's crate gave it (`version`, if given, must be every image's). |
| `GET` | `/v1/release/latest` |  | The newest .dgb of the project's out/release. |
| `GET` | `/v1/release/{id}` |  | A release of the project's out/release by name, once it verifies with the project's key. |
| `GET` | `/v1/kept` |  | The packages kept in the project (out/deploy), newest version first, each with the version it gives each node. |
| `GET` | `/v1/kept/{id}` |  | A kept package's bytes, once they verify. |
| `POST` | `/v1/kept` |  | Keeps a package about to be written, once it verifies with the project's key; answers its id. |
| `GET` | `/v1/bootcore/{node}` |  | The node's signed boot core (the ROM path): [provision.&lt;node&gt;] bootcore, else &lt;provision_dir&gt;/&lt;node&gt;/bootcore.img. |
| `GET` | `/v1/stage0/{node}` |  | Stage0's bytes for the ROM path's full rewrite of an MCU with a stage0, as provision wrote them: &lt;provision_dir&gt;/&lt;node&gt;/stage0.bin. Stage0 carries no signature; the host checks it fits the MCU's stage0 region before writing it. |
| `GET` | `/v1/layout` |  | 呼び名 and editor positions (an empty layout before the first save). |
| `PUT` | `/v1/layout` |  | A layout of this format, written whole. |
| `POST` | `/v1/conform` | refused | The 12 conformance tests on `node` through the hub, after the page hands over the port. |
| `GET` | `/v1/conform/{node}` | refused | The node's last conformance result (404 before the first run). |
| `POST` | `/v1/ai/topology` | refused | A job; the model's questions go to the page. |
| `POST` | `/v1/ai/port` | refused | Porting DAGON to a board: answers 501, this dagon has no porting job. |
| `GET` | `/v1/jobs/{id}` |  | A job's state, stage, progress, log lines, question and result. |
| `POST` | `/v1/jobs/{id}/answer` |  | `{answer}` to the job's open question (409 when it has none). |
| `POST` | `/v1/jobs/{id}/cancel` |  | Stops a running job. |
| `POST` | `/v1/device` | refused | `{state: "released"}`: the page has closed the hub's port, so the CLI job waiting for it can use it. |
| `GET` | `/v1/project` |  | The open project and the recent ones. |
| `POST` | `/v1/project/open` | refused | A folder with a dagon.toml. |
| `POST` | `/v1/project/new` | refused | `dagon new` there (empty 構成 unless from_sample), key pair in ~/.dagon/keys/&lt;name&gt; when none. |
| `POST` | `/v1/project/init` | refused | Dagon.toml in a folder that has a 構成 but none, for that YAML (`yaml`, or the folder's only one); key pair in ~/.dagon/keys/&lt;folder&gt; when none. Then opens it. |
| `GET` | `/v1/fs` | refused | The folders in a folder under home (default: home), each marked when it holds a dagon.toml. Key folders are left out. |
| `POST` | `/v1/gen` |  | `dagon gen` for the open project; dagon.toml is edited under the files lock. |
| `POST` | `/v1/flash` | refused | `device` names one of the candidates the writer reports; with several and none named, the job ends with the candidates (written: false) and writes nothing. |
| `POST` | `/v1/doctor` | refused | The tools the project's MCUs need, by its definitions. Nobody can confirm a command here, so a check the project itself defines is reported as one for the terminal's `dagon doctor`. |
| `POST` | `/v1/sim/write` |  | Bytes for the simulated hub's USB port. |
| `GET` | `/v1/sim/read` |  | What the simulated hub answered (empty when nothing came within the wait). |

`/v1/events` is a WebSocket: the page sends the passphrase as its
first message, then receives serve's events (job progress, logs, results,
questions and the hub hand-over).

A route marked *refused* answers 403 while `dagon demo` runs: the demo has
its own simulator and project and never touches real hardware, other
projects or an agent.
