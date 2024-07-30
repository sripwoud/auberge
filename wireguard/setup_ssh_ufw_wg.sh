#!/usr/bin/bash

set -euo pipefail

backup_file() {
  local file="$1"
  [ -f "$file" ] && cp "$file" "$file.bak"
}

update() {
  apt update
  apt upgrade -y
}

config_ufw() {
  ssh_port="$1"
  wg_port="$2"
  command -v ufw >/dev/null 2>&1 || apt install ufw


  ufw default deny incoming comment 'deny all incoming traffic'
  ufw default allow outgoing comment 'allow all outgoing traffic'
  ufw allow OpenSSH comment 'allow ssh connections'
  ufw allow "$ssh_port"/tcp comment 'allow ssh connections'
  ufw allow 80/tcp comment 'allow http connections'
  ufw allow 443/tcp comment 'allow https connections'
  ufw allow proto udp to any port 443 comment 'allow QUIC'
  ufw allow proto udp to any port "$wg_port" comment 'allow WireGuard'
  ufw enable
  ufw reload
}

config_editor() {
  update-alternatives --config editor
}


config_ssh() {
  ssh_port="$1"
  file="/etc/ssh/sshd_config"
  backup_file "$file"

  cat <<EOF | tee -a /etc/ssh/sshd_config.d/00-hardening.conf >/dev/null
PermitRootLogin no
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM yes
X11Forwarding no
PrintMotd no
AcceptEnv LANG LC_*
Subsystem sftp /usr/lib/openssh/sftp-server
EOF
  echo "Port $ssh_port" >>/etc/ssh/sshd_config
}

# https://github.com/angristan/wireguard-install
install_wireguard() {
  curl -O https://raw.githubusercontent.com/angristan/wireguard-install/master/wireguard-install.sh
  chmod +x wireguard-install.sh
  ./wireguard-install.sh
}

print_end_message() {
  ssh_port="$1"
  wg_port="$2"
  echo "Installation complete. You may need to manually open port $ssh_port and $wg_port in your cloud provider's firewall settings."
  echo "You can now connect to your server using the following command: ssh -p $ssh_port <username>@<ip>"
  echo "Rebooting now"
}

maybe_reboot() {
  read -rp "Reboot now? [y/N] " response
  if [[ "$response" =~ ^([yY][eE][sS]|[yY])$ ]]; then
    reboot
  fi
}

main() {
  ssh_port="$1"
  wg_port="$2"

  update
  config_editor

  config_ssh "$ssh_port"
  config_ufw "$ssh_port" "$wg_port"
  install_wireguard
  print_end_message "$ssh_port" "$wg_port"
}

main "$@"
