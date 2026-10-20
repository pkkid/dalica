//! The AccessKit tree egui builds, reshaped into the window's: properties set on the nodes the
//! widgets made ([`Patch`]), and new nodes that gather existing ones under a name ([`Wrap`]: a
//! landmark around a region's nodes, a named group around a section's cells). A pure function of
//! the tree update and a [`Shape`], so it is tested without a window.
//!
//! **How a wrap finds its nodes.** egui parents a node under the nearest `Ui` that has one, so a
//! region's nodes hang below the region's own container, which hangs below the window's content.
//! A wrap names the nodes it is about (its *members*: the widgets drawn in the region). It gathers
//! them as tightly as it can: at the deepest node that holds every member (a lone member: at its
//! parent), the children that hold a member, put in its place where the first of them was. A wrap
//! whose children would hold a member of another wrap of the same batch is left out, never half
//! made: every node keeps exactly one parent.
//!
//! The work is linear in the tree's size times its depth, and the tree holds only what is on
//! screen (the light table's visible cells, the tree pane's visible rows), never the library.

use std::collections::{HashMap, HashSet};

use egui::accesskit::{Node, NodeId, Rect, Role, TreeUpdate};

/// How deep the split looks for a wrap's place; deeper trees are left as they are.
const MAX_DEPTH: usize = 64;

/// Properties set on a node egui made, when it is in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub node: NodeId,
    pub role: Option<Role>,
    pub label: Option<String>,
    pub value: Option<String>,
    pub description: Option<String>,
    pub selected: Option<bool>,
    pub level: Option<usize>,
    pub expanded: Option<bool>,
    /// Its place among its siblings: the 1-based position and the set's size (a tree's row, whose
    /// siblings are not all drawn).
    pub set: Option<(usize, usize)>,
}

impl Patch {
    pub fn new(node: NodeId) -> Patch {
        Patch { node, role: None, label: None, value: None, description: None, selected: None, level: None, expanded: None, set: None }
    }

    fn apply(&self, n: &mut Node) {
        if let Some(role) = self.role {
            n.set_role(role);
        }
        if let Some(label) = &self.label {
            n.set_label(label.as_str());
        }
        if let Some(value) = &self.value {
            n.set_value(value.as_str());
        }
        if let Some(description) = &self.description {
            n.set_description(description.as_str());
        }
        if let Some(selected) = self.selected {
            n.set_selected(selected);
        }
        if let Some(level) = self.level {
            n.set_level(level);
        }
        if let Some(expanded) = self.expanded {
            n.set_expanded(expanded);
        }
        if let Some((position, size)) = self.set {
            n.set_position_in_set(position);
            n.set_size_of_set(size);
        }
    }
}

/// A new node gathering the nodes that hold `members`.
#[derive(Debug, Clone, PartialEq)]
pub struct Wrap {
    /// The new node's id: unique in the tree (a wrap whose id is taken is left out).
    pub id: NodeId,
    pub role: Role,
    pub label: String,
    /// Where it is on screen, in points (egui's root node scales points to pixels).
    pub bounds: Option<Rect>,
    pub members: Vec<NodeId>,
}

/// What one frame reshapes: the patches, then each batch of wraps in turn (a later batch may
/// gather the nodes an earlier one made: the sections' groups into the light table's landmark).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shape {
    pub patches: Vec<Patch>,
    pub batches: Vec<Vec<Wrap>>,
}

/// Applies `shape` to `update` (a full tree, as egui sends every frame). Returns how many wraps
/// were made.
pub fn reshape(update: &mut TreeUpdate, shape: &Shape) -> usize {
    let mut index: HashMap<NodeId, usize> = update.nodes.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
    for p in &shape.patches {
        if let Some((_, n)) = index.get(&p.node).and_then(|i| update.nodes.get_mut(*i)) {
            p.apply(n);
        }
    }
    let Some(root) = update.tree.as_ref().map(|t| t.root) else { return 0 };
    let mut made = 0;
    for batch in &shape.batches {
        made += wrap_batch(update, &mut index, root, batch);
    }
    made
}

/// Each node's parent.
fn parents(update: &TreeUpdate) -> HashMap<NodeId, NodeId> {
    let mut out = HashMap::with_capacity(update.nodes.len());
    for (id, n) in &update.nodes {
        for c in n.children() {
            out.insert(*c, *id);
        }
    }
    out
}

