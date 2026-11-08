# Dogitor server — HTTP/WS API + live media

This is the server side of the Dogitor v2 companion app: an `axum` HTTP +
WebSocket API and a **MediaMTX**-based single-camera fan-out for live view and
recording. Telegram is unchanged and still handles alerts.

## Architecture

The macOS camera is single-consumer, so live view and triggered recording must
share **one** capture. Dogitor runs exactly one FFmpeg **publisher** (camera
video + the cpal PCM audio piped on stdin) that publishes H.264/Opus to a local
MediaMTX path. MediaMTX fans that one stream out:

```
cpal (mic, sole owner) ──PCM(stdin)─┐
                                    ▼
camera ──► FFmpeg publisher ──RTSP──► MediaMTX ──► WHEP  (live → app)
        (Rust-owned, on demand)            └─────► FFmpeg copy-reader ──► clip.mp4
```

- **Live**: the app connects via WHEP (WebRTC). Because the phone and server are
  on the same Tailscale tailnet, ICE uses host candidates only — **no STUN/TURN**.
- **Recording**: on a detector trigger, Dogitor spawns a copy-reader that reads
  the already-encoded stream from MediaMTX (`-c:v copy -c:a aac`) and writes one
  MP4 per event. No camera re-open, no video re-encode.
- The publisher is brought up on demand and reaped when idle (see `src/live.rs`).
  The mic is owned solely by cpal; the publisher only ever receives PCM on stdin.

## Prerequisites

The [README](../README.md#prerequisites) prerequisites (Rust, FFmpeg, MediaMTX,
bot + API tokens), plus, for remote live view:

- **Tailscale** on both the server and the phone, joined to the same tailnet.

## Setup

