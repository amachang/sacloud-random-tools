#! /bin/bash

set -x

if ! apt update; then
    echo "Error: apt update failed"
    exit 1
fi

if ! apt install -y zsh; then
    echo "Error: apt install zsh failed"
    exit 1
fi

touch /home/ubuntu/setup-log.txt
touch /home/ubuntu/setup-error.txt

chown ubuntu:ubuntu /home/ubuntu/setup-log.txt
chown ubuntu:ubuntu /home/ubuntu/setup-error.txt

if [ -f /home/ubuntu/root-setup.zsh ]; then
    if zsh /home/ubuntu/root-setup.zsh > /home/ubuntu/setup-log.txt 2> /home/ubuntu/setup-error.txt; then
        echo "Error: root-setup.zsh failed"
        exit 1
    fi
fi

exit $?