fn wrap_batch(update: &mut TreeUpdate, index: &mut HashMap<NodeId, usize>, root: NodeId, batch: &[Wrap]) -> usize {
    let mut parent = parents(update);
    // Which wraps' members each node holds (itself or below), in the tree as egui made it.
    let mut marks: HashMap<NodeId, Vec<usize>> = HashMap::new();
    let mut live = Vec::new();
    let mut ids = HashSet::new();
    for (k, w) in batch.iter().enumerate() {
        if index.contains_key(&w.id) || !ids.insert(w.id) {
            continue; // the id is taken: never two nodes of one id
        }
        let members: Vec<NodeId> = w.members.iter().filter(|m| index.contains_key(m)).copied().collect();
        if members.is_empty() {
            continue;
        }
        for m in &members {
            for node in chain(&parent, *m) {
                let set = marks.entry(node).or_default();
                if !set.contains(&k) {
                    set.push(k);
                }
            }
        }
        live.push((k, members));
    }
    let mut made = 0;
    for (k, members) in live {
        let Some(w) = batch.get(k) else { continue };
        // The deepest node that holds every member; a lone member is gathered from its parent.
        let chains: Vec<Vec<NodeId>> = members.iter().map(|m| chain(&parent, *m).into_iter().rev().collect()).collect();
        let Some(first) = chains.first() else { continue };
        let depth = (0..first.len()).take_while(|i| chains.iter().all(|c| c.get(*i) == first.get(*i))).count();
        let Some(&lca) = depth.checked_sub(1).and_then(|d| first.get(d)) else { continue };
        if lca == root && depth == 1 && chains.iter().all(|c| c.len() == 1) {
            continue; // only the root itself
        }
        let (at, gathered): (NodeId, Vec<NodeId>) = if members.contains(&lca) {
            match parent.get(&lca) {
                Some(p) => (*p, vec![lca]),
                None => continue,
            }
        } else {
            let children = index.get(&lca).and_then(|i| update.nodes.get(*i)).map(|(_, n)| n.children().to_vec()).unwrap_or_default();
            (lca, children.into_iter().filter(|c| marks.get(c).is_some_and(|m| m.contains(&k))).collect())
        };
        // Only nodes that hold nothing of another wrap: every node keeps one parent and one place.
        if gathered.is_empty() || gathered.iter().any(|c| marks.get(c).is_some_and(|m| m.iter().any(|j| *j != k))) {
            continue;
        }
        let Some(children) = index.get(&at).and_then(|i| update.nodes.get(*i)).map(|(_, n)| n.children().to_vec()) else { continue };
        let mut kept = Vec::with_capacity(children.len());
        let mut placed = false;
        for c in &children {
            if gathered.contains(c) {
                if !placed {
                    kept.push(w.id);
                    placed = true;
                }
            } else {
                kept.push(*c);
            }
        }
        let mut n = Node::new(w.role);
        if !w.label.is_empty() {
            n.set_label(w.label.as_str());
        }
        if let Some(b) = w.bounds {
            n.set_bounds(b);
        }
        let inside: Vec<NodeId> = children.iter().filter(|c| gathered.contains(c)).copied().collect();
        for c in &inside {
            parent.insert(*c, w.id);
        }
        parent.insert(w.id, at);
        n.set_children(inside);
        index.insert(w.id, update.nodes.len());
        update.nodes.push((w.id, n));
        if let Some((_, p)) = index.get(&at).and_then(|i| update.nodes.get_mut(*i)) {
            p.set_children(kept);
        }
        made += 1;
    }
    made
}

