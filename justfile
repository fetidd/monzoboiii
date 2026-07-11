pi_host := "raspberrypi"
pi_run_dir := "/home/ben/monzoboiii-run"

# Build a native binary for local testing
build:
    cargo build --release

# Cross-compile the release binaries for the Pi (aarch64), via podman
build-pi:
    CROSS_CONTAINER_ENGINE=podman cross build --release --target aarch64-unknown-linux-gnu

# Push freshly built binaries to the Pi's runtime directory
push-pi: build-pi
    ssh {{pi_host}} 'mkdir -p {{pi_run_dir}}/bin'
    rsync -avz target/aarch64-unknown-linux-gnu/release/monzoboiii target/aarch64-unknown-linux-gnu/release/monzoctl {{pi_host}}:{{pi_run_dir}}/bin/

# Install/update the systemd unit on the Pi from the checked-in template
provision-pi:
    scp deploy/monzoboiii.service {{pi_host}}:/tmp/monzoboiii.service
    ssh {{pi_host}} 'sudo mv /tmp/monzoboiii.service /etc/systemd/system/monzoboiii.service && sudo systemctl daemon-reload && sudo systemctl enable monzoboiii'

# Build, push, and restart the live service on the Pi -- the everyday deploy command
reload: push-pi
    ssh {{pi_host}} 'sudo systemctl restart monzoboiii'
    ssh {{pi_host}} 'sudo systemctl status monzoboiii --no-pager'

# Run diagnostics against the Pi's live instance
diagnose-pi:
    ssh {{pi_host}} '{{pi_run_dir}}/bin/monzoctl diagnose'

# Tail the live service logs on the Pi
logs-pi:
    ssh {{pi_host}} 'sudo journalctl -u monzoboiii -f'

# Redirect the Pi's public webhook tunnel to a local dev build on this machine.
# Stops prod on the Pi, opens a reverse SSH tunnel (pi:PORT -> here:PORT), and
# restores prod automatically on Ctrl-C. Run `cargo run` here in another shell
# while this is active. PORT must match the `port` in config.toml (currently 3687).
webhook-tunnel port="3687":
    #!/usr/bin/env bash
    set -euo pipefail
    ssh {{pi_host}} 'sudo systemctl stop monzoboiii'
    trap 'ssh {{pi_host}} "sudo systemctl start monzoboiii"' EXIT
    ssh -N -R {{port}}:localhost:{{port}} {{pi_host}}
