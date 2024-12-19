use std::{
    error::Error,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use chi_pol_cb::ChildPolicyBuilder;
use grpc::{
    client::{
        load_balancing::{
            child_manager_cb::{ChildManager, ChildUpdate},
            ChannelControllerCallbacks as ChannelController, LbConfig,
            LbPolicyBuilderCallbacks as LbPolicyBuilder, LbPolicyCallbacks as LbPolicy, LbState,
            PickResult, Picker, QueuingPicker, SubchannelUpdateFn,
        },
        name_resolution::{ResolverData, ResolverUpdate},
    },
    service::Request,
};
use tonic::transport::channel;

use crate::*;

#[derive(Clone)]
pub struct DelegatingPolicy {
    child_manager: ChildManager<Endpoint>,
}

impl DelegatingPolicy {
    pub fn new(channel_controller: Arc<dyn ChannelController>) -> Self {
        Self {
            child_manager: ChildManager::<Endpoint>::new(
                Box::new(|resolver_update| {
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
                }),
                Box::new(|child_states, channel_controller| {
                    let child_states = child_states
                        .iter()
                        .map(|child| child.state.lock().unwrap())
                        .collect::<Vec<_>>();
                    let connectivity_states =
                        child_states.iter().map(|state| state.connectivity_state);
                    let connectivity_state = effective_state(connectivity_states.into_iter());
                    if connectivity_state == ConnectivityState::Ready
                        || connectivity_state == ConnectivityState::TransientFailure
                    {
                        let children = child_states
                            .into_iter()
                            .map(|lbstate| {
                                if lbstate.connectivity_state == connectivity_state {
                                    return Some(lbstate.picker.clone());
                                }
                                None
                            })
                            .flatten()
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
                }),
                channel_controller,
            ),
        }
    }
}

impl LbPolicy for DelegatingPolicy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<&dyn LbConfig>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.child_manager.resolver_update(update, config)
    }
}

// This is how the channel's controller would be wrapped by a middle LB policy
// that wants to intercept calls.  This benchmark does not have any specific
// behavior injected.
pub struct WrappedController {
    channel_controller: Arc<dyn ChannelController>,
    parent: ChildManager<Endpoint>,
}

impl ChannelController for WrappedController {
    fn new_subchannel(&self, address: &Address, updates: SubchannelUpdateFn) -> Subchannel {
        self.channel_controller.new_subchannel(address, updates)
    }

    fn update_picker(&self, _: LbState) {}

    fn request_resolution(&self) {
        self.channel_controller.request_resolution();
    }
}

struct RRPickerPicker {
    idx: AtomicUsize,
    children: Vec<Arc<dyn Picker>>,
}
impl Picker for RRPickerPicker {
    fn pick(&self, request: &Request) -> PickResult {
        let idx = self.idx.fetch_add(1, Ordering::Relaxed);
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
