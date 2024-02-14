#!/bin/zsh

set -x

autoload -Uz catch
autoload -Uz throw

#-- ensure packages --

function ensure_packages() {
    echo "Ensure packages..."

    # needrestart の interactive モードを無効化
    if ! [[ -f /etc/needrestart/conf.d/50-autorestart.conf ]]; then
        mkdir -p /etc/needrestart/conf.d || throw AptError
        echo "\$nrconf{restart} = 'a';" >> /etc/needrestart/conf.d/50-autorestart.conf || throw AptError
    fi

    apt-get update || throw AptError
    apt-get install -y software-properties-common || throw AptError
    add-apt-repository -y ppa:neovim-ppa/stable || throw AptError

    apt-get update || throw AptError
    apt-get install -y jq coreutils openresolv lua5.4 neovim wireguard build-essential || throw AptError

    echo "Ensure packages...done"
}

# -- setup user --

function setup_user() {
    echo "Setup user..."

    if [ -f /home/ubuntu/user-setup.zsh ]; then
        sudo -i -u ubuntu zsh /home/ubuntu/user-setup.zsh || throw UserSetupError
    fi

    echo "Setup user...done"
}

# -- wireguard setup --

function setup_wireguard() {
    echo "Setup WireGuard..."

    # WireGuard の情報を取得
    local wireguard_interface_private_key={{wireguard.interface.private_key}}
    local -a wireguard_interface_address_list={{wireguard.interface.address}}
    local -a wireguard_interface_dns_list={{wireguard.interface.dns}}
    local wireguard_peer_public_key={{wireguard.peer.public_key}}
    local wireguard_peer_endpoint={{wireguard.peer.endpoint}}

    # デフォルトゲートウェイとインターフェースを取得
    local route_info=$(ip route show table main | grep default || throw WireGuardError)
    local default_gateway=$(echo "$route_info" | awk '{print $3}' || throw WireGuardError)
    local interface=$(echo "$route_info" | awk '{print $5}' || throw WireGuardError)

    # ssh 用のルーティングテーブルを作成
    if ! grep -q "2 ssh" /etc/iproute2/rt_tables; then
        echo "2 ssh" >> /etc/iproute2/rt_tables || throw WireGuardError
    fi

    # WireGuard の設定ファイルを作成
    cat <<EOF >/etc/wireguard/wg0.conf || throw WireGuardError
[Interface]
PrivateKey = $wireguard_interface_private_key
Address = ${(j:,:)wireguard_interface_address_list}
DNS = ${(j:,:)wireguard_interface_dns_list}
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
    chmod 600 /etc/wireguard/wg0.conf || throw WireGuardError

    echo "Setup WireGuard...done"
}

# -- disable if wireguard up --

function disable_auto_start_and_stop_wireguard_for_update() {
    echo "Disable auto start and stop WireGuard for update..."

    # WireGuardコマンドが存在するか確認
    if command -v wg-quick > /dev/null; then
        # WireGuardサービスが有効化されているか確認し、一時的に無効化
        if systemctl is-enabled --quiet wg-quick@wg0.service; then
            systemctl disable wg-quick@wg0.service || throw WireGuardStopError
        fi

        # WireGuardサービスが実行中の場合は停止
        if systemctl is-active --quiet wg-quick@wg0.service; then
            systemctl stop wg-quick@wg0.service || throw WireGuardStopError

            local -i wait_loop_count=0
            while true; do
                if ! systemctl is-active --quiet wg-quick@wg0.service; then
                    break
                fi
                echo "Waiting for WireGuard interface wg0 to be down..."
                sleep 1

                ((wait_loop_count += 1))
                if (( wait_loop_count > 30 )); then
                    echo "Timeout: Failed to stop WireGuard service"
                    throw WireGuardStopTimeoutError
                fi
            done
        fi
    fi

    echo "Disable auto start and stop WireGuard for update...done"
}

# -- enable wireguard --

function enable_auto_start_wireguard() {
    echo "Enable auto start WireGuard..."

    command -v systemctl > /dev/null || throw WireGuardStartError

    if ! systemctl is-enabled --quiet wg-quick@wg0.service; then
        systemctl enable wg-quick@wg0.service || throw WireGuardStartError
    fi

    if ! systemctl is-active --quiet wg-quick@wg0.service; then
        systemctl start wg-quick@wg0.service || throw WireGuardStartError

        wait_loop_count=0
        while true; do
            if systemctl is-active --quiet wg-quick@wg0.service; then
                break
            fi
            echo "Waiting for WireGuard interface wg0 to be up..."
            sleep 1

            ((wait_loop_count += 1))
            if ((wait_loop_count > 30)); then
                echo "Timeout: Failed to start WireGuard service"
                throw WireGuardStartTimeoutError
            fi
        done
    fi

    echo "Enable auto start WireGuard...done"
}

# -- main --

{
    if [[ -f /home/ubuntu/root_setup_not_yet_started_once ]]; then
        rm /home/ubuntu/root_setup_not_yet_started_once
    fi

    disable_auto_start_and_stop_wireguard_for_update

    ensure_packages
    setup_user
    # add new setup here

    setup_wireguard
} always {
    enable_auto_start_wireguard

    if [[ -f /home/ubuntu/root_setup_not_yet_finished_once ]]; then
        rm /home/ubuntu/root_setup_not_yet_finished_once
    fi

    if catch '*'; then
        echo "Setup Error: $e"

        exit 1
    else
        echo "Setup complete"

        if [[ -f /home/ubuntu/root_setup_not_yet_success_once ]]; then
            rm /home/ubuntu/root_setup_not_yet_success_once
        fi
        exit 0
    fi
}

