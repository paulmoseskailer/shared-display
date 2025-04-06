# Async Shared Display

Enabling concurrent screen-sharing applications with embedded Rust.

## How to Run

See `examples` folder.

```
cargo run --example hello_world
```

## Roadmap

- [x] `SharableBufferedDisplay` Trait
- [x] basic toolkit functionality for easy development
- [ ] handle resizing of partitions at runtime
- [ ] `SharableNoBufferDisplay` Trait
- [ ] integrate buffer compression
- [ ] submit PRs for dependencies: `embedded-graphics`, `simulator`
