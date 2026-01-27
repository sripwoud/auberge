# fail2ban

Intrusion prevention system that protects against brute-force attacks by banning suspicious IPs.

Official Documentation: [https://github.com/fail2ban/fail2ban](https://github.com/fail2ban/fail2ban)

## Deployment

```bash
auberge ansible run --tags fail2ban
```

## Configuration

Auberge configures fail2ban to monitor:

- SSH login attempts
- Caddy access logs
- Other service logs

## Related

- [Applications Overview](../overview.md)
