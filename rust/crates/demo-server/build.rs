// The web client (crates/demo-web) is built by trunk into web-dist/ and
// embedded. Create a placeholder so the server builds before trunk has run.
fn main() {
    let dir = std::path::Path::new("web-dist");
    let index = dir.join("index.html");
    if !index.exists() {
        std::fs::create_dir_all(dir).expect("create web-dist");
        std::fs::write(
            &index,
            "<!doctype html><meta charset=utf-8><title>demo</title>\
             <p>web client not built; run <code>make demo-web</code> in <code>rust/</code>.</p>",
        )
        .expect("write placeholder");
    }
    println!("cargo:rerun-if-changed=web-dist");
}
