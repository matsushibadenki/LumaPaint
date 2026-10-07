//! Dirty rectangles for disjoint retained text frames. Clear before copying new frames.
use std::collections::HashMap;

pub(crate) struct Key<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub rectangle: [f32; 4],
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub clears: Vec<[u32; 4]>,
    pub copies: Vec<usize>,
}

pub(crate) fn plan<'a>(
    old: impl IntoIterator<Item = Key<'a>>,
    new: impl IntoIterator<Item = Key<'a>>,
) -> Plan {
    let mut old: HashMap<_, _> = old.into_iter().map(|key| (key.id, key)).collect();
    let mut plan = Plan::default();
    let mut clear = |rectangle: [f32; 4]| {
        let rectangle = rectangle.map(|value| value as u32);
        if !plan.clears.contains(&rectangle) {
            plan.clears.push(rectangle);
        }
    };
    for (index, new) in new.into_iter().enumerate() {
        if let Some(old) = old.remove(new.id) {
            if old.source == new.source && old.rectangle == new.rectangle {
                continue;
            }
            clear(old.rectangle);
        }
        clear(new.rectangle);
        plan.copies.push(index);
    }
    for old in old.into_values() {
        clear(old.rectangle);
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key<'a>(id: &'a str, source: &'a str, x: f32) -> Key<'a> {
        Key {
            id,
            source,
            rectangle: [x, 20., 10., 8.],
        }
    }
    #[test]
    fn edit_move_remove_and_unchanged_frames_have_local_work() {
        let unchanged = plan(
            [key("a", "1", 10.), key("b", "1", 40.)],
            [key("a", "1", 10.), key("b", "1", 40.)],
        );
        assert_eq!(unchanged, Plan::default());
        let edit = plan(
            [key("a", "1", 10.), key("b", "1", 40.)],
            [key("a", "2", 10.), key("b", "1", 40.)],
        );
        assert_eq!(edit.copies, [0]);
        assert_eq!(edit.clears, [[10, 20, 10, 8]]);
        let move_remove = plan(
            [key("a", "1", 10.), key("b", "1", 40.)],
            [key("a", "1", 70.)],
        );
        assert_eq!(move_remove.copies, [0]);
        assert!(move_remove.clears.contains(&[10, 20, 10, 8]));
        assert!(move_remove.clears.contains(&[40, 20, 10, 8]));
        assert!(move_remove.clears.contains(&[70, 20, 10, 8]));
    }
}
