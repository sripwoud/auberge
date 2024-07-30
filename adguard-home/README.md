# Deploy AdGuard Home on [Fly.io](https://fly.io)

## Requirements
- [flyctl](https://fly.io/docs/hands-on/install-flyctl/)
- create a persistent volume to mount the `/opt/adguardhome` directory on.  
  `region` must be one of `fly platform regions`
  ```commandline
  fly volume create -a adg_data -r region -s 1
  ```

## Deploy
```commandline
flyctl launch
```

You don't need neither Postgres nor Redis.  
But accept the creation of a dedicated IPv4 address as this is [required to be able to expose multiple ports](https://fly.io/docs/js/the-basics/listening-ports/#exposing-multiple-ports).
> If you want your port other than 80/443 to be available on IPv4 you will need to allocate a dedicated IPv4 address.

Or you can run later:
```commandline
flyctl ips allocate-v4 -a adg-home
```