# Run an example with debug logging for bevy_window_manager
# Usage: .\run_debug.ps1 <example_name>

if ($args.Count -eq 0) {
    Write-Host "Usage: .\run_debug.ps1 <example_name>"
    exit 1
}

$env:RUST_LOG = "info,wgpu=error,wgpu_hal=error,naga=warn,bevy_window_manager=debug"
cargo run --example $args[0]
