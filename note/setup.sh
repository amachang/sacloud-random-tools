#! /bin/bash
#@sacloud-once
#@sacloud-apikey required permission=create AK "API Key"
#@sacloud-text required shellarg server_id "Server ID"
#@sacloud-text required shellarg zone "Zone"
#@sacloud-text required shellarg package_list_json "Package list (json)"
#@sacloud-text required shellarg wireguard_interface_private_key "WireGuard Interface Private Key"
#@sacloud-text required shellarg wireguard_interface_address_list_json "WireGuard Interface Address List (json)"
#@sacloud-text required shellarg wireguard_interface_dns_list_json "WireGuard Interface DNS List (json)"
#@sacloud-text required shellarg wireguard_peer_public_key "WireGuard Peer Public Key"
#@sacloud-text required shellarg wireguard_peer_endpoint "WireGuard Peer Endpoint"

# 元々はスクリプトをアップデートして、再起動することで実行することを想定していたが、
# スクリプトをアップデートしても、再起動時に更新されるわけではないようなので毎回やる意味もなさそう
# そのため @sacloud-once で実行するインストールスクリプトとして扱うことにする

_motd() {
    LOG=$(ls /root/.sacloud-api/notes/*log)
    server_id=@@@server_id@@@
    zone=@@@zone@@@
    case $1 in
        start)
            echo -e "\n#-- Startup-script is \\033[0;32mrunning\\033[0;39m. --#\n\nPlease check the log file: ${LOG}\n" > /etc/motd
            curl --user "$SACLOUD_APIKEY_ACCESS_TOKEN:$SACLOUD_APIKEY_ACCESS_TOKEN_SECRET" \
                -X 'PUT' -d '{"Server": {"Tags": ["setup-running"]}}' "https://secure.sakura.ad.jp/cloud/zone/$zone/api/cloud/1.1/server/$server_id"
            ;;
        fail)
            echo -e "\n#-- Startup-script \\033[0;31mfailed\\033[0;39m. --#\n\nPlease check the log file: ${LOG}\n" > /etc/motd
            curl --user "$SACLOUD_APIKEY_ACCESS_TOKEN:$SACLOUD_APIKEY_ACCESS_TOKEN_SECRET" \
                -X 'PUT' -d '{"Server": {"Tags": ["setup-failed"]}}' "https://secure.sakura.ad.jp/cloud/zone/$zone/api/cloud/1.1/server/$server_id"
            exit 1
            ;;
        end)
            curl --user "$SACLOUD_APIKEY_ACCESS_TOKEN:$SACLOUD_APIKEY_ACCESS_TOKEN_SECRET" \
                -X 'PUT' -d '{"Server": {"Tags": ["setup-done"]}}' "https://secure.sakura.ad.jp/cloud/zone/$zone/api/cloud/1.1/server/$server_id"
            cp -f /dev/null /etc/motd
            ;;
    esac
}

set -eux
trap '_motd fail' ERR

#-- ensure packages --

function ensure_packages() {
    echo "Ensure packages..."

    # needrestart の interactive モードを無効化
    if ! [ -f /etc/needrestart/conf.d/50-autorestart.conf ]; then
        echo "\$nrconf{restart} = 'a';" >> /etc/needrestart/conf.d/50-autorestart.conf
    fi

    apt-get update
    apt-get install -y software-properties-common
    add-apt-repository -y ppa:neovim-ppa/stable

    apt-get update
    apt-get install -y jq coreutils openresolv lua5.4 neovim wireguard build-essential

    package_len=$(echo @@@package_list_json@@@ | jq -r 'length')
    if [ "$package_len" -gt 0 ]; then
        echo @@@package_list_json@@@ | jq -r '.[]' | xargs apt-get install -y
    fi

    echo "Ensure packages...done"
}

# -- rustup setup --

function setup_rust() {
    echo "Setup rust..."

    tmp_script=$(mktemp)
    cat <<EOF >"$tmp_script"
        set -eux

        if ! [ -d "\$HOME/.cargo" ]; then
            curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        fi

        if ! grep -q ".cargo/env" "\$HOME/.bashrc"; then
            echo "source \$HOME/.cargo/env" >> "\$HOME/.bashrc"
        fi

        source "\$HOME/.cargo/env"

        rustup update
        rustup default stable
EOF
    chmod +x "$tmp_script"
    sudo chown ubuntu:ubuntu "$tmp_script"
    sudo -i -u ubuntu sh -c "bash $tmp_script"
    rm -f "$tmp_script"

    echo "Setup rust...done"
}

# -- setup developement environment --

function setup_development_environment() {
    echo "Setup development environment..."

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

call plug#end()

EOF
    sudo chown ubuntu:ubuntu "$tmp_init_vim"

    tmp_script=$(mktemp)
    cat <<EOF >"$tmp_script"
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
    chmod +x "$tmp_script"
    sudo chown ubuntu:ubuntu "$tmp_script"
    sudo -i -u ubuntu sh -c "bash $tmp_script"
    rm -f "$tmp_script"

    rm -f "$tmp_init_vim"

    echo "Setup development environment...done"
}

# -- wireguard setup --

function setup_wireguard() {
    echo "Setup WireGuard..."

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

    # ssh 用のルーティングテーブルを作成
    if ! grep -q "2 ssh" /etc/iproute2/rt_tables; then
        echo "2 ssh" >> /etc/iproute2/rt_tables
    fi

    # WireGuard の設定ファイルを作成
    cat <<EOF >/etc/wireguard/wg0.conf 
[Interface]
PrivateKey = $wireguard_interface_private_key
Address = $wireguard_interface_address_list
DNS = $wireguard_interface_dns_list
MTU = 1280

PostUp = ip route add default via $default_gateway dev $interface table ssh
PostUp = ip rule add fwmark 0x2 table ssh
PostUp = /sbin/iptables -A OUTPUT -t mangle -o wg0 -p tcp --sport 22 -j MARK --set-mark 2

PreDown = /sbin/iptables -D OUTPUT -t mangle -o wg0 -p tcp --sport 22 -j MARK --set-mark 2 || true
PreDown = ip rule del fwmark 0x2 table ssh || true
PreDown = ip route del default via $default_gateway dev $interface table ssh || true

[Peer]
PublicKey = $wireguard_peer_public_key
Endpoint = $wireguard_peer_endpoint:51820
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

        wait_loop_count=0
        while true; do
            if ! systemctl is-active --quiet wg-quick@wg0.service; then
                break
            fi
            echo "Waiting for WireGuard interface wg0 to be down..."
            sleep 1

            wait_loop_count=$((wait_loop_count + 1))
            if [ "$wait_loop_count" -gt 30 ]; then
                echo "Timeout: Failed to stop WireGuard service"
                false
            fi
        done
    fi

    echo "Disable auto start and stop WireGuard for update...done"
}

# -- enable wireguard --

function enable_auto_start_wireguard() {
    echo "Enable auto start WireGuard..."

    if ! systemctl is-enabled --quiet wg-quick@wg0.service; then
        systemctl enable wg-quick@wg0.service
    fi

    if ! systemctl is-active --quiet wg-quick@wg0.service; then
        systemctl start wg-quick@wg0.service
    fi

    wait_loop_count=0
    while true; do
        if systemctl is-active --quiet wg-quick@wg0.service; then
            break
        fi
        echo "Waiting for WireGuard interface wg0 to be up..."
        sleep 1

        wait_loop_count=$((wait_loop_count + 1))
        if [ "$wait_loop_count" -gt 30 ]; then
            echo "Timeout: Failed to start WireGuard service"
            false
        fi
    done

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

