//! Persistent compressed binary trie for stable spatial item IDs.
//! An edit copies at most one machine-word-length path, independent of item count.
use std::sync::Arc;

#[derive(Clone, Debug)]
enum Node {
    Leaf {
        key: usize,
        value: usize,
    },
    Branch {
        prefix: usize,
        bit: u32,
        children: [Arc<Node>; 2],
    },
}
#[derive(Clone, Debug, Default)]
pub(super) struct Positions {
    root: Option<Arc<Node>>,
}
fn side(key: usize, bit: u32) -> usize {
    (key >> bit) & 1
}
fn prefix(node: &Node) -> usize {
    match node {
        Node::Leaf { key, .. } => *key,
        Node::Branch { prefix, .. } => *prefix,
    }
}
fn attach(root: &mut Arc<Node>, key: usize, value: usize, bit: u32) {
    let leaf = Arc::new(Node::Leaf { key, value });
    let old = root.clone();
    let children = if side(key, bit) == 0 {
        [leaf, old]
    } else {
        [old, leaf]
    };
    *root = Arc::new(Node::Branch {
        prefix: key,
        bit,
        children,
    });
}
fn insert(root: &mut Arc<Node>, key: usize, value: usize) -> Option<usize> {
    let difference = key ^ prefix(root);
    match root.as_ref() {
        Node::Leaf { .. } if difference != 0 => {
            attach(
                root,
                key,
                value,
                usize::BITS - 1 - difference.leading_zeros(),
            );
            return None;
        }
        Node::Branch { bit, .. }
            if difference != 0 && usize::BITS - 1 - difference.leading_zeros() > *bit =>
        {
            attach(
                root,
                key,
                value,
                usize::BITS - 1 - difference.leading_zeros(),
            );
            return None;
        }
        _ => {}
    }
    match Arc::make_mut(root) {
        Node::Leaf { value: old, .. } => Some(std::mem::replace(old, value)),
        Node::Branch { bit, children, .. } => insert(&mut children[side(key, *bit)], key, value),
    }
}
fn remove(root: &mut Arc<Node>, key: usize) -> (Option<usize>, bool) {
    match root.as_ref() {
        Node::Leaf { key: stored, value } => (
            if *stored == key { Some(*value) } else { None },
            *stored == key,
        ),
        Node::Branch { bit, .. } => {
            let direction = side(key, *bit);
            let Node::Branch { children, .. } = Arc::make_mut(root) else {
                unreachable!()
            };
            let (removed, empty) = remove(&mut children[direction], key);
            if empty {
                *root = children[1 - direction].clone();
            }
            (removed, false)
        }
    }
}
impl Positions {
    pub fn get(&self, key: &usize) -> Option<&usize> {
        let mut node = self.root.as_deref()?;
        loop {
            match node {
                Node::Leaf { key: stored, value } => return (*stored == *key).then_some(value),
                Node::Branch { bit, children, .. } => node = &children[side(*key, *bit)],
            }
        }
    }
    pub fn append_keys(&self, output: &mut Vec<usize>) {
        fn visit(node: &Node, output: &mut Vec<usize>) {
            match node {
                Node::Leaf { key, .. } => output.push(*key),
                Node::Branch { children, .. } => {
                    visit(&children[0], output);
                    visit(&children[1], output);
                }
            }
        }
        if let Some(root) = &self.root {
            visit(root, output);
        }
    }
    pub fn contains_key(&self, key: &usize) -> bool {
        self.get(key).is_some()
    }
    pub fn insert(&mut self, key: usize, value: usize) -> Option<usize> {
        if let Some(root) = &mut self.root {
            insert(root, key, value)
        } else {
            self.root = Some(Arc::new(Node::Leaf { key, value }));
            None
        }
    }
    pub fn remove(&mut self, key: &usize) -> Option<usize> {
        // An absent ID must not copy a published path.
        self.get(key)?;
        let (removed, empty) = remove(self.root.as_mut().unwrap(), *key);
        if empty {
            self.root = None;
        }
        removed
    }
}
impl std::ops::Index<&usize> for Positions {
    type Output = usize;
    fn index(&self, key: &usize) -> &usize {
        self.get(key).expect("missing spatial item")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    fn pointers(node: &Arc<Node>, output: &mut HashSet<usize>) {
        output.insert(Arc::as_ptr(node) as usize);
        if let Node::Branch { children, .. } = node.as_ref() {
            for child in children {
                pointers(child, output);
            }
        }
    }
    #[test]
    fn edits_match_hash_map_with_sparse_and_extreme_keys() {
        let mut positions = Positions::default();
        let mut expected = HashMap::new();
        for step in 0usize..6000 {
            let key = match step % 4 {
                0 => usize::MAX - step % 257,
                1 => step % 257,
                _ => (step % 257) << (usize::BITS / 2),
            };
            if step % 5 == 0 {
                assert_eq!(positions.remove(&key), expected.remove(&key));
            } else {
                assert_eq!(positions.insert(key, step), expected.insert(key, step));
            }
            for (&key, value) in &expected {
                assert_eq!(positions.get(&key), Some(value));
            }
        }
    }
    #[test]
    fn snapshot_edit_copies_only_a_bounded_path() {
        for count in [1000, 10_000, 100_000] {
            let mut positions = Positions::default();
            for key in 0..count {
                positions.insert(key, key);
            }
            let snapshot = positions.clone();
            let mut old = HashSet::new();
            pointers(snapshot.root.as_ref().unwrap(), &mut old);
            positions.remove(&(count / 2));
            positions.insert(usize::MAX, 7);
            let mut new = HashSet::new();
            pointers(positions.root.as_ref().unwrap(), &mut new);
            assert!(new.difference(&old).count() <= 2 * usize::BITS as usize);
            assert_eq!(snapshot.get(&(count / 2)), Some(&(count / 2)));
            assert_eq!(positions.get(&(count / 2)), None);
            assert_eq!(snapshot.get(&usize::MAX), None);
            assert_eq!(positions.get(&usize::MAX), Some(&7));
        }
    }
}
