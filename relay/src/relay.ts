import { LEAGUES_PATH, POE1_CATEGORIES, type Category } from "./categories.ts";
import pkg from "../package.json" with { type: "json" };

export const POE_NINJA_ORIGIN = "https://poe.ninja";
export const USER_AGENT = `Lootwise-relay/${pkg.version} (+https://github.com/hawasman/lootwise)`;

/** Free-tier Workers allow 50 outbound `fetch()` subrequests per invocation. */
export const SUBREQUEST_LIMIT = 50;

/** Configured League value meaning "the current challenge League". */
export const CURRENT_LEAGUE = "@current";

/** Outbound requests in flight at once; Workers allow 6 open connections. */
const CONCURRENCY = 6;

/**
 * Where the relay keeps what it fetched: R2 in the Worker, a folder in the
 * mirror script. Both produce the same keys and etag semantics (MD5 of the
 * stored bytes, which is R2's etag for a single-part upload).
 */
export interface Store {
  readText(key: string): Promise<string | null>;
  /**
   * Store `body` verbatim under `key`. `contentLength` is the exact byte length
   * when known, else null.
   */
  write(
    key: string,
    body: ReadableStream<Uint8Array> | string,
    contentLength: number | null,
  ): Promise<{ etag: string }>;
}

export interface CategoryEntry {
  /** HTTP status of the latest attempt; 0 when no response arrived. */
  status: number;
  /** Whether the latest attempt left the stored object current. */
  ok: boolean;
  /** When the stored object was last fetched or confirmed unchanged (304); null if never. */
  fetchedAt: string | null;
  /** When the latest attempt was made. */
  attemptedAt: string;
  /** Store etag of the stored object; null if never stored. */
  etag: string | null;
  /** poe.ninja's ETag for the stored object, sent back as If-None-Match. */
  upstreamEtag: string | null;
  /** Why the latest attempt failed. */
  error?: string;
}

export interface Manifest {
  version: 1;
  league: string;
  updatedAt: string;
  categories: Record<string, CategoryEntry>;
}

export interface RelayOptions {
  /** A League id, or {@link CURRENT_LEAGUE}. */
  league: string;
  store: Store;
  fetch: typeof fetch;
  now?: () => Date;
  categories?: readonly Category[];
}

export const objectKey = (league: string, name: string): string => `v1/${league}/${name}.json`;
export const manifestKey = (league: string): string => objectKey(league, "manifest");

export class SubrequestBudgetExhausted extends Error {
  constructor() {
    super(`subrequest budget of ${SUBREQUEST_LIMIT} exhausted`);
  }
}

/**
 * Fetch every category of one League into the store and write its manifest.
 * Response bodies are streamed to the store unparsed. A failed category keeps
 * its previous object and is marked `ok: false` in the manifest.
 */
export async function relayLeague(options: RelayOptions): Promise<Manifest> {
  const now = options.now ?? (() => new Date());
  const categories = options.categories ?? POE1_CATEGORIES;
  let used = 0;
  const request = (path: string, headers: Record<string, string> = {}): Promise<Response> => {
    if (used >= SUBREQUEST_LIMIT) return Promise.reject(new SubrequestBudgetExhausted());
    used++;
    return options.fetch(`${POE_NINJA_ORIGIN}${path}`, {
      headers: { "User-Agent": USER_AGENT, Accept: "application/json", ...headers },
      // A followed redirect is another subrequest the budget cannot see.
      redirect: "manual",
    });
  };

  const league =
    options.league === CURRENT_LEAGUE ? await currentLeague(request) : options.league;
  assertSafeLeague(league);

  const previous = await readManifest(options.store, league);
  const entries: Record<string, CategoryEntry> = {};
  await forEachLimited(categories, CONCURRENCY, async (category) => {
    entries[category.name] = await relayCategory(
      category,
      league,
      previous?.categories[category.name],
      options.store,
      request,
      now,
    );
  });

  const manifest: Manifest = {
    version: 1,
    league,
    updatedAt: now().toISOString(),
    // Previous entries for categories no longer relayed are kept.
    categories: { ...previous?.categories, ...entries },
  };
  await options.store.write(manifestKey(league), JSON.stringify(manifest, null, 2), null);
  return manifest;
}

