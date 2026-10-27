// `/search?q=`: the tool's search, one row per hit.
import { useApi } from "../api";
import { Loaded } from "../Loaded";
import { Snippet } from "../Palette";
import { Link, nodePath } from "../router";
import { Row } from "../Row";
import type { Search } from "../types/Search";

export function SearchPage({ q }: { q: string }) {
  const load = useApi<Search>(`search?q=${encodeURIComponent(q)}`);
  return (
    <>
      <h1>Search</h1>
      <p className="sub">
        hits for <span className="id">{q}</span>, best first
      </p>
      <Loaded load={load}>
        {(s) =>
          s.rows.length === 0 ? (
            <p className="empty">nothing</p>
          ) : (
            <table>
              <tbody>
                {s.rows.map((h) => (
                  <Row key={h.id} href={nodePath(h.id)}>
                    <td className="id">
                      <Link to={nodePath(h.id)}>{h.id}</Link>
                    </td>
                    <td className="k">{h.kind}</td>
                    <td>
                      <Snippet text={h.snippet} />
                    </td>
                  </Row>
                ))}
              </tbody>
            </table>
          )
        }
      </Loaded>
    </>
  );
}
