//! Persistent radix pages: snapshots share storage; an edit copies only its path.
use std::{
    ops::{Index, IndexMut},
    sync::Arc,
};

const PAGE: usize = 256;
const FANOUT: usize = 32;
const LEVELS: usize = 4;

#[derive(Clone, Debug)]
enum Tree<T: Clone> {
    Branch(Vec<Option<Arc<Tree<T>>>>),
    Leaf(Vec<T>),
}

#[derive(Clone, Debug)]
pub struct Pages<T: Clone> {
    root: Arc<Tree<T>>,
    len: usize,
}
impl<T: Clone> Default for Pages<T> {
    fn default() -> Self {
        Self {
            root: Arc::new(Tree::Branch(vec![None; FANOUT])),
            len: 0,
        }
    }
}
impl<T: Clone> Pages<T> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn push(&mut self, value: T) {
        assert!(
            self.len < PAGE * FANOUT.pow(LEVELS as u32),
            "scene page capacity exceeded"
        );
        let index = self.len;
        let leaf = Self::leaf_mut(&mut self.root, LEVELS, index / PAGE);
        leaf.push(value);
        self.len += 1;
    }
    fn leaf_mut(root: &mut Arc<Tree<T>>, level: usize, page: usize) -> &mut Vec<T> {
        if level == 0 {
            let Tree::Leaf(values) = Arc::make_mut(root) else {
                unreachable!()
            };
            return values;
        }
        let Tree::Branch(children) = Arc::make_mut(root) else {
            unreachable!()
        };
        let slot = page / FANOUT.pow((level - 1) as u32) % FANOUT;
        let child = children[slot].get_or_insert_with(|| {
            Arc::new(if level == 1 {
                Tree::Leaf(Vec::with_capacity(PAGE))
            } else {
                Tree::Branch(vec![None; FANOUT])
            })
        });
        Self::leaf_mut(child, level - 1, page)
    }
}
impl<T: Clone> Index<usize> for Pages<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        assert!(index < self.len);
        let page = index / PAGE;
        let mut node = self.root.as_ref();
        for level in (1..=LEVELS).rev() {
            let Tree::Branch(children) = node else {
                unreachable!()
            };
            let slot = page / FANOUT.pow((level - 1) as u32) % FANOUT;
            node = children[slot].as_ref().unwrap().as_ref();
        }
        let Tree::Leaf(values) = node else {
            unreachable!()
        };
        &values[index % PAGE]
    }
}
impl<T: Clone> IndexMut<usize> for Pages<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        assert!(index < self.len);
        &mut Self::leaf_mut(&mut self.root, LEVELS, index / PAGE)[index % PAGE]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Counted(usize, Arc<AtomicUsize>);
    impl Clone for Counted {
        fn clone(&self) -> Self {
            self.1.fetch_add(1, Ordering::Relaxed);
            Self(self.0, self.1.clone())
        }
    }
    #[test]
    fn million_node_snapshot_copies_only_one_page_on_edit() {
        let copies = Arc::new(AtomicUsize::new(0));
        let mut pages = Pages::default();
        for i in 0..1_000_000 {
            pages.push(Counted(i, copies.clone()));
        }
        let snapshot = pages.clone();
        assert_eq!(copies.load(Ordering::Relaxed), 0);
        pages[500_000].0 = 7;
        assert_eq!(copies.load(Ordering::Relaxed), PAGE);
        assert_eq!(snapshot[500_000].0, 500_000);
        assert_eq!(pages[500_000].0, 7);
        pages.push(Counted(1_000_000, copies.clone()));
        assert_eq!(snapshot.len(), 1_000_000);
        assert_eq!(pages[1_000_000].0, 1_000_000);
    }
}
