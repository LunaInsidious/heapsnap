use heapsnap::cancel::CancelToken;
use heapsnap::parser::read_snapshot;

#[test]
fn parse_invalid_json() {
    let data = b"{ invalid json";
    let mut reader: &[u8] = &data[..]; // スライスに変換
    let result = read_snapshot(&mut reader);
    assert!(matches!(
        result,
        Err(heapsnap::error::SnapshotError::Json(_))
    ));
}

#[test]
fn parse_missing_meta() {
    let json = r#"{ "snapshot": {}, "nodes": [], "edges": [], "strings": [] }"#;
    let mut reader = json.as_bytes();
    let result = read_snapshot(&mut reader);
    assert!(matches!(
        result,
        Err(heapsnap::error::SnapshotError::InvalidData { .. })
    ));
}

#[test]
fn parse_cancelled() {
    let json = r#"{ "snapshot": {}, "nodes": [], "edges": [], "strings": [] }"#;
    let mut reader = json.as_bytes();
    let token = CancelToken::new();
    token.cancel();

    let mut progress_reader =
        heapsnap::progress::ProgressReader::new(&mut reader, false, None, token);
    let result = read_snapshot(&mut progress_reader);
    assert!(matches!(
        result,
        Err(heapsnap::error::SnapshotError::Cancelled)
    ));
}
