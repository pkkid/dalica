//! The file-operation journal `file-ops.json` (`data-model.md` §8): it round-trips through the
//! disk, large operations batch their item writes, a failed write changes nothing, `plan` answers
//! every item state from what is on disk, and hostile journals never panic and are set aside.

use std::path::{Path, PathBuf};

use dalica_core::Timestamp;
use dalica_fs::faults::{ATOMIC_POINTS, Fault, arm, disarm_all};
use dalica_store::carry::Mode;
use dalica_store::intents::{self, Action, Direction, Error, Facts, ITEM_WRITES, Intent, Intents, Item, Kind, Problem, Seen, State, decide, plan};
use proptest::prelude::*;
use serde_json::{Value, json};

const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

fn abs(dir: &Path, name: &str) -> PathBuf {
    name.split('/').fold(dir.to_path_buf(), |p, c| p.join(c))
}

fn op(dir: &Path, id: u64, kind: Kind, n: usize) -> Intent {
    let items = (0..n)
        .map(|i| Item {
            from: abs(dir, &format!("a/IMG_{i:04}.jpg")),
            to: kind.has_target().then(|| abs(dir, &format!("b/IMG_{i:04}.jpg"))),
            state: State::Pending,
        })
        .collect();
    Intent { id, kind, direction: Direction::Do, items }
}

fn on_disk(dir: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(intents::FILE)).unwrap()).unwrap()
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn the_journal_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut j = Intents::open(d, NOW).unwrap();
    assert!(j.problems.is_empty() && j.intents.is_empty());
    assert_eq!(files(d), Vec::<String>::new(), "nothing written before the first operation");
    let mv = op(d, 7, Kind::Move, 3);
    let trash = Intent { direction: Direction::Undo, ..op(d, 8, Kind::Trash, 1) };
    j.intents.begin(mv.clone()).unwrap();
    j.intents.begin(trash.clone()).unwrap();
    // The documented shape.
    let disk = on_disk(d);
    assert_eq!(disk["version"], 1);
    assert_eq!(disk["ops"][0]["id"], 7);
    assert_eq!(disk["ops"][0]["kind"], "move");
    assert_eq!(disk["ops"][0]["direction"], "do");
    assert_eq!(disk["ops"][0]["items"][1], json!({ "from": abs(d, "a/IMG_0001.jpg"), "to": abs(d, "b/IMG_0001.jpg"), "state": "pending" }));
    assert_eq!(disk["ops"][1]["items"][0]["to"], Value::Null);
    assert_eq!(disk["ops"][1]["direction"], "undo");
    // Progress, a clash's new name, each on disk at once (a small operation).
    j.intents.item(7, 0, State::Done).unwrap();
    j.intents.item(7, 1, State::Placed).unwrap();
    j.intents.retarget(7, 2, abs(d, "b/IMG_0002-001.jpg")).unwrap();
    assert_eq!(on_disk(d)["ops"][0]["items"][1]["state"], "placed");
    let reopened = Intents::open(d, NOW).unwrap();
    assert!(reopened.problems.is_empty());
    let ops: Vec<Intent> = reopened.intents.ops().cloned().collect();
    assert_eq!(ops, j.intents.ops().cloned().collect::<Vec<_>>());
    assert_eq!(ops[0].items[0].state, State::Done);
    assert_eq!(ops[0].items[2].to.as_deref(), Some(abs(d, "b/IMG_0002-001.jpg").as_path()));
    assert_eq!(reopened.intents.next_id(), 9);
    drop(reopened);
    // Ending both leaves an empty journal.
    j.intents.end(7).unwrap();
    j.intents.end(8).unwrap();
    assert_eq!(on_disk(d), json!({ "version": 1, "ops": [] }));
    assert!(Intents::open(d, NOW).unwrap().intents.is_empty());
    assert_eq!(j.intents.next_id(), 1);
}

#[test]
fn calls_that_cannot_be_journalled_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut j = Intents::open(d, NOW).unwrap().intents;
    j.begin(op(d, 1, Kind::Copy, 2)).unwrap();
    let before = std::fs::read(d.join(intents::FILE)).unwrap();
    assert_eq!(j.begin(op(d, 1, Kind::Copy, 1)), Err(Error::Duplicate(1)));
    let mut relative = op(d, 2, Kind::Move, 1);
    relative.items[0].from = PathBuf::from("a/IMG.jpg");
    assert!(matches!(j.begin(relative), Err(Error::Refused(_))));
    let mut dots = op(d, 3, Kind::Move, 1);
    dots.items[0].to = Some(d.join("b").join("..").join("..").join("etc"));
    assert!(matches!(j.begin(dots), Err(Error::Refused(_))));
    let mut nowhere = op(d, 4, Kind::Rename, 1);
    nowhere.items[0].to = None;
    assert!(matches!(j.begin(nowhere), Err(Error::Refused(_))));
    assert_eq!(j.item(1, 2, State::Done), Err(Error::UnknownItem { op: 1, index: 2 }));
    assert_eq!(j.item(9, 0, State::Done), Err(Error::UnknownOp(9)));
    assert_eq!(j.end(9), Err(Error::UnknownOp(9)));
    assert!(matches!(j.retarget(1, 0, PathBuf::from("rel")), Err(Error::Refused(_))));
    assert_eq!(std::fs::read(d.join(intents::FILE)).unwrap(), before);
    assert_eq!(j.ops().count(), 1);
}

