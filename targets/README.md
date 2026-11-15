# MCU and board definitions

Every MCU package (`mcu/<id>/`) carries `dagon-mcu.toml` and every board
package (`boards/<id>/`) carries `dagon-board.toml`. Their keys and rules are
in [MCU and board definitions](../site/src/content/docs/reference/definitions.md),
generated from the types DAGON reads them into; the boards and MCUs of this
tree are listed in [Boards and MCUs](../site/src/content/docs/reference/boards.md).

A project changes one of these definitions without copying or editing
it: its `dagon.toml` gives only the fields that change, in
`[mcu_override.<mcu id>]` or `[board_override.<board id>]`, for every node
on that MCU or board, and `dagon gen` writes each node's merged
definitions into the project's `boot/<id>/`. The merge rules are under
Project definitions on the same definitions page.
