Trouw Gordijn (Wedding Curtain)

A small Rust web app to let guests send a short congratulatory message to a WLED-driven LED curtain at your wedding. The app exposes a simple, wedding-themed page with:

- A text field to submit a message (rotates per minute)
- Color picker
- Brown queue window with a live timer for the current item

It reaches the on-premise WLED controller by creating an SSH tunnel via your onsite laptop (x220) connected through Tailscale.

High-level

- Public web app (this service) runs where you deploy it (server/VPS/Heroku/etc.).
- It opens an SSH local port forward to `x220-nixos.tail19d694.ts.net`.
- Traffic to `http://127.0.0.1:<LOCAL_TUNNEL_PORT>` on the server is forwarded over SSH to the WLED device reachable from the x220.
- The app calls WLED’s HTTP/JSON API through that tunnel to update brightness/color and (optionally) display text if your WLED setup supports it.

Requirements

- The server running this app must have `ssh` available and network access to Tailscale.
- Your x220 laptop must be online on Tailscale as `x220-nixos.tail19d694.ts.net` and be able to reach the WLED device on the local network.
- SSH key-based auth from the server to the x220 is recommended (no interactive password prompts). Put the public key in `~/.ssh/authorized_keys` on the x220 user.
- WLED device reachable from the x220 (e.g., `192.168.1.50:80`).
- If you want to show scrolling text on the LEDs, configure your WLED with a preset or a usermod that accepts text via API (see “Text support” below).

Configure

Set environment variables for the service:

- `BIND_HOST` (default `0.0.0.0`) – where to listen
- `BIND_PORT` (default `8080`) – port to listen
- `SSH_HOST` (default `x220-nixos.tail19d694.ts.net`) – Tailscale DNS name of your onsite laptop
- `SSH_USER` (optional) – user on the x220 to SSH as
- `WLED_HOST` (default `127.0.0.1`) – the WLED host as seen from the x220 (e.g., `192.168.1.50`)
- `WLED_PORT` (default `80`) – WLED port
- `LOCAL_TUNNEL_PORT` (default `18080`) – local port on the server for the tunnel
- `TEXT_PRESET_ID` (optional) – WLED preset ID that shows scrolling text (if you configured one)
- `TEXT_PARAM_KEY` (optional) – HTTP param to send text to WLED via `/win`, e.g., `TT` for some text usermods

You can also add a `.env` file in the project root to set these values in development.

Example `.env`:

```
BIND_PORT=8080
SSH_USER=arthur
SSH_HOST=x220-nixos.tail19d694.ts.net
WLED_HOST=192.168.1.50
WLED_PORT=80
LOCAL_TUNNEL_PORT=18080
TEXT_PRESET_ID=12
TEXT_PARAM_KEY=TT
```

Run

Build and run (requires network access to fetch crates):

```
cargo run --release
```

Visit `http://localhost:8080`.

When the app starts, it will supervise an SSH tunnel:

```
ssh -NT -o ExitOnForwardFailure=yes -o ServerAliveInterval=10 -o ServerAliveCountMax=3 \
  -L 127.0.0.1:18080:192.168.1.50:80 arthur@x220-nixos.tail19d694.ts.net
```

Adjust details as per your env vars.

Built-in HTTPS (Let’s Encrypt)

You can enable automatic HTTPS with Let’s Encrypt directly in the app (no reverse proxy required).

- Requires DNS A/AAAA records for your domain pointing to this server, and inbound ports 80 and 443 open.
- Build with the `acme` feature:

```
cargo build --release --features acme
```

- Set these env vars:
  - `ACME_DOMAIN` — the domain to issue a cert for (e.g., `example.com`)
  - `ACME_CONTACT_EMAIL` — optional email for the ACME account (recommended)
  - `ACME_CACHE_DIR` — directory for certificate cache (default `./acme-cache`)

- Run the binary. It will:
  - Serve HTTPS on `:443` with automatic certificates
  - Serve HTTP on `:80` that redirects to HTTPS

Binding low ports on Linux:

Either run as root, or grant the binary the capability to bind privileged ports:

```
sudo setcap 'cap_net_bind_service=+ep' target/release/trouw-gordijn
```

If `ACME_DOMAIN` is not set or the `acme` feature is not enabled, the app serves plain HTTP on `BIND_HOST:BIND_PORT` as before.

Message rotation & queue

- Messages are queued and displayed for 60 seconds each.
- After display, messages are removed from the queue (consumed).
- If there’s only one message, it remains on screen beyond 60s.
- If a new message arrives and the current single message has already run 60s, the display switches to the new one immediately.

Language selection

- The UI supports Dutch, French, and German.
- Use the flag buttons in the top-right to switch; the choice is saved in localStorage.
- Subtitle, labels, submit button, hints, queue title/empty state, and footer are translated.

How messages are sent

- The app first sets brightness/color using WLED’s JSON API: `POST /json/state` with `{ on, bri, seg[0].col }`.
- If `TEXT_PRESET_ID` is set, it switches to that preset (`{"ps": <id>}`) — you should create/configure a preset that renders text on your LED matrix/curtain.
- If `TEXT_PARAM_KEY` is set (e.g., `TT`), the app also calls `/win?TT=<urlencoded text>`. Some WLED text usermods or forks expose such a parameter. If your setup doesn’t support this, leave `TEXT_PARAM_KEY` empty and rely on the preset instead.

Because WLED text capabilities vary (matrix/usermods), you may need to:

1) Install/enable a usermod that supports text input via HTTP parameter; or
2) Use a preset that renders text, and configure how that preset reads text (if supported by your build).

Out of the box (no usermods), the app will reliably set brightness and color. Text display needs compatible WLED setup.

Upload couple photo

- POST `/upload` with a file field named `photo`.
- The server stores it at `uploads/couple.jpg`.
- The homepage displays this image as a hero banner.

Security & safety

- The public page is intentionally simple; rate-limit/protection can be added via proxies (Cloudflare/Nginx) or Axum middleware if needed.
- Consider restricting who can access the page (e.g., via a long unguessable URL embedded in the QR code, or basic auth in front).
- Ensure the server user has only the SSH key needed to access the x220 and nothing more.

Nice-to-haves (future)

- Rate limiting per IP to avoid spammy submissions
- Moderation queue
- Emoji support & glyph mapping for LED matrices
- Live status indicator from WLED
- Multiple presets/effects selection

With love — have a magical wedding and beautiful lights!
