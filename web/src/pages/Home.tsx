// Project home (18, Project home; mock `project-anvil.html`): three panes, each one summary line with counts that
// opens in place. Blueprint, then the loop of milestones, then what moves. Only Moving opens by default: it holds what
// the level decides, so the page fits one screen however large the tree (18, check 5).
import { useApi } from "../api";
import { DECISION_GROUPS, GROUPS, OTHER_GROUPS, count, many, words } from "../intents";
import { Loaded } from "../Loaded";
import { Link, pagePath } from "../router";
import { Row } from "../Row";
import type { Forge } from "../types/Forge";
import type { Home as HomeObject } from "../types/Home";
import type { HomeMilestone } from "../types/HomeMilestone";
import { BlueprintPane } from "./Blueprint";

const OPEN = new Set(["planned", "approved"]);

export function Home({ project }: { project: string }) {
  const load = useApi<HomeObject>(project);
  return (
    <>
      <h1>{project}</h1>
      <Loaded load={load}>
        {(home) => (
          <>
            <p className="sub">
              {home.version ? (
                <>
                  blueprint <span className="id">{home.version}</span>
                </>
              ) : (
                "no blueprint version"
              )}{" "}
              · {many(home.designs.length, "design")} · {many(home.milestones.filter(isOpen).length, "open milestone")} ·{" "}
              {moving(home)}
            </p>
            <BlueprintPane home={home} />
            <LoopPane home={home} />
            <MovingPane home={home} project={project} />
          </>
        )}
      </Loaded>
    </>
  );
}

const isOpen = (m: HomeMilestone) => OPEN.has(m.status ?? "");

/** Decisions due are the two decision groups; the rest moving is the other three, as on Projects. */
function moving(home: HomeObject): string {
  return `${many(count(home.intents, DECISION_GROUPS), "decision")} due · ${many(count(home.intents, OTHER_GROUPS))} more moving`;
}

/** A milestone's issues in words: "2 ready, 1 working", or "no issues". */
export function issueWords(m: HomeMilestone): string {
  const parts = Object.entries(m.issues).map(([k, n]) => `${n} ${words(k)}`);
  return parts.length ? parts.join(", ") : "no issues";
}

function MilestoneRow({ m }: { m: HomeMilestone }) {
  return (
    <Row href={pagePath(m.id, "milestone")}>
      <td className="id">
        <Link to={pagePath(m.id, "milestone")}>{m.id.split("/").slice(-1)[0]}</Link>
      </td>
      <td>{m.title ?? "untitled"}</td>
      <td>
        {m.status ?? "no status"}
        {m.intent ? `, ${words(m.intent)}` : ""}
      </td>
      <td className="sub">{issueWords(m)}</td>
    </Row>
  );
}

/** The loop pane (18, Project home 2): open milestones first; done and abandoned behind one disclosure. */
function LoopPane({ home }: { home: HomeObject }) {
  const open = home.milestones.filter(isOpen);
  const closed = home.milestones.filter((m) => !isOpen(m));
  return (
    <details className="pane">
      <summary>
        <span className="t">Loop</span>
        <span className="n">
          {many(open.length, "open milestone")} · {many(closed.length)} done or abandoned
        </span>
      </summary>
      <div className="in">
        {open.length === 0 ? (
          <p className="empty">no open milestone: nothing</p>
        ) : (
          <table className="tree">
            <tbody>
              {open.map((m) => (
                <MilestoneRow key={m.id} m={m} />
              ))}
            </tbody>
          </table>
        )}
        {closed.length > 0 && (
          <details className="sub">
            <summary>{closed.length} done or abandoned</summary>
            <table className="tree">
              <tbody>
                {closed.map((m) => (
                  <MilestoneRow key={m.id} m={m} />
                ))}
              </tbody>
            </table>
          </details>
        )}
      </div>
    </details>
  );
}

/** One row per group: its count, then only the intents that hold a count, each with its count. */
function GroupRows({ groups, home, project }: { groups: typeof GROUPS; home: HomeObject; project: string }) {
  return (
    <table className="tree">
      <tbody>
        {groups.map((g) => {
          const n = count(home.intents, [g]);
          return (
            <Row key={g.group} href={`/${project}/moving`}>
              <td>{g.group}</td>
              <td>{n ? <strong>{n}</strong> : "nothing"}</td>
              <td className="sub">
                {g.intents
                  .filter((i) => home.intents[i])
                  .map((i) => `${home.intents[i]} ${words(i)}`)
                  .join(", ")}
              </td>
            </Row>
          );
        })}
      </tbody>
    </table>
  );
}

/** The forge in words: the snapshot's age, or not read. Home and Moving carry the same two fields. */
export function forgeWords(page: { forge?: Forge; forge_age?: number }): string {
  if (page.forge === "snapshot") return `forge read ${page.forge_age ?? 0} s ago`;
  return page.forge === "not read" ? "forge not read" : "no forge read";
}

/** The moving pane (18, Project home 3): the two groups that owe a decision; the other three behind one disclosure. */
function MovingPane({ home, project }: { home: HomeObject; project: string }) {
  const rest = many(count(home.intents, OTHER_GROUPS));
  return (
    <details className="pane" open>
      <summary>
        <span className="t">Moving</span>
        <span className="n">
          {many(count(home.intents, DECISION_GROUPS), "decision")} due · {rest} more · {forgeWords(home)}
        </span>
      </summary>
      <div className="in">
        <GroupRows groups={DECISION_GROUPS} home={home} project={project} />
        <details className="sub">
          <summary>{rest} more: blocked, ready, working</summary>
          <GroupRows groups={OTHER_GROUPS} home={home} project={project} />
        </details>
        <p className="sub views">
          <Link to={`/${project}/moving`} className="more">
            open Moving
          </Link>
        </p>
      </div>
    </details>
  );
}
