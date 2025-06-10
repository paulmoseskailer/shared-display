# How to Run

On Raspberry Pi Pico with ssd1351 screen.
Connect a debug probe to the Pi and the ssd1351 screen as defined in [main.rs](./src/main.rs#L122).
Then simply 

```bash
cargo run
```

A version that uses framebuffer compression can be run with

```bash
cargo run --features compressed
```

# How to Measure memory usage

```bash
cargo run [--features compressed] | tee log.txt
python plot_usage.py
```

