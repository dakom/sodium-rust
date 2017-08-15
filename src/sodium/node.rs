use sodium::SodiumCtx;
use sodium::Transaction;
use sodium::TransactionHandlerRef;
use sodium::WeakTransactionHandlerRef;
use std::any::Any;
use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::cell::Ref;
use std::cell::RefMut;
use std::cmp::Ordering;
use std::cmp::PartialEq;
use std::cmp::PartialOrd;
use std::collections::HashSet;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr;
use std::rc::Rc;
use std::rc::Weak;

pub trait IsNode {
    fn downcast_to_node_ref(&self) -> &Node;
    fn downcast_to_node_mut(&mut self) -> &mut Node;
}

pub struct Node {
    pub id: u32,
    pub rank: u64,
    pub listeners: Vec<Target>
}

impl Node {
    pub fn new(sodium_ctx: &mut SodiumCtx, rank: u64) -> Node {
        Node {
            id: sodium_ctx.new_id(),
            rank: rank,
            listeners: Vec::new()
        }
    }
}

pub struct GhostNode {
    node: *mut Node
}

pub struct Target {
    pub id: u32,
    pub node: Rc<RefCell<IsNode>>,
    pub action: WeakTransactionHandlerRef<Any>
}

impl Clone for Target {
    fn clone(&self) -> Self {
        Target {
            id: self.id.clone(),
            node: self.node.clone(),
            action: self.action.clone()
        }
    }
}

impl IsNode for Node {
    fn downcast_to_node_ref(&self) -> &Node {
        self
    }
    fn downcast_to_node_mut(&mut self) -> &mut Node {
        self
    }
}

impl IsNode for GhostNode {
    fn downcast_to_node_ref(&self) -> &Node {
        unsafe { &*self.node }
    }
    fn downcast_to_node_mut(&mut self) -> &mut Node {
        unsafe { &mut *self.node }
    }
}

impl Target {
    pub fn new<A:'static>(sodium_ctx: &mut SodiumCtx, node: Rc<RefCell<IsNode>>, action: TransactionHandlerRef<A>) -> Target {
        Target {
            id: sodium_ctx.new_id(),
            node: node,
            action: action.into_any().downgrade()
        }
    }
}

impl IsNode {
    pub fn link_to<A:'static>(&mut self, sodium_ctx: &mut SodiumCtx, target: Rc<RefCell<IsNode>>, action: TransactionHandlerRef<A>) -> (Target,bool) {
        let changed;
        {
            let target2: &RefCell<IsNode> = target.borrow();
            let mut target3: RefMut<IsNode> = target2.borrow_mut();
            let target4: &mut IsNode = target3.deref_mut();
            changed = target4.ensure_bigger_than(self.downcast_to_node_ref().rank, &mut HashSet::new());
        }
        let t = Target::new(sodium_ctx, target, action);
        self.downcast_to_node_mut().listeners.push(t.clone());
        (t, changed)
    }

    pub fn unlink_to(&mut self, target: Target) {
        let id = target.id.clone();
        self.downcast_to_node_mut().listeners.retain(
            move |target| {
                let id2 = target.id.clone();
                id != id2
            }
        )
    }

    pub fn ensure_bigger_than(&mut self, limit: u64, visited: &mut HashSet<u32>) -> bool {
        let listeners;
        let rank;
        {
            let self_ = self.downcast_to_node_mut();
            if self_.rank > limit || visited.contains(&self_.id) {
                return false;
            }
            visited.insert(self_.id.clone());
            self_.rank = limit + 1;
            listeners = self_.listeners.clone();
            rank = self_.rank.clone();
        }
        for target in listeners {
            let node: &RefCell<IsNode> = target.node.borrow();
            let mut node2: RefMut<IsNode> = node.borrow_mut();
            let node3: &mut IsNode = node2.deref_mut();
            node3.ensure_bigger_than(rank, visited);
        }
        return true;
    }
}

impl Ord for IsNode + 'static {
    fn cmp(&self, other: &(IsNode + 'static)) -> Ordering {
        self.downcast_to_node_ref().rank.cmp(&other.downcast_to_node_ref().rank)
    }
}

impl PartialOrd for IsNode + 'static {
    fn partial_cmp(&self, other: &(IsNode + 'static)) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for IsNode {}

impl PartialEq for IsNode + 'static {
    fn eq(&self, other: &(IsNode + 'static)) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
