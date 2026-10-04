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
        let Some(bounds) = bounds.filter(|b| valid(*b)) else {
            return self.unknown.contains(&item);
        };
        let Some(&position) = self.leaves.get(&item) else {
            return false;
        };
        self.nodes[position].bounds = bounds;
        let mut parent = self.nodes[position].parent;
        while let Some(position) = parent {
            let [a, b] = self.nodes[position].children.unwrap();
            self.nodes[position].bounds = union(self.nodes[a].bounds, self.nodes[b].bounds);
            parent = self.nodes[position].parent;
        }
        true
    }
    pub fn query(&self, bounds: Bounds) -> Candidates {
        // Invalid queries cannot safely reject anything.
        let mut candidates = Candidates {
            items: self.unknown.as_ref().clone(),
            visited_nodes: 0,
        };
        let mut stack = if self.nodes.is_empty() {
            vec![]
        } else {
            vec![0]
        };
        while let Some(position) = stack.pop() {
            candidates.visited_nodes += 1;
            let node = &self.nodes[position];
            if valid(bounds) && !intersects(node.bounds, bounds) {
                continue;
            }
            if let Some([a, b]) = node.children {
                stack.extend([a, b]);
            } else {
                candidates.items.push(node.item);
            }
        }
        candidates.items.sort_unstable();
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
