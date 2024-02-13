#!/bin/zsh

set -x

autoload -Uz catch
autoload -Uz throw

# -- zsh setup --

function setup_zsh() {
    echo "Setup zsh..."

    chsh -s $(which zsh) || throw ZshError

    if ! [[ -f "$HOME/.zshrc" ]]; then
        touch "$HOME/.zshrc" || throw ZshError
    fi

    if ! grep -q "compinit" "$HOME/.zshrc"; then
        echo "autoload -Uz compinit" >> "$HOME/.zshrc" || throw ZshError
        echo "compinit" >> "$HOME/.zshrc" || throw ZshError
    fi

    echo "Setup zsh...done"
}

# -- rustup setup --

function setup_rust() {
    echo "Setup Rust..."

    if ! [[ -d "$HOME/.cargo" ]]; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y || throw RustError
    fi

    if ! grep -q '.cargo/env' "$HOME/.bashrc"; then
        echo "source $HOME/.cargo/env" >> "$HOME/.zshrc" || RustError
    fi

    source "$HOME/.cargo/env" || RustError

    rustup update || RustError
    rustup default stable || RustError

    echo "Setup Rust...done"
}

# -- setup vim --

function setup_vim() {
    echo "Setup Vim..."

    curl -fLo "$HOME/.local/share/nvim/site/autoload/plug.vim" --create-dirs \
        https://raw.githubusercontent.com/junegunn/vim-plug/master/plug.vim || throw VimError

    if ! [[ -d "$HOME/.config/nvim" ]]; then
        mkdir -p "$HOME/.config/nvim" || throw VimError
    fi

    cat <<EOF >"$HOME/.config/nvim/init.vim || throw VimError"
"" general settings

set encoding=utf-8 " not necessary in unix env, but for windows env
set nu
set expandtab
set tabstop=4
set softtabstop=4
set shiftwidth=4
set incsearch
set backspace=indent,eol,start

" escape for terminal mode
tnoremap <ESC> <c-\\><c-n>


"" plugins

call plug#begin()

Plug 'vim-syntastic/syntastic'
Plug 'nvim-treesitter/nvim-treesitter', {'do': ':TSUpdate'}

" rust
Plug 'rust-lang/rust.vim'

" python
Plug 'vim-scripts/indentpython.vim'
Plug 'nvie/vim-flake8'

call plug#end()

EOF

    nvim --headless +PlugInstall +qall || throw VimError

    if ! grep -q "vim=nvim" "$HOME/.zshrc"; then
        echo "alias vim=nvim" >> "$HOME/.zshrc" || throw VimError
    fi

    echo "Setup Vim...done"
}

# -- main --

setup_zsh
setup_rust
setup_vim
# add new setup here

echo "User setup complete"

