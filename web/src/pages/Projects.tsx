// Projects (18, Projects; mock `projects.html`): one row per project, with its version, the decisions due and the
// items moving.
import { useApi } from "../api";
import { DECISION_GROUPS, OTHER_GROUPS, count, many } from "../intents";
import { Loaded } from "../Loaded";
import { Link, projectOf } from "../router";
import { Row } from "../Row";
import type { Projects as ProjectsObject } from "../types/Projects";

export function Projects() {
  const load = useApi<ProjectsObject>("projects");
  return (
    <>
      <h1>Projects</h1>
      <p className="sub">one vault, one row per project: the decisions due and the items moving</p>
      <Loaded load={load}>
        {(p) =>
          p.rows.length === 0 ? (
            <p className="empty">nothing</p>
          ) : (
            <table>
              <thead>
                <tr>
                  <th>project</th>
                  <th>title</th>
                  <th>blueprint</th>
                  <th>decisions due</th>
                  <th>items moving</th>
                </tr>
              </thead>
              <tbody>
                {p.rows.map((row) => {
                  const name = projectOf(row.id) ?? row.id;
                  const due = count(row.intents, DECISION_GROUPS);
                  return (
                    <Row key={row.id} href={`/${name}`}>
                      <td className="id">
                        <Link to={`/${name}`}>{name}</Link>
                      </td>
                      <td>{row.title ?? "untitled"}</td>
                      <td className={row.version ? "id" : "na"}>{row.version ?? "no version"}</td>
                      <td>{due ? <strong>{many(due)}</strong> : "none"}</td>
                      <td>{many(count(row.intents, OTHER_GROUPS))}</td>
                    </Row>
                  );
                })}
              </tbody>
            </table>
          )
        }
      </Loaded>
    </>
  );
}
