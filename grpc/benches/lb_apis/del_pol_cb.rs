use std::{
    error::Error,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use grpc::{
    client::{
        load_balancing::{
            child_manager_cb::{ChildManagerCallbacks, ChildUpdate},
            ChannelControllerCallbacks, LbConfig, LbPolicyBuilderCallbacks, LbPolicyCallbacks,
            LbState, PickResult, Picker, QueuingPicker,
        },
        name_resolution::{ResolverData, ResolverUpdate},
    },
    service::Request,
};

use crate::*;

pub struct DelegatingPolicyCallbacks {
    child_manager: ChildManagerCallbacks<Endpoint>,
}

impl DelegatingPolicyCallbacks {
    pub fn new() -> Self {
        Self {
            child_manager: ChildManagerCallbacks::<Endpoint>::new(Box::new(|resolver_update| {
                let ResolverUpdate::Data(rd) = resolver_update else {
                    return Err("bad update".into());
                };
                let mut v = vec![];
                for endpoint in rd.endpoints {
                    let child_policy_builder: Box<dyn LbPolicyBuilderCallbacks> =
                        Box::new(ChildPolicyBuilderCallbacks {});
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

    fn update_picker(&mut self, channel_controller: &mut dyn ChannelControllerCallbacks) {
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

impl LbPolicyCallbacks for DelegatingPolicyCallbacks {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<&dyn LbConfig>,
        channel_controller: &mut dyn ChannelControllerCallbacks,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let _ = self
            .child_manager
            .resolver_update(update, config, channel_controller)?;
        self.update_picker(channel_controller);
        Ok(())
    }
}

// This is how the channel's controller would be wrapped by a middle LB policy
// that wants to intercept calls.  This benchmark does not have any specific
// behavior injected.
pub struct WrappedControllerCallbacks<'a> {
    channel_controller: &'a mut dyn ChannelControllerCallbacks,
    children: Arc<Mutex<Vec<(Box<dyn LbPolicyCallbacks>, LbState)>>>,
    idx: usize,
}

impl<'a> ChannelControllerCallbacks for WrappedControllerCallbacks<'a> {
    fn new_subchannel(
        &mut self,
        address: &Address,
        updates: Box<
            dyn Fn(Subchannel, SubchannelState, &mut dyn ChannelControllerCallbacks) + Send + Sync,
        >,
    ) -> Subchannel {
        let children = self.children.clone();
        let idx = self.idx;
        self.channel_controller.new_subchannel(
            address,
            Box::new(move |subchannel, subchannel_state, channel_controller| {
                let mut wc = WrappedControllerCallbacks {
                    channel_controller,
                    children: children.clone(),
                    idx,
                };
                updates(subchannel, subchannel_state, &mut wc);
            }),
        )
    }

    fn update_picker(&mut self, update: LbState) {
        let mut children = self.children.lock().unwrap();
        let child = &mut children[self.idx];
        child.1 = update;
        //update_picker(children, self.channel_controller)
    }

    fn request_resolution(&mut self) {
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
