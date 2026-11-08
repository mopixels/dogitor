# Dogitor app (React Native + Expo) — *planned*

> Status: **not built yet.** The server side (HTTP/WS API + MediaMTX live media)
> is implemented — see [SERVER.md](SERVER.md). This file is a stub for the app
> pass and will be filled in when the Expo client is built.

## Planned shape

- **Expo + custom dev client** (NOT Expo Go): `react-native-webrtc` has native
  code, so the app needs `expo-dev-client` + `npx expo prebuild` (or EAS Build).
- `expo-router` for navigation, **TanStack Query** for API calls,
  `expo-secure-store` for the server URL + bearer token.
- Imports the shared contract from [`shared/api-types.ts`](../shared/api-types.ts).

### Screens

1. **Status / Home** — ARM/DISARM toggle, live dB meter (from `WS /api/events`),
   threshold slider, last-trigger, recording indicator, Go Live + Snapshot.
2. **Live** — `RTCPeerConnection` + WHEP handshake (POST SDP offer → answer)
   against `whep_url`, rendered with `RTCView`. Re-POST `live/start` as a
   heartbeat; `live/stop` on leave.
3. **Clips / History** — list `/api/clips`; play via `expo-video` streaming
   `/api/clips/:name` (HTTP range); share/save.
4. **Settings** — server URL (Tailscale MagicDNS), bearer token, Test connection.

## Setup notes (to expand)

- Both the phone and the home server must be on the **same Tailscale tailnet**.
- Auth: every API/WS/media request carries the bearer token (header, or
  `?token=` for the WebSocket and media URLs). That includes the **WHEP**
  requests to `whep_url`: MediaMTX checks them against the same token, so send
  `Authorization: Bearer <token>` on the SDP offer POST (and any PATCH/DELETE
  to the session URL). The server only answers tailnet clients, so the app
  can't be tested over plain LAN Wi-Fi.
- Because both peers are on the tailnet, WebRTC needs **no STUN/TURN**.
