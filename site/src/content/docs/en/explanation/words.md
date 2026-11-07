---
title: "What the words mean"
description: "node, hub, child, grandchild, topology, link, image and package as these pages use them."
---

- **node**: one microcontroller of your setup.
- **hub**: the node on the PC's USB. It relays writes to the other nodes.
- **child**: a node the hub reaches over I2C or UART.
- **grandchild**: a node beyond a child.
- **topology**: the nodes, how they are linked, and the write order (`topology.yaml`).
- **link**: a connection between nodes.
- **image**: a program written to a node, made of a signed header and a body.
- **package**: a file (`.dgb`) holding the images signed for a topology.
