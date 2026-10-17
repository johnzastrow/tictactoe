// The web client (crates/pt-web) is built by trunk into web-dist/ and
// embedded. Create a placeholder so the server builds before trunk has run.
fn main() {
    let dir = std::path::Path::new("web-dist");
    let index = dir.join("index.html");
    if !index.exists() {
        std::fs::create_dir_all(dir).expect("create web-dist");
        std::fs::write(
            &index,
            "<!doctype html><meta charset=utf-8><title>pt</title>\
             <p>The web client has not been built. Run <code>make web</code> in <code>rust/</code>.</p>",
        )
        .expect("write placeholder");
    }
    println!("cargo:rerun-if-changed=web-dist");
}
