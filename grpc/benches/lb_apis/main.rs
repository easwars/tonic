use bencher::{benchmark_group, benchmark_main, Bencher};
use rand::prelude::*;
use std::sync::Arc;

use grpc::client::{
    load_balancing::{
        ChannelController, ChannelControllerCallbacks, LbPolicy, LbPolicyCallbacks, Subchannel,
        SubchannelState, SubchannelUpdate,
    },
    name_resolution::{Address, Endpoint, ResolverData, ResolverUpdate},
    ConnectivityState,
};

benchmark_group!(benches, broadcast, callbacks);
benchmark_main!(benches);

mod chi_pol_cb;
mod child_policy;
mod del_pol_cb;
mod delegating_policy;

pub(crate) use chi_pol_cb::*;
pub(crate) use child_policy::*;
pub(crate) use del_pol_cb::*;
pub(crate) use delegating_policy::*;

static NUM_ENDPOINTS: i32 = 1000;
static NUM_ADDRS_PER_ENDPOINT: i32 = 1;
static NUM_SUBCHANNELS_PER_UPDATE: i32 = 1;

fn broadcast(bench: &mut Bencher) {
    let mut lb = DelegatingPolicy::new();

    // Create the ResolverData containing many endpoints and addresses.
    let mut rd = ResolverData::default();
    for i in 0..NUM_ENDPOINTS {
        let mut endpoint = Endpoint::default();
        for j in 0..NUM_ADDRS_PER_ENDPOINT {
            let mut address = Address::default();
            address.address = format!("{i}:{j}");
            address.address_type = "foo".to_string();
            endpoint.addresses.push(address);
        }
        rd.endpoints.push(endpoint);
    }
    let mut channel_controller = StubChannelController::new();
    let _ = lb.resolver_update(ResolverUpdate::Data(rd), None, &mut channel_controller);
    let num_subchannels = channel_controller.subchannels.len();

    bench.iter(|| {
        //let mut update = SubchannelUpdate::new();
        // Update random subchannels to a random state.
        //for _ in 0..NUM_SUBCHANNELS_PER_UPDATE {
        let connectivity_state = thread_rng().gen_range(0..4);
        let connectivity_state = match connectivity_state {
            0 => ConnectivityState::Idle,
            1 => ConnectivityState::Connecting,
            2 => ConnectivityState::Ready,
            _ => ConnectivityState::TransientFailure,
        };
        let sc = channel_controller.subchannels[thread_rng().gen_range(0..num_subchannels)].clone();
        lb.subchannel_update(
            &sc,
            &SubchannelState {
                connectivity_state,
                last_connection_error: None,
            },
            &mut channel_controller,
        );
    });
}

pub struct StubChannelController {
    pub subchannels: Vec<Subchannel>,
}

impl StubChannelController {
    pub fn new() -> Self {
        Self {
            subchannels: vec![],
        }
    }
}

impl ChannelController for StubChannelController {
    fn new_subchannel(&mut self, _: &grpc::client::name_resolution::Address) -> Subchannel {
        // Just return a new, empty subchannel, ignoring the address and connect
        // notifications.
        let sc = Subchannel::new(Arc::default());
        self.subchannels.push(sc.clone());
        sc
    }

    fn update_picker(&mut self, _: grpc::client::load_balancing::LbState) {
        // Do nothing with the update.
    }

    fn request_resolution(&mut self) {
        // No resolver to notify.
    }
}

fn callbacks(bench: &mut Bencher) {
    let mut lb = DelegatingPolicyCallbacks::new();

    // Create the ResolverData containing many endpoints and addresses.
    let mut rd = ResolverData::default();
    for i in 0..NUM_ENDPOINTS {
        let mut endpoint = Endpoint::default();
        for j in 0..NUM_ADDRS_PER_ENDPOINT {
            let mut address = Address::default();
            address.address = format!("{i}:{j}");
            address.address_type = "foo".to_string();
            endpoint.addresses.push(address);
        }
        rd.endpoints.push(endpoint);
    }
    let mut channel_controller = StubChannelControllerCallbacks::new();
    let _ = lb.resolver_update(ResolverUpdate::Data(rd), None, &mut channel_controller);
    let num_subchannels = channel_controller.subchannels.len();

    bench.iter(|| {
        // Update random subchannels to a random state.
        for _ in 0..NUM_SUBCHANNELS_PER_UPDATE {
            let connectivity_state = thread_rng().gen_range(0..4);
            let connectivity_state = match connectivity_state {
                0 => ConnectivityState::Idle,
                1 => ConnectivityState::Connecting,
                2 => ConnectivityState::Ready,
                _ => ConnectivityState::TransientFailure,
            };
            channel_controller.send_update(
                thread_rng().gen_range(0..num_subchannels),
                connectivity_state,
            );
        }
    });
}

pub struct StubChannelControllerCallbacks {
    pub subchannels: Vec<
        Arc<(
            Subchannel,
            Box<
                dyn Fn(Subchannel, SubchannelState, &mut dyn ChannelControllerCallbacks)
                    + Send
                    + Sync,
            >,
        )>,
    >,
}

impl StubChannelControllerCallbacks {
    pub fn new() -> Self {
        Self {
            subchannels: vec![],
        }
    }
}

impl StubChannelControllerCallbacks {
    fn send_update(&mut self, n: usize, connectivity_state: ConnectivityState) {
        let x = self.subchannels[n].clone();
        x.1(
            x.0.clone(),
            SubchannelState {
                connectivity_state,
                last_connection_error: None,
            },
            self,
        );
    }
}

impl ChannelControllerCallbacks for StubChannelControllerCallbacks {
    fn new_subchannel(
        &mut self,
        _: &Address,
        updates: Box<
            dyn Fn(Subchannel, SubchannelState, &mut dyn ChannelControllerCallbacks) + Send + Sync,
        >,
    ) -> Subchannel {
        // Just return a new, empty subchannel, ignoring the address and connect
        // notifications.
        let sc = Subchannel::new(Arc::default());
        self.subchannels.push(Arc::new((sc.clone(), updates)));
        sc
    }

    fn update_picker(&mut self, _: grpc::client::load_balancing::LbState) {
        // Do nothing with the update.
    }

    fn request_resolution(&mut self) {
        // No resolver to notify.
    }
}
