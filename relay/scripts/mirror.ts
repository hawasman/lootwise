// Run the relay's fetch logic against poe.ninja and write the R2 layout to a
// local folder: `pnpm dev:mirror <dir> [--league <name>]`.
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { unstable_readConfig } from "wrangler";
import { CURRENT_LEAGUE, relayLeague } from "../src/relay.ts";
import { folderStore } from "./folder-store.ts";

const { positionals, values } = parseArgs({
  allowPositionals: true,
  options: { league: { type: "string" } },
});
const [dir] = positionals;
if (dir === undefined || positionals.length !== 1) {
  console.error(`usage: pnpm dev:mirror <dir> [--league <name|${CURRENT_LEAGUE}>]`);
  process.exit(2);
}

/** The Leagues the deployed Worker's cron triggers relay, in trigger order. */
function configuredLeagues(): string[] {
  const config = unstable_readConfig({
    config: fileURLToPath(new URL("../wrangler.toml", import.meta.url)),
  });
  const leagueByCron = config.vars["LEAGUE_BY_CRON"] as Record<string, string | undefined>;
  const crons: string[] = config.triggers.crons ?? [];
  return crons.map((cron) => {
    const league = leagueByCron[cron];
    if (league === undefined) throw new Error(`wrangler.toml: no LEAGUE_BY_CRON entry for "${cron}"`);
    return league;
  });
}

const store = folderStore(dir);
let failures = 0;
for (const league of values.league === undefined ? configuredLeagues() : [values.league]) {
  const manifest = await relayLeague({ league, store, fetch });
  const entries = Object.entries(manifest.categories);
  const failed = entries.filter(([, entry]) => !entry.ok);
  failures += failed.length;
  console.log(`${manifest.league}: ${entries.length - failed.length}/${entries.length} categories current`);
  for (const [name, entry] of failed) console.log(`  ${name}: ${entry.error}`);
}
process.exitCode = failures ? 1 : 0;
