use qmonster::store::{InsightsWindow, SqliteInsightsStore};
use tempfile::tempdir;

#[test]
fn insights_query_empty_db_returns_zero_state() {
    let td = tempdir().unwrap();
    let db_path = td.path().join("qmonster.db");
    let store = SqliteInsightsStore::open(&db_path).unwrap();

    let snapshot = store
        .snapshot(InsightsWindow {
            since_ms: 1_000,
            until_ms: 2_000,
        })
        .unwrap();

    assert_eq!(snapshot.window.since_ms, 1_000);
    assert_eq!(snapshot.window.until_ms, 2_000);
    assert!(snapshot.situations.is_empty());
    assert_eq!(snapshot.cache.hot_count, 0);
    assert_eq!(snapshot.cache.cold_count, 0);
    assert_eq!(snapshot.cache.drift_count, 0);
    assert!(snapshot.timeline.is_empty());
    assert!(snapshot.actions.is_empty());
    assert_eq!(snapshot.ignored_available, false);
}
