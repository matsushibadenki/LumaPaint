//! Conservative BVH; unknown bounds always remain candidates.
use std::sync::Arc;

pub type Bounds = [f64; 4];
fn intersects(a: Bounds, b: Bounds) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
fn union(a: Bounds, b: Bounds) -> Bounds {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}
fn valid(a: Bounds) -> bool {
    a.iter().all(|v| v.is_finite()) && a[0] <= a[2] && a[1] <= a[3]
}
#[derive(Clone, Debug)]
struct Node {
    bounds: Bounds,
    parent: Option<usize>,
    children: Option<[usize; 2]>,
    item: usize,
    height: usize,
}
#[derive(Clone, Debug, Default)]
struct FreeSlots {
    pages: super::pages::Pages<usize>,
    len: usize,
}
impl FreeSlots {
    fn push(&mut self, value: usize) {
        if self.len == self.pages.len() {
            self.pages.push(value);
        } else {
            self.pages[self.len] = value;
        }
        self.len += 1;
    }
    fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.pages[self.len])
    }
}

#[derive(Clone, Debug, Default)]
pub struct SpatialIndex {
    nodes: super::pages::Pages<Node>,
    leaves: Arc<super::positions::Positions>,
    unknown: Arc<super::positions::Positions>,
    root: Option<usize>,
    free: Arc<FreeSlots>,
}
#[derive(Debug, Default)]
pub struct Candidates {
    /// Original drawing order, independent of tree traversal order.
    pub items: Vec<usize>,
    pub visited_nodes: usize,
}
impl SpatialIndex {
    pub fn build(items: impl IntoIterator<Item = (usize, Option<Bounds>)>) -> Self {
        let mut index = Self::default();
        let mut known = Vec::new();
        for (id, bounds) in items {
            if let Some(bounds) = bounds.filter(|b| valid(*b)) {
                known.push((id, bounds));
            } else {
                Arc::make_mut(&mut index.unknown).insert(id, id);
            }
        }
        if !known.is_empty() {
            index.root = Some(index.branch(&mut known, None));
        }
        index
    }
    fn branch(&mut self, items: &mut [(usize, Bounds)], parent: Option<usize>) -> usize {
        let bounds = items.iter().map(|(_, b)| *b).reduce(union).unwrap();
        let position = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            parent,
            children: None,
            item: items[0].0,
            height: 0,
        });
        if items.len() == 1 {
            Arc::make_mut(&mut self.leaves).insert(items[0].0, position);
        } else {
            let axis = usize::from(bounds[3] - bounds[1] > bounds[2] - bounds[0]);
            let middle = items.len() / 2;
            items.select_nth_unstable_by(middle, |(a, x), (b, y)| {
                (x[axis] + x[axis + 2])
                    .total_cmp(&(y[axis] + y[axis + 2]))
                    .then(a.cmp(b))
            });
            let (left, right) = items.split_at_mut(middle);
            let children = [
                self.branch(left, Some(position)),
                self.branch(right, Some(position)),
            ];
            self.nodes[position].children = Some(children);
            self.recompute(position);
        }
        position
    }
    fn allocate(&mut self, node: Node) -> usize {
        if let Some(position) = Arc::make_mut(&mut self.free).pop() {
            self.nodes[position] = node;
            position
        } else {
            let position = self.nodes.len();
            self.nodes.push(node);
            position
        }
    }

    fn recompute(&mut self, position: usize) {
        if let Some([a, b]) = self.nodes[position].children {
            self.nodes[position].bounds = union(self.nodes[a].bounds, self.nodes[b].bounds);
            self.nodes[position].height = 1 + self.nodes[a].height.max(self.nodes[b].height);
        }
    }

    fn rotate(&mut self, position: usize, left: bool) -> usize {
        let side = usize::from(left);
        let pivot = self.nodes[position].children.unwrap()[side];
        let middle = self.nodes[pivot].children.unwrap()[1 - side];
        let parent = self.nodes[position].parent;
        self.nodes[position].children.as_mut().unwrap()[side] = middle;
        self.nodes[middle].parent = Some(position);
        self.nodes[pivot].children.as_mut().unwrap()[1 - side] = position;
        self.nodes[position].parent = Some(pivot);
        self.nodes[pivot].parent = parent;
        if let Some(parent) = parent {
            let children = self.nodes[parent].children.as_mut().unwrap();
            children[usize::from(children[1] == position)] = pivot;
        } else {
            self.root = Some(pivot);
        }
        self.recompute(position);
        self.recompute(pivot);
        crate::performance::count("spatial_balance_rotations", 1);
        pivot
    }

    fn update_ancestors(&mut self, mut parent: Option<usize>) {
        while let Some(position) = parent {
            let old = (self.nodes[position].bounds, self.nodes[position].height);
            self.recompute(position);
            let [a, b] = self.nodes[position].children.unwrap();
            let mut top = position;
            if self.nodes[a].height > self.nodes[b].height + 1 {
                let [aa, ab] = self.nodes[a].children.unwrap();
                if self.nodes[ab].height > self.nodes[aa].height {
                    self.rotate(a, true);
                }
                top = self.rotate(position, false);
            } else if self.nodes[b].height > self.nodes[a].height + 1 {
                let [ba, bb] = self.nodes[b].children.unwrap();
                if self.nodes[ba].height > self.nodes[bb].height {
                    self.rotate(b, false);
                }
                top = self.rotate(position, true);
            }
            crate::performance::count("spatial_refit_parent_visits", 1);
            if top == position && old == (self.nodes[top].bounds, self.nodes[top].height) {
                crate::performance::count("spatial_refit_early_stops", 1);
                break;
            }
            parent = self.nodes[top].parent;
        }
    }

    fn remove_known(&mut self, item: usize) {
        let position = Arc::make_mut(&mut self.leaves).remove(&item).unwrap();
        if let Some(parent) = self.nodes[position].parent {
            let [a, b] = self.nodes[parent].children.unwrap();
            let sibling = if a == position { b } else { a };
            let grandparent = self.nodes[parent].parent;
            self.nodes[sibling].parent = grandparent;
            if let Some(grandparent) = grandparent {
                let children = self.nodes[grandparent].children.as_mut().unwrap();
                children[usize::from(children[1] == parent)] = sibling;
                self.update_ancestors(Some(grandparent));
            } else {
                self.root = Some(sibling);
            }
            Arc::make_mut(&mut self.free).push(parent);
        } else {
            self.root = None;
        }
        Arc::make_mut(&mut self.free).push(position);
    }

    fn insert_known(&mut self, item: usize, bounds: Bounds) {
        let leaf = self.allocate(Node {
            bounds,
            parent: None,
            children: None,
            item,
            height: 0,
        });
        let Some(mut sibling) = self.root else {
            self.root = Some(leaf);
            Arc::make_mut(&mut self.leaves).insert(item, leaf);
            return;
        };
        // Descend by the smallest increase in bounding area. Only this branch
        // and its ancestors change; untouched persistent pages stay shared.
        while let Some([a, b]) = self.nodes[sibling].children {
            let growth = |position: usize| {
                let old = self.nodes[position].bounds;
                let new = union(old, bounds);
                (new[2] - new[0]) * (new[3] - new[1]) - (old[2] - old[0]) * (old[3] - old[1])
            };
            sibling = if growth(a).total_cmp(&growth(b)).is_le() {
                a
            } else {
                b
            };
        }
        let ancestor = self.nodes[sibling].parent;
        let parent = self.allocate(Node {
            bounds: union(bounds, self.nodes[sibling].bounds),
            parent: ancestor,
            children: Some([sibling, leaf]),
            item,
            height: 0,
        });
        self.nodes[sibling].parent = Some(parent);
        self.nodes[leaf].parent = Some(parent);
        if let Some(ancestor) = ancestor {
            let children = self.nodes[ancestor].children.as_mut().unwrap();
            children[usize::from(children[1] == sibling)] = parent;
            self.update_ancestors(Some(parent));
        } else {
            self.root = Some(parent);
            self.recompute(parent);
        }
        Arc::make_mut(&mut self.leaves).insert(item, leaf);
    }

    /// Insert or update a stable item ID. Query output remains sorted by ID.
    pub fn upsert(&mut self, item: usize, bounds: Option<Bounds>) {
        if self.refit(item, bounds) {
            return;
        }
        if let Some(bounds) = bounds.filter(|b| valid(*b)) {
            self.insert_known(item, bounds);
        } else {
            Arc::make_mut(&mut self.unknown).insert(item, item);
        }
        crate::performance::count("spatial_insertions", 1);
    }

    /// Remove one stable item ID, retaining published snapshots and reusing slots.
    pub fn remove(&mut self, item: usize) -> bool {
        if self.leaves.contains_key(&item) {
            self.remove_known(item);
        } else if Arc::make_mut(&mut self.unknown).remove(&item).is_some() {
            // Unknown bounds remain conservative candidates until removal.
        } else {
            return false;
        }
        crate::performance::count("spatial_removals", 1);
        true
    }

    /// Returns false for an absent item. Known/unknown transitions update only
    /// the affected branch instead of requiring a complete index rebuild.
    pub fn refit(&mut self, item: usize, bounds: Option<Bounds>) -> bool {
        let _timer = crate::performance::time("spatial_refit_host");
        let bounds = bounds.filter(|b| valid(*b));
        let position = self.leaves.get(&item).copied();
        match (position, bounds) {
            (Some(_), None) => {
                self.remove_known(item);
                Arc::make_mut(&mut self.unknown).insert(item, item);
                crate::performance::count("spatial_membership_updates", 1);
                return true;
            }
            (None, Some(bounds)) => {
                if Arc::make_mut(&mut self.unknown).remove(&item).is_none() {
                    return false;
                }
                self.insert_known(item, bounds);
                crate::performance::count("spatial_membership_updates", 1);
                return true;
            }
            (None, None) => return self.unknown.contains_key(&item),
            (Some(_), Some(_)) => {}
        }
        let position = position.unwrap();
        let bounds = bounds.unwrap();
        if self.nodes[position].bounds == bounds {
            crate::performance::count("spatial_refit_unchanged_leaf", 1);
            return true;
        }
        self.nodes[position].bounds = bounds;
        self.update_ancestors(self.nodes[position].parent);
        true
    }
    pub fn query(&self, bounds: Bounds) -> Candidates {
        let mut candidates = Candidates::default();
        candidates.visited_nodes = self.query_into(bounds, &mut candidates.items, &mut Vec::new());
        candidates
    }

    /// Reuse caller-owned result and traversal buffers on repeated viewport queries.
    pub fn query_into(
        &self,
        bounds: Bounds,
        items: &mut Vec<usize>,
        stack: &mut Vec<usize>,
    ) -> usize {
        items.clear();
        self.unknown.append_keys(items);
        stack.clear();
        if let Some(root) = self.root {
            stack.push(root);
        }
        let mut visited_nodes = 0;
        // Invalid queries cannot safely reject anything.
        while let Some(position) = stack.pop() {
            visited_nodes += 1;
            let node = &self.nodes[position];
            if valid(bounds) && !intersects(node.bounds, bounds) {
                continue;
            }
            if let Some([a, b]) = node.children {
                stack.extend([a, b]);
            } else {
                items.push(node.item);
            }
        }
        items.sort_unstable();
        visited_nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_unknown_membership_and_free_slot_snapshots_remain_immutable() {
        let mut index = SpatialIndex::build((0..10000).map(|id| (id, None)));
        let snapshot = index.clone();
        assert!(index.remove(5000));
        index.upsert(10001, None);
        let old = snapshot.query([0.; 4]).items;
        let current = index.query([0.; 4]).items;
        assert_eq!(old.len(), 10000);
        assert!(old.contains(&5000));
        assert!(!old.contains(&10001));
        assert!(!current.contains(&5000));
        assert!(current.contains(&10001));
        let mut slots = FreeSlots::default();
        for value in 0..10000 {
            slots.push(value);
        }
        let published = slots.clone();
        for _ in 0..1000 {
            slots.pop();
        }
        for value in 0..1000 {
            slots.push(value);
        }
        assert_eq!(slots.pages.len(), 10000);
        assert_eq!(published.pages[9999], 9999);
        assert_eq!(slots.pages[9999], 999);
    }

    fn assert_tree(index: &SpatialIndex, position: usize, parent: Option<usize>) -> usize {
        let node = &index.nodes[position];
        assert_eq!(node.parent, parent);
        if let Some([a, b]) = node.children {
            let ah = assert_tree(index, a, Some(position));
            let bh = assert_tree(index, b, Some(position));
            assert!(ah.abs_diff(bh) <= 1);
            assert_eq!(node.height, 1 + ah.max(bh));
            assert_eq!(
                node.bounds,
                union(index.nodes[a].bounds, index.nodes[b].bounds)
            );
        } else {
            assert_eq!(node.height, 0);
            assert_eq!(index.leaves[&node.item], position);
        }
        node.height
    }

    #[test]
    fn sorted_insertions_and_removals_stay_balanced_and_preserve_snapshots() {
        let mut index = SpatialIndex::default();
        for item in 0..10_000 {
            let x = item as f64 * 10.;
            index.upsert(item, Some([x, 0., x + 2., 2.]));
        }
        assert!(assert_tree(&index, index.root.unwrap(), None) < 20);
        let published = index.clone();
        let allocated = index.nodes.len();
        for item in (0..10_000).step_by(2) {
            assert!(index.remove(item));
            assert!(!index.remove(item));
        }
        assert_tree(&index, index.root.unwrap(), None);
        assert!(index.query([50000., 0., 50002., 2.]).items.is_empty());
        assert_eq!(published.query([50000., 0., 50002., 2.]).items, [5000]);
        for item in (0..10_000).step_by(2) {
            let x = item as f64 * 10.;
            index.upsert(item, Some([x, 0., x + 2., 2.]));
        }
        assert_eq!(index.nodes.len(), allocated);
        assert_tree(&index, index.root.unwrap(), None);
        let result = index.query([50000., 0., 50002., 2.]);
        assert_eq!(result.items, [5000]);
        assert!(result.visited_nodes < 100);
        index.upsert(5000, None);
        assert_eq!(index.query([-10.; 4]).items, [5000]);
        assert!(index.remove(5000));
        assert!(index.query([-10.; 4]).items.is_empty());
    }

    #[test]
    fn structural_edits_match_fresh_index_after_each_batch() {
        let mut index = SpatialIndex::default();
        let mut items = std::collections::BTreeMap::new();
        for step in 0..3000 {
            let item = step * 31 % 257;
            if step % 5 == 0 {
                assert_eq!(index.remove(item), items.remove(&item).is_some());
            } else {
                let x = (step * 13 % 300) as f64;
                let bounds = (step % 7 != 0).then_some([x, 0., x + 4., 4.]);
                items.insert(item, bounds);
                index.upsert(item, bounds);
            }
            if step % 17 == 0 {
                let rebuilt = SpatialIndex::build(items.iter().map(|(&id, &b)| (id, b)));
                for bounds in [[0., 0., 50., 50.], [150., 0., 170., 200.], [f64::NAN; 4]] {
                    assert_eq!(index.query(bounds).items, rebuilt.query(bounds).items);
                }
                if let Some(root) = index.root {
                    assert_tree(&index, root, None);
                }
            }
        }
    }

    #[test]
    fn membership_transitions_preserve_snapshots_and_reuse_nodes() {
        let mut tree = SpatialIndex::build(
            (0..10_000).map(|i| (i, Some([i as f64 * 10., 0., i as f64 * 10. + 5., 5.]))),
        );
        let published = tree.clone();
        let original_nodes = tree.nodes.len();
        for _ in 0..100 {
            assert!(tree.refit(7000, None));
            assert!(tree.query([-100., -100., -90., -90.]).items.contains(&7000));
            assert!(tree.refit(7000, Some([-100., -100., -95., -95.])));
            assert_eq!(tree.query([-100., -100., -95., -95.]).items, [7000]);
            assert!(tree.query([70_000., 0., 70_005., 5.]).items.is_empty());
            assert_eq!(tree.nodes.len(), original_nodes);
        }
        assert_eq!(published.query([70_000., 0., 70_005., 5.]).items, [7000]);
        assert!(published.query([-100., -100., -95., -95.]).items.is_empty());
        assert!(!tree.refit(20_000, Some([0.; 4])));
        assert!(!tree.refit(20_000, None));
    }

    #[test]
    fn single_leaf_and_unknown_only_transitions_keep_a_valid_root() {
        let mut tree = SpatialIndex::build([(42, None), (7, Some([0.; 4]))]);
        assert!(tree.refit(7, Some([f64::NAN; 4])));
        assert!(tree.root.is_none());
        assert_eq!(tree.query([100.; 4]).items, [7, 42]);
        assert!(tree.refit(42, Some([100.; 4])));
        assert!(tree.refit(7, Some([0.; 4])));
        assert_eq!(tree.query([100.; 4]).items, [42]);
        assert_eq!(tree.query([0.; 4]).items, [7]);
        assert_eq!(tree.query([f64::NAN; 4]).items, [7, 42]);
        assert!(tree.refit(42, None));
        assert!(tree.refit(7, None));
        assert_eq!(tree.nodes.len(), 3);
    }

    #[test]
    fn mixed_membership_edits_match_linear_scan() {
        let mut boxes: Vec<Option<Bounds>> = (0..257)
            .map(|i| Some([i as f64, i as f64, i as f64 + 2., i as f64 + 2.]))
            .collect();
        let mut tree = SpatialIndex::build(boxes.iter().copied().enumerate());
        for step in 0..1200 {
            let item = step * 37 % boxes.len();
            boxes[item] = if step % 3 == 0 {
                None
            } else {
                let x = (step * 13 % 300) as f64;
                Some([x, 0., x + 4., 4.])
            };
            assert!(tree.refit(item, boxes[item]));
            for query in [[0., 0., 50., 50.], [150., 0., 170., 200.], [f64::NAN; 4]] {
                let expected: Vec<_> = boxes
                    .iter()
                    .enumerate()
                    .filter_map(|(id, bounds)| {
                        (bounds.is_none_or(|b| !valid(query) || intersects(b, query))).then_some(id)
                    })
                    .collect();
                assert_eq!(tree.query(query).items, expected, "step {step}");
            }
        }
    }

    #[test]
    fn scratch_queries_match_allocating_queries_and_reuse_capacity() {
        let tree =
            SpatialIndex::build((0..1000).map(|i| (i, Some([i as f64, 0., i as f64 + 1., 1.]))));
        let mut items = Vec::with_capacity(1000);
        let mut stack = Vec::with_capacity(64);
        let item_address = items.as_ptr();
        let stack_address = stack.as_ptr();
        for bounds in [
            [0., 0., 999., 1.],
            [100., 0., 110., 1.],
            [2000., 0., 2001., 1.],
            [f64::NAN; 4],
        ] {
            let expected = tree.query(bounds);
            let visited = tree.query_into(bounds, &mut items, &mut stack);
            assert_eq!(items, expected.items);
            assert_eq!(visited, expected.visited_nodes);
            assert!(stack.is_empty());
            assert_eq!(items.as_ptr(), item_address);
            assert_eq!(stack.as_ptr(), stack_address);
        }
        SpatialIndex::default().query_into([0.; 4], &mut items, &mut stack);
        assert!(items.is_empty());
    }

    #[test]
    fn contained_refit_stops_without_changing_ancestor_bounds() {
        let mut tree = SpatialIndex::build([
            (0, Some([0., 0., 100., 100.])),
            (1, Some([10., 10., 20., 20.])),
        ]);
        let root = tree.nodes[0].bounds;
        assert!(tree.refit(1, Some([70., 70., 80., 80.])));
        assert_eq!(tree.nodes[0].bounds, root);
        assert_eq!(tree.query([11., 11., 12., 12.]).items, vec![0]);
        assert_eq!(tree.query([71., 71., 72., 72.]).items, vec![0, 1]);
        let published = tree.clone();
        assert!(tree.refit(1, Some([70., 70., 80., 80.])));
        assert_eq!(published.query([71., 71., 72., 72.]).items, vec![0, 1]);
        assert!(tree.refit(1, Some([170., 170., 180., 180.])));
        assert_eq!(tree.query([171., 171., 172., 172.]).items, vec![1]);
    }

    #[test]
    fn published_index_retains_old_bounds_after_refit() {
        let mut tree = SpatialIndex::build((0..100_000).map(|i| {
            let x = i as f64 * 10.;
            (i, Some([x, 0., x + 5., 5.]))
        }));
        let published = tree.clone();
        assert!(Arc::ptr_eq(&tree.leaves, &published.leaves));
        assert!(tree.refit(10, Some([2_000_000., 0., 2_000_005., 5.])));
        assert_eq!(published.query([100., 0., 105., 5.]).items, vec![10]);
        assert!(tree.query([100., 0., 105., 5.]).items.is_empty());
        let moved = tree.query([2_000_000., 0., 2_000_005., 5.]);
        assert_eq!(moved.items, vec![10]);
        assert!(moved.visited_nodes < 100);
        assert!(published
            .query([2_000_000., 0., 2_000_005., 5.])
            .items
            .is_empty());
    }
    #[test]
    fn refit_matches_linear_scan_and_preserves_order() {
        let mut boxes: Vec<_> = (0..10000)
            .map(|i| [i as f64 * 10., 0., i as f64 * 10. + 5., 5.])
            .collect();
        let mut tree = SpatialIndex::build(boxes.iter().enumerate().map(|(i, b)| (i, Some(*b))));
        let query = [102., 0., 120., 5.];
        let expected = |boxes: &[Bounds]| {
            boxes
                .iter()
                .enumerate()
                .filter(|(_, b)| intersects(**b, query))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        assert_eq!(tree.query(query).items, expected(&boxes));
        assert!(tree.query(query).visited_nodes < 100);
        boxes[8000] = [110., 0., 113., 5.];
        assert!(tree.refit(8000, Some(boxes[8000])));
        assert_eq!(tree.query(query).items, expected(&boxes));
        boxes[11] = [-100., 0., -95., 5.];
        assert!(tree.refit(11, Some(boxes[11])));
        assert_eq!(tree.query(query).items, expected(&boxes));
        assert!(tree.refit(11, None));
        assert!(tree.query(query).items.contains(&11));
    }
    #[test]
    fn unknown_empty_touching_and_invalid_queries_are_conservative() {
        assert!(SpatialIndex::default().query([0.; 4]).items.is_empty());
        let tree = SpatialIndex::build([
            (1, None),
            (2, Some([0., 0., 10., 10.])),
            (3, Some([f64::NAN; 4])),
        ]);
        assert_eq!(tree.query([10., 10., 10., 10.]).items, [1, 2, 3]);
        assert_eq!(tree.query([100., 100., 110., 110.]).items, [1, 3]);
        assert_eq!(tree.query([f64::NAN; 4]).items, [1, 2, 3]);
    }
}
