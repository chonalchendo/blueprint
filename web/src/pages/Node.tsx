// The node page (`/node/<id>`): any node or record by id, its header fields and its body as text. The full Design,
// Milestone and node pages are 32, issue 5.
import { useApi } from "../api";
import { facts, header } from "../header";
import { Loaded } from "../Loaded";
import type { Show } from "../types/Show";

export function Node({ id }: { id: string }) {
  const load = useApi<Show>(`node/${id}`);
  return <Loaded load={load}>{(show) => <NodeView show={show} />}</Loaded>;
}

export function NodeView({ show }: { show: Show }) {
  const h = header(show);
  return (
    <article>
      <h1>{h.title ?? h.id.split("/").slice(-1)[0]}</h1>
      <p className="sub">
        <span className="id">{h.id}</span> · {h.kind}
        {show.record ? " record" : ""} · {h.status ? `status ${h.status}` : "no status"}
      </p>
      <dl className="facts">
        {facts(show).map(([k, v]) => (
          <div key={k} style={{ display: "contents" }}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
      {show.body ? <pre className="body">{show.body}</pre> : <p className="empty">no body</p>}
    </article>
  );
}
