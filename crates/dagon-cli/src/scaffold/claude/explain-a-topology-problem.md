---
name: explain-a-topology-problem
description: Explain a problem the DAGON 構成 check reports (topology.yaml), in plain words, and propose the smallest fix. Use when `dagon topo check`, `dagon gen` or DAGON Console shows a 構成 problem the developer does not understand.
argument-hint: [rule id or message]
---

# Explain a 構成 problem

Arguments: `$ARGUMENTS` (a rule id such as `V-07`, or the message shown;
if empty, explain every problem).

## Steps

1. **Read the problems.** Run `dagon topo check topology.yaml --json`.
   Each issue has a severity (error or warning), a rule id, a path in the
   YAML (such as `links[0].addr`) and a message.
2. **Place it.** Open `topology.yaml` at that path and name the node or
   link it is about, using ノード, 親機, 子機 and 構成.
3. **Explain** in two or three sentences: what the rule protects (for
   example, two 子機 on one I2C bus cannot share an address; a pin must
   exist on the 親機's header), what in this 構成 breaks it, and what
   happens on the device if it is left as is.
4. **Propose the smallest fix** as a YAML change. When the fix needs a
   hardware fact (a pin, an address), ask instead of guessing.
5. **Apply only after the developer agrees,** then run the check again
   and show that the problem is gone. Errors stop `dagon gen`, signing
   and updates; warnings do not, so say which one it is.

Do not change the checker, the rules or the DAGON crates to make a
problem disappear.
