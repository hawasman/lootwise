import type { Store } from "./relay.ts";

const httpMetadata = { contentType: "application/json" };

export function r2Store(bucket: R2Bucket): Store {
  return {
    async readText(key) {
      const object = await bucket.get(key);
      return object === null ? null : object.text();
    },
    async write(key, body, contentLength) {
      if (typeof body === "string") {
        return { etag: (await bucket.put(key, body, { httpMetadata })).etag };
      }
      if (contentLength === null) {
        // R2 needs a known length to accept a stream. Buffering copies bytes
        // without parsing them, so it stays within the CPU budget.
        const bytes = await new Response(body).arrayBuffer();
        return { etag: (await bucket.put(key, bytes, { httpMetadata })).etag };
      }
      const fixed = new FixedLengthStream(contentLength);
      const [object] = await Promise.all([
        bucket.put(key, fixed.readable, { httpMetadata }),
        body.pipeTo(fixed.writable),
      ]);
      return { etag: object.etag };
    },
  };
}
