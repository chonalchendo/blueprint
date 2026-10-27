// The node page (`/node/<id>`): any node or record by id, its header fields, then its body as a document (19,
// principle 9), so a convention, a decision, a thread, a claim, a review or a run reads as the vault does. A
// project's milestone and design have pages of their own, which the line under the title links.
import { useApi } from "../api";
import { Document } from "../doc/Document";
import { facts, header } from "../header";
import { Loaded } from "../Loaded";
import { Link, nodePath, pagePath } from "../router";
import type { Show } from "../types/Show";

export function Node({ id }: { id: string }) {
  // Each segment is encoded, so a `?` or `#` in an id stays in the path the server reads.
  const load = useApi<Show>(`node/${id.split("/").map(encodeURIComponent).join("/")}`);
  return <Loaded load={load}>{(show) => <NodeView show={show} />}</Loaded>;
}

export function NodeView({ show }: { show: Show }) {
  const h = header(show);
  const page = pagePath(h.id, h.kind);
  return (
    <article className="node">
      <h1>{h.title ?? h.id.split("/").slice(-1)[0]}</h1>
      <p className="sub">
        <span className="id">{h.id}</span> · {h.kind}
        {show.record ? " record" : ""} · {h.status ? `status ${h.status}` : "no status"}
        {page !== nodePath(h.id) && (
          <>
            {" "}
            · <Link to={page}>the {h.kind} page</Link>
          </>
        )}
      </p>
      <dl className="facts">
        {facts(show).map(([k, v]) => (
          <div key={k} style={{ display: "contents" }}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
      {show.body ? <Document body={show.body} /> : <p className="empty">no body</p>}
    </article>
  );
}
