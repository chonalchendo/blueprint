// What a page shows before its object arrives, and when the server refuses it.
import type { ReactNode } from "react";
import type { Load } from "./api";

export function Loaded<T>({ load, children }: { load: Load<T>; children: (data: T) => ReactNode }) {
  if (load.state === "loading") return <p className="sub">reading</p>;
  if (load.state === "refused") {
    const r = load.refusal;
    return (
      <div className="banner" role="alert">
        refused <span className="id">{r.code}</span>: {r.subject}. {r.rule}. Fix: <span className="id">{r.fix}</span>
      </div>
    );
  }
  return <>{children(load.data)}</>;
}
