# egui-term

`egui-term` is a minimal terminal backend for `egui`.

It provides:

- `State`: collect terminal input and build `egui::RawInput` (similar role to `egui-winit::State`)
- `run` / `run_with`: an out-of-the-box event loop for terminal GUI apps
- kitty graphics rendering via offscreen `egui-wgpu`

## Run demo

```bash
cargo run --example demo
```

Exit with `q`, `Esc`, or `Ctrl+C`.

## Integration test

```bash
./scripts/integration.sh
```

The script runs unit tests and then starts the demo in a pseudo-tty, checks that kitty graphics escape sequences are emitted, and exits.
