# Lootwise price relay

The Cloudflare Worker from [ADR 0004](../docs/adr/0004-static-price-relay-on-cloudflare.md).
On a schedule it fetches every poe.ninja Path of Exile 1 economy overview for a
League and stores each response verbatim in R2. Lootwise downloads these files
and never contacts poe.ninja itself.

## Layout

```
v1/{league}/{category}.json   poe.ninja's response body, byte for byte
v1/{league}/manifest.json     per-category fetch state
```

`{league}` is poe.ninja's League id as-is (it can contain spaces, e.g.
`Hardcore Allflame`); URL-encode it when downloading.

`{category}` is the poe.ninja overview type prefixed with its endpoint, because
poe.ninja reuses some types across endpoints (48 in total, see
[`src/categories.ts`](src/categories.ts)):

| Prefix      | poe.ninja endpoint                                    |
| ----------- | ----------------------------------------------------- |
| `exchange-` | `/poe1/api/economy/exchange/current/overview`         |
| `item-`     | `/poe1/api/economy/stash/current/item/overview`       |
| `currency-` | `/poe1/api/economy/stash/current/currency/overview`   |

`manifest.json`:

```json
{
  "version": 1,
  "league": "Allflame",
  "updatedAt": "2026-11-09T12:17:58.750Z",
  "categories": {
    "exchange-Oil": {
      "status": 200,
      "ok": true,
      "fetchedAt": "2026-11-09T12:17:57.995Z",
      "attemptedAt": "2026-11-09T12:17:57.995Z",
      "etag": "5f70aa8449b6bd03e7d027bc3b73f93a",
      "upstreamEtag": "W/5f70aa8449b6bd03e7d027bc3b73f93a"
    }
  }
}
```

| Field          | Meaning                                                                                  |
| -------------- | ---------------------------------------------------------------------------------------- |
| `status`       | HTTP status of the latest attempt; `0` when no response arrived.                         |
| `ok`           | `false` when the latest attempt failed. The previous object, if any, is kept unchanged.  |
| `fetchedAt`    | When the stored object was last fetched or confirmed unchanged (304); `null` if never.   |
| `attemptedAt`  | When the latest attempt was made.                                                        |
| `etag`         | R2's etag of the stored object (MD5 of its bytes); `null` if never stored.               |
| `upstreamEtag` | poe.ninja's ETag, sent back as `If-None-Match` so unchanged data costs a 304.            |
| `error`        | Why the latest attempt failed (only when `ok` is `false`).                               |

## Configuration

`wrangler.toml` has one cron trigger per League and maps each to a League in
`LEAGUE_BY_CRON`. `@current` resolves at run time to the first entry of
poe.ninja's `/poe1/api/economy/leagues`, the current challenge League. The
default covers the current challenge League and `Standard`, each every 15
minutes (poe.ninja refreshes PoE1 overviews about that often). Keep
`LEAGUE_BY_CRON` keys identical to `triggers.crons`.

## Running locally

Node 22.18 or newer and pnpm.

```sh
pnpm install
pnpm typecheck
pnpm test                 # vitest in workerd against recorded poe.ninja responses (test/fixtures)
```

Mirror the relay into a local folder, with no Cloudflare account. This runs
the same fetch logic against live poe.ninja and writes the same layout (etags
are MD5, as R2 computes them). Exits non-zero if any category failed.

```sh
pnpm dev:mirror ./mirror                    # current League and Standard
pnpm dev:mirror ./mirror --league Standard  # one League; @current also works
```

Run the Worker itself in local workerd with a local R2 bucket:

```sh
pnpm dev
curl "http://localhost:8787/__scheduled?cron=0,15,30,45+*+*+*+*"   # relay the current League
pnpm exec wrangler r2 object get "lootwise-prices/v1/Standard/manifest.json" --local --pipe
```

After changing `wrangler.toml`, run `pnpm types` to regenerate
`worker-configuration.d.ts`.

## Deploying (later)

Not done yet; it needs the Lootwise Cloudflare account.

1. `pnpm exec wrangler login`
2. `pnpm exec wrangler r2 bucket create lootwise-prices`
3. `pnpm exec wrangler deploy`

Serving the bucket on a custom domain behind Cloudflare's cache is a separate
step (ADR 0004).

## Free-tier limits it respects

| Limit                                     | How                                                                                                         |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| 10 ms CPU per invocation                  | Response bodies are never parsed: piped to R2, or buffered unparsed when their length is unknown (compressed). Only the small leagues list is parsed. |
| 50 `fetch()` subrequests per invocation   | One League per invocation: 48 categories + 1 leagues lookup = 49. A counter refuses a 51st request and marks the category failed. Redirects are not followed, since each hop is another subrequest; a 3xx is recorded as a failure. |
| 6 simultaneous connections                | At most 6 requests in flight.                                                                               |
| 5 cron triggers                           | One trigger per League, so at most 5 Leagues.                                                               |
| R2 writes (1M Class A ops/month)          | 2 Leagues × 49 writes × 96 runs/day ≈ 9,400/day ≈ 282k/month. Unchanged categories (304) are not rewritten.  |

R2 binding calls count against the separate 1,000 internal-service limit, not
the 50 subrequests.

poe.ninja's API guidelines are followed: a descriptive
`User-Agent: Lootwise-relay/<version> (+https://github.com/hawasman/lootwise)`,
conditional requests with its ETags, and polling no faster than its refresh
interval.
