use std::path::Path;

use heapsnap::analysis::retainers::{
    find_retaining_paths, find_target_by_id, RetainersOptions,
};
use heapsnap::cancel::CancelToken;
use heapsnap::parser::{read_snapshot_file, ReadOptions};

#[test]
fn retainers_paths_fixture_small() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot = read_snapshot_file(path, options).expect("snapshot");

    let target = find_target_by_id(&snapshot, 3).expect("target");
    let result = find_retaining_paths(
        &snapshot,
        target,
        RetainersOptions {
            max_paths: 5,
            max_depth: 10,
            cancel: CancelToken::new(),
        },
    )
    .expect("paths");

    assert_eq!(result.paths.len(), 1);
    assert_eq!(result.paths[0].len(), 2);
}

#[test]
fn retainers_cancelled() {
    let path = Path::new("fixtures/small.heapsnapshot");
    let options = ReadOptions::new(false, CancelToken::new());
    let snapshot = read_snapshot_file(path, options).expect("snapshot");

    let target = find_target_by_id(&snapshot, 3).expect("target");
    let token = CancelToken::new();
    token.cancel();

    let result = find_retaining_paths(
        &snapshot,
        target,
        RetainersOptions {
            max_paths: 5,
            max_depth: 10,
            cancel: token,
        },
    );

    assert!(matches!(result, Err(heapsnap::error::SnapshotError::Cancelled)));
}
