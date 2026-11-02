use std::cell::Cell;

pub struct DisjointSet {
    parents: Vec<Cell<usize>>,
    members_of: Vec<Vec<usize>>,
}

impl DisjointSet {
    fn get_parent(&self, id: usize) -> usize {
        self.parents[id].get()
    }

    fn set_parent(&self, id: usize, new: usize) {
        self.parents[id].set(new);
    }

    // find with path splitting
    pub fn root_of(&self, mut child: usize) -> usize {
        let mut parent = self.get_parent(child);

        if child == parent {
            return child;
        };

        loop {
            let grandparent = self.get_parent(parent);
            if parent == grandparent {
                return parent;
            }

            self.set_parent(child, grandparent);
            child = parent;
            parent = grandparent;
        }
    }

    // union
    pub fn union(&mut self, x: usize, y: usize) {
        let root_x = self.root_of(x);
        let root_y = self.root_of(y);

        if root_x == root_y {
            return;
        }

        let (root, merger) = if self.members_of[root_x].len() >= self.members_of[root_y].len() {
            (root_x, root_y)
        } else {
            (root_y, root_x)
        };

        self.set_parent(merger, root);
        let moved = std::mem::take(&mut self.members_of[merger]);
        self.members_of[root].extend(moved);
    }

    pub fn in_same_group(&self, x: usize, y: usize) -> bool {
        self.root_of(x) == self.root_of(y)
    }

    pub fn members_of(&self, id: usize) -> &[usize] {
        let root = self.root_of(id);
        &self.members_of[root]
    }

    pub fn get_roots(&self) -> Vec<usize> {
        (0..self.len())
            .filter(|&index| self.parents[index].get() == index)
            .collect()
    }

    pub fn with_len(len: usize) -> Self {
        Self {
            parents: (0..len).map(Cell::new).collect(),
            members_of: (0..len).map(|x| vec![x]).collect(),
        }
    }

    fn len(&self) -> usize {
        self.parents.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Members in a fixed order, so groups compare regardless of union order
    fn sorted_members(disjoint_set: &DisjointSet, id: usize) -> Vec<usize> {
        let mut members = disjoint_set.members_of(id).to_vec();
        members.sort();
        members
    }

    #[test]
    fn every_element_starts_in_its_own_group() {
        let disjoint_set = DisjointSet::with_len(3);

        assert_eq!(disjoint_set.get_roots(), [0, 1, 2]);
        for id in 0..3 {
            assert_eq!(disjoint_set.members_of(id), [id]);
        }
    }

    #[test]
    fn union_is_transitive() {
        // 0-1 and 1-2 are linked, 0-2 never is
        let mut disjoint_set = DisjointSet::with_len(4);
        disjoint_set.union(0, 1);
        disjoint_set.union(1, 2);

        assert!(disjoint_set.in_same_group(0, 2));
        assert!(!disjoint_set.in_same_group(0, 3));
        assert_eq!(disjoint_set.get_roots().len(), 2);
    }

    #[test]
    fn repeated_and_self_unions_change_nothing() {
        let mut disjoint_set = DisjointSet::with_len(3);
        disjoint_set.union(0, 0);
        assert_eq!(disjoint_set.get_roots(), [0, 1, 2]);

        // Both directions, twice: a member moved more than once would show up as a duplicate
        disjoint_set.union(0, 1);
        disjoint_set.union(1, 0);
        disjoint_set.union(0, 1);

        assert_eq!(sorted_members(&disjoint_set, 0), [0, 1]);
        assert_eq!(disjoint_set.get_roots().len(), 2);
    }

    #[test]
    fn members_are_the_same_from_any_member() {
        let mut disjoint_set = DisjointSet::with_len(5);
        disjoint_set.union(3, 1);
        disjoint_set.union(4, 3);

        for id in [1, 3, 4] {
            assert_eq!(sorted_members(&disjoint_set, id), [1, 3, 4], "from {id}");
        }
        assert_eq!(sorted_members(&disjoint_set, 0), [0]);
        assert_eq!(sorted_members(&disjoint_set, 2), [2]);
    }

    #[test]
    fn deep_tree_ends_in_one_group() {
        // Union by size keeps a one-at-a-time chain flat, so merge equal-sized groups in rounds
        // instead: each round adds a level, giving a tree 10 deep that path splitting must
        // shorten without losing anyone
        let len = 1 << 10;
        let mut disjoint_set = DisjointSet::with_len(len);
        let mut step = 1;
        while step < len {
            for id in (0..len).step_by(2 * step) {
                disjoint_set.union(id, id + step);
            }
            step *= 2;
        }

        let root = disjoint_set.root_of(len - 1);
        for id in 0..len {
            assert_eq!(disjoint_set.root_of(id), root, "root of {id}");
        }
        assert_eq!(disjoint_set.get_roots(), [root]);
        assert_eq!(
            sorted_members(&disjoint_set, 0),
            (0..len).collect::<Vec<_>>()
        );
    }
}
