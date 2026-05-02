# BrainWorms (the dashboard)

> The dashboard ships as embedded assets inside `wormhole.exe`. This directory holds the JSON snapshots cron jobs write at runtime, plus the static config files that drive widget rendering.

## Files

- `index.html`: a tiny placeholder. The real dashboard HTML is baked into the binary via `include_bytes!` and served from `GET /`. This placeholder only matters if you want to serve the page from disk during development. Edit it freely; nothing in production reads from here unless you point `tower_http::services::ServeDir` at this directory.
- `widgets.json`: widget config the dashboard reads to know what panels to render.
- `nodes.json`: node config naming the agent runtimes the dashboard talks to.
- `data/`: where cron jobs drop the JSON snapshots widgets render.

## How the dashboard works (one paragraph)

The HTML page reads `widgets.json` once on load. For each widget entry it kicks off a fetch loop that polls the file at `data/<widget-id>.json` every `refresh_s` seconds. The renderer for each widget kind is a function inside `index.html`. Cron jobs in your workspace are responsible for writing fresh JSON into `data/`; the dashboard never produces data. If a JSON file is stale or missing, the widget renders an "unhealthy" state.

## Customizing widgets

You can edit `widgets.json` and add new widget entries pointing at new JSON files. Add a cron job that writes the JSON, refresh the page, watch the new widget render. The 30-second-edit property is the design goal.

## Authentication

`/api/*` requires the bearer token from `~/wormhole/.token`. The HTML page itself loads unauthenticated so the browser can prompt the user for the token; from then on every API call carries it.

## Loopback only

Default bind: `127.0.0.1`. LAN exposure is not in scope for v0.1.0. To bind to `0.0.0.0` the user must edit config and accept the warning.
