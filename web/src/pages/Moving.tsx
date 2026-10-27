// Moving (18, Moving; 32, design change 7; mock `moving.html`): one screen, five groups in the order the human acts.
// Every word on a row comes from the server: the intent, the holder, the head in words, and the command or the wait.
// The page only lays them out.
import { useApi } from "../api";
import { Command } from "../Command";
import { many, words } from "../intents";
import { Loaded } from "../Loaded";
import { Link, changePath, nodePath } from "../router";
import { Row } from "../Row";
import type { Moving as MovingObject } from "../types/Moving";
import type { MovingGroup } from "../types/MovingGroup";
import type { MovingRow } from "../types/MovingRow";
import { forgeWords } from "./Home";

export function Moving({ project }: { project: string }) {
  const load = useApi<MovingObject>(`${project}/moving`);
  return (
    <>
      <h1>Moving</h1>
      <Loaded load={load}>{(m) => <Groups m={m} />}</Loaded>
    </>
  );
}

function Groups({ m }: { m: MovingObject }) {
  const rows = (gs: MovingGroup[]) => gs.reduce((n, g) => n + g.rows.length, 0);
  // The server sends the groups in page order, and the first two owe the human a decision (32, design change 7).
  const due = rows(m.groups.slice(0, 2));
  // The focal row is the first that carries a command: the first decision on the page.
  const focal = m.groups.flatMap((g) => g.rows).find((r) => r.command)?.id;
  return (
    <>
      <p className="sub">
        {many(due, "decision")} due · {many(rows(m.groups.slice(2)))} more moving · {forgeWords(m)}
      </p>
      {m.groups.map((g) => (
        <section key={g.group} aria-label={g.label}>
          <h2 className="group">
            {g.label}
            <span className="n">{g.rows.length}</span>
          </h2>
          {g.rows.length === 0 ? (
            <p className="empty">nothing</p>
          ) : (
            <table className="moving">
              <tbody>
                {g.rows.map((r) => (
                  <MovingRowView key={r.id} r={r} focal={r.id === focal} project={m.project} />
                ))}
              </tbody>
            </table>
          )}
        </section>
      ))}
    </>
  );
}

/** The mock's row card: the id in mono with its kind, the holder, the title over the state in words, then the
 * command chip or the wait. An issue's row opens its Change page; any other row its node. */
function MovingRowView({ r, focal, project }: { r: MovingRow; focal: boolean; project: string }) {
  const href = r.kind === "issue" ? changePath(project, r.id) : nodePath(r.id);
  return (
    <Row href={href} command={r.command} className={focal ? "focal" : undefined}>
      <td className="id" title={r.id}>
        <span className="one">
          <Link to={href}>{r.id.split("/").slice(-1)[0]}</Link>
        </span>
        <span className="kind">{r.kind}</span>
      </td>
      <td className="holder">
        <span className="one">{r.holder ?? "—"}</span>
      </td>
      <td title={r.title}>
        <span className="title one">{r.title ?? "untitled"}</span>
        <span className="state one">
          {words(r.intent)}
          {r.head ? ` · ${r.head}` : ""}
        </span>
      </td>
      <td className="next">{r.command ? <Command text={r.command} /> : <span className="wait">{r.wait}</span>}</td>
    </Row>
  );
}