/// A failed write (an error or a crash at any step of the atomic replace) leaves the file as it was
/// and the journal in memory as it was: the operation is not entered, so it does not start.
#[test]
fn a_failed_write_enters_nothing() {
    for point in ATOMIC_POINTS {
        for fault in [Fault::Error, Fault::Crash] {
            let dir = tempfile::tempdir().unwrap();
            let d = dir.path();
            let mut j = Intents::open(d, NOW).unwrap().intents;
            j.begin(op(d, 1, Kind::Move, 1)).unwrap();
            let before = std::fs::read(d.join(intents::FILE)).unwrap();
            arm(point, fault);
            let r = j.begin(op(d, 2, Kind::Move, 1));
            disarm_all();
            let after = std::fs::read(d.join(intents::FILE)).unwrap();
            if r.is_ok() {
                // Failed after the rename (verify): the new file is whole.
                assert_eq!(on_disk(d)["ops"].as_array().unwrap().len(), 2, "{point} {fault:?}");
                continue;
            }
            assert!(
                after == before || serde_json::from_slice::<Value>(&after).unwrap()["ops"].as_array().unwrap().len() == 2,
                "{point} {fault:?}: torn"
            );
            assert_eq!(j.ops().count(), 1, "{point} {fault:?}");
            // Whatever the disk holds reads back without damage.
            let reopened = Intents::open(d, NOW).unwrap();
            assert!(reopened.problems.is_empty(), "{point} {fault:?}");
            drop(reopened);
            // And the journal goes on: the next write says what the journal holds.
            j.begin(op(d, 3, Kind::Copy, 1)).unwrap();
            let ids: Vec<u64> = on_disk(d)["ops"].as_array().unwrap().iter().map(|o| o["id"].as_u64().unwrap()).collect();
            assert_eq!(ids, [1, 3], "{point} {fault:?}");
        }
    }
}

#[test]
fn a_large_operation_writes_its_progress_in_batches() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut j = Intents::open(d, NOW).unwrap().intents;
    let n = ITEM_WRITES * 10;
    j.begin(op(d, 1, Kind::Move, n)).unwrap();
    let state = |i: usize| on_disk(d)["ops"][0]["items"][i]["state"].as_str().unwrap().to_string();
    let mut writes = 0;
    let mut last = std::fs::read(d.join(intents::FILE)).unwrap();
    for i in 0..n {
        j.item(1, i, State::Done).unwrap();
        let now = std::fs::read(d.join(intents::FILE)).unwrap();
        if now != last {
            writes += 1;
            last = now;
        }
    }
    assert_eq!(writes, ITEM_WRITES, "{n} updates, {writes} writes");
    assert_eq!(state(n - 1), "done");
    // An update not yet written is written by `flush` (and by the next begin, retarget or end).
    j.item(1, 0, State::Placed).unwrap();
    assert_eq!(state(0), "done");
    j.flush().unwrap();
    assert_eq!(state(0), "placed");
    // A small operation writes every update.
    j.begin(op(d, 2, Kind::Copy, 3)).unwrap();
    j.item(2, 1, State::Done).unwrap();
    assert_eq!(on_disk(d)["ops"][1]["items"][1]["state"], "done");
}

fn all_facts() -> Vec<Facts> {
    let seen = [Seen::Absent, Seen::Present, Seen::Unknown];
    let mut out = Vec::new();
    for from in seen {
        for to in seen {
            for same in [false, true] {
                if same && !(from == Seen::Present && to == Seen::Present) {
                    continue;
                }
                out.push(Facts { from, to, same });
            }
        }
    }
    out
}

