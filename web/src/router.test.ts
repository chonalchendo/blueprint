import { expect, test } from "vitest";
import { crumbs } from "./App";
import { route } from "./router";

test("each path names its page, and an id keeps its slashes and dots", () => {
  expect(route("/")).toEqual({ page: "projects" });
  expect(route("/bp")).toEqual({ page: "home", project: "bp" });
  expect(route("/node/projects/bp/issues/0411.x")).toEqual({ page: "node", id: "projects/bp/issues/0411.x" });
  expect(route("/search", "?q=vault%20index")).toEqual({ page: "search", q: "vault index" });
  expect(route("/bp/moving")).toEqual({ page: "none", path: "/bp/moving", project: "bp" });
});

test("the crumb trail is the route", () => {
  expect(crumbs(route("/bp"))).toEqual([["projects", "/"], ["bp", "/bp"]]);
  expect(crumbs(route("/node/projects/bp/designs/system"))).toEqual([
    ["projects", "/"],
    ["bp", "/bp"],
    ["system", "/node/projects/bp/designs/system"],
  ]);
  expect(crumbs(route("/node/shared/conventions/c"))).toEqual([["projects", "/"], ["c", "/node/shared/conventions/c"]]);
});
