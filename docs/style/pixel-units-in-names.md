## Qualify pixel-valued fields with `logical_` or `physical_`

Window management in this crate constantly shuttles values between logical pixels
(scale-independent, what Bevy's `Window.resolution.width()` returns) and physical
pixels (scaled, what winit's `outer_position()` and `set_physical_resolution` work
in). A bug here is silent — an `i32` that means "80 logical px" and an `i32` that
means "80 physical px at 2x scale" look identical at the call site.

To prevent that, every field whose value is a pixel count — position, width,
height, size — must carry an explicit `logical_` or `physical_` prefix. This
overrides the general "avoid repeated field affixes" rule: the qualifier is
carrying information, not noise.

```rust
// bad — unit is invisible at the call site
pub struct WindowState {
    pub position: Option<(i32, i32)>,
    pub width:    u32,
    pub height:   u32,
}

// good — call site cannot mistake one unit for the other
pub struct WindowState {
    pub logical_position: Option<(i32, i32)>,
    pub logical_width:    u32,
    pub logical_height:   u32,
}
```

### Scope

Applies to fields and locals storing:

- window position (`IVec2`, `(i32, i32)`, `Option<IVec2>`)
- window or content size (`UVec2`, `u32` width/height)
- any derived pixel count (decoration extents, frame offsets, clamping bounds)

Does **not** apply to:

- scale factors (`f64`) — dimensionless ratios, not pixel counts
- monitor indices, mode enums, strings, timers
- local variables where the unit is obvious from one line of context (e.g.
  `let logical_w = window.resolution.width();` — already explicit)

### When both units live in one struct

Name them as a pair: `physical_size` + `logical_size`, `physical_position` +
`logical_position`. Do not use a bare name for "the default one":

```rust
// bad — which is `size`?
pub struct TargetPosition {
    pub size:         UVec2,
    pub logical_size: UVec2,
}

// good
pub struct TargetPosition {
    pub physical_size: UVec2,
    pub logical_size:  UVec2,
}
```

### When only one unit is present

Still qualify it. Future additions will add the other flavor, and the qualifier
prevents a silent ambiguity at the point of extension.

### Wire format

When a struct is serialized, keep the explicit Rust names and rely on
`#[serde(rename = ...)]` only if the wire format diverges from the Rust field
name. If the wire format already uses `logical_*` / `physical_*` spellings, no
rename attribute is needed — the field name is the wire name.
