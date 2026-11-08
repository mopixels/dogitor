# Dogitor

A sound-triggered home monitor. Leave an always-on laptop/desktop at home; it
listens on the microphone, and when a loud sound (e.g. a dog barking) crosses a
threshold it:

1. sends you a Telegram notification, and
2. records a short video + audio clip (webcam video, mic audio) until the room
   goes quiet again, then delivers the clip into the same Telegram chat.

Telegram handles alerts and remote control. The server also exposes an
HTTP/WebSocket API and WebRTC live view (see **[docs/SERVER.md](docs/SERVER.md)**)
for a planned React Native + Expo companion app — **not built yet**, see
[docs/CLIENT.md](docs/CLIENT.md).

## How it works

- **`cpal`** owns the microphone and computes the audio level (dBFS) for every
  buffer. It is the single source of truth for "loud" vs "quiet".
- A **hysteresis state machine** starts recording after sound stays loud briefly,
  and stops after it stays quiet for a while (or hits the max clip length).
- One **FFmpeg publisher** captures the *camera only* plus the PCM that `cpal`
  pipes into its stdin, and publishes to a local **MediaMTX** hub. MediaMTX fans
  that single stream out to the clip recorder, snapshots, and live view
  (WebRTC/WHEP) — so the camera is opened once and live + recording coexist.
  **MediaMTX is therefore required for recording, not just live view.**
- A **Telegram bot** (`teloxide`) sends notifications/clips and accepts commands.
  The bot connects outbound to Telegram, so there's no port forwarding or NAT
  setup on the home network.

## Two ways to run it

Both run the exact same monitor (`dogitor::server::start`); they differ only in
how they're launched and configured.

|                 | **Mac app** (`dogitor_gui` → `Dogitor.app`)     | **Headless CLI** (`dogitor`)                 |
| --------------- | ----------------------------------------------- | -------------------------------------------- |
| Platform        | macOS                                           | macOS or Linux                               |
| Config          | Settings tab → `~/Library/Application Support/Dogitor/config.toml` | `./config.toml` (+ optional `./.env`) |
| MediaMTX        | started/stopped for you                         | you run it yourself                          |
| `mediamtx.yml`  | `~/Library/Application Support/Dogitor/mediamtx.yml` | `./mediamtx.yml`                        |
| Clips           | `~/Library/Application Support/Dogitor/clips/`  | `./clips/` (`[recording] output_dir`)        |
| Logs            | `~/Library/Application Support/Dogitor/dogitor.log` | stdout                                   |

The two configs are independent — editing one does not affect the other.

## Prerequisites

- **FFmpeg** and **MediaMTX** on your `PATH`
  - macOS: `brew install ffmpeg mediamtx`
  - Linux: `sudo apt install ffmpeg`, MediaMTX from
    <https://github.com/bluenviron/mediamtx/releases>
- Rust (stable) — `cargo`, `rustc` (only to build from source; not needed
  for the downloaded Mac app)
- A Telegram bot token: message **[@BotFather](https://t.me/BotFather)**,
  `/newbot`, copy the token.
- An **API token** — any long, random, URL-safe string. It's required to start
  even if you don't use the API yet (the Mac app's Settings can generate one).

## Mac app (recommended on macOS)

### Download

