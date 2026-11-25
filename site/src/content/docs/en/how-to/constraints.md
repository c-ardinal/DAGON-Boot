---
title: "Keeping node versions compatible"
description: "Declare which nodes need each other's versions, and what DAGON does when one node stays on its version."
---

Nodes that talk to each other often need matching versions.
`constraints` in `topology.yaml` declares that, and DAGON keeps it in two places: when it makes a package, and when a write stops at a node.

## Declare the constraints

```yaml nocheck
constraints:
  - lockstep: [hub, seg]
  - requires: { node: seg, needs: hub, min: 1.2.0 }
  - min_bootcore: { node: seg, min: 1.0.0 }
```

| Constraint | Meaning |
|---|---|
| `lockstep: [ids]` | These nodes run the versions of one package together. |
| `requires: { node, needs, min }` | `node` needs `needs` at version `min` or later. |
| `min_bootcore: { node, min }` | `node` needs a boot core at version `min` or later. |

A constraint that names a node the topology does not have is refused (V-15).

## When a package is made

`dagon deploy` checks the constraints against the images of the package (RB-05) and makes no package that breaks one.
The constraints go into the package's signed manifest, so the write judges them from the package and not from the file on the PC.

## When a write stops at a node

`dagon deploy` writes the nodes one by one, and each node confirms its new version before the next one starts.
When a node fails, the nodes after it are not started.
The failed node stays on the version it ran, or goes back to it.

DAGON then judges the constraints with the versions the nodes run:

- `lockstep`: when one named node is off the package's version, every named node that this write moved goes back to the version it ran before.
- `requires`: when `needs` is below `min`, `node` goes back to the version it ran before. A node that is only needed stays on the new version.
- A node that goes back can break a constraint of another node, which goes back as well. This repeats until every constraint holds.
- A node no constraint ties to the failed node keeps the new version.

A node goes back the same way as with `dagon rollback`: the version in its other slot starts and is confirmed.
The result of each node tells what happened:

| Result | Meaning |
|---|---|
| `done, <version>` | Runs the new version, confirmed. |
| `back on <version>` | Was written and confirmed, then went back for a constraint. |
| `on <version>, against a compatibility constraint ...` | A constraint does not hold and the node could not go back: it has one slot, its other slot holds no valid image, or it ran the new version before this write. |
| `stopped: ...` | The node the write stopped at. When it runs the new version, confirmed (the replies to its confirm were lost), it goes back for a constraint as well and keeps this result. |
| `not started` | A node after it. |

The command ends with an error whenever a node failed, so a script sees a write that left the nodes on mixed versions.

DAGON judges only the nodes of the write that answer. A node that does not answer is not judged, and no node goes back for it.
`min_bootcore` is not judged after a failure: a write puts a node's boot core in place before its app and stops when the core fails, so the new app never runs over an older core.
