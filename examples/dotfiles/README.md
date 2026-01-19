# Example Dotfiles Structure

This shows a recommended dotfiles repository structure for use with spuff.

## Structure

```
dotfiles/
├── install.sh          # Main installation script (auto-executed by spuff)
├── .bashrc             # Bash configuration
├── .bash_aliases       # Bash aliases
├── .gitconfig          # Git configuration
├── .tmux.conf          # Tmux configuration
├── .vimrc              # Vim configuration
├── starship.toml       # Starship prompt config
└── scripts/            # Additional scripts
    ├── setup-git.sh
    └── setup-tools.sh
```

## How spuff uses dotfiles

When you specify `dotfiles: https://github.com/user/dotfiles` in your config:

1. Spuff clones the repo to `~/dotfiles`
2. If `install.sh` exists, it's executed
3. If `Makefile` with `install` target exists, `make install` runs
4. Otherwise, common dotfiles are symlinked automatically

## Example install.sh

```bash
#!/bin/bash
set -e

DOTFILES_DIR="$HOME/dotfiles"

# Symlink dotfiles
ln -sf "$DOTFILES_DIR/.bashrc" "$HOME/.bashrc"
ln -sf "$DOTFILES_DIR/.bash_aliases" "$HOME/.bash_aliases"
ln -sf "$DOTFILES_DIR/.gitconfig" "$HOME/.gitconfig"
ln -sf "$DOTFILES_DIR/.tmux.conf" "$HOME/.tmux.conf"
ln -sf "$DOTFILES_DIR/.vimrc" "$HOME/.vimrc"

# Create config directories
mkdir -p "$HOME/.config"
ln -sf "$DOTFILES_DIR/starship.toml" "$HOME/.config/starship.toml"

# Run additional setup
if [ -f "$DOTFILES_DIR/scripts/setup-git.sh" ]; then
    "$DOTFILES_DIR/scripts/setup-git.sh"
fi

echo "Dotfiles installed!"
```

## Tips

1. **Keep it lightweight**: Heavy installations slow down VM creation
2. **Use SSH URLs carefully**: Requires SSH agent forwarding
3. **Test locally first**: Ensure install.sh works before using with spuff
4. **Idempotent scripts**: install.sh may run multiple times
5. **No secrets**: Don't store API keys or tokens in dotfiles
