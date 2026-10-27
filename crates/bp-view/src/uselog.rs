//! The use log (18, Interfaces 3; 32, design change 10): `.bp/use.log` under the vault, one line per page object
//! served. Append only; nothing leaves the machine.

use std::io::Write as _;
use std::path::Path;

/// One line: the UTC time, the page route and the id, or `-`.
pub fn line(now: i64, route: &str, id: &str) -> String {
    format!("{} {route} {id}\n", bp_read::clock::instant(now))
}

/// Appends `line` to the vault's use log.
pub fn append(root: &Path, line: &str) -> std::io::Result<()> {
    let path = root.join(".bp/use.log");
    std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_utc_route_and_id() {
        assert_eq!(line(1_792_281_600, "/bp/moving", "-"), "2026-10-18T00:00:00Z /bp/moving -\n");
        assert_eq!(
            line(0, "/node/projects/bp/project", "projects/bp/project"),
            "1970-01-01T00:00:00Z /node/projects/bp/project projects/bp/project\n"
        );
    }

    #[test]
    fn append_keeps_every_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".bp")).unwrap();
        append(dir.path(), &line(0, "/bp", "-")).unwrap();
        append(dir.path(), &line(1, "/", "-")).unwrap();
        let text = std::fs::read_to_string(dir.path().join(".bp/use.log")).unwrap();
        assert_eq!(text, "1970-01-01T00:00:00Z /bp -\n1970-01-01T00:00:01Z / -\n");
    }
}
