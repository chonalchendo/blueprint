//! Rebuilds the crate when `web/dist` changes: the release binary embeds it (`client.rs`), and the derive alone tracks
//! only the files it embedded, so a binary built before `npm run build` would keep an empty client. Cargo counts a
//! missing watched path as changed on every build, so the script makes the (gitignored) directory first; an empty
//! one embeds nothing, and the placeholder serves.

fn main() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    // An error leaves the path missing, and cargo then rebuilds every time, which is slow but correct.
    let _ = std::fs::create_dir_all(dist);
    println!("cargo:rerun-if-changed=../../web/dist");
}
