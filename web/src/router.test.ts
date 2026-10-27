import { expect, test } from "vitest";
import { crumbs } from "./App";
import { route } from "./router";

test("each path names its page, and an id keeps its slashes and dots", () => {
  expect(route("/")).toEqual({ page: "projects" });
  expect(route("/bp")).toEqual({ page: "home", project: "bp" });
  expect(route("/node/projects/bp/issues/0411.x")).toEqual({ page: "node", id: "projects/bp/issues/0411.x" });
  expect(route("/search", "?q=vault%20index")).toEqual({ page: "search", q: "vault index" });
  expect(route("/bp/moving")).toEqual({ page: "moving", project: "bp" });
  expect(route("/bp/change/projects/bp/issues/0411.x")).toEqual({ page: "change", project: "bp", id: "projects/bp/issues/0411.x" });
  expect(route("/bp/change/..")).toEqual({ page: "none", path: "/bp/change/..", project: "bp" });
  expect(route("/bp/other/0411")).toEqual({ page: "none", path: "/bp/other/0411", project: "bp" });
});

test("a malformed escape or a dot segment names no page", () => {
  expect(route("/node/projects/bp/%E0")).toEqual({ page: "none", path: "/node/projects/bp/%E0" });
  const up = "/node/projects%2F..%2F..%2Fapi%2Fprojects";
  expect(route(up)).toEqual({ page: "none", path: up });
  expect(route("/node/projects/bp/designs/system%3Fx")).toEqual({ page: "node", id: "projects/bp/designs/system?x" });
});

test("the crumb trail is the route", () => {
  expect(crumbs(route("/bp"))).toEqual([["projects", "/"], ["bp", "/bp"]]);
  expect(crumbs(route("/bp/moving"))).toEqual([["projects", "/"], ["bp", "/bp"], ["moving", "/bp/moving"]]);
  expect(crumbs(route("/node/projects/bp/designs/system"))).toEqual([
    ["projects", "/"],
    ["bp", "/bp"],
    ["system", "/node/projects/bp/designs/system"],
  ]);
  expect(crumbs(route("/node/shared/conventions/c"))).toEqual([["projects", "/"], ["c", "/node/shared/conventions/c"]]);
  expect(crumbs(route("/bp/change/projects/bp/issues/0411.x"))).toEqual([
    ["projects", "/"],
    ["bp", "/bp"],
    ["change 0411.x", "/bp/change/projects/bp/issues/0411.x"],
  ]);
});
