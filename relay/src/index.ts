import { r2Store } from "./r2-store.ts";
import { relayLeague } from "./relay.ts";

export default {
  async scheduled(controller, env) {
    const leagueByCron: Record<string, string | undefined> = env.LEAGUE_BY_CRON;
    const league = leagueByCron[controller.cron];
    if (league === undefined) {
      console.error(`no League configured for cron "${controller.cron}"`);
      return;
    }
    const manifest = await relayLeague({
      league,
      store: r2Store(env.PRICES),
      fetch: (input, init) => fetch(input, init),
    });
    const failed = Object.entries(manifest.categories).filter(([, entry]) => !entry.ok);
    console.log(
      `relayed League "${manifest.league}": ${failed.length} failed` +
        (failed.length ? ` (${failed.map(([name]) => name).join(", ")})` : ""),
    );
  },
} satisfies ExportedHandler<Env>;
