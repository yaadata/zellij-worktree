# zellij-worktree

A Zellij plugin for managing git worktrees.

![demo](demo.gif)

## Features

- **List worktrees**: View all worktrees on plugin open
- **Open in new tab**: Select a worktree and open it in a new tab
- **Create worktrees**: Create new worktrees and open them in new tabs
- **Delete worktrees**: Delete worktrees with confirmation
- **Deterministic placement**: Worktrees default to
  `<repository>/.worktrees/<normalized-branch>`, with an optional shared root

## Configuration and Installation

Add to your `~/.config/zellij/config.kdl`:

```kdl
plugins {
    worktree location="https://github.com/sharph/zellij-worktree/releases/latest/download/zellij-worktree.wasm"
}
```

Add a keybinding:

```kdl
shared_except "locked" "tab" {
    bind "Ctrl w" {
        LaunchOrFocusPlugin "https://github.com/sharph/zellij-worktree/releases/latest/download/zellij-worktree.wasm" {
            floating true
        }
    }
}
```

### Worktree root override

Set `worktree_root` to override both the location and name of the worktree root:

```kdl
plugins {
    worktree location="https://github.com/sharph/zellij-worktree/releases/latest/download/zellij-worktree.wasm" {
        worktree_root "dir:$HOME/.zellij/worktrees"
    }
}
```

Without an override, worktrees live inside the current checkout at
`<repository>/.worktrees/<normalized-branch>`. Every override must start with
`dir:`, for example `dir:~/.zellij/worktrees`, `dir:$HOME`, or
`dir:/absolute/path`.

Overrides use
`<worktree_root>/<repository-name>-<short-hash>/<normalized-branch>-<short-hash>`.
Existing worktrees still open at their original paths.

### Build from source

```bash
git clone https://github.com/sharph/zellij-worktree
cd zellij-worktree
mise install
mise exec -- just install
```

The default destination is `$ZELLIJ_CONFIG_DIR/plugins`, falling back to
Zellij's standard `~/.config/zellij/plugins` directory. To install the plugin
in another directory, pass it to the recipe:

```bash
mise exec -- just install /path/to/plugins
```

Then update your config to use the local plugin:

```kdl
shared_except "locked" "tab" {
    bind "Ctrl w" {
        LaunchOrFocusPlugin "file:~/.config/zellij/plugins/zellij-worktree.wasm" {
            floating true
        }
    }
}
```

## Usage

### Open Worktree

1. Press your keybinding (e.g., `Ctrl+w`)
2. Use `j`/`k` or arrow keys to navigate the list
3. Press `Enter` to open the selected worktree in a new tab

### Create Worktree

1. Open the plugin
2. Press `n` to create a new worktree
3. Type a branch name. Paths and revision shortcuts are not accepted.
4. Press `Enter` to create the worktree and open a new tab

By default, worktrees are created at
`<current-checkout>/.worktrees/<normalized-branch>`. Creating from a linked
worktree uses that checkout's own `.worktrees` directory. Add `.worktrees/` to
your repository's ignore rules to keep these directories out of its
untracked-file listing; the plugin does not change ignore files.

Only the directory name replaces `/` and `\` with `-`: branch `yadi/feature-a`
uses directory `yadi-feature-a` locally and `yadi-feature-a-<short-hash>` under
an override. If the branch already has a worktree, its existing location opens
instead, including the main checkout.

Occupied destinations and normalized-name collisions produce an error. Existing
worktrees are never moved or automatically deleted. The former `base_path`
setting is ignored; use `worktree_root` to override placement.

### Delete Worktree

1. Open the plugin
2. Use `j`/`k` or arrow keys to navigate to the worktree
3. Press `d` to request deletion
4. Press `Enter` to confirm deletion
5. Press `Esc` to cancel

### Keybindings

| Key            | Action                                  |
| -------------- | --------------------------------------- |
| `Esc`          | Close plugin / Cancel action            |
| `Ctrl+c`       | Close plugin                            |
| `Enter`        | Open selected worktree / Confirm action |
| `j`/`k` or ↑/↓ | Navigate list                           |
| `n`            | Create new worktree                     |
| `d`            | Delete selected worktree                |

## Requirements

- Zellij 0.45.1 or later
- Git

## License

MIT
