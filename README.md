# bevy_window_manager

> **No longer developed here.** This crate has moved into the
> [`hana`](https://github.com/natepiano/hana) workspace and continues as
> [**`bevy_clerestory`**](https://github.com/natepiano/hana/tree/main/crates/bevy_clerestory).

`bevy_window_manager` provided primary-window position/size persistence across
launches and multi-monitor, scale-factor-correct placement for Bevy apps on
macOS, Windows, and Linux (X11 and Wayland).

Development continues under the new name. For the latest version, new features,
and issues, use `bevy_clerestory`:

```toml
[dependencies]
bevy_clerestory = "0.1"
```

The published `bevy_window_manager` releases (through 0.22.0) stay on crates.io
and are **not yanked**, so existing builds keep resolving. The crate name is
open to transfer — open an issue on the
[`hana`](https://github.com/natepiano/hana) repo if you'd like to take it over.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.
