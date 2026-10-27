// The client reads `/api/<route>` and nothing else (18, Interfaces 4). A refusal arrives as the tool's envelope.
import { useEffect, useState } from "react";

export type Refusal = { code: string; subject: string; rule: string; fix: string };

export type Load<T> = { state: "loading" } | { state: "ok"; data: T } | { state: "refused"; refusal: Refusal };

/** The page object at `/api/<path>`, read again whenever the path changes. */
export function useApi<T>(path: string): Load<T> {
  const [load, setLoad] = useState<{ path: string; load: Load<T> }>({ path, load: { state: "loading" } });
  useEffect(() => {
    let live = true;
    read<T>(path).then((l) => live && setLoad({ path, load: l }));
    return () => {
      live = false;
    };
  }, [path]);
  return load.path === path ? load.load : { state: "loading" };
}

export async function read<T>(path: string): Promise<Load<T>> {
  try {
    const res = await fetch(`/api/${path}`);
    const body = await res.json();
    if (res.ok) return { state: "ok", data: body as T };
    return { state: "refused", refusal: body as Refusal };
  } catch (e) {
    const why = e instanceof Error ? e.message : String(e);
    return { state: "refused", refusal: { code: "unreachable", subject: `/api/${path}`, rule: why, fix: "start bp-view, then reload" } };
  }
}
