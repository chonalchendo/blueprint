// Tests only: `fetch` answering `/api/<path>` from a table, 404 with a refusal for any other path.
import { vi } from "vitest";

export function stubApi(table: Record<string, unknown>): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const path = String(url).replace(/^\/api\//, "");
      const hit = path in table;
      const body = hit ? table[path] : { code: "no_node", subject: path, rule: "no such route", fix: "bp list" };
      return new Response(JSON.stringify(body), { status: hit ? 200 : 404 });
    }),
  );
}
