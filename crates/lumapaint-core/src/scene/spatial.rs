//! Conservative BVH; unknown bounds always remain candidates.
use std::{collections::HashMap, sync::Arc};

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
}
#[derive(Clone, Debug, Default)]
pub struct SpatialIndex {
    nodes: super::pages::Pages<Node>,
    leaves: Arc<HashMap<usize, usize>>,
    unknown: Arc<Vec<usize>>,
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
                Arc::make_mut(&mut index.unknown).push(id);
            }
        }
        if !known.is_empty() {
            index.branch(&mut known, None);
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
        }
        position
    }
    /// Returns false only when the known/unknown membership changes.
    pub fn refit(&mut self, item: usize, bounds: Option<Bounds>) -> bool {
        let _timer = crate::performance::time("spatial_refit_host");
        let Some(bounds) = bounds.filter(|b| valid(*b)) else {
            return self.unknown.contains(&item);
        };
        let Some(&position) = self.leaves.get(&item) else {
            return false;
        };
        if self.nodes[position].bounds == bounds {
            crate::performance::count("spatial_refit_unchanged_leaf", 1);
            return true;
        }
        self.nodes[position].bounds = bounds;
        let mut parent = self.nodes[position].parent;
        while let Some(position) = parent {
            let [a, b] = self.nodes[position].children.unwrap();
            crate::performance::count("spatial_refit_parent_visits", 1);
            let bounds = union(self.nodes[a].bounds, self.nodes[b].bounds);
            // Ancestors depend only on this union. Once it is unchanged, all
            // remaining ancestors are unchanged too, even if the leaf moved.
            if self.nodes[position].bounds == bounds {
                crate::performance::count("spatial_refit_early_stops", 1);
                break;
            }
            self.nodes[position].bounds = bounds;
            parent = self.nodes[position].parent;
        }
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
        items.extend_from_slice(&self.unknown);
        stack.clear();
        if !self.nodes.is_empty() {
            stack.push(0);
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
        assert!(!tree.refit(11, None));
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
