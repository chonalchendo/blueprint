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
    /// `/api/node/<id>`: the `show` object of any node or record; the id is the rest of the path.
    Node(String),
    /// `/api/search?q=<terms>`: the `search` object.
    Search(String),
}

impl Route {
    /// The route of `path`, the text after `/api/`, with the query's `q`. `None` names no route.
    pub fn parse(path: &str, q: Option<&str>) -> Option<Self> {
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
        for none in ["", "node/", "/moving", "bp/moving/x", "bp/change/x"] {
            assert_eq!(Route::parse(none, None), None, "{none}");
        }
    }
}
