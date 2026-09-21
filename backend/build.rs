// `sqlx::migrate!` embeds the migrations at compile time, but cargo only knows to rebuild
// when a file it already tracks changes. A commit that only *adds* a migration changes
// nothing cargo is watching, so the old binary — with the old schema — is reused. Locally
// the next source edit hides that; in a Docker build with a cached target dir it ships.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
