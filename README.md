# Async Shared Display

Enabling concurrent screen-sharing applications with embedded Rust.

## How to Use

See `examples` folder.

```
cargo run --example hello_world
```

## Roadmap

- [x] `SharableBufferedDisplay` Trait
- [x] basic toolkit functionality for easy development
- [ ] handle resizing of partitions at runtime
- [ ] integrate buffer compression
- [ ] submit PRs for dependencies: `embedded-graphics`, `simulator`(, `ssd1351`)

## Some Notes on Design Decisions

- `core` is a sub-crate because the toolkit uses nightly Rust and drivers need to implement the `SharableBufferedDisplay`/`SharableNoBufferDisplay` Trait. They should be able to do so without switching to nightly.
