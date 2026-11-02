---
name: dagon-topology
description: Turn a natural-language description of a DAGON board setup (hub, nodes, buses, addresses, pins) into a topology YAML file and fix it until `dagon topo check --json` reports zero errors. Use when the user describes their boards and wants a topology file.
argument-hint: [output.yaml] [description...]
---

# Build a DAGON topology from a description

Arguments: `$ARGUMENTS` (optional output path, default `topology.yaml`, then the description;
if no description was given, ask for one).

## Steps

1. **Read the schema.** The topology format is defined by `crates/dagon-topology`; read it
   and any example topology in the repository before writing. Do not invent keys.
2. **Extract facts.** From the description, list hub, nodes, MCU and board per node, link
   per node (I2C / UART and their parameters), addresses and pins.
3. **Ask, don't guess.** Anything that cannot be derived with certainty from the description
   or a cited board document (pin numbers, I2C addresses, UART port, baud rate, boot pins)
   is asked of the user in one batched question. Never fill in a plausible-looking value.
4. **Write the YAML** to the output path.
5. **Check and fix.** Run `dagon topo check <file> --json`. For each reported error, fix the
   YAML (or ask the user if the fix needs a fact you do not have) and run the check again.
   Repeat until the check reports 0 errors. Do not edit the checker or the schema crate to
   make an error go away.
6. **Report** the file path, the final check output summary (errors 0, warnings if any) and
   every value the user supplied in step 3.
