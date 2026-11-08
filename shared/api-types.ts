/**
 * Dogitor API contract — the single source of truth shared between the Rust
 * server (`src/api.rs`, `src/events.rs`) and the Expo app.
 *
 * Auth: every request requires the shared bearer token, sent as
 * `Authorization: Bearer <token>` or, for URLs that can't set headers
 * (WebSocket, <video> media), as a `?token=<token>` query parameter. The WHEP
 * endpoint (`whep_url`) is checked against the same token — send the bearer
 * header on the SDP offer POST. Only localhost/tailnet clients are served.
 *
 * Timestamps are RFC 3339 strings (e.g. "2026-06-21T15:30:12.123+03:00").
 * Audio levels are dBFS (negative; 0 is loudest).
 */

// --- Responses -------------------------------------------------------------

/** GET /api/status, and the body returned by arm/disarm/threshold. */
export interface StatusResponse {
  armed: boolean;
  recording: boolean;
  current_db: number;
  trigger_db: number;
  last_trigger: string | null;
  last_clip: string | null;
}

/** GET /api/clips → array of these (newest first). */
export interface ClipInfo {
  name: string;
  started_at: string;
  duration_s: number;
  size_bytes: number;
  thumb_url: string | null;
}

/** POST /api/snapshot → a relative URL to the captured still (a clips route). */
export interface UrlResponse {
  url: string;
}

/** POST /api/live/start → the WHEP endpoint to negotiate WebRTC against. */
export interface WhepResponse {
  whep_url: string;
}

// --- Requests --------------------------------------------------------------

/** POST /api/threshold */
export interface ThresholdRequest {
  db: number;
}

// --- Events (WS /api/events) ----------------------------------------------

/** Throttled (~10 Hz) live audio level. */
export interface LevelEvent {
  type: "level";
  db: number;
}

/** Armed / recording flags changed. */
export interface StateEvent {
  type: "state";
  armed: boolean;
  recording: boolean;
}

/** A sound crossed the threshold and recording began. */
export interface TriggerEvent {
  type: "trigger";
  db: number;
  at: string;
}

/** A finished clip is available (file name within the clips dir). */
export interface ClipEvent {
  type: "clip";
  name: string;
}

/** Discriminated union pushed over the events WebSocket. */
export type ApiEvent = LevelEvent | StateEvent | TriggerEvent | ClipEvent;

// --- Endpoints -------------------------------------------------------------

/** Route builders, relative to the server base URL (e.g. http://host:8088). */
export const Endpoints = {
  status: () => "/api/status",
  arm: () => "/api/arm",
  disarm: () => "/api/disarm",
  threshold: () => "/api/threshold",
  snapshot: () => "/api/snapshot",
  clips: () => "/api/clips",
  clip: (name: string) => `/api/clips/${encodeURIComponent(name)}`,
  liveStart: () => "/api/live/start",
  liveStop: () => "/api/live/stop",
  events: () => "/api/events",
} as const;
