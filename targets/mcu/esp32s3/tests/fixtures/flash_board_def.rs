/// The erase regions of nodes on this board, as (start, len, unit,
/// unit_max_ms): its `[flash] erase_regions` when it carries its own flash
/// part, else its MCU's. Its `BoardDef` returns them.
pub const ERASE_REGIONS: [(u32, u32, u32, u32); 1] = [
    (0x0, 0x1000000, 0x1000, 900),
];
