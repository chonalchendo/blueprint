// Milestone (18, Milestone; 32, design change 8; mock `milestone-*.html`): a checklist as a gate strip, then its rows:
// the finish line with each block's last run, the deltas it carries, the concerns and waivers, and the issues with
// their intents, each opening its Change page. It ends in the milestone row's own Moving command, the page's one chip,
// else the wait in words. A row whose record kind the vault lacks reads "not in vault" (18, Open). The body follows,
// folded; the rows sit above it inside the document, so a link to a body section opens the fold.
import { useEffect, useRef } from "react";
import { useApi } from "../api";
import { Command } from "../Command";
import { Document } from "../doc/Document";
import { outlineOf } from "../doc/markdown";
import { header } from "../header";
import { many, words } from "../intents";
import { Loaded } from "../Loaded";
import { Link, changePath, nodePath, pagePath } from "../router";
import { Row } from "../Row";
import type { FinishBlock } from "../types/FinishBlock";
import type { Milestone as MilestoneObject } from "../types/Milestone";
import { ConcernRows } from "./Design";

const tail = (id: string) => id.split("/").slice(-1)[0];

export function Milestone({ project, id }: { project: string; id: string }) {
  const load = useApi<MilestoneObject>(`${project}/milestone/${id.split("/").map(encodeURIComponent).join("/")}`);
  return <Loaded load={load}>{(m) => <MilestoneView m={m} project={project} />}</Loaded>;
}

/** A block's last run in words: "pass at 1a2b3c4 on 2026-10-27", "not run", or "not in vault" with no run record. */
function verdict(b: FinishBlock, absent: boolean): string {
  const r = b.last_run;
  if (!r) return absent ? "not in vault" : "not run";
  return `${r.verdict} at ${r.head.slice(0, 7) || "no commit"} on ${r.at.slice(0, 10)}`;
}

/** A `## Finish line` section that names no check block: its lines of prose and its anchor. The converted vault's
 * milestones hold their finish lines this way. */
