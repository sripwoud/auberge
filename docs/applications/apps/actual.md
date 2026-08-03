# Actual Budget

Local-first budgeting with EU bank sync. Auberge deploys the sync server (`@actual-app/sync-server`) — a sync relay plus encrypted blob store; every client keeps a full local copy of the budget. Docs: [actualbudget.org](https://actualbudget.org)

- **URL**: tailnet only — see [Tailnet-only apps](cli-reference/dns/set-all.md#tailnet-only-apps) (default subdomain: `actual`, override with `actual_subdomain`)
- **Port**: internal (Caddy proxy)
- **Data**: `/var/lib/actual/` — `server-files/account.sqlite` (server accounts, file registry) + `user-files/` (budget blobs)
- **Pinned version**: 26.8.0 (Node.js 22 from NodeSource; Debian trixie ships Node 20, sync-server requires >=22)

## Deploy

```bash
auberge deploy actual
```

No config keys required.

## First visit

1. Open `https://{actual_subdomain}.{domain}` from a tailnet device and set the server password — the first visitor claims the server (Actual's own onboarding; safe because the vhost is tailnet-only).
2. Create or import a budget; add the same server URL + password on other devices to sync.
3. Bank sync: More → Bank Sync → Set up Enable Banking (Application ID + credential file from [enablebanking.com](https://enablebanking.com/cp/applications)).

## Notes

?> Backed up by default (unit stopped, `/var/lib/actual` rsynced). Losing the server does not lose budgets — clients hold full copies and re-upload — but it does lose the server password and Enable Banking credentials. See [Backup & Restore](backup-restore/overview.md).

?> With end-to-end encryption enabled (Settings → Encryption, client-side key), budget blobs and sync messages are ciphertext on disk and in restic snapshots. Enable Banking credentials in `account.sqlite` stay server-readable by design — the server performs the bank pulls. See [ADR-0016](https://github.com/sripwoud/auberge/blob/master/meta/adr/0016-actual-bare-metal-npm-enable-banking.md).
