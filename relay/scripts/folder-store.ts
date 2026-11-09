import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { Store } from "../src/relay.ts";

/**
 * A {@link Store} over a local folder with the same layout as the R2 bucket.
 * The etag is the MD5 of the stored bytes, as R2 reports for a single-part put.
 */
export function folderStore(root: string): Store {
  return {
    async readText(key) {
      try {
        return await readFile(join(root, key), "utf8");
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
        throw error;
      }
    },
    async write(key, body) {
      const path = join(root, key);
      await mkdir(dirname(path), { recursive: true });
      // Write beside the target and rename, so a failed write keeps the previous file.
      const temporary = `${path}.${randomUUID()}.tmp`;
      const hash = createHash("md5");
      try {
        const file = await open(temporary, "w");
        try {
          if (typeof body === "string") {
            hash.update(body);
            await file.writeFile(body);
          } else {
            for await (const chunk of body) {
              hash.update(chunk);
              await file.write(chunk);
            }
          }
        } finally {
          await file.close();
        }
        await rename(temporary, path);
      } catch (error) {
        await rm(temporary, { force: true });
        throw error;
      }
      return { etag: hash.digest("hex") };
    },
  };
}
