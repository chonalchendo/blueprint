// Design (18, Design; 32, design changes 8 and 9; mock `design-system.html`): disclosure first. The title, the
// version banner, the facts, coverage per concern on the product design, the TL;DR and the diagrams it carries; then
// the body behind one disclosure, each diagram inline where the body cites it, the outline fixed at the right; then
// the deltas proposed against it. At a tag (`?at=`) the body is the tag's, and the sections changed since sit above
// it. Every word past the body comes from the server.
import { useApi } from "../api";
import { Document } from "../doc/Document";
import { html } from "../doc/markdown";
import { header } from "../header";
import { many } from "../intents";
import { Loaded } from "../Loaded";
import { Link, nodePath, pagePath } from "../router";
import type { BodyDiff } from "../types/BodyDiff";
import type { Concern } from "../types/Concern";
import type { Design as DesignObject } from "../types/Design";
import type { Heading } from "../types/Heading";

const enc = (id: string) => id.split("/").map(encodeURIComponent).join("/");

/** The API path of a design's page, and of its diagrams under it. */
const api = (project: string, id: string) => `${project}/design/${enc(id)}`;

/** The query of a tag: a tag's `/` reads as itself. */
const atQuery = (tag: string) => `?at=${encodeURIComponent(tag).replace(/%2F/g, "/")}`;

export function Design({ project, id, at }: { project: string; id: string; at?: string }) {
  const load = useApi<DesignObject>(api(project, id) + (at ? atQuery(at) : ""));
  return <Loaded load={load}>{(d) => <DesignView d={d} project={project} at={at} />}</Loaded>;
}

export function DesignView({ d, project, at }: { d: DesignObject; project: string; at?: string }) {
  const h = header(d);
  const under = typeof d.under === "string" ? d.under : undefined;
  const sections = d.outline.filter((o) => o.depth === 2).length;
  const proposed = d.deltas.filter((x) => x.state === "proposed").length;
  // `show` carries the diagrams of a design only, so the type has them optional; a design page always has them.
  const diagrams = d.diagrams ?? [];
  const figures = diagrams.map((g) => ({ name: g.name, title: g.title, src: `/api/${api(project, d.id)}/diagram/${encodeURIComponent(g.name)}` }));
  const absent = (kind: string) => d.rows_not_in_vault.includes(kind);
  const here = `/${project}/design/${d.id}`;
  // The lead and the TL;DR show above the body, so the body leaves them out.
  const tldr = d.tldr ? d.outline.find((o) => o.depth === 2 && o.heading === "TL;DR")?.anchor : undefined;
  const shown = tldr ? ["", tldr] : [""];
  return (
    <article className="design">
      <h1>{h.title ?? d.id.split("/").slice(-1)[0]}</h1>
      <div className="vbanner" role="note">
        <span className="v">
          <VersionWords words={d.version} tag={d.version_tag} />
        </span>
        <span>status {h.status ?? "none"}</span>
        <span>{absent("delta") ? "deltas: not in vault" : `${many(proposed, "delta")} proposed`}</span>
        {d.since && (
          <Link to={here + atQuery(d.since)}>
            diff since <span className="id">{d.since}</span>
          </Link>
        )}
        {d.tag_fix && <span className="sub">{d.tag_fix}</span>}
        {at && <Link to={here}>current body</Link>}
      </div>
      <p className="facts-line">
        <span>
          level <b>{d.level}</b>
        </span>
        <span>
          under <b>{under ? <Link to={pagePath(under, "design")}>{under.split("/").slice(-1)[0]}</Link> : "nothing"}</b>
        </span>
        <span>
          <b>{many(diagrams.length, "diagram")}</b>
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
        shown={shown}
      >
        {at && d.diff && d.version === `at ${at}` && <Changes diff={d.diff} tag={at} outline={d.outline} />}
        <h2 className="group" id={tldr}>
          TL;DR
        </h2>
        {d.lead || d.tldr ? (
          <>
            {d.lead && <div className="tldr" data-body="" dangerouslySetInnerHTML={{ __html: html(d.lead) }} />}
            {d.tldr && <div className="tldr sub" data-body="" dangerouslySetInnerHTML={{ __html: html(d.tldr) }} />}
          </>
        ) : (
          <p className="empty">nothing</p>
        )}
        <h2 className="group">
          Diagrams<span className="n">{diagrams.length}</span>
        </h2>
        {diagrams.length === 0 ? (
          <p className="empty">nothing</p>
        ) : (
          <div className="gates strip">
            {diagrams.map((g) => (
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

/** A version in the server's words, with the tag they end with in mono (19: an id reads in Geist Mono). */
export function VersionWords({ words, tag }: { words: string; tag?: string }) {
  if (!tag || !words.endsWith(tag)) return <>{words}</>;
  return (
    <>
      {words.slice(0, -tag.length)}
      <span className="id">{tag}</span>
    </>
  );
}

/** The sections changed since `tag` (18, As-of diff; 19: added and removed lines). Each heading jumps to its section
 * in the body at the tag, when the tag holds it; under it, the removed lines, then the added, each marked by a `−`
 * or `+`, not by colour alone. */
export function Changes({ diff, tag, outline }: { diff: BodyDiff; tag: string; outline: Heading[] }) {
  const lines = (of: BodyDiff["added"], section: string) => of.find((x) => x.section === section)?.lines ?? [];
  return (
    <>
      <h2 className="group">
        Changed since {tag}
        <span className="n">{many(diff.sections.length, "section")}</span>
      </h2>
      {diff.sections.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <ul className="asof">
          {diff.sections.map((s, i) => {
            const anchor = outline.find((o) => o.depth === 2 && o.heading === s)?.anchor;
            const name = s || "before the first heading";
            return (
              <li key={i}>
                {anchor ? <a href={`#${anchor}`}>{name}</a> : <span>{name}</span>}
                <div className="lines">
                  {lines(diff.removed, s).map((l, j) => (
                    <div key={`r${j}`} className="d">
                      − {l}
                    </div>
                  ))}
                  {lines(diff.added, s).map((l, j) => (
                    <div key={`a${j}`} className="a">
                      + {l}
                    </div>
                  ))}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </>
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
