use std::{
    collections::{HashMap, HashSet},
    error::Error,
    hash::Hash,
    iter::zip,
    mem,
    sync::{Arc, Mutex},
};

use super::{
    ChannelController, ChannelControllerCallbacks, LbConfig, LbPolicy, LbPolicyBuilder,
    LbPolicyBuilderCallbacks, LbPolicyCallbacks, LbPolicyOptions, LbState, SubchannelUpdate,
    SubchannelUpdateFn, WorkScheduler,
};
use crate::client::name_resolution::{Address, ResolverData, ResolverUpdate};

use super::{Subchannel, SubchannelState};

// An LbPolicy implementation that manages multiple children.
// Calls in must all be synchronized via the parent channel's channel_controller.
#[derive(Clone)]
pub struct ChildManagerCallbacks<T: Clone> {
    // inner must be in a mutex even though it is supposed to be only accessed
    // synchronously, because there is no way to guarantee accesses are
    // performed that way, because the callback to update the state cannot pass
    // a &mut self ChildManagerCallbacks as a parameter.
    inner: Arc<Mutex<Inner<T>>>,
}

struct Inner<T> {
    subchannel_child_map: HashMap<Subchannel, usize>,
    children: Vec<Child<T>>,
    shard_update: Box<ResolverUpdateSharder<T>>,
}

impl<T: Clone> Inner<T> {
    fn resolve_child_controller(
        &mut self,
        channel_controller: WrappedControllerCallbacks<T>,
        child_idx: usize,
    ) {
        /*println!(
            "resolving child controller: {:?}, {:?}",
            channel_controller.created_subchannels.len(),
            channel_controller
                .picker_update
                .clone()
                .map_or("none".to_string(), |s| format!(
                    "{:?}",
                    s.connectivity_state
                ))
        );*/
        for csc in channel_controller.created_subchannels {
            self.subchannel_child_map.insert(csc, child_idx);
        }
        if let Some(state) = channel_controller.picker_update {
            self.children[child_idx].state = state;
        };
    }
}

struct Child<T> {
    identifier: T,
    policy: Box<dyn LbPolicyCallbacks>,
    state: LbState,
}

pub struct ChildUpdate<T> {
    pub child_identifier: T,
    pub child_policy_builder: Box<dyn LbPolicyBuilderCallbacks>,
    pub child_update: ResolverUpdate,
}

// Shards a ResolverUpdate into ChildUpdates
pub type ResolverUpdateSharder<T> =
    fn(
        ResolverUpdate,
    ) -> Result<Box<dyn Iterator<Item = ChildUpdate<T>>>, Box<dyn Error + Send + Sync>>;

impl<T: Clone + PartialEq + Hash + Eq> ChildManagerCallbacks<T> {
    pub fn new(shard_update: Box<ResolverUpdateSharder<T>>) -> Self {
        // Need: access last picker updates, probably just have user call a method to get.
        Self {
            inner: Arc::new(Mutex::new(Inner {
                subchannel_child_map: HashMap::default(),
                children: Vec::default(),
                shard_update,
            })),
        }
    }

    // Returns all children that have produced a state update.
    // ChannelControllerCallbacks is a parameter to enforce correct usage, but
    // is not used.
    pub fn child_states(&mut self) -> Vec<(T, LbState)> {
        self.inner
            .lock()
            .unwrap()
            .children
            .iter()
            .map(|child| (child.identifier.clone(), child.state.clone()))
            .collect::<Vec<_>>()
    }
}

// ChildManager implements LbPolicy forwarding
impl<T: Clone + PartialEq + Hash + Eq + Send + 'static> LbPolicyCallbacks
    for ChildManagerCallbacks<T>
{
    fn resolver_update(
        &mut self,
        resolver_update: ResolverUpdate,
        config: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelControllerCallbacks,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut inner = self.inner.lock().unwrap();
        // First determine if the incoming update is valid.
        let child_updates = (inner.shard_update)(resolver_update)?;

        // Replace self.children with an empty vec.
        let mut old_children = vec![];
        mem::swap(&mut inner.children, &mut old_children);

        // Replace the subchannel map with an empty map.
        let mut old_subchannel_child_map = HashMap::new();
        mem::swap(
            &mut inner.subchannel_child_map,
            &mut old_subchannel_child_map,
        );
        // Reverse the subchannel map.
        let mut old_child_subchannels_map: HashMap<usize, Vec<Subchannel>> = HashMap::new();
        for (subchannel, child_idx) in old_subchannel_child_map {
            old_child_subchannels_map
                .entry(child_idx)
                .or_insert_with(|| vec![])
                .push(subchannel);
        }

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
                for subchannel in old_child_subchannels_map
                    .remove(&old_idx)
                    .into_iter()
                    .flatten()
                {
                    inner.subchannel_child_map.insert(subchannel, new_idx);
                }
                inner.children.push(Child {
                    identifier,
                    state,
                    policy,
                });
            } else {
                let policy = builder.build(LbPolicyOptions {
                    work_scheduler: Arc::new(NopWorkScheduler {}), /* TODO */
                });
                let state = LbState::initial();
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
            let policy = &mut inner.children[child_idx].policy;
            let child_update = updates.next().unwrap();
            let mut channel_controller: WrappedControllerCallbacks<'_, T> =
                WrappedControllerCallbacks::new(channel_controller, self.clone());
            let _ = policy.resolver_update(child_update, config, &mut channel_controller);
            inner.resolve_child_controller(channel_controller, child_idx);
        }
        Ok(())
    }
}

pub struct WrappedControllerCallbacks<'a, T: Clone> {
    channel_controller: &'a mut dyn ChannelControllerCallbacks,
    created_subchannels: Vec<Subchannel>,
    picker_update: Option<LbState>,
    parent: ChildManagerCallbacks<T>,
}

impl<'a, T: Clone + Send> WrappedControllerCallbacks<'a, T> {
    fn new(
        channel_controller: &'a mut dyn ChannelControllerCallbacks,
        parent: ChildManagerCallbacks<T>,
    ) -> Self {
        Self {
            channel_controller,
            created_subchannels: vec![],
            picker_update: None,
            parent,
        }
    }
}

impl<'a, T: Clone + Send + 'static> ChannelControllerCallbacks
    for WrappedControllerCallbacks<'a, T>
{
    fn new_subchannel(&mut self, address: &Address, updates: SubchannelUpdateFn) -> Subchannel {
        let parent = self.parent.clone();
        let subchannel = self.channel_controller.new_subchannel(
            address,
            Box::new(move |subchannel, subchannel_state, channel_controller| {
                let mut inner = parent.inner.lock().unwrap();
                let Some(&child_idx) = inner.subchannel_child_map.get(&subchannel) else {
                    return;
                };
                let mut channel_controller: WrappedControllerCallbacks<'_, T> =
                    WrappedControllerCallbacks::new(channel_controller, parent.clone());
                updates(subchannel, subchannel_state, &mut channel_controller);
                inner.resolve_child_controller(channel_controller, child_idx);
            }),
        );
        self.created_subchannels.push(subchannel.clone());
        subchannel
    }

    fn update_picker(&mut self, update: LbState) {
        self.picker_update = Some(update);
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
