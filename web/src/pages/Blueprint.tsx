// The blueprint pane (18, Project home 1; mock `project-anvil.html`): coverage per concern, then the design tree.
// Product, system and containers are rows; each parent's components fold under it with their count. Data model,
// glossary and conventions stand beside the tree; retired and deprecated designs sit behind one disclosure.
import { Fragment } from "react";
import { many, words } from "../intents";
import { Link, nodePath, pagePath } from "../router";
import { Row } from "../Row";
import type { Home } from "../types/Home";
import type { HomeDesign } from "../types/HomeDesign";
import { VersionWords } from "./Design";

const GONE = new Set(["retired", "deprecated"]);

/** The tree's levels, from each row's `design_kind`, which the path names (12, Layout). */
export function tree(designs: HomeDesign[]) {
  const live = designs.filter((d) => !GONE.has(d.status ?? ""));
  const of = (k: string) => live.filter((d) => d.design_kind === k);
  const containers = of("container");
  const components = of("component");
  const held = new Set(containers.map((c) => c.id));
  return {
    product: of("product")[0],
    system: of("system")[0],
    containers: containers.map((c) => ({ design: c, components: components.filter((x) => x.under === c.id) })),
    // A component under the system, or under no container the tree shows, folds under the system.
    loose: components.filter((x) => !x.under || !held.has(x.under)),
    dataModel: of("data-model")[0],
    glossary: of("glossary")[0],
    gone: designs.filter((d) => GONE.has(d.status ?? "")),
  };
}

function DesignRow({ d, depth }: { d: HomeDesign; depth: number }) {
  const dim = d.status !== "active" ? "r" : undefined;
  return (
    <Row href={pagePath(d.id, "design")}>
      <td className={depth ? `d${depth}` : undefined}>
        <span className="id">
          <Link to={pagePath(d.id, "design")}>{d.id.split("/").slice(-1)[0]}</Link>
        </span>
      </td>
      <td className={dim}>{d.title ?? "untitled"}</td>
      <td className="k">{d.design_kind ?? d.kind}</td>
      <td className={dim}>
        {d.status ?? "no status"}
        {d.intent ? `, ${words(d.intent)}` : ""}
      </td>
      <td className="sub">{d.diagrams ? `${d.diagrams} diagram${d.diagrams === 1 ? "" : "s"}` : "no diagrams"}</td>
      <td className="sub">
        <VersionWords words={d.version} tag={d.version_tag} />
      </td>
    </Row>
  );
}

function Fold({ noun, rows, depth }: { noun: string; rows: HomeDesign[]; depth: number }) {
  if (rows.length === 0) return null;
  return (
    <tr>
      <td colSpan={6} style={{ padding: `0 0 0 ${24 * depth}px` }}>
        <details className="sub">
          <summary>{many(rows.length, noun)}</summary>
          <table className="tree">
            <tbody>
              {rows.map((d) => (
                <DesignRow key={d.id} d={d} depth={0} />
              ))}
            </tbody>
          </table>
        </details>
      </td>
    </tr>
  );
}

function View({ label, d }: { label: string; d?: HomeDesign }) {
  return (
    <>
      {label}{" "}
      {d ? (
        <Link to={pagePath(d.id, "design")} className="id">
          {d.id.split("/").slice(-1)[0]}
        </Link>
      ) : (
        "none"
      )}
    </>
  );
}

export function BlueprintPane({ home }: { home: Home }) {
  const t = tree(home.designs);
  const shown = home.designs.length - t.gone.length;
  return (
    <details className="pane">
      <summary>
        <span className="t">Blueprint</span>
        <span className="n">
          {many(shown, "design")} in the tree{t.gone.length ? ` · ${t.gone.length} retired or deprecated` : ""} · coverage per
          concern: not in the vault
        </span>
      </summary>
      <div className="in">
        {home.designs.length === 0 ? (
          <p className="empty">nothing</p>
        ) : (
          <table className="tree">
            <tbody>
              {t.product && <DesignRow d={t.product} depth={0} />}
              {t.system && <DesignRow d={t.system} depth={1} />}
              <Fold noun="component" rows={t.loose} depth={2} />
              {t.containers.map(({ design, components }) => (
                <Fragment key={design.id}>
                  <DesignRow d={design} depth={2} />
                  <Fold noun="component" rows={components} depth={3} />
                </Fragment>
              ))}
            </tbody>
          </table>
        )}
        {t.gone.length > 0 && (
          <details className="sub">
            <summary>{t.gone.length} retired or deprecated</summary>
            <table className="tree">
              <tbody>
                {t.gone.map((d) => (
                  <DesignRow key={d.id} d={d} depth={0} />
                ))}
              </tbody>
            </table>
          </details>
        )}
        <p className="sub views">
          views beside the tree: <View label="data model" d={t.dataModel} /> · <View label="glossary" d={t.glossary} /> ·
          conventions{" "}
          {home.conventions.length === 0
            ? "none"
            : home.conventions.map((c, i) => (
                <Fragment key={c}>
                  {i ? ", " : ""}
                  <Link to={nodePath(c)} className="id">
                    {c.split("/").slice(-1)[0]}
                  </Link>
                </Fragment>
              ))}
        </p>
      </div>
    </details>
  );
}
