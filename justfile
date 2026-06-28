release:
    cargo build --release

reload: release
    sudo systemctl restart monzoboiii
