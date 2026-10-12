//! `plan export --since` (decision fael-board:sync-backup): the feed from 0 is everything,
//! from head is nothing, and an edit after head returns the chunk it touched, whole.

use super::super::Fields;
use super::super::Store;
use super::super::chunk::tests::{NOW, add, start};
use super::feed;

#[test]
fn since_head_is_empty_and_an_edit_after_returns_that_chunk_whole() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    start(&mut s, &b, "/wt").unwrap();
    let all = feed(&s, 0).unwrap();
    assert_eq!(all["v"], 1);
    assert_eq!(all["db_id"].as_str().unwrap().len(), 26);
    let head = all["head"].as_i64().unwrap();
    assert!(head > 0);
    assert_eq!(all["plans"].as_array().unwrap().len(), 1);
    assert_eq!(all["chunks"].as_array().unwrap().len(), 2);
    assert_eq!(all["runs"][0]["worktree"], "/wt");

    let none = feed(&s, head).unwrap();
    assert_eq!(
        (
            none["head"].as_i64(),
            none["events"].as_array().unwrap().len()
        ),
        (Some(head), 0)
    );
    assert!(none["chunks"].as_array().unwrap().is_empty());
    assert!(none["plans"].as_array().unwrap().is_empty());

    let f = Fields {
        brief: Some("do a, better".into()),
        ..Fields::default()
    };
    s.edit(&a, &f, NOW).unwrap();
    let next = feed(&s, head).unwrap();
    let chunks = next["chunks"].as_array().unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        (&chunks[0]["uid"], &chunks[0]["brief"]),
        (&a.clone().into(), &"do a, better".into())
    );
    assert!(
        next["runs"].as_array().unwrap().is_empty(),
        "b's run was not touched"
    );
    assert!(next["head"].as_i64().unwrap() > head);
}