The API and recording work locally with the defaults. For **remote live view**
(phone on the tailnet) two settings must point at this machine's tailnet
address. Which files you edit depends on how you run Dogitor (see the
[README](../README.md#two-ways-to-run-it)):

| Setting                   | Mac app (`Dogitor.app`)                                   | Headless CLI          |
| ------------------------- | --------------------------------------------------------- | --------------------- |
| API token, `whep_url`     | Settings tab (→ `~/Library/Application Support/Dogitor/config.toml`) | `config.toml` / `.env` |
| `webrtcAdditionalHosts`   | `~/Library/Application Support/Dogitor/mediamtx.yml`      | `./mediamtx.yml`      |
| Starting MediaMTX         | automatic                                                 | manual (below)        |

1. **API token** — `[api].token` (or `DOGITOR_API_TOKEN`): a long, random,
   URL-safe string. Required to start at all.
2. **`[live].whep_url`** — your server's Tailscale MagicDNS name + WebRTC port,
   e.g. `http://dogitor.your-tailnet.ts.net:8889/cam/whep`. The default
   (`127.0.0.1`) only works on the server itself.
3. **`webrtcAdditionalHosts`** in the relevant `mediamtx.yml` — this machine's
   tailnet IP (`tailscale ip -4`). The shipped value `100.64.0.1` is a
   placeholder. The Mac app installs the repo's `mediamtx.yml` into Application
   Support and leaves your edits alone afterwards, so edit that copy (then quit
   and reopen the app). It only replaces the file when upgrading a pre-auth
   config (keeping your `webrtcAdditionalHosts`, old file saved as
   `mediamtx.yml.bak`), and keeps `authHTTPAddress` matched to `[api].port`.
4. **CLI only** — start MediaMTX, then Dogitor (order doesn't matter — the
   publisher is on-demand, and MediaMTX waits for it):
   ```sh
   mediamtx ./mediamtx.yml
   # in another shell:
   export DOGITOR_API_TOKEN="…"  DOGITOR_BOT_TOKEN="…"   # or use .env
   cargo run --release
   ```

### Access control

Dogitor is reachable only from the server itself and over Tailscale:

| Surface | Listens on | Who gets in |
| ------- | ---------- | ----------- |
| HTTP API `:8088` | all interfaces | localhost + tailnet (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`) **with** the API token. LAN clients get `403` before auth runs. |
| MediaMTX RTSP `:8554` | `127.0.0.1` only (TCP) | localhost: Dogitor's publisher and clip/snapshot readers. |
| MediaMTX WebRTC/WHEP `:8889` (+ ICE UDP `:8189`) | all interfaces | tailnet devices **with** the API token, read-only. |
| Telegram bot | — | the one registered chat (see the README). |

MediaMTX runs with `authMethod: http`: for every publish/read it asks
Dogitor's loopback-only hook, `POST /internal/mediamtx-auth`, which allows
localhost to publish/read, allows tailnet devices to *read* if they present
the API token (`Authorization: Bearer <token>` on the WHEP request, or
`?token=<token>` on the WHEP URL), and denies everything else. If Dogitor
isn't running, the hook is unreachable and MediaMTX denies everything. The
hook URL in `mediamtx.yml` (`authHTTPAddress`) must use Dogitor's `[api].port`.

### macOS permissions

The **camera** is opened by the FFmpeg publisher Dogitor spawns; the
**microphone** is owned by Dogitor itself (cpal). Child processes inherit the
parent's permission, so:

- **Mac app:** approve the prompts for Dogitor — nothing else to do.
- **CLI:** grant Camera and Microphone to the terminal app you launch from
  (System Settings → Privacy & Security).

## API reference

Base URL: `http://<server>:8088` (tailnet or localhost only). Every request
needs the bearer token, via `Authorization: Bearer <token>` or
`?token=<token>` (for WS/media URLs). Shared
TypeScript types live in [`shared/api-types.ts`](../shared/api-types.ts).

| Method | Path                | Body / Query        | Returns |
| ------ | ------------------- | ------------------- | ------- |
| GET    | `/api/status`       | —                   | `StatusResponse` |
| POST   | `/api/arm`          | —                   | `StatusResponse` |
| POST   | `/api/disarm`       | —                   | `StatusResponse` |
| POST   | `/api/threshold`    | `{ "db": -30 }` (−120…0) | `StatusResponse` (400 if out of range) |
| POST   | `/api/snapshot`     | —                   | `{ "url": "/api/clips/snap-….jpg" }` |
| GET    | `/api/clips`        | —                   | `ClipInfo[]` (newest first) |
| GET    | `/api/clips/:name`  | `Range:` supported  | MP4/JPEG bytes (206 on range) |
| POST   | `/api/live/start`   | —                   | `{ "whep_url": "…" }` |
| POST   | `/api/live/stop`    | —                   | 204 |
| GET    | `/api/events` (WS)  | `?token=…`          | JSON `ApiEvent` stream |

`live/start` takes a TTL lease on the publisher (`[live].lease_ttl_secs`); the
app should re-POST it periodically while the Live screen is open, and POST
`live/stop` when leaving. The publisher is also reaped automatically.

Events pushed over `/api/events`:
`{"type":"level","db":-42.1}` · `{"type":"state","armed":true,"recording":false}`
· `{"type":"trigger","db":-18.3,"at":"…"}` · `{"type":"clip","name":"clip-….mp4"}`.

## Verification (curl / websocat)

```sh
TOK=your-api-token
BASE=http://localhost:8088

# Status / control
curl -s -H "Authorization: Bearer $TOK" $BASE/api/status | jq
curl -s -X POST -H "Authorization: Bearer $TOK" $BASE/api/disarm | jq .armed
curl -s -X POST -H "Authorization: Bearer $TOK" \
     -H 'content-type: application/json' -d '{"db":-25}' $BASE/api/threshold | jq .trigger_db

# Clips + HTTP range (expect: HTTP/1.1 206 Partial Content)
curl -s -H "Authorization: Bearer $TOK" $BASE/api/clips | jq
curl -s -D - -o /dev/null -r 0-1023 \
     -H "Authorization: Bearer $TOK" "$BASE/api/clips/<name>.mp4" | head -n1

# Live events (talk near the mic → level moves; loud sound → trigger then clip)
websocat "ws://localhost:8088/api/events?token=$TOK"

# Snapshot
curl -s -X POST -H "Authorization: Bearer $TOK" $BASE/api/snapshot | jq

# Live view: get the WHEP URL, then open it in any WHEP player on the tailnet.
curl -s -X POST -H "Authorization: Bearer $TOK" $BASE/api/live/start | jq
```

To confirm WebRTC works with no STUN/TURN, open the `whep_url` in a WHEP-capable
player from a device on the tailnet — append `?token=$TOK` (or have the player
send the bearer header) — and verify the connection succeeds using only the
Tailscale `100.x` host candidate.

To confirm the lockdown, from another machine on the **LAN** (not via
Tailscale): `curl http://<server-lan-ip>:8088/api/status` → `403`, and
`ffprobe rtsp://<server-lan-ip>:8554/cam` → connection refused.
