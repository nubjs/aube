//! Own integration-test binary (= own process) for the embedder
//! cache-root override, because `set_cache_root` is once-per-process:
//! registering it in the shared unit-test binary would poison every
//! other test's view of the platform default.

#[test]
fn cache_dir_follows_the_registered_cache_root() {
    let root = std::env::temp_dir().join("cache-root-override-test");
    aube_util::env::set_cache_root(&root);
    assert_eq!(
        aube_store::dirs::cache_dir().as_deref(),
        Some(root.as_path()),
        "registered root must replace the <XDG_CACHE_HOME>/aube default"
    );
    assert_eq!(
        aube_store::dirs::global_links_dir().as_deref(),
        Some(root.join("global-links").as_path()),
        "derived subpaths must land below the registered root"
    );
}
