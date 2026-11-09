import { createScheduledController, env } from "cloudflare:test";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker from "../src/index.ts";
import { r2Store } from "../src/r2-store.ts";
import { relayLeague, type Manifest } from "../src/relay.ts";
import CURRENCY_FRAGMENT from "./fixtures/currency-Fragment.json?raw";
import EXCHANGE_OIL from "./fixtures/exchange-Oil.json?raw";
import ITEM_VIAL from "./fixtures/item-Vial.json?raw";
import LEAGUES from "./fixtures/leagues.json?raw";

const CURRENT_CRON = "0,15,30,45 * * * *";
const STANDARD_CRON = "7,22,37,52 * * * *";
// First entry of the recorded leagues response.
const CURRENT = "Allflame";
const OIL_ETAG = "W/5f70aa8449b6bd03e7d027bc3b73f93a";

type Override = (url: URL, request: Request) => Response | undefined;

const recordedResponse = (body: string, etag: string): Response =>
  new Response(body, {
    headers: {
      "Content-Type": "application/json",
      "Content-Length": String(new TextEncoder().encode(body).byteLength),
      ETag: etag,
    },
  });

/** Answer poe.ninja requests with recorded responses; returns every request made. */
function stubPoeNinja(override?: Override): Request[] {
  const requests: Request[] = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const request = new Request(input, init);
    requests.push(request);
    const url = new URL(request.url);
    const answer = override?.(url, request);
    if (answer) return answer;
    if (url.pathname === "/poe1/api/economy/leagues") return recordedResponse(LEAGUES, "W/leagues");
    if (url.pathname.includes("/exchange/")) return recordedResponse(EXCHANGE_OIL, OIL_ETAG);
    if (url.pathname.includes("/item/")) return recordedResponse(ITEM_VIAL, "W/item");
    if (url.pathname.includes("/currency/")) return recordedResponse(CURRENCY_FRAGMENT, "W/currency");
    return new Response("not found", { status: 404 });
  });
  return requests;
}

const runCron = (cron: string): Promise<void> =>
  worker.scheduled(createScheduledController({ cron, scheduledTime: Date.now() }), env);

async function keys(prefix: string): Promise<string[]> {
  const listed = await env.PRICES.list({ prefix });
  return listed.objects.map((object) => object.key);
}

async function manifest(league: string): Promise<Manifest> {
  const object = await env.PRICES.get(`v1/${league}/manifest.json`);
  if (!object) throw new Error(`no manifest for ${league}`);
  return object.json<Manifest>();
}

const text = async (key: string): Promise<string | undefined> =>
  (await env.PRICES.get(key))?.text();

const isType = (url: URL, type: string, endpoint: string): boolean =>
  url.pathname.includes(`/${endpoint}/`) && url.searchParams.get("type") === type;

