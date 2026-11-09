# Lootwise

A Path of Exile pricing tool that tells a player what their items and stash are worth, both on demand in-game and over time.

## Language

### Pricing

**Price Check**:
A player-triggered request to value the item or items the player is looking at in-game.
_Avoid_: Lookup, appraisal, quote

**Screen Price Check**:
A Price Check whose items are identified by reading their names off a game screen, used where the game offers no copyable item text.
_Avoid_: OCR check, scan

**Screen Profile**:
A known game screen that a Screen Price Check can read, together with the set of names that may appear on it.
_Avoid_: Template, region

**Valuation**:
The value Lootwise assigns to an item, together with where that value came from and how confident it is.
_Avoid_: Appraisal, quote, estimate

**Commodity Item**:
An item whose value is determined by its identity alone (currency, fragments, uniques by variant, gems), so it can be valued from aggregated market data.
_Avoid_: Bulk item, standard item

**Live Search**:
A query against current listings on the official trade site, used to value items that are not Commodity Items.
_Avoid_: Trade lookup, market query

### Scope

**Game**:
Which Path of Exile title an item, league, or price belongs to; currently only Path of Exile 1.
_Avoid_: Version, edition

**League**:
The economy an item, price, or Snapshot belongs to; values are never compared across leagues.
_Avoid_: Realm, server

### Wealth

**Snapshot**:
The valued contents of a player's chosen stash tabs at one moment in one League, recording which tabs it covered.
_Avoid_: Capture, checkpoint

**Session**:
A player-started and player-stopped period of play whose earnings are measured between its first and last Snapshot.
_Avoid_: Run, farm, map session

### Extension

**Plugin**:
Third-party code that adds price sources, valuation rules, or item details to Lootwise.
_Avoid_: Extension, add-on, mod