function proseLine(body: string | undefined): { lines: number; anchor: string } | undefined {
  const anchor = outlineOf(body ?? "").find((h) => h.depth === 2 && h.heading === "Finish line")?.anchor;
  if (!body || !anchor) return undefined;
  const lines = body.split("\n");
  const after = lines.slice(lines.findIndex((l) => /^##\s+Finish line\s*$/.test(l)) + 1);
  const end = after.findIndex((l) => /^#{1,2}\s/.test(l));
  return { lines: (end < 0 ? after : after.slice(0, end)).filter((l) => l.trim()).length, anchor };
}

const tint = (word: string) => (word.startsWith("pass") ? "ok" : /^(fail|timed-out|re-asked|open)/.test(word) ? "bad" : "na");

export function MilestoneView({ m, project }: { m: MilestoneObject; project: string }) {
  const h = header(m);
  const absent = (kind: string) => m.rows_not_in_vault.includes(kind);
  const noRuns = absent("run");
  const passed = m.finish_line.filter((b) => b.last_run?.verdict === "pass").length;
  const settled = m.issues.filter((i) => i.status === "resolved" || i.status === "abandoned").length;
  const noConcerns = absent("concern-answer") && absent("waiver");
  const asked = m.concerns.filter((c) => c.state === "open" || c.state === "re-asked").length;
  const chip = useRef<HTMLDivElement>(null);
  // The chip is the page's one command, so `c` copies it as soon as the page shows (19, principle 6).
  useEffect(() => void (m.command && chip.current?.focus({ preventScroll: true })), [m.id, m.command]);
  const prose = m.finish_line.length === 0 ? proseLine(m.body) : undefined;
  const inProse = prose && `${many(prose.lines, "line")} in prose, no check block: stamp cannot run them`;
  const strip: [string, string][] = [
    ["Finish line", m.finish_line.length === 0 ? (prose ? `${many(prose.lines, "line")} in prose` : "no blocks") : noRuns ? "runs not in vault" : `${passed} of ${many(m.finish_line.length, "block")} pass`],
    ["Carried deltas", absent("delta") ? "not in vault" : many(m.deltas.length, "delta")],
    ["Concerns", noConcerns ? "not in vault" : asked ? `${asked} asked again` : many(m.concerns.length, "answer")],
    ["Issues", m.issues.length === 0 ? "no issues" : `${settled} of ${many(m.issues.length, "issue")} settled`],
  ];
  const rows = (
    <>
      <h2 className="group">Checklist</h2>
      <div className="gates strip">
        {strip.map(([k, v]) => (
          <div className="g" key={k}>
            <span className="k">{k}</span>
            <span className={/^(no |nothing|not in vault)/.test(v) ? "na" : ""}>{v}</span>
          </div>
        ))}
      </div>
      <h2 className="group">
        Finish line<span className="n">{many(m.finish_line.length, "block")}</span>
      </h2>
      {m.finish_line.length === 0 ? (
        prose ? (
          <p className="empty">
            <a href={`#${prose.anchor}`}>{inProse}</a>
          </p>
        ) : (
          <p className="empty">nothing: the body names no check block</p>
        )
      ) : (
        <table className="checklist">
          <tbody>
            {m.finish_line.map((b) => (
              <tr key={b.name}>
                <td className="id">{b.name}</td>
                <td className="sub">
                  check {b.kind} · {b.severity}
                </td>
                <td className={tint(verdict(b, noRuns))}>{verdict(b, noRuns)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <h2 className="group">Carried deltas</h2>
      {absent("delta") ? (
        <p className="empty">not in vault: the vault holds no delta record yet</p>
      ) : m.deltas.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <ul className="deltas">
          {m.deltas.map((d) => (
            <li key={d.id}>
              <Link to={nodePath(d.id)} className="id">
                {tail(d.id)}
              </Link>{" "}
              → <Link to={pagePath(d.target, "design")} className="id">{d.target}</Link> <span className="sub">{d.state}</span>
            </li>
          ))}
        </ul>
      )}
      <h2 className="group">Concerns and waivers</h2>
      {noConcerns ? (
        <p className="empty">not in vault: the vault holds no concern answer or waiver yet</p>
      ) : m.concerns.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <ConcernRows rows={m.concerns} />
      )}
      <h2 className="group">
        Issues<span className="n">{many(m.issues.length, "issue")}</span>
      </h2>
      {m.issues.length === 0 ? (
        <p className="empty">nothing</p>
      ) : (
        <table className="issues">
          <tbody>
            {m.issues.map((i) => (
              <Row key={i.id} href={changePath(project, i.id)}>
                <td className="id">
                  <Link to={changePath(project, i.id)}>{tail(i.id)}</Link>
                </td>
                <td>{i.title ?? "untitled"}</td>
                <td className={i.status === "resolved" ? "ok" : "na"}>
                  {i.intent ? words(i.intent) : (i.status ?? "no status")}
                  {i.note ? <span className="sub">: {i.note}</span> : null}
                </td>
              </Row>
            ))}
          </tbody>
        </table>
      )}
      <h2 className="group">Next</h2>
      <div ref={chip} className={m.command ? "land strong" : "land"} data-row="" data-command={m.command} tabIndex={-1}>
        {m.command ? <Command text={m.command} /> : <p className="wait">{m.note}</p>}
      </div>
    </>
  );
  return (
    <article className="milestone">
      <h1>
        Milestone <span className="id">{tail(m.id)}</span>
      </h1>
      <p className="sub">
        {h.title ?? "untitled"} · status {h.status ?? "none"} · {m.intent ? words(m.intent) : "no intent"} ·{" "}
        <Link to={nodePath(m.id)} className="id">
          {m.id}
        </Link>
      </p>
      {m.body ? (
        <Document body={m.body} fold={`Body · ${many(m.sections?.length ?? 0, "section")}`}>
          {rows}
        </Document>
      ) : (
        rows
      )}
    </article>
  );
}
