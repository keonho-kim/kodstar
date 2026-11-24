// Purpose: petgraph BFS에 방향별 정점과 바이트 제한 발견 집합을 제공.
// Pattern: Graph View.
// Usage: memory 탐색이 이미 읽은 이웃 목록을 한 번 확장.
// Related: crates/kodstar-core/src/application/query/traversal/memory.rs.

use petgraph::visit::{GraphBase, IntoNeighbors, VisitMap};
use roaring::RoaringTreemap;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Orientation {
    In,
    Out,
}

impl Orientation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Node {
    pub id: i64,
    pub direction: Orientation,
    pub depth: usize,
    pub parent: Option<i64>,
    pub via: Option<i64>,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        // Discovery metadata does not change identity; the first parent wins.
        self.id == other.id && self.direction == other.direction
    }
}

pub(super) struct Layer<'a>(pub &'a [Node]);

impl GraphBase for Layer<'_> {
    type NodeId = Node;
    type EdgeId = ();
}

impl<'a> IntoNeighbors for &Layer<'a> {
    type Neighbors = std::iter::Copied<std::slice::Iter<'a, Node>>;
    fn neighbors(self, _: Node) -> Self::Neighbors {
        self.0.iter().copied()
    }
}

pub(super) struct Discovered {
    nodes: [RoaringTreemap; 2],
    groups: BTreeSet<u64>,
    bytes: usize,
    pub overflow: bool,
}

impl Discovered {
    pub fn new(bytes: usize) -> Self {
        Self {
            nodes: Default::default(),
            groups: BTreeSet::new(),
            bytes,
            overflow: false,
        }
    }
}

impl VisitMap<Node> for Discovered {
    fn visit(&mut self, node: Node) -> bool {
        if self.is_visited(&node) {
            return false;
        }
        let role = node.direction as usize;
        let group = (role as u64) << 48 | node.id as u64 >> 16;
        // Conservative charge includes the bitmap, conversion buffers and tree nodes.
        if !self.groups.contains(&group) && (self.groups.len() + 1) * 32768 > self.bytes {
            self.overflow = true;
            return false;
        }
        self.groups.insert(group);
        self.nodes[role].insert(node.id as u64);
        true
    }

    fn is_visited(&self, node: &Node) -> bool {
        self.nodes[node.direction as usize].contains(node.id as u64)
    }

    fn unvisit(&mut self, node: Node) -> bool {
        self.nodes[node.direction as usize].remove(node.id as u64)
    }
}
