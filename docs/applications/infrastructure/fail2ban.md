# fail2ban

Intrusion prevention system that protects against brute-force attacks by banning suspicious IPs.

Official Documentation: [https://github.com/fail2ban/fail2ban](https://github.com/fail2ban/fail2ban)

## Deployment

```bash
auberge ansible run --tags fail2ban
```

## Configuration

Auberge configures fail2ban with three jails:

| Jail        | Purpose                            | Backend           |
| ----------- | ---------------------------------- | ----------------- |
| `sshd`      | Brute-force SSH login protection   | systemd journal   |
| `sshd-ddos` | SSH connection flooding protection | systemd journal   |
| `recidive`  | Repeat offender escalation         | fail2ban log file |

SSH jails use `backend = systemd` with `journalmatch` to read from the systemd journal, as modern Debian does not write to `/var/log/auth.log`.

Default thresholds are defined in `ansible/roles/fail2ban/defaults/main.yml`.

## Related

- [Applications Overview](../overview.md)
