//! The API's routes (32, design changes 2 and 6): the path after `/api/` names one page object.

/// One `/api/` request.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// `/api/health`: the server answers.
    Health,
    /// `/api/projects`: the `list project` rows, each with its version and intents.
    Projects,
    /// `/api/<project>`: the project's home.
    Home(String),
    /// `/api/<project>/moving`: the project's rows with an intent, in five groups.
    Moving(String),
    /// `/api/<project>/change/<issue>[?diff=1]`: one issue's gates, stack, deltas and diff.
    Change {
        /// The project of the route.
        project: String,
        /// The issue's id: the rest of the path.
        issue: String,
        /// `?diff=1`: the page carries the diff text.
        diff: bool,
    },
    /// `/api/<project>/milestone/<id>`: one milestone's checklist.
    Milestone {
        /// The project of the route.
        project: String,
        /// The milestone's id: the rest of the path.
        id: String,
    },
    /// `/api/<project>/design/<id>[?at=<tag>]`: one design's page.
    Design {
        /// The project of the route.
        project: String,
        /// The design's id: the rest of the path.
        id: String,
        /// `?at=<tag>`; the caller sets it from the query.
        at: Option<String>,
    },
    /// `/api/<project>/design/<id>/diagram/<name>`: the SVG of one diagram of the design, for the page to inline.
    Diagram {
        /// The project of the route.
        project: String,
        /// The design's id.
        id: String,
        /// The diagram's name.
        name: String,
    },
    /// `/api/node/<id>`: the `show` object of any node or record; the id is the rest of the path.
    Node(String),
    /// `/api/search?q=<terms>`: the `search` object.
    Search(String),
}

impl Route {
    /// The route of `path`, the text after `/api/`, with the query's `q`. `None` names no route. A change route
    /// asks for no diff text; the caller sets `diff` from the query.
    pub fn parse(path: &str, q: Option<&str>) -> Option<Self> {
        // The project is the first segment; the id is the rest of the path after the page's name.
        let page = |name: &str| {
            let (project, id) = path.split_once(name)?;
            (!project.is_empty() && !project.contains('/') && !id.is_empty()).then(|| (project.to_string(), id))
        };
        if let Some((project, issue)) = page("/change/") {
            return Some(Self::Change { project, issue: issue.into(), diff: false });
        }
        if let Some((project, id)) = page("/milestone/") {
            return Some(Self::Milestone { project, id: id.into() });
        }
        if let Some((project, rest)) = page("/design/") {
            // A diagram's name holds no `/`, so the last `/diagram/` splits it from a design id that may hold one.
            return Some(match rest.rsplit_once("/diagram/") {
                Some((id, name)) if !id.is_empty() && !name.is_empty() && !name.contains('/') => {
                    Self::Diagram { project, id: id.into(), name: name.into() }
                }
                _ => Self::Design { project, id: rest.into(), at: None },
            });
        }
        match path.split_once('/') {
            None if path == "health" => Some(Self::Health),
            None if path == "projects" => Some(Self::Projects),
            None if path == "search" => Some(Self::Search(q.unwrap_or_default().to_string())),
            None if !path.is_empty() => Some(Self::Home(path.to_string())),
            Some(("node", id)) if !id.is_empty() => Some(Self::Node(id.to_string())),
            Some((project, "moving")) if !project.is_empty() => Some(Self::Moving(project.to_string())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_path_names_its_route() {
        assert_eq!(Route::parse("health", None), Some(Route::Health));
        assert_eq!(Route::parse("projects", None), Some(Route::Projects));
        assert_eq!(Route::parse("bp", None), Some(Route::Home("bp".into())));
        assert_eq!(Route::parse("bp/moving", None), Some(Route::Moving("bp".into())));
        assert_eq!(Route::parse("search", Some("vault")), Some(Route::Search("vault".into())));
        assert_eq!(Route::parse("search", None), Some(Route::Search(String::new())), "the search refuses it");
        let id = "projects/bp/issues/0001.a/claims/s1";
        assert_eq!(Route::parse(&format!("node/{id}"), None), Some(Route::Node(id.into())), "the id takes the rest");
        let change = |issue: &str| Route::Change { project: "bp".into(), issue: issue.into(), diff: false };
        assert_eq!(
            Route::parse("bp/change/projects/bp/issues/0411.x", None),
            Some(change("projects/bp/issues/0411.x"))
        );
        assert_eq!(Route::parse("bp/change/x", None), Some(change("x")), "the page refuses an id that is no issue");
        let m = "projects/bp/milestones/m";
        assert_eq!(
            Route::parse(&format!("bp/milestone/{m}"), None),
            Some(Route::Milestone { project: "bp".into(), id: m.into() })
        );
        let d = "projects/bp/designs/containers/view";
        let design = Route::Design { project: "bp".into(), id: d.into(), at: None };
        assert_eq!(Route::parse(&format!("bp/design/{d}"), None), Some(design));
        assert_eq!(
            Route::parse(&format!("bp/design/{d}/diagram/context"), None),
            Some(Route::Diagram { project: "bp".into(), id: d.into(), name: "context".into() })
        );
        let odd = Route::Design { project: "bp".into(), id: format!("{d}/diagram/a/b"), at: None };
        assert_eq!(Route::parse(&format!("bp/design/{d}/diagram/a/b"), None), Some(odd), "the page refuses it");
        for none in [
            "",
            "node/",
            "/moving",
            "bp/moving/x",
            "bp/change/",
            "/change/x",
            "a/b/change/x",
            "bp/design/",
            "/milestone/x",
        ] {
            assert_eq!(Route::parse(none, None), None, "{none}");
        }
    }
}
