/**
 * Every poe.ninja PoE1 economy overview, per https://poe.ninja/docs/api.
 *
 * `name` is the object name under `v1/{league}/`. poe.ninja reuses some `type`
 * values across endpoints (`Currency` and `Fragment` exist on both the exchange
 * and the stash currency overview), so names carry the endpoint as a prefix.
 */
export interface Category {
  readonly name: string;
  /** Path and query on poe.ninja, without the League parameter. */
  readonly path: string;
}

const EXCHANGE_TYPES = [
  "Currency",
  "Fragment",
  "Runegraft",
  "AllflameEmber",
  "Tattoo",
  "Omen",
  "DjinnCoin",
  "Ducat",
  "EnshroudingCrystal",
  "DivinationCard",
  "Artifact",
  "Oil",
  "DeliriumOrb",
  "Scarab",
  "Astrolabe",
  "Fossil",
  "Resonator",
  "Essence",
] as const;

const ITEM_TYPES = [
  "Wombgift",
  "Corpse",
  "Incubator",
  "UniqueWeapon",
  "UniqueArmour",
  "UniqueAccessory",
  "UniqueFlask",
  "UniqueJewel",
  "ForbiddenJewel",
  "ShrineBelt",
  "UniqueTincture",
  "UniqueRelic",
  "SkillGem",
  "ImbuedGem",
  "ClusterJewel",
  "Map",
  "BlightedMap",
  "BlightRavagedMap",
  "UniqueMap",
  "ValdoMap",
  "Invitation",
  "Memory",
  "IncursionTemple",
  "ScryingOrb",
  "BaseType",
  "Flask",
  "Beast",
  "Vial",
] as const;

const STASH_CURRENCY_TYPES = ["Currency", "Fragment"] as const;

const endpoint = (prefix: string, path: string, types: readonly string[]): Category[] =>
  types.map((type) => ({ name: `${prefix}-${type}`, path: `${path}?type=${type}` }));

export const POE1_CATEGORIES: readonly Category[] = [
  ...endpoint("exchange", "/poe1/api/economy/exchange/current/overview", EXCHANGE_TYPES),
  ...endpoint("item", "/poe1/api/economy/stash/current/item/overview", ITEM_TYPES),
  ...endpoint("currency", "/poe1/api/economy/stash/current/currency/overview", STASH_CURRENCY_TYPES),
];

export const LEAGUES_PATH = "/poe1/api/economy/leagues";
