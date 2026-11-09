# No player-facing backend; stash access through official GGG OAuth

Lootwise runs on the player's machine with no Lootwise-operated service holding player data or accounts: it calls the official trade API and the GGG account API directly, and keeps Snapshots and cached prices in a local SQLite database. The one exception is a stateless price relay that republishes poe.ninja data, because poe.ninja asks desktop clients to proxy through their own backend instead of calling its API directly (see ADR 0004). Stash access uses GGG's OAuth (`account:stashes`) rather than a pasted POESESSID, because Lootwise is distributed publicly and must stay within GGG's terms; the refresh token lives in the OS keychain. POESESSID is permitted only as an opt-in development fallback while the OAuth client awaits GGG approval.

## Considered Options

A hosted backend would allow our own price history and community features (bulk trade, services, shared strategies), but those are out of scope and would make Lootwise an operated service rather than a tool.

## Consequences

Public release is gated on GGG approving the OAuth client. Lootwise registers as a public client (PKCE, loopback redirect) with refresh tokens disabled, since GGG requires refresh tokens to be stored server-side and a confidential client would mean storing player tokens on our infrastructure. The access token lasts 10 hours and is held only in the OS keychain, so wealth tracking asks the player to sign in again after it expires. Price Check needs no sign-in. Public clients share GGG's rate limits, and all GGG traffic from one install goes through a single rate limiter driven by `X-Rate-Limit-*` headers.