/** A budgeted GET of a poe.ninja path. */
type PoeNinjaRequest = (path: string, headers?: Record<string, string>) => Promise<Response>;

async function relayCategory(
  category: Category,
  league: string,
  previous: CategoryEntry | undefined,
  store: Store,
  request: PoeNinjaRequest,
  now: () => Date,
): Promise<CategoryEntry> {
  const attemptedAt = now().toISOString();
  const failed = (status: number, error: string): CategoryEntry => ({
    status,
    ok: false,
    fetchedAt: previous?.fetchedAt ?? null,
    attemptedAt,
    etag: previous?.etag ?? null,
    upstreamEtag: previous?.upstreamEtag ?? null,
    error,
  });

  const conditional: Record<string, string> =
    previous?.etag && previous.upstreamEtag ? { "If-None-Match": previous.upstreamEtag } : {};
  let response: Response;
  try {
    response = await request(
      `${category.path}&league=${encodeURIComponent(league)}`,
      conditional,
    );
  } catch (error) {
    return failed(0, errorMessage(error));
  }

  if (response.status === 304 && previous?.etag) {
    return { ...previous, status: 304, ok: true, fetchedAt: attemptedAt, attemptedAt, error: undefined };
  }
  if (response.status !== 200 || !response.body) {
    await response.body?.cancel();
    return failed(response.status, `unexpected HTTP status ${response.status}`);
  }

  try {
    const { etag } = await store.write(
      objectKey(league, category.name),
      response.body,
      exactLength(response),
    );
    return {
      status: 200,
      ok: true,
      fetchedAt: attemptedAt,
      attemptedAt,
      etag,
      upstreamEtag: response.headers.get("ETag"),
    };
  } catch (error) {
    return failed(response.status, `store write failed: ${errorMessage(error)}`);
  }
}

async function currentLeague(request: PoeNinjaRequest): Promise<string> {
  const response = await request(LEAGUES_PATH);
  if (response.status !== 200) {
    await response.body?.cancel();
    throw new Error(`poe.ninja leagues endpoint returned HTTP ${response.status}`);
  }
  // A few hundred bytes; the only response the relay parses.
  const leagues: unknown = await response.json();
  const first = Array.isArray(leagues) ? (leagues[0] as { id?: unknown } | undefined) : undefined;
  if (typeof first?.id !== "string") {
    throw new Error("poe.ninja leagues endpoint returned no current League");
  }
  return first.id;
}

async function readManifest(store: Store, league: string): Promise<Manifest | null> {
  const text = await store.readText(manifestKey(league));
  if (text === null) return null;
  try {
    return JSON.parse(text) as Manifest;
  } catch {
    return null;
  }
}

/** The League id becomes a key segment and, in the mirror, a directory name. */
function assertSafeLeague(league: string): void {
  if (league === "" || league === "." || league === ".." || /[/\\\0]/.test(league)) {
    throw new Error(`refusing unsafe League id ${JSON.stringify(league)}`);
  }
}

/** Byte length of the body as delivered, or null when it is unknown. */
function exactLength(response: Response): number | null {
  const header = response.headers.get("Content-Length");
  // A decoded compressed body no longer matches Content-Length.
  if (header === null || response.headers.has("Content-Encoding")) return null;
  const length = Number(header);
  return Number.isSafeInteger(length) && length >= 0 ? length : null;
}

async function forEachLimited<T>(
  items: readonly T[],
  limit: number,
  run: (item: T) => Promise<void>,
): Promise<void> {
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < items.length) await run(items[next++]!);
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
}

const errorMessage = (error: unknown): string =>
  error instanceof Error ? error.message : String(error);
