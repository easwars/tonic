use std::{
    collections::{HashMap, HashSet},
    error::Error,
    hash::Hash,
    iter::zip,
    mem,
    sync::{Arc, Mutex},
};

use super::{
    ChannelControllerCallbacks as ChannelController, LbConfig,
    LbPolicyBuilderCallbacks as LbPolicyBuilder, LbPolicyCallbacks as LbPolicy, LbPolicyOptions,
    LbPolicySingle, LbState, SubchannelUpdate, SubchannelUpdateFn, WorkScheduler,
};
use crate::client::name_resolution::{Address, ResolverData, ResolverUpdate};

use super::{Subchannel, SubchannelState};

// An LbPolicy implementation that manages multiple children.
// Calls in must all be synchronized via the parent channel's channel_controller.
#[derive(Clone)]
pub struct ChildManager<T: Clone> {
    // inner must be in a mutex even though it is supposed to be only accessed
    // synchronously, because there is no way to guarantee accesses are
    // performed that way, because the callback to update the state cannot pass
    // a &mut self ChildManagerCallbacks as a parameter.
    inner: Arc<Mutex<Inner<T>>>,
}

struct Inner<T> {
    children: Vec<Child<T>>,
    shard_update: Box<ResolverUpdateSharder<T>>,
}

struct Child<T> {
    identifier: T,
    policy: Box<dyn LbPolicy>,
    state: Arc<Mutex<LbState>>,
}

pub struct ChildUpdate<T> {
    pub child_identifier: T,
    pub child_policy_builder: Box<dyn LbPolicyBuilder>,
    pub child_update: ResolverUpdate,
}

// Shards a ResolverUpdate into ChildUpdates
pub type ResolverUpdateSharder<T> =
    fn(
        ResolverUpdate,
    ) -> Result<Box<dyn Iterator<Item = ChildUpdate<T>>>, Box<dyn Error + Send + Sync>>;

impl<T: Clone + PartialEq + Hash + Eq> ChildManager<T> {
    pub fn new(shard_update: Box<ResolverUpdateSharder<T>>) -> Self {
        // Need: access last picker updates, probably just have user call a method to get.
        Self {
            inner: Arc::new(Mutex::new(Inner {
                children: Vec::default(),
                shard_update,
            })),
        }
    }

    // Returns all children that have produced a state update.
    // ChannelControllerCallbacks is a parameter to enforce correct usage, but
    // is not used.
    pub fn child_states(&mut self) -> Vec<(T, Arc<Mutex<LbState>>)> {
        self.inner
            .lock()
            .unwrap()
            .children
            .iter()
            .map(|child| (child.identifier.clone(), child.state.clone()))
            .collect()
    }

    pub fn map_child_states<V>(
        &mut self,
        f: impl FnMut((&T, &Arc<Mutex<LbState>>)) -> V,
    ) -> Vec<V> {
        self.inner
            .lock()
            .unwrap()
            .children
            .iter()
            .map(|child| (&child.identifier, &child.state))
            .map(f)
            .collect()
    }
}

// ChildManager implements LbPolicy forwarding
impl<T: Clone + PartialEq + Hash + Eq + Send + 'static> LbPolicy for ChildManager<T> {
    fn resolver_update(
        &mut self,
        resolver_update: ResolverUpdate,
        config: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut inner = self.inner.lock().unwrap();
        // First determine if the incoming update is valid.
        let child_updates = (inner.shard_update)(resolver_update)?;

        // Replace self.children with an empty vec.
        let mut old_children = vec![];
        mem::swap(&mut inner.children, &mut old_children);

        // Hash the old children for efficient lookups.
        let old_children = old_children
            .into_iter()
            .enumerate()
            .map(|(old_idx, e)| (e.identifier, (e.policy, e.state, old_idx)));
        let mut old_children: HashMap<T, _> = old_children.collect();

        // Split the child updates into the IDs and builders, and the
        // ResolverUpdates.
        let (ids_builders, updates): (Vec<_>, Vec<_>) = child_updates
            .map(|e| ((e.child_identifier, e.child_policy_builder), e.child_update))
            .unzip();

        // Transfer children whose identifiers appear before and after the
        // update, and create new children.  Add entries back into the
        // subchannel map.
        for (new_idx, (identifier, builder)) in ids_builders.into_iter().enumerate() {
            if let Some((policy, state, old_idx)) = old_children.remove(&identifier) {
                inner.children.push(Child {
                    identifier,
                    state,
                    policy,
                });
            } else {
                let policy = builder.build(LbPolicyOptions {
                    work_scheduler: Arc::new(NopWorkScheduler {}), /* TODO */
                });
                let state = Arc::new(Mutex::new(LbState::initial()));
                inner.children.push(Child {
                    identifier,
                    state,
                    policy,
                });
            };
        }
        // Anything left in old_children will just be Dropped and cleaned up.

        // Call resolver_update on all children.
        let mut updates = updates.into_iter();
        for child_idx in 0..inner.children.len() {
            let child_state = inner.children[child_idx].state.clone();
            let policy = &mut inner.children[child_idx].policy;
            let child_update = updates.next().unwrap();
            let mut channel_controller =
                WrappedControllerCallbacks::new(channel_controller, child_state);
            let _ = policy.resolver_update(child_update, config, &mut channel_controller);
        }
        Ok(())
    }
}

pub struct WrappedControllerCallbacks<'a> {
    channel_controller: &'a mut dyn ChannelController,
    parent_lb_state_for_child: Arc<Mutex<LbState>>,
}

impl<'a> WrappedControllerCallbacks<'a> {
    fn new(
        channel_controller: &'a mut dyn ChannelController,
        parent_lb_state_for_child: Arc<Mutex<LbState>>,
    ) -> Self {
        Self {
            channel_controller,
            parent_lb_state_for_child,
        }
    }
}

impl<'a> ChannelController for WrappedControllerCallbacks<'a> {
    fn new_subchannel(&mut self, address: &Address, updates: SubchannelUpdateFn) -> Subchannel {
        let plsfc = self.parent_lb_state_for_child.clone();
        let subchannel = self.channel_controller.new_subchannel(
            address,
            Box::new(move |subchannel, subchannel_state, channel_controller| {
                let mut channel_controller =
                    WrappedControllerCallbacks::new(channel_controller, plsfc.clone());
                updates(subchannel, subchannel_state, &mut channel_controller);
            }),
        );
        subchannel
    }

    fn update_picker(&mut self, update: LbState) {
        *self.parent_lb_state_for_child.lock().unwrap() = update;
    }

    fn request_resolution(&mut self) {
        self.channel_controller.request_resolution();
    }
}

pub struct NopWorkScheduler {}
impl WorkScheduler for NopWorkScheduler {
    fn schedule_work(&self) { /* do nothing */
    }
}
