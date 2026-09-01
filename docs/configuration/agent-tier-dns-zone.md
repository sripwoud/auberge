# Agent Tier DNS Zone

The AI agent fleet ([ruche](https://github.com/sripwoud/auberge/issues/747)) answers ACME's DNS-01 challenge with a Cloudflare API token scoped to its own domain, never the token that guards the parent domain ([ADR-0054](https://github.com/sripwoud/auberge/blob/master/meta/adr/0054-agent-workloads-run-on-a-dedicated-disposable-host.md), [ADR-0068](https://github.com/sripwoud/auberge/blob/master/meta/adr/0068-the-agent-tier-holds-its-own-dns-zone.md)):

- **Blast-radius containment** — a Cloudflare token is zone-scoped, not record-scoped. A token able to complete DNS-01 for the parent domain can rewrite MX and every other record in that zone; a token scoped to a separate domain can only touch the agent tier.
- **`ruche` is assumed compromisable** (ADR-0054) — the token it holds must cost only the agent tier if it leaks.

!> The agent tier's domain is a **separate registered domain, not a subdomain** of the parent domain. Cloudflare only supports delegating a subdomain as its own independently-managed zone on Enterprise plans; on Free/Pro the dashboard refuses it outright ("provide the root domain, not a subdomain"). A second domain sidesteps that and reuses the existing Cloudflare integration unchanged.

!> `essaim.{agents_domain}` becomes the aoe dashboard's origin (#740). An installed PWA binds to its exact origin, so settle the domain before the first install — changing it later forces a delete-and-reinstall on every device.

## Config keys

| Key                               | Secret | Holds                                                  |
| --------------------------------- | ------ | ------------------------------------------------------ |
| `agents_domain`                   | no     | the agent tier's own domain, e.g. `agents-example.com` |
| `agents_cloudflare_dns_api_token` | yes    | API token scoped to the `agents_domain` zone only      |

## Provisioning

Getting a domain onto Cloudflare and minting a scoped token both sit behind an interactive Cloudflare login — those steps are the checklist below.

1. **Get the domain onto Cloudflare.** Either transfer an existing registration in (Cloudflare dashboard → Domains → Add a site → Transfer a domain) or keep it registered elsewhere and just point its nameservers at Cloudflare (Add a site → Connect a domain). Either way it must be a root domain — Cloudflare rejects a subdomain here. Wait for the zone to show **Active**.

2. **Mint the token**, scoped to the new zone only:

   | Permission  | Level |
   | ----------- | ----- |
   | Zone → DNS  | Edit  |
   | Zone → Zone | Read  |

   My Profile → API Tokens → Create Token → "Edit zone DNS" template, with **Zone Resources** narrowed to the single `agents_domain` zone. Copy the token immediately — it is shown only once.

3. **Store both values:**

   ```bash
   auberge config set agents_domain agents-example.com
   auberge config set agents_cloudflare_dns_api_token YOUR_TOKEN
   ```

4. **Verify** the token authenticates and is scoped to exactly the one zone:

   ```bash
   curl -s -H "Authorization: Bearer YOUR_TOKEN" \
     https://api.cloudflare.com/client/v4/zones | jq -r '.result[].name'
   # expect exactly: agents-example.com
   ```

5. `ruche`'s meta role (#743) resolves `agents_cloudflare_dns_api_token` at deploy time; Caddy's `caddy_dns_api_token` indirection (#740) is what points the agent tier's Caddy at this token instead of the parent domain's.

## Rotation

Cloudflare tokens carry no forced expiry unless one is set at creation — set a TTL when minting and note the rotation date. To rotate: create a replacement token scoped to `agents_domain` the same way, `auberge config set agents_cloudflare_dns_api_token NEW_TOKEN`, revoke the old token, then redeploy the agent tier once #740 lands.
