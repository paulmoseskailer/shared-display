# Async Shared Display

Enabling concurrent screen-sharing applications with embedded Rust.

Extends [`embedded-graphics`](https://github.com/embedded-graphics/embedded-graphics) to provide easy development of concurrent, async screen-sharing applications for any display type.

## How to Run

See `examples/` for examples with the [`embedded-graphics-simulator` display](https://github.com/embedded-graphics/simulator).

```
cargo run --example hello_world
```

## How to add support for a new display type

In order to use any display, all that is required is to implement the `SharableBufferedDisplay` trait for the display type.
The display needs to use a framebuffer and implement the async version of `DrawTarget` from [my fork of `embedded-graphics`](https://github.com/paulmoseskailer/embedded-graphics) (has no PR yet due to unresolved issues with providing both sync and async versions simultaneously).
Any display implementing `SharableBufferedDisplay` can be shared by creating a `SharedDisplay::new(display)` and apps can be launched with `SharedDisplay::launch_new_app(app_fn, partition_area)`.

See my forks of [`embedded-graphics-simulator`](https://github.com/paulmoseskailer/simulator/blob/master/src/display.rs#L264) and [`ssd1351` display driver](https://github.com/paulmoseskailer/ssd1351/blob/async_draw/src/mode/graphics.rs#L239) for example implementations of the `SharableBufferedDisplay` type.
Examples on how to use the `SharedDisplay` (with the simulator) can be found in `examples/` (see [How to Run](#how-to-run)).

## Roadmap

- [x] `SharableBufferedDisplay` Trait
- [x] basic toolkit functionality for easy development
- [ ] handle resizing of partitions at runtime
- [ ] provide an elegant solution for non-buffered displays
- [ ] integrate buffer compression
- [ ] submit PRs for dependencies: `embedded-graphics`, `simulator`

## Some Notes on Design Decisions

- `core` is a sub-crate because the toolkit uses nightly Rust and drivers need to implement the `SharableBufferedDisplay` Trait. They should be able to do so without switching to nightly.
