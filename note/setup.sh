#! /bin/bash
#@sacloud-text required shellarg package_list_json "Package list (json)"
#@sacloud-text required shellarg wireguard_interface_private_key "WireGuard Interface Private Key"
#@sacloud-text required shellarg wireguard_interface_address_list_json "WireGuard Interface Address List (json)"
#@sacloud-text required shellarg wireguard_interface_dns_list_json "WireGuard Interface DNS List (json)"
#@sacloud-text required shellarg wireguard_peer_public_key "WireGuard Peer Public Key"
#@sacloud-text required shellarg wireguard_peer_endpoint "WireGuard Peer Endpoint"

_motd() {
    LOG=$(ls /root/.sacloud-api/notes/*log)
    case $1 in
        start)
            echo -e "\n#-- Startup-script is \\033[0;32mrunning\\033[0;39m. --#\n\nPlease check the log file: ${LOG}\n" > /etc/motd
            ;;
        fail)
            echo -e "\n#-- Startup-script \\033[0;31mfailed\\033[0;39m. --#\n\nPlease check the log file: ${LOG}\n" > /etc/motd
            exit 1
            ;;
        end)
            cp -f /dev/null /etc/motd
            ;;
    esac
}

set -eux
trap '_motd fail' ERR

#-- ensure packages --

function ensure_packages() {
    echo "Ensure packages..."

    apt-get update
    apt-get install -y jq coreutils

    package_len=$(echo @@@package_list_json@@@ | jq -r 'length')
    if [ "$package_len" -gt 0 ]; then
        echo @@@package_list_json@@@ | jq -r '.[]' | xargs apt-get install -y
    fi

    echo "Ensure packages...done"
}

# -- rustup setup --

function setup_rust() {
    echo "Setup rust..."

    sudo -i -u ubuntu sh -c '
        set -eux

        if ! [ -d "$HOME/.cargo" ]; then
            curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        fi

        if ! grep -q ".cargo/env" "$HOME/.bashrc"; then
            echo "source $HOME/.cargo/env" >> "$HOME/.bashrc"
        fi

        source "$HOME/.cargo/env"

        rustup update
        rustup default stable
    '

    echo "Setup rust...done"
}

# -- setup developement environment --

function setup_development_environment() {
    echo "Setup development environment..."

    # NeoVim
    apt-get install -y neovim

    tmp_init_vim=$(mktemp)

    cat <<EOF >"$tmp_init_vim"
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

" nu
Plug 'nvim-lua/plenary.nvim'
Plug 'jose-elias-alvarez/null-ls.nvim'
Plug 'LhKipp/nvim-nu', {'do': ':TSInstall nu'}

"copilot
Plug 'github/copilot.vim'

call plug#end()


"" plugin settings

lua require'nvim-treesitter.configs'.setup{highlight={enable=true}}
EOF

    sudo -i -u ubuntu sh -c <<EOF
        set -eux

        curl -fLo "\$HOME/.local/share/nvim/site/autoload/plug.vim" --create-dirs \
            https://raw.githubusercontent.com/junegunn/vim-plug/master/plug.vim

        if ! [ -d "\$HOME/.config/nvim" ]; then
            mkdir -p "\$HOME/.config/nvim"
        fi

        cat "$tmp_init_vim" > "\$HOME/.config/nvim/init.vim"

        nvim --headless +PlugInstall +qall

        if ! grep -q "vim=nvim" "\$HOME/.bashrc"; then
            echo "alias vim=nvim" >> "\$HOME/.bashrc"
        fi
EOF

    rm -f "$tmp_init_vim"

    echo "Setup development environment...done"
}

# -- wireguard setup --

function setup_wireguard() {
    echo "Setup WireGuard..."

    apt-get install -y wireguard

    # デフォルトゲートウェイとインターフェースを取得
    route_info=$(ip route show table main | grep default)
    default_gateway=$(echo "$route_info" | awk '{print $3}')
    interface=$(echo "$route_info" | awk '{print $5}')

    # WireGuard の情報を取得
    wireguard_interface_private_key=@@@wireguard_interface_private_key@@@
    wireguard_interface_address_list=$(echo @@@wireguard_interface_address_list_json@@@ | jq -r '.[]' | tr '\n' ', ')
    wireguard_interface_dns_list=$(echo @@@wireguard_interface_dns_list_json@@@ | jq -r '.[]' | tr '\n' ', ')
    wireguard_peer_public_key=@@@wireguard_peer_public_key@@@
    wireguard_peer_endpoint=@@@wireguard_peer_endpoint@@@

    # WireGuard の設定ファイルを作成
    cat <<EOF >/etc/wireguard/wg0.conf 
[Interface]
PrivateKey = $wireguard_interface_private_key
Address = $wireguard_interface_address_list
DNS = $wireguard_interface_dns_list
MTU = 1280

PostUp = ip route add 10.0.0.0/8 via $default_gateway dev $interface
PostUp = ip route add 172.16.0.0/12 via $default_gateway dev $interface
PostUp = ip route add 192.168.0.0/16 via $default_gateway dev $interface
PostUp = ip route add default via $default_gateway dev $interface table ssh
PostUp = ip rule add fwmark 0x2 table ssh
PostUp = /sbin/iptables -A OUTPUT -t mangle -o wg0 -p tcp --sport 22 -j MARK --set-mark 2

PreDown = /sbin/iptables -D OUTPUT -t mangle -o wg0 -p tcp --sport 22 -j MARK --set-mark 2
PreDown = ip rule del fwmark 0x2 table ssh
PreDown = ip route del default via $default_gateway dev $interface table ssh
PreDown = ip route del 10.0.0.0/8 via $default_gateway dev $interface
PreDown = ip route del 172.16.0.0/12 via $default_gateway dev $interface
PreDown = ip route del 192.168.0.0/16 via $default_gateway dev $interface

[Peer]
PublicKey = $wireguard_peer_public_key
Endpoint = $wireguard_peer_endpoint
PersistentKeepalive = 25
AllowedIPs = 0.0.0.0/0, ::/0
EOF
    chmod 600 /etc/wireguard/wg0.conf

    echo "Setup WireGuard...done"
}

# -- disable if wireguard up --

function disable_auto_start_and_stop_wireguard_for_update() {
    echo "Disable auto start and stop WireGuard for update..."

    # WireGuardコマンドが存在するか確認
    if command -v wg-quick > /dev/null; then
        # WireGuardサービスが有効化されているか確認し、一時的に無効化
        if systemctl is-enabled --quiet wg-quick@wg0.service; then
            systemctl disable wg-quick@wg0.service
        fi

        # WireGuardサービスが実行中の場合は停止
        if systemctl is-active --quiet wg-quick@wg0.service; then
            systemctl stop wg-quick@wg0.service
        fi

        # 念の為確認
        if systemctl is-active --quiet wg-quick@wg0.service; then
            echo "WireGuard interface wg0 is still up. Please check the status."
            exit 1
        fi
        if ip link show wg0 > /dev/null 2>&1; then
            echo "WireGuard interface wg0 is still up. Please check the status."
            exit 1
        fi
    fi

    echo "Disable auto start and stop WireGuard for update...done"
}

# -- enable wireguard --

function enable_auto_start_wireguard() {
    echo "Enable auto start WireGuard..."

    if ! systemctl is-active --quiet wg-quick@wg0.service; then
        systemctl enable wg-quick@wg0.service
    fi

    if ! systemctl is-active --quiet wg-quick@wg0.service; then
        systemctl start wg-quick@wg0.service
    fi

    echo "Enable auto start WireGuard...done"
}

# -- main --

_motd start

disable_auto_start_and_stop_wireguard_for_update
ensure_packages
setup_rust
setup_development_environment
setup_wireguard
enable_auto_start_wireguard

_motd end

exit 0

