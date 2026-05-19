#[test]
#[ignore = "requires rvf-cli"]
fn test_reconciler_tombstones_deleted_files() {
    // 1. Index workspace with 10 files
    // 2. Delete 3 files from disk
    // 3. Run VecReconciler
    // 4. Verify 3 files tombstoned in VEC_SEG metadata
    // 5. Verify search_codebase returns 0 results for deleted paths
}

#[test]
#[ignore = "requires rvf-cli"]
fn test_compact_removes_tombstoned_entries() {
    // Follow up from above: run claudebox compact
    // Verify tombstoned entries are physically absent
    // Verify HNSW index is valid and queryable
}
