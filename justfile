plugin := "target/wasm32-wasip1/release/zellij-worktree.wasm"
zellij_config_dir := env_var_or_default("ZELLIJ_CONFIG_DIR", home_directory() / ".config/zellij")

install destination=(zellij_config_dir / "plugins"):
    cargo build --release
    mkdir -p "{{ destination }}"
    cp "{{ plugin }}" "{{ destination }}/"
