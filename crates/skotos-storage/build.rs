// Re-embed `sqlx::migrate!` migrations whenever a file is added or changed.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
