#!/usr/bin/bash

create_sudo_user() {
  local username="$1"
  adduser --gecos "" "$username"
  usermod -aG sudo "$username"
}

main() {
  create_sudo_user "$1"
}
