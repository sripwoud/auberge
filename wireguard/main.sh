#!/usr/bin/bash

main() {
  local BASE_URL="https://raw.githubusercontent.com/sripwoud/wireguard-install/main/"
  temp_dir=$(mktemp -d)

  for file in "install.sh" "create_sudo_user.sh" "setup_ssh_ufw_wg.sh"; do
    wget -qO "$temp_dir/$file" "$BASE_URL$file"
  done

  chmod +x "$temp_dir/install.sh"

  "$temp_dir/install.sh"

  rm -rf "$temp_dir"
}

main
