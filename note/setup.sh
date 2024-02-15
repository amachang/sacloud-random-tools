#! /bin/bash

set -x

if ! command -v zsh > /dev/null; then

    # 時々、 DNS 解決が遅れることがあるため、いろいろ確認
    while ! nslookup -timeout=1 -type=A archive.ubuntu.com > /dev/null; do
        echo "Waiting for DNS resolution of archive.ubuntu.com..."
        sleep 5
    done

    while ! nc -zv archive.ubuntu.com 80 > /dev/null; do
        echo "Waiting for connection to archive.ubuntu.com port 80..."
        sleep 5
    done

    while ! curl -s --head http://archive.ubuntu.com/ > /dev/null; do
        echo "Waiting for response from archive.ubuntu.com..."
        sleep 5
    done

    if ! apt update; then
        echo "Error: apt update failed"
        exit 1
    fi

    if ! apt install -y zsh; then
        echo "Error: apt install zsh failed"
        exit 1
    fi
fi

touch /home/ubuntu/setup-log.txt
touch /home/ubuntu/setup-error.txt

chown ubuntu:ubuntu /home/ubuntu/setup-log.txt
chown ubuntu:ubuntu /home/ubuntu/setup-error.txt

if [ -f /home/ubuntu/root-setup.zsh ]; then
    if ! zsh /home/ubuntu/root-setup.zsh > /home/ubuntu/setup-log.txt 2> /home/ubuntu/setup-error.txt; then
        echo "Error: root-setup.zsh failed"
        exit 1
    fi
    rm /home/ubuntu/root-setup.zsh
fi

exit 0

