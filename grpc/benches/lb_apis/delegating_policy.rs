use std::{
    borrow::BorrowMut,
    cell::{RefCell, RefMut},
    collections::HashSet,
    error::Error,
    hash::Hash,
    marker::PhantomData,
    mem,
    ops::DerefMut,
    rc::Rc,
    sync::atomic::{AtomicU32, AtomicUsize},
};

use grpc::client::{
    load_balancing::{
        child_manager::{ChildManager, ChildUpdate},
        ChannelController, LbConfig, LbPolicy, LbPolicyBuilder, LbPolicyOptions, LbState, Pick,
        PickResult, Picker, QueuingPicker, SubchannelUpdate, WorkScheduler,
    },
    name_resolution::{Address, ResolverData, ResolverUpdate},
};

use crate::*;

pub struct DelegatingPolicy {
    child_manager: ChildManager<Endpoint>,
}

impl DelegatingPolicy {
    pub fn new() -> Self {
        Self {
            child_manager: ChildManager::<Endpoint>::new(Box::new(|resolver_update| {
                let ResolverUpdate::Data(rd) = resolver_update else {
                    return Err("bad update".into());
                };
                let mut v = vec![];
                for endpoint in rd.endpoints {
                    let child_policy_builder: Box<dyn LbPolicyBuilder> =
                        Box::new(ChildPolicyBuilder {});
                    let mut rd = ResolverData::default();
                    rd.endpoints.push(endpoint.clone());
                    let child_update = ResolverUpdate::Data(rd);
                    v.push(ChildUpdate {
                        child_identifier: endpoint,
                        child_policy_builder,
                        child_update,
                    });
                }
                Ok(Box::new(v.into_iter()))
            })),
        }
    }

    fn update_picker(&mut self, channel_controller: &mut dyn ChannelController) {
        let connectivity_states = self
            .child_manager
            .child_states()
            .map(|(_, lbstate)| lbstate.connectivity_state);
        let connectivity_state = effective_state(connectivity_states);
        if connectivity_state == ConnectivityState::Ready
            || connectivity_state == ConnectivityState::TransientFailure
        {
            let children = self
                .child_manager
                .child_states()
                .filter_map(|(_, lbstate)| {
                    if lbstate.connectivity_state == connectivity_state {
                        return Some(lbstate.picker.clone());
                    }
                    None
                })
                .collect();
            channel_controller.update_picker(LbState {
                connectivity_state,
                picker: Arc::new(RRPickerPicker::new(children)),
            });
        } else {
            channel_controller.update_picker(LbState {
                connectivity_state,
                picker: Arc::new(QueuingPicker {}),
            });
        }
    }
}

impl LbPolicy for DelegatingPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelController,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let _ = self
            .child_manager
            .resolver_update(update, config, channel_controller)?;
        self.update_picker(channel_controller);
        Ok(())
    }

    fn subchannel_update(
        &mut self,
        subchannel: &Subchannel,
        state: &SubchannelState,
        channel_controller: &mut dyn ChannelController,
    ) {
        self.child_manager
            .subchannel_update(subchannel, state, channel_controller);
        self.update_picker(channel_controller);
    }

    fn work(&mut self, channel_controller: &mut dyn ChannelController) {
        self.child_manager.work(channel_controller);
        self.update_picker(channel_controller);
    }
}

pub struct NopWorkScheduler {}
impl WorkScheduler for NopWorkScheduler {
    fn schedule_work(&self) { /* do nothing */
    }
}

struct RRPickerPicker {
    idx: AtomicUsize,
    children: Vec<Arc<dyn Picker>>,
}
impl Picker for RRPickerPicker {
    fn pick(&self, request: &grpc::service::Request) -> grpc::client::load_balancing::PickResult {
        let idx = self.idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.children[idx % self.children.len()].pick(request)
    }
}

impl RRPickerPicker {
    fn new(children: Vec<Arc<dyn Picker>>) -> Self {
        Self {
            idx: AtomicUsize::new(0),
            children,
        }
    }
}