Grab **`Dogitor-<version>-universal.dmg`** from the
[latest release](https://github.com/mopixels/dogitor/releases/latest)
(macOS 11+, Apple Silicon and Intel).

1. Open the `.dmg` and drag **Dogitor** onto **Applications**.
2. Install the helper tools: `brew install ffmpeg mediamtx`
   ([Homebrew](https://brew.sh)).
3. Open Dogitor. The app is ad-hoc signed, not notarized (that needs a paid
   Apple Developer account), so macOS blocks the first launch with "cannot
   verify…". Click **Done**, then **System Settings → Privacy & Security →
   Open Anyway**. You only do this once per download.
   Terminal alternative: `xattr -dr com.apple.quarantine /Applications/Dogitor.app`

### Or build it yourself

```sh
./scripts/bundle-mac.sh
```

This builds `Dogitor.app` for this Mac (with its own icon, ad-hoc signed) and
installs it into `/Applications`. Re-run the script whenever you change the
source. To run from source without bundling:
`cargo run --release --bin dogitor_gui`.

Ad-hoc signing means a rebuild can trigger fresh camera/mic permission prompts
(the signature identity changes). If that gets annoying, sign with a free Apple
ID "Personal Team" identity instead.

### First run

1. The app opens straight into **Settings**: paste the bot token, generate an
   API token, pick the camera, adjust thresholds, then **Save & Start** (writes
   the config and relaunches).
2. In Telegram, send **`/start`** to your bot. The first chat to do so becomes
   the bot's owner, and the app saves its id automatically. From then on the
   bot ignores every other chat. To hand it to a different chat, clear the chat
   id in Settings → **Save & Restart**, then `/start` from the new chat.
3. Approve the Camera and Microphone prompts — they're attributed to Dogitor
   itself.

## Headless CLI

1. Configure:
   ```sh
   cp config.toml.example config.toml
   ```
   Provide both tokens, either as env vars, in a `.env` file in the working
   directory, or in `config.toml` (`[telegram] token`, `[api] token`):
   ```sh
   # .env
   DOGITOR_BOT_TOKEN=123456:ABC-...
   DOGITOR_API_TOKEN=some-long-random-string
   ```
   Env vars / `.env` take precedence over `config.toml`.
2. Find your camera and put its index in `config.toml` (`[recording] video_device`):
   ```sh
   ffmpeg -f avfoundation -list_devices true -i ""   # macOS
   ```
   On Linux use the device path, e.g. `/dev/video0`.
3. Start MediaMTX, then Dogitor (order doesn't matter):
   ```sh
   mediamtx ./mediamtx.yml
   # in another shell:
   cargo run --release            # or: cargo run --release -- path/to/config.toml
   ```
4. In Telegram, send **`/start`** to your bot and paste the chat id it replies
   with into `config.toml` (`[telegram] chat_id`). Until you do, the first chat
   to `/start` after each restart claims the bot, so don't skip this.

**macOS permissions:** when run from a terminal, macOS attributes Camera and
Microphone access to the *terminal app* (Terminal / iTerm). Grant both under
**System Settings → Privacy & Security** if you aren't prompted. The FFmpeg
child processes inherit it.

## Telegram commands

| Command            | Action                                            |
| ------------------ | ------------------------------------------------- |
| `/start`           | Claim the bot for this chat (only while no chat is registered) and show its id |
| `/status`          | Armed?, current level, threshold, last trigger    |
| `/arm` / `/disarm` | Enable / disable monitoring                       |
| `/threshold -30`   | Set the trigger threshold (dBFS) live             |
| `/snapshot`        | Capture and send a still photo now                |
| `/last`            | Resend the most recent clip                       |

Bot usernames are public, so anyone can message your bot. It only answers the
registered chat and silently ignores everyone else (attempts are logged as
`telegram: ignoring command from unregistered chat`).

## Tuning

`dBFS` is negative: `0` is the loudest possible, quiet rooms sit around
`-50…-60`. Watch the live level (Mac app Status tab, `/status`, or the logs)
and set `trigger_db` a bit above the ambient level. Key knobs (Mac app
Settings, or `config.toml`):

- `trigger_db` / `quiet_db` — start/stop thresholds (keep `quiet_db` below
  `trigger_db` for hysteresis).
- `trigger_hold_secs` — how long sound must persist before triggering (rejects
  short pops).
- `quiet_timeout_secs` — how long quiet must persist before stopping.
- `cooldown_secs` — ignore new triggers for this long after a clip.
- `max_clip_secs` — hard cap per clip. Keep clips modest: the Telegram **Bot API
  caps uploads at 50 MB**. At 640×480 this is plenty; raise resolution with care.

## HTTP API & live view

The HTTP/WebSocket API (status, arm/disarm, threshold, clips, snapshots, events)
and WebRTC live view are documented in **[docs/SERVER.md](docs/SERVER.md)**,
including the Tailscale setup and `curl`/`websocat` checks. Shared API types:
[`shared/api-types.ts`](shared/api-types.ts).

## Notes / limits

- **Telegram 50 MB upload cap.** For longer/larger clips, self-host the Telegram
  Bot API server (raises it to 2 GB).
- **Remote live view** needs the viewing device and the server on the same
  Tailscale tailnet, plus `webrtcAdditionalHosts` and `whep_url` set to the
  server's tailnet address — see [docs/SERVER.md](docs/SERVER.md). The defaults
  only work on the server machine itself.

## Releasing

Bump `version` in `Cargo.toml`, commit, then tag and push:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

[`.github/workflows/release.yml`](.github/workflows/release.yml) runs the tests,
builds the universal `.dmg` with
[`scripts/release-mac.sh`](scripts/release-mac.sh) and publishes a GitHub
Release. Running the workflow manually (Actions tab) builds the `.dmg` as a
downloadable artifact without releasing. `scripts/release-mac.sh` also works
locally (needs `rustup`; output in `dist/`).

## Logging

Uses `tracing`. Increase verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug cargo run                    # CLI: logs to the terminal
tail -f ~/Library/Application\ Support/Dogitor/dogitor.log   # Mac app
```
