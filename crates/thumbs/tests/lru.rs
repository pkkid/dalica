//! The byte-budgeted LRU against a simple model: it never exceeds its budget and evicts the least
//! recently used entries first. And the thumbnail job on a `scenes` photo.

use dalica_thumbs::{Lru, thumbnail};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Step {
    Insert(u8, usize),
    Get(u8),
    Remove(u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![(0u8..20, 0usize..1500).prop_map(|(k, b)| Step::Insert(k, b)), (0u8..20).prop_map(Step::Get), (0u8..20).prop_map(Step::Remove),]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn never_over_budget_and_evicts_least_recent_first(steps in proptest::collection::vec(step(), 1..200)) {
        let budget = 1000;
        let mut lru: Lru<u8, u32> = Lru::new(budget);
        // The model: (key, bytes), least recently used first.
        let mut model: Vec<(u8, usize)> = Vec::new();
        for s in steps {
            match s {
                Step::Insert(k, bytes) => {
                    model.retain(|e| e.0 != k);
                    let mut evicted_model = Vec::new();
                    if bytes > budget {
                        evicted_model.push(k);
                    } else {
                        while model.iter().map(|e| e.1).sum::<usize>() + bytes > budget {
                            evicted_model.push(model.remove(0).0);
                        }
                        model.push((k, bytes));
                    }
                    prop_assert_eq!(lru.insert(k, u32::from(k), bytes), evicted_model);
                }
                Step::Get(k) => {
                    let hit = lru.get(&k).copied();
                    let pos = model.iter().position(|e| e.0 == k);
                    prop_assert_eq!(hit.is_some(), pos.is_some());
                    if let Some(i) = pos {
                        let e = model.remove(i);
                        model.push(e);
                    }
                }
                Step::Remove(k) => {
                    prop_assert_eq!(lru.remove(&k).is_some(), model.iter().any(|e| e.0 == k));
                    model.retain(|e| e.0 != k);
                }
            }
            prop_assert!(lru.used() <= budget);
            prop_assert_eq!(lru.used(), model.iter().map(|e| e.1).sum::<usize>());
            prop_assert_eq!(lru.len(), model.len());
        }
    }
}

#[test]
fn the_thumbnail_job_decodes_upright_at_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IMG_0001.jpg");
    let exif = dalica_scenes::Exif { orientation: 6, ..dalica_scenes::Exif::default() };
    std::fs::write(&path, dalica_scenes::jpeg(&dalica_scenes::Scene::new(3, 600, 400), 85, Some(&exif)).unwrap()).unwrap();
    let t = thumbnail(&path, 160, &dalica_image::Limits::default()).unwrap();
    assert_eq!((t.width, t.height), (160, 107));
    assert!(thumbnail(&dir.path().join("missing.jpg"), 160, &dalica_image::Limits::default()).is_err());
    std::fs::write(dir.path().join("x.jpg"), b"not an image at all, but long enough to sniff").unwrap();
    assert!(matches!(thumbnail(&dir.path().join("x.jpg"), 160, &dalica_image::Limits::default()), Err(dalica_thumbs::Error::Image(_))));
}
