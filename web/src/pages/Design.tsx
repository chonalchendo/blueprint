// Design (18, Design; 32, design change 8; mock `design-system.html`): disclosure first. The title, the version
// banner, the facts, coverage per concern on the product design, the TL;DR and the diagrams it carries; then the body
// behind one disclosure, each diagram inline where the body cites it, the outline fixed at the right; then the
// deltas proposed against it. Every word past the body comes from the server.
import { useApi } from "../api";
import { Document } from "../doc/Document";
import { html } from "../doc/markdown";
import { header } from "../header";
import { many } from "../intents";
import { Loaded } from "../Loaded";
import { Link, nodePath, pagePath } from "../router";
import type { Concern } from "../types/Concern";
import type { Design as DesignObject } from "../types/Design";

const enc = (id: string) => id.split("/").map(encodeURIComponent).join("/");

/** The API path of a design's page, and of its diagrams under it. */
const api = (project: string, id: string) => `${project}/design/${enc(id)}`;

export function Design({ project, id, at }: { project: string; id: string; at?: string }) {
  const load = useApi<DesignObject>(api(project, id) + (at ? `?at=${encodeURIComponent(at)}` : ""));
  return <Loaded load={load}>{(d) => <DesignView d={d} project={project} />}</Loaded>;
}

export function DesignView({ d, project }: { d: DesignObject; project: string }) {
  const h = header(d);
  const under = typeof d.under === "string" ? d.under : undefined;
  const sections = d.outline.filter((o) => o.depth === 2).length;
  const proposed = d.deltas.filter((x) => x.state === "proposed").length;
  const figures = d.diagrams.map((g) => ({ name: g.name, title: g.title, src: `/api/${api(project, d.id)}/diagram/${encodeURIComponent(g.name)}` }));
  const absent = (kind: string) => d.rows_not_in_vault.includes(kind);
  return (
    <article className="design">
      <h1>{h.title ?? d.id.split("/").slice(-1)[0]}</h1>
      <div className="vbanner" role="note">
        <span className="v">{d.version}</span>
        <span>status {h.status ?? "none"}</span>
        <span>{absent("delta") ? "deltas: not in vault" : `${many(proposed, "delta")} proposed`}</span>
      </div>
      <p className="facts-line">
        <span>
          level <b>{d.level}</b>
        </span>
        <span>
          under <b>{under ? <Link to={pagePath(under, "design")}>{under.split("/").slice(-1)[0]}</Link> : "nothing"}</b>
        </span>
        <span>
          <b>{many(d.diagrams.length, "diagram")}</b>
        </span>
        <span>
          body <b>{many(sections, "section")}</b>
        </span>
      </p>
      {d.coverage && <Coverage rows={d.coverage} absent={absent("concern-answer")} />}
      <Document
        body={d.body ?? ""}
        outline={d.outline}
        diagrams={figures}
        fold={`Body · ${many(sections, "section")}, with each diagram where the body cites it`}
      >
        <h2 className="group">TL;DR</h2>
        {d.lead || d.tldr ? (
          <>
            {d.lead && <div className="tldr" data-body="" dangerouslySetInnerHTML={{ __html: html(d.lead) }} />}
            {d.tldr && <div className="tldr sub" data-body="" dangerouslySetInnerHTML={{ __html: html(d.tldr) }} />}
          </>
        ) : (
          <p className="empty">nothing</p>
        )}
        <h2 className="group">
          Diagrams<span className="n">{d.diagrams.length}</span>
        </h2>
        {d.diagrams.length === 0 ? (
          <p className="empty">nothing</p>
        ) : (
          <div className="gates strip">
            {d.diagrams.map((g) => (
              <a className="g" key={g.name} href={`#fig-${g.name}`}>
                <span className="k">{g.type ?? "diagram"}</span>
                <span className="id">{g.name}</span>
              </a>
            ))}
          </div>
        )}
      </Document>
      <h2 className="group">
        Deltas against it<span className="n">{absent("delta") ? "not in vault" : d.deltas.length}</span>
      </h2>
      {absent("delta") ? (
        <p className="empty">not in vault: the vault holds no delta record yet</p>
      ) : d.deltas.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <ul className="deltas">
          {d.deltas.map((x) => (
            <li key={x.id}>
              <Link to={nodePath(x.id)} className="id">
                {x.id.split("/").slice(-1)[0]}
              </Link>{" "}
              <span className="sub">{x.state}</span>
              {x.cause && (
                <>
                  {" "}
                  · caused by <Link to={nodePath(x.cause)} className="id">{x.cause.split("/").slice(-1)[0]}</Link>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}

/** Coverage per concern (18, Design): each concern's latest answer, or "not in vault" while no record exists. */
export function Coverage({ rows, absent }: { rows: Concern[]; absent: boolean }) {
  return (
    <>
      <h2 className="group">
        Coverage per concern<span className="n">{absent ? "not in vault" : rows.length}</span>
      </h2>
      {absent ? (
        <p className="empty">not in vault: the vault holds no concern answer yet</p>
      ) : rows.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <ConcernRows rows={rows} />
      )}
    </>
  );
}

export function ConcernRows({ rows }: { rows: Concern[] }) {
  return (
    <table className="concerns">
      <tbody>
        {rows.map((c) => (
          <tr key={`${c.key} ${c.state}`}>
            <td className="id">{c.key}</td>
            <td className={c.state === "designed" ? "ok" : c.state === "open" || c.state === "re-asked" ? "bad" : "na"}>{c.state}</td>
            <td>{c.answer ?? "no answer"}</td>
            <td className="sub">{c.valid_until ? `until ${c.valid_until}` : c.state === "waived" ? "asked again at the next stamp" : ""}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
