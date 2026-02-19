---
title: Headless Deployment
---

Quick deployment patterns for running `tandem-engine` as a background service.

## Docker (Simple)

```dockerfile
FROM debian:bookworm-slim

RUN useradd -r -s /usr/sbin/nologin tandem
COPY tandem-engine /usr/local/bin/tandem-engine

USER tandem
WORKDIR /home/tandem
VOLUME ["/data"]

ENV TANDEM_STATE_DIR=/data
EXPOSE 39731

ENTRYPOINT ["tandem-engine", "serve", "--hostname", "0.0.0.0", "--port", "39731", "--web-ui", "--web-ui-prefix", "/admin"]
```

Run:

```bash
docker run -d --name tandem \
  -p 127.0.0.1:39731:39731 \
  -v tandem-data:/data \
  -e TANDEM_API_TOKEN=tk_your_token \
  -e TANDEM_WEB_UI=true \
  tandem-engine:0.3.8
```

Use a reverse proxy/TLS in front of this port for non-local use.

## systemd (Linux)

`/etc/systemd/system/tandem.service`:

```ini
[Unit]
Description=Tandem Engine (Headless)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=tandem
Group=tandem
EnvironmentFile=/etc/tandem/env
ExecStart=/usr/local/bin/tandem-engine serve --hostname 127.0.0.1 --port 39731 --web-ui --web-ui-prefix /admin
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/srv/tandem

[Install]
WantedBy=multi-user.target
```

`/etc/tandem/env`:

```bash
TANDEM_API_TOKEN=tk_your_token
TANDEM_STATE_DIR=/srv/tandem
TANDEM_WEB_UI=true
TANDEM_WEB_UI_PREFIX=/admin
```

Enable/start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable tandem
sudo systemctl start tandem
sudo systemctl status tandem
```

## Verify

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Tandem-Token: tk_your_token"
```

Web admin:

- `http://127.0.0.1:39731/admin`

## Reverse Proxy (TLS)

### Caddy (recommended quick setup)

`/etc/caddy/Caddyfile`:

```caddy
tandem.example.com {
  reverse_proxy 127.0.0.1:39731
}
```

Then reload Caddy:

```bash
sudo systemctl reload caddy
```

Caddy will provision TLS certificates automatically when DNS is configured.

### Nginx (minimal example)

```nginx
server {
  listen 443 ssl http2;
  server_name tandem.example.com;

  ssl_certificate /etc/letsencrypt/live/tandem.example.com/fullchain.pem;
  ssl_certificate_key /etc/letsencrypt/live/tandem.example.com/privkey.pem;

  location / {
    proxy_pass http://127.0.0.1:39731;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
  }
}
```

### Cloudflare Tunnel (no public port)

Run a tunnel to the local Tandem service:

```bash
cloudflared tunnel --url http://127.0.0.1:39731
```

For a named tunnel (recommended for persistent setup), create a tunnel and map
your hostname in Cloudflare DNS, then run it as a service:

```bash
cloudflared tunnel create tandem
cloudflared tunnel route dns tandem tandem.example.com
cloudflared tunnel run tandem
```

## Notes

- Keep token auth enabled (`TANDEM_API_TOKEN`).
- Terminate TLS at a reverse proxy if exposed beyond localhost.
- For channel features, set channel env vars or config values in state config.
