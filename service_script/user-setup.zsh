#!/bin/zsh

set -x

autoload -Uz catch
autoload -Uz throw

# -- zsh setup --

function setup_zsh() {
    echo "Setup zsh..."

    local -a zshrc_lines={{zshrc_lines}}

    chsh -s $(which zsh) || throw ZshError

    if ! [[ -f "$HOME/.zshrc" ]]; then
        touch "$HOME/.zshrc" || throw ZshError
    fi

    if ! grep -q "compinit" "$HOME/.zshrc"; then
        echo "autoload -Uz compinit" >> "$HOME/.zshrc" || throw ZshError
        echo "compinit" >> "$HOME/.zshrc" || throw ZshError
    fi

    if ! grep -q "LC_ALL" "$HOME/.zshrc"; then
        echo "export LANG=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_COLLATE=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_CTYPE=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_MESSAGES=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_MONETARY=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_NUMERIC=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_TIME=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
        echo "export LC_ALL=en_US.UTF-8" >> "$HOME/.zshrc" || throw ZshError
    fi

    for line in $zshrc_lines; do
        if ! grep -q "$line" "$HOME/.zshrc"; then
            echo "$line" >> "$HOME/.zshrc" || throw ZshError
        fi
    done

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
    cargo install sccache || RustError

    if ! grep -q 'RUSTC_WRAPPER' "$HOME/.zshrc"; then
        echo "export RUSTC_WRAPPER=$HOME/.cargo/bin/sccache" >> "$HOME/.zshrc" || RustError
    fi

    echo "Setup Rust...done"
}

# -- setup vim --

function setup_vim() {
    echo "Setup Vim..."


    if ! [[ -f "$HOME/nvim.appimage" ]]; then
        curl -L https://github.com/neovim/neovim/releases/latest/download/nvim.appimage -o "$HOME/nvim.appimage" || throw VimError
        chmod u+x "$HOME/nvim.appimage" || throw VimError
    fi

    if ! [[ -d "$HOME/.config/nvim" ]]; then
        mkdir -p "$HOME/.config/nvim" || throw VimError
    fi

    cat <<EOF >"$HOME/.config/nvim/init.lua" || throw VimError
vim.opt.number = true
vim.opt.expandtab = true
vim.opt.autoindent = true
vim.opt.smartindent = true
vim.opt.incsearch = true
vim.opt.tabstop = 4
vim.opt.softtabstop = 4
vim.opt.shiftwidth = 4
vim.opt.backspace = 'indent,eol,start'

-- lazy.nvim

local lazypath = vim.fn.stdpath('data') .. '/lazy/lazy.nvim'
if not vim.loop.fs_stat(lazypath) then
    vim.fn.system({
        'git',
        'clone',
        '--filter=blob:none',
        'https://github.com/folke/lazy.nvim.git',
        '--branch=stable', -- latest stable release
        lazypath,
    })
end
vim.opt.rtp:prepend(lazypath)

-- plugins

require('lazy').setup({
    { 'neovim/nvim-lspconfig', tag = 'v0.1.7' },
})

-- lspconfig: https://github.com/neovim/nvim-lspconfig

local lspconfig = require('lspconfig')

-- - Global mappings.
-- - See ':help vim.diagnostic.*' for documentation on any of the below functions
vim.keymap.set('n', '<space>e', vim.diagnostic.open_float)
vim.keymap.set('n', '[d', vim.diagnostic.goto_prev)
vim.keymap.set('n', ']d', vim.diagnostic.goto_next)
vim.keymap.set('n', '<space>q', vim.diagnostic.setloclist)

-- - Use LspAttach autocommand to only map the following keys
-- - after the language server attaches to the current buffer
vim.api.nvim_create_autocmd('LspAttach', {
  group = vim.api.nvim_create_augroup('UserLspConfig', {}),
  callback = function(ev)
    -- Enable completion triggered by <c-x><c-o>
    vim.bo[ev.buf].omnifunc = 'v:lua.vim.lsp.omnifunc'

    -- Buffer local mappings.
    -- See ':help vim.lsp.*' for documentation on any of the below functions
    local opts = { buffer = ev.buf }
    vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, opts)
    vim.keymap.set('n', 'gd', vim.lsp.buf.definition, opts)
    vim.keymap.set('n', 'K', vim.lsp.buf.hover, opts)
    vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, opts)
    vim.keymap.set('n', '<C-k>', vim.lsp.buf.signature_help, opts)
    vim.keymap.set('n', '<space>wa', vim.lsp.buf.add_workspace_folder, opts)
    vim.keymap.set('n', '<space>wr', vim.lsp.buf.remove_workspace_folder, opts)
    vim.keymap.set('n', '<space>wl', function()
      print(vim.inspect(vim.lsp.buf.list_workspace_folders()))
    end, opts)
    vim.keymap.set('n', '<space>D', vim.lsp.buf.type_definition, opts)
    vim.keymap.set('n', '<space>rn', vim.lsp.buf.rename, opts)
    vim.keymap.set({ 'n', 'v' }, '<space>ca', vim.lsp.buf.code_action, opts)
    vim.keymap.set('n', 'gr', vim.lsp.buf.references, opts)
    vim.keymap.set('n', '<space>f', function()
      vim.lsp.buf.format { async = true }
    end, opts)
  end,
})

-- - Rust

local on_attach = function(client)
    require('completion').on_attach(client)
end

lspconfig.rust_analyzer.setup({
    on_attach = on_attach,
    settings = {
        ['rust-analyzer'] = {
            imports = {
                granularity = {
                    group = 'module',
                },
                prefix = 'self',
            },
            cargo = {
                buildScripts = {
                    enable = true,
                },
            },
            procMacro = {
                enable = true
            },
        }
    }
})
EOF
    if ! grep -q "vim=nvim" "$HOME/.zshrc"; then
        echo "alias vim=$HOME/nvim.appimage" >> "$HOME/.zshrc" || throw VimError
    fi

    echo "Setup Vim...done"
}

# -- git setup --

function setup_git() {
    echo "Setup Git..."

    local user={{git.user}}
    local email={{git.email}}

    if ! grep -q "name" "$HOME/.gitconfig"; then
        echo "Enter your git username: "
        git config --global user.name "$user" || throw GitError
    fi

    if ! grep -q "email" "$HOME/.gitconfig"; then
        echo "Enter your git email: "
        git config --global user.email "$email" || throw GitError
    fi

    echo "Setup Git...done"
}

# -- main --

setup_zsh
setup_rust
setup_vim
# add new setup here

echo "User setup complete"