/// `node` and its ancestors up to the root, nearest first (bounded, in case of a cycle).
fn chain(parent: &HashMap<NodeId, NodeId>, node: NodeId) -> Vec<NodeId> {
    let mut out = vec![node];
    let mut at = node;
    while let Some(p) = parent.get(&at) {
        if out.len() > MAX_DEPTH * 4 || out.contains(p) {
            break;
        }
        out.push(*p);
        at = *p;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Tree, TreeId};

    fn node(role: Role, children: &[u64]) -> Node {
        let mut n = Node::new(role);
        n.set_children(children.iter().map(|c| NodeId(*c)).collect::<Vec<_>>());
        n
    }

    /// root 0 → content 1 → { region 2 → {3, 4}, region 5 → {6}, bottom 7 → { tray 8 → {9}, buttons 10 → {11} } }
    fn sample() -> TreeUpdate {
        let nodes = vec![
            (NodeId(0), node(Role::Window, &[1])),
            (NodeId(1), node(Role::GenericContainer, &[2, 5, 7])),
            (NodeId(2), node(Role::GenericContainer, &[3, 4])),
            (NodeId(3), node(Role::Button, &[])),
            (NodeId(4), node(Role::TreeItem, &[])),
            (NodeId(5), node(Role::GenericContainer, &[6])),
            (NodeId(6), node(Role::ListBoxOption, &[])),
            (NodeId(7), node(Role::GenericContainer, &[8, 10])),
            (NodeId(8), node(Role::GenericContainer, &[9])),
            (NodeId(9), node(Role::ListBoxOption, &[])),
            (NodeId(10), node(Role::GenericContainer, &[11])),
            (NodeId(11), node(Role::Button, &[])),
        ];
        TreeUpdate { nodes, tree: Some(Tree::new(NodeId(0))), tree_id: TreeId::ROOT, focus: NodeId(0) }
    }

    fn get(u: &TreeUpdate, id: u64) -> &Node {
        &u.nodes.iter().find(|(n, _)| *n == NodeId(id)).unwrap().1
    }

    fn wrap(id: u64, label: &str, members: &[u64]) -> Wrap {
        Wrap { id: NodeId(id), role: Role::Region, label: label.into(), bounds: None, members: members.iter().map(|m| NodeId(*m)).collect() }
    }

    #[test]
    fn a_wrap_gathers_its_members_where_they_meet_in_place() {
        let mut u = sample();
        let shape = Shape { patches: vec![], batches: vec![vec![wrap(100, "Tree", &[3, 4]), wrap(101, "Light table", &[6])]] };
        assert_eq!(reshape(&mut u, &shape), 2);
        // The tree's two nodes inside their region; the light table's one where it was.
        assert_eq!(get(&u, 2).children(), [NodeId(100)]);
        assert_eq!((get(&u, 100).role(), get(&u, 100).label(), get(&u, 100).children()), (Role::Region, Some("Tree"), &[NodeId(3), NodeId(4)][..]));
        assert_eq!(get(&u, 5).children(), [NodeId(101)]);
        assert_eq!(get(&u, 101).children(), [NodeId(6)]);
    }

    #[test]
    fn a_wrap_of_nodes_in_several_containers_gathers_the_containers() {
        let mut u = sample();
        let shape = Shape { patches: vec![], batches: vec![vec![wrap(100, "Bottom", &[9, 11]), wrap(101, "Tree", &[3])]] };
        assert_eq!(reshape(&mut u, &shape), 2);
        assert_eq!(get(&u, 7).children(), [NodeId(100)]);
        assert_eq!(get(&u, 100).children(), [NodeId(8), NodeId(10)]);
    }

    #[test]
    fn wraps_in_one_container_each_gather_their_own() {
        let mut u = sample();
        let shape =
            Shape { patches: vec![], batches: vec![vec![wrap(100, "Tray", &[9]), wrap(101, "Tray buttons", &[11]), wrap(102, "Tree", &[4])]] };
        assert_eq!(reshape(&mut u, &shape), 3);
        assert_eq!(get(&u, 8).children(), [NodeId(100)]);
        assert_eq!(get(&u, 100).children(), [NodeId(9)]);
        assert_eq!(get(&u, 2).children(), [NodeId(3), NodeId(102)]);
    }

    #[test]
    fn members_that_never_separate_are_left_alone_and_every_node_keeps_one_parent() {
        let mut u = sample();
        // A spans regions 2 and 5, and region 2 also holds B's member: A has no place of its own;
        // B is gathered inside region 2, round its one node.
        let shape = Shape { patches: vec![], batches: vec![vec![wrap(100, "A", &[3, 6, 9]), wrap(101, "B", &[4])]] };
        assert_eq!(reshape(&mut u, &shape), 1);
        let mut seen = HashSet::new();
        for (_, n) in &u.nodes {
            for c in n.children() {
                assert!(seen.insert(*c), "{c:?} has two parents");
            }
        }
        assert!(!u.nodes.iter().any(|(id, _)| *id == NodeId(100)), "A is not made");
        assert_eq!(get(&u, 2).children(), [NodeId(3), NodeId(101)]);
        assert_eq!(get(&u, 101).children(), [NodeId(4)]);
    }

    #[test]
    fn patches_set_properties_and_unknown_or_taken_ids_change_nothing() {
        let mut u = sample();
        let mut p = Patch::new(NodeId(4));
        p.level = Some(2);
        p.expanded = Some(true);
        p.description = Some("20 photos".into());
        let ghost = Patch { label: Some("nobody".into()), ..Patch::new(NodeId(999)) };
        let taken = wrap(3, "Taken", &[4]);
        let before = u.nodes.len();
        let made = reshape(&mut u, &Shape { patches: vec![p, ghost], batches: vec![vec![taken], vec![wrap(100, "Gone", &[999])]] });
        assert_eq!(made, 0);
        assert_eq!(u.nodes.len(), before);
        let n = get(&u, 4);
        assert_eq!((n.level(), n.is_expanded(), n.description()), (Some(2), Some(true), Some("20 photos")));
    }

    #[test]
    fn a_later_batch_gathers_what_an_earlier_one_made() {
        let mut u = sample();
        // Two sections' groups, then the landmark round both.
        let groups = vec![wrap(100, "Section A", &[9]), wrap(101, "Section B", &[11])];
        let landmarks = vec![wrap(200, "Light table", &[9, 11]), wrap(201, "Tree", &[3])];
        assert_eq!(reshape(&mut u, &Shape { patches: vec![], batches: vec![groups, landmarks] }), 4);
        assert_eq!(get(&u, 8).children(), [NodeId(100)]);
        assert_eq!(get(&u, 7).children(), [NodeId(200)]);
        assert_eq!(get(&u, 200).children(), [NodeId(8), NodeId(10)]);
    }
}
