#!/usr/bin/bash

set -euo pipefail

ask_input() {
  local input="$1"

  while true; do
    printf "%s" "Enter $input: "
    read -r input
    if [ -n "$input" ]; then
      echo "$input"
      break
    else
      echo "$input cannot be empty"
    fi
  done
}

ask_ssh_port() {
  local ssh_port

  while true; do
    printf "%s" "Enter ssh port: "
    read -r ssh_port
    if [[ -n $ssh_port ]]; then
      if [[ $ssh_port = 22 ]]; then
        echo "Port 22 is the default port for SSH. Please choose a different port."
        continue
      fi
      if [[ $ssh_port -lt 1024 ]]; then
        echo "Ports below 1024 are reserved. Please choose a port above 1024."
        continue
      fi
      if [[ $ssh_port -gt 65535 ]]; then
        echo "Ports above 65535 are invalid. Please choose a port below 65535."
        continue
      fi
      if [[ $ssh_port =~ ^[0-9]+$ ]]; then
        echo "$ssh_port"
        break
      else
        echo "Invalid port number"
      fi
    else
      echo "Port cannot be empty"
    fi
  done
}

main() {
  local ssh_port
  ssh_port=$(ask_input "ssh port")

  local ssh_public_key_file
  ssh_public_key_file=$(ask_input "ssh public key file path")

  local wg_port
  wg_port=$(ask_input "WireGuard port")

  local username
  username=$(ask_input "username")

  local server_ip
  server_ip=$(ask_input "server ip")

  ssh "root@$server_ip" 'bash -s' <"create_sudo_user.sh $username"
  ssh-copy-id -p "$ssh_public_key_file" "$username@$server_ip"
  ssh "root@$server_ip" 'bash -s' <"setup_ssh_ufw_wg.sh $ssh_port $wg_port" 2>&1 | tee -a "output.log"
}

main