/// `plan` answers every kind, state and fact, and keeps its promises: it waits whenever a side it
/// needs could not be looked at, rolls forward only onto a destination that is there, sends a
/// source to the trash only when the copy verified, and never carries for a trash.
#[test]
fn plan_answers_every_item_state() {
    for kind in Kind::ALL {
        for state in State::ALL {
            for f in all_facts() {
                let d = decide(kind, state, f);
                let needs_to = kind.has_target();
                let blind = f.from == Seen::Unknown || (needs_to && f.to == Seen::Unknown);
                assert_eq!(d.action == Action::Wait, blind, "{kind:?} {state:?} {f:?}: {d:?}");
                assert!(!d.why.is_empty());
                match d.action {
                    Action::Forward { finish, carry } => {
                        if needs_to {
                            assert_eq!(f.to, Seen::Present, "{kind:?} {state:?} {f:?}");
                        } else {
                            assert_eq!((f.from, carry, finish), (Seen::Absent, None, false), "{kind:?} {state:?} {f:?}");
                        }
                        if finish {
                            assert!(f.same && f.from == Seen::Present && state != State::Done, "{kind:?} {state:?} {f:?}");
                            assert!(matches!(kind, Kind::Move | Kind::Rename | Kind::MoveFolder | Kind::RenameFolder));
                        }
                        let want = match kind {
                            Kind::Move | Kind::Rename => Some(Mode::Move),
                            Kind::Copy => Some(Mode::Copy),
                            _ => None,
                        };
                        assert_eq!(carry, want, "{kind:?}");
                    }
                    Action::Lost => assert!(f.from == Seen::Absent && f.to != Seen::Present, "{kind:?} {state:?} {f:?}"),
                    Action::Back | Action::Wait => {}
                }
            }
        }
    }
    // The table's main rows.
    let f = |from, to, same| Facts { from, to, same };
    let (a, p) = (Seen::Absent, Seen::Present);
    assert_eq!(decide(Kind::Move, State::Pending, f(p, a, false)).action, Action::Back, "never moved");
    assert_eq!(decide(Kind::Move, State::Pending, f(a, p, false)).action, Action::Forward { finish: false, carry: Some(Mode::Move) }, "renamed");
    assert_eq!(decide(Kind::Move, State::Placed, f(p, p, true)).action, Action::Forward { finish: true, carry: Some(Mode::Move) }, "copy placed");
    assert_eq!(
        decide(Kind::Move, State::Pending, f(p, p, true)).action,
        Action::Forward { finish: true, carry: Some(Mode::Move) },
        "placed, unwritten"
    );
    assert_eq!(decide(Kind::Move, State::Placed, f(p, p, false)).action, Action::Back, "not the copy");
    assert_eq!(decide(Kind::Move, State::Done, f(p, p, false)).action, Action::Forward { finish: false, carry: Some(Mode::Move) }, "no trash");
    assert_eq!(decide(Kind::Move, State::Done, f(a, a, false)).action, Action::Lost);
    assert_eq!(decide(Kind::Copy, State::Pending, f(p, p, true)).action, Action::Forward { finish: false, carry: Some(Mode::Copy) });
    assert_eq!(decide(Kind::Copy, State::Pending, f(a, p, false)).action, Action::Back, "cannot be checked");
    assert_eq!(decide(Kind::Copy, State::Done, f(a, p, false)).action, Action::Forward { finish: false, carry: Some(Mode::Copy) });
    assert_eq!(decide(Kind::Trash, State::Pending, f(a, a, false)).action, Action::Forward { finish: false, carry: None });
    assert_eq!(decide(Kind::TrashFolder, State::Done, f(p, a, false)).action, Action::Back);
    assert_eq!(decide(Kind::MoveFolder, State::Placed, f(p, p, true)).action, Action::Forward { finish: true, carry: None });
    // `plan` decides per item, in order; an item without facts waits.
    let dir = tempfile::tempdir().unwrap();
    let three = op(dir.path(), 1, Kind::Move, 3);
    let decided = plan(&three, &[f(p, a, false), f(a, p, false)]);
    assert_eq!(
        decided.iter().map(|d| d.action).collect::<Vec<_>>(),
        [Action::Back, Action::Forward { finish: false, carry: Some(Mode::Move) }, Action::Wait]
    );
}

/// Writes `bytes` as the journal, opens it, and checks the promises: no panic, an unusable file
/// kept aside byte for byte, and the journal left usable.
fn open_hostile(bytes: &[u8]) -> (Vec<Intent>, Vec<Problem>) {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join(intents::FILE), bytes).unwrap();
    let opened = Intents::open(d, NOW).unwrap();
    let ops: Vec<Intent> = opened.intents.ops().cloned().collect();
    match &opened.problems[..] {
        [] => assert_eq!(std::fs::read(d.join(intents::FILE)).unwrap(), bytes, "a usable journal is not rewritten"),
        [Problem::SetAside { kept, kept_ops, .. }] => {
            assert_eq!(std::fs::read(kept).unwrap(), bytes, "kept as it was");
            assert!(kept.file_name().unwrap().to_string_lossy().starts_with("file-ops.json.corrupt-20261003T040000Z"));
            assert_eq!(*kept_ops, ops.len());
            if ops.is_empty() {
                assert!(!d.join(intents::FILE).exists());
            } else {
                // What read is in a new journal.
                let again = Intents::open(d, NOW).unwrap();
                assert!(again.problems.is_empty());
                assert_eq!(again.intents.ops().cloned().collect::<Vec<_>>(), ops);
            }
        }
        other => panic!("{other:?}"),
    }
    // Still usable: a new operation can be entered.
    let mut j = opened.intents;
    let id = j.next_id();
    j.begin(op(d, id, Kind::Move, 1)).unwrap();
    (ops, opened.problems)
}

