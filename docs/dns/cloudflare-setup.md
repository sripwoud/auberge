# Cloudflare Setup

Auberge uses Cloudflare for DNS management and DNS-01 ACME certificate provisioning.

## Required token permissions

| Permission  | Level |
| ----------- | ----- |
| Zone → DNS  | Edit  |
| Zone → Zone | Read  |

Scope the token to a specific zone — not all zones.

## Create the API token

In the Cloudflare dashboard, go to My Profile → API Tokens → Create Token and use the "Edit zone DNS" template. See [Cloudflare's token docs](https://developers.cloudflare.com/fundamentals/api/get-started/create-token/) for step-by-step UI instructions.

Copy the token immediately — it is shown only once.

## Store the token

```bash
auberge config set cloudflare_dns_api_token YOUR_TOKEN
auberge config set domain example.com
```

## Verify

```bash
auberge dns list
```

A successful response lists configured subdomains for your domain. DNS records for app subdomains are created automatically on first deployment — you do not need to create them manually.

## Troubleshooting

| Error                    | Cause                         | Fix                                     |
| ------------------------ | ----------------------------- | --------------------------------------- |
| Authentication error     | Invalid or expired token      | Recreate token, re-run `config set`     |
| Zone not found           | Wrong domain in config        | `auberge config set domain example.com` |
| Insufficient permissions | Missing Zone:Read or DNS:Edit | Recreate token with correct permissions |
