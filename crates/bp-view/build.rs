//! Rebuilds the crate when `web/dist` appears or changes: the release binary embeds it (`client.rs`), and the derive
//! alone tracks only the files it embedded, so a binary built before `npm run build` would keep an empty client.

fn main() {
    println!("cargo:rerun-if-changed=../../web/dist");
}