beforeEach(async () => {
  const existing = await keys("");
  if (existing.length) await env.PRICES.delete(existing);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("scheduled relay", () => {
  it("writes every PoE1 category of the current League verbatim to R2", async () => {
    stubPoeNinja();

    await runCron(CURRENT_CRON);

    const written = await keys("v1/");
    expect(written).toHaveLength(49);
    expect(written.every((key) => key.startsWith(`v1/${CURRENT}/`))).toBe(true);
    expect(written).toContain(`v1/${CURRENT}/manifest.json`);
    expect(await text(`v1/${CURRENT}/exchange-Oil.json`)).toBe(EXCHANGE_OIL);
    expect(await text(`v1/${CURRENT}/item-Vial.json`)).toBe(ITEM_VIAL);
    expect(await text(`v1/${CURRENT}/currency-Fragment.json`)).toBe(CURRENCY_FRAGMENT);
  });

  it("records fetch time, R2 etag and HTTP status per category in the manifest", async () => {
    stubPoeNinja();
    const before = Date.now();

    await runCron(CURRENT_CRON);

    const written = await manifest(CURRENT);
    expect(written.league).toBe(CURRENT);
    expect(Object.keys(written.categories)).toHaveLength(48);
    const oil = written.categories["exchange-Oil"]!;
    const object = await env.PRICES.head(`v1/${CURRENT}/exchange-Oil.json`);
    expect(oil).toMatchObject({ status: 200, ok: true, etag: object!.etag, upstreamEtag: OIL_ETAG });
    expect(Date.parse(oil.fetchedAt!)).toBeGreaterThanOrEqual(before);
    expect(Object.values(written.categories).every((entry) => entry.ok)).toBe(true);
  });

  it("relays a named League without looking up the current one", async () => {
    const requests = stubPoeNinja();

    await runCron(STANDARD_CRON);

    expect(requests.some((request) => request.url.endsWith("/leagues"))).toBe(false);
    expect(requests.every((request) => new URL(request.url).searchParams.get("league") === "Standard")).toBe(true);
    expect(await keys("v1/")).toHaveLength(49);
    expect((await manifest("Standard")).league).toBe("Standard");
  });

  it("keeps the previous object and marks the manifest when a fetch fails", async () => {
    stubPoeNinja();
    await runCron(CURRENT_CRON);
    const first = await manifest(CURRENT);
    vi.restoreAllMocks();
    stubPoeNinja((url) => {
      if (isType(url, "Oil", "exchange")) return new Response("busy", { status: 503 });
      if (isType(url, "Vial", "item")) throw new TypeError("network connection lost");
      return undefined;
    });

    await runCron(CURRENT_CRON);

    const second = await manifest(CURRENT);
    expect(await text(`v1/${CURRENT}/exchange-Oil.json`)).toBe(EXCHANGE_OIL);
    expect(second.categories["exchange-Oil"]).toMatchObject({
      status: 503,
      ok: false,
      etag: first.categories["exchange-Oil"]!.etag,
      fetchedAt: first.categories["exchange-Oil"]!.fetchedAt,
    });
    expect(await text(`v1/${CURRENT}/item-Vial.json`)).toBe(ITEM_VIAL);
    expect(second.categories["item-Vial"]).toMatchObject({
      status: 0,
      ok: false,
      etag: first.categories["item-Vial"]!.etag,
      error: "network connection lost",
    });
    expect(second.categories["exchange-Currency"]).toMatchObject({ status: 200, ok: true });
  });

  it("marks a category that has never been fetched successfully", async () => {
    stubPoeNinja((url) =>
      isType(url, "Oil", "exchange") ? new Response("gone", { status: 404 }) : undefined,
    );

    await runCron(CURRENT_CRON);

    expect(await env.PRICES.head(`v1/${CURRENT}/exchange-Oil.json`)).toBeNull();
    expect((await manifest(CURRENT)).categories["exchange-Oil"]).toMatchObject({
      status: 404,
      ok: false,
      etag: null,
      fetchedAt: null,
    });
  });

  it("revalidates with poe.ninja's ETag and keeps the object on 304", async () => {
    stubPoeNinja();
    await runCron(CURRENT_CRON);
    const first = await manifest(CURRENT);
    vi.restoreAllMocks();
    const requests = stubPoeNinja((url, request) =>
      isType(url, "Oil", "exchange") && request.headers.get("If-None-Match") === OIL_ETAG
        ? new Response(null, { status: 304 })
        : undefined,
    );

    await runCron(CURRENT_CRON);

    const oilRequest = requests.find((request) => isType(new URL(request.url), "Oil", "exchange"));
    expect(oilRequest!.headers.get("If-None-Match")).toBe(OIL_ETAG);
    expect(await text(`v1/${CURRENT}/exchange-Oil.json`)).toBe(EXCHANGE_OIL);
    expect((await manifest(CURRENT)).categories["exchange-Oil"]).toMatchObject({
      status: 304,
      ok: true,
      etag: first.categories["exchange-Oil"]!.etag,
    });
  });

  it("stays within 50 subrequests, identifies itself and does not follow redirects", async () => {
    const requests = stubPoeNinja();

    await runCron(CURRENT_CRON);

    expect(requests.length).toBeLessThanOrEqual(50);
    for (const request of requests) {
      expect(request.headers.get("User-Agent")).toBe(
        "Lootwise-relay/0.1.0 (+https://github.com/hawasman/lootwise)",
      );
      expect(request.redirect).toBe("manual");
    }
  });

  it("does nothing for a cron with no configured League", async () => {
    const requests = stubPoeNinja();

    await runCron("59 23 * * *");

    expect(requests).toHaveLength(0);
    expect(await keys("")).toHaveLength(0);
  });
});

describe("relayLeague", () => {
  it("stops fetching at the subrequest budget and marks the rest failed", async () => {
    const requests = stubPoeNinja();
    const categories = Array.from({ length: 60 }, (_, index) => ({
      name: `exchange-${index}`,
      path: `/poe1/api/economy/exchange/current/overview?type=T${index}`,
    }));

    const written = await relayLeague({
      league: "Standard",
      store: r2Store(env.PRICES),
      fetch: (input, init) => fetch(input, init),
      categories,
    });

    expect(requests).toHaveLength(50);
    const failed = Object.values(written.categories).filter((entry) => !entry.ok);
    expect(failed).toHaveLength(10);
    expect(failed.every((entry) => entry.error === "subrequest budget of 50 exhausted")).toBe(true);
  });
});