#[test]
fn hostile_journals_never_panic_and_are_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    let good = |id: u64| json!({ "id": id, "kind": "move", "items": [{ "from": abs(dir.path(), "a/x.jpg"), "to": abs(dir.path(), "b/x.jpg"), "state": "pending" }] });
    let cases: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"{ not json".to_vec(),
        b"[1, 2]".to_vec(),
        json!({ "version": 2, "ops": [] }).to_string().into_bytes(),
        json!({ "ops": [] }).to_string().into_bytes(),
        json!({ "version": 1, "ops": {} }).to_string().into_bytes(),
        format!("{}{}", "[".repeat(100_000), "]".repeat(100_000)).into_bytes(),
        format!("{{\"version\":1,\"ops\":[{}]}}", "{\"a\":".repeat(5_000)).into_bytes(),
        vec![0xff, 0xfe, 0x00, 0x7b],
    ];
    for bytes in &cases {
        let (ops, problems) = open_hostile(bytes);
        assert!(ops.is_empty() && problems.len() == 1, "{:?}", String::from_utf8_lossy(&bytes[..bytes.len().min(40)]));
    }
    // Bad operations beside good ones: the good ones are kept.
    let mixed = json!({ "version": 1, "ops": [
        good(1),
        { "id": 2, "kind": "teleport", "items": [] },
        { "id": 3, "kind": "move", "items": [{ "from": "relative/x.jpg", "to": "/b/x.jpg", "state": "pending" }] },
        { "id": 4, "kind": "move", "items": [{ "from": abs(dir.path(), "a/x.jpg"), "state": "pending" }] },
        { "id": 5, "kind": "copy", "items": [{ "from": abs(dir.path(), "a/x.jpg"), "to": abs(dir.path(), "b/x.jpg"), "state": "gone" }] },
        good(1),
        { "id": -1, "kind": "trash", "items": [] },
        good(6),
        7,
    ] });
    let (ops, problems) = open_hostile(mixed.to_string().as_bytes());
    assert_eq!(ops.iter().map(|o| o.id).collect::<Vec<_>>(), [1, 6]);
    assert!(matches!(&problems[..], [Problem::SetAside { kept_ops: 2, why, .. }] if why.contains("used twice") && why.contains("not absolute")));
    // A usable journal is used as it is, unknown keys and all.
    let fine = json!({ "version": 1, "ops": [good(3)], "later": true });
    let (ops, problems) = open_hostile(fine.to_string().as_bytes());
    assert_eq!((ops.len(), problems.len()), (1, 0));
}

#[test]
fn a_journal_past_the_size_limit_is_set_aside_unread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(intents::FILE);
    let f = std::fs::File::create(&path).unwrap();
    // Sparse where the file system allows it: nothing is read.
    f.set_len(intents::MAX_BYTES + 1).unwrap();
    drop(f);
    let opened = Intents::open(dir.path(), NOW).unwrap();
    assert!(matches!(&opened.problems[..], [Problem::SetAside { kept_ops: 0, why, .. }] if why.contains("larger")), "{:?}", opened.problems);
    assert!(!path.exists());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        open_hostile(&bytes);
    }

    #[test]
    fn random_operations_never_panic(
        ops in proptest::collection::vec(
            (any::<i64>(), prop_oneof![Just("move"), Just("copy"), Just("trash"), Just("moveFolder"), Just("x")],
             proptest::collection::vec((prop_oneof![Just(json!("/a/x")), Just(json!("x")), Just(json!(null)), Just(json!(7)), Just(json!("/a/../b"))],
                                        prop_oneof![Just(json!("/b/x")), Just(json!(null)), Just(json!([1]))],
                                        prop_oneof![Just("pending"), Just("placed"), Just("done"), Just("?")]), 0..4)),
            0..8)
    ) {
        let ops: Vec<Value> = ops
            .into_iter()
            .map(|(id, kind, items)| json!({ "id": id, "kind": kind,
                "items": items.into_iter().map(|(from, to, state)| json!({ "from": from, "to": to, "state": state })).collect::<Vec<_>>() }))
            .collect();
        let (read, _) = open_hostile(json!({ "version": 1, "ops": ops }).to_string().as_bytes());
        // Whatever was kept plans without panicking.
        for intent in &read {
            let facts = vec![Facts { from: Seen::Present, to: Seen::Absent, same: false }; intent.items.len()];
            prop_assert_eq!(plan(intent, &facts).len(), intent.items.len());
        }
    }
}
