# Prices come from a write-only Cloudflare relay served as static files

poe.ninja asks desktop clients to proxy through their own backend, so Lootwise does not call it directly. Instead, scheduled Cloudflare Workers (free tier) fetch poe.ninja overviews every 15 minutes, one trigger per covered League, and store each response verbatim in R2 under `/v1/{league}/{category}.json` with a per-League `manifest.json` of fetch times and hashes; GGG's public Currency Exchange digests are relayed the same way. Lootwise downloads these files from an R2 custom domain behind Cloudflare's cache and never contacts poe.ninja itself, falling back to its last cached prices with lowered confidence when the relay is unreachable or stale.

## Considered Options

- **GitHub Actions on a schedule writing static files**: rejected because scheduled runs queue unpredictably.
- **Clients calling a Worker API**: rejected because the free tier's 100k requests/day would cap Lootwise at a few dozen players.
- **Workers KV as the store**: rejected because its 1,000 writes/day is below the ~2,900 needed.

## Consequences

The Worker must not parse responses, since free-tier invocations get 10 ms CPU; all normalisation of poe.ninja's format happens in Lootwise. Each invocation is limited to 50 subrequests, which bounds the categories per League per trigger, and the 5-trigger limit bounds the number of covered Leagues.
