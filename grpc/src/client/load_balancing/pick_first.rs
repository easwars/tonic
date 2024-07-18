use std::{
    error::Error,
    mem,
    sync::{Arc, Mutex},
};

use crate::{
    client::{load_balancing::LbState, name_resolution::ResolverUpdate, ConnectivityState},
    service::Request,
};

use super::{
    LbConfig, LbPolicy, LbPolicyBuilder, LbPolicyOptions, Pick, Picker, Subchannel, SubchannelPool,
};

pub static POLICY_NAME: &str = "pick_first";

struct Builder {}

impl LbPolicyBuilder for Builder {
    fn build(
        &self,
        channel: Arc<dyn SubchannelPool>,
        options: LbPolicyOptions,
    ) -> Box<dyn LbPolicy> {
        Box::new(Policy {
            ch: channel,
            sc: Arc::default(),
        })
    }

    fn name(&self) -> &'static str {
        POLICY_NAME
    }
}

pub fn reg() {
    super::GLOBAL_LB_REGISTRY.add_builder(Builder {})
}

#[derive(Clone)]
struct Policy {
    ch: Arc<dyn SubchannelPool>,
    sc: Arc<Mutex<Option<Subchannel>>>,
}

impl LbPolicy for Policy {
    fn resolver_update(
        &mut self,
        update: ResolverUpdate,
        config: Option<Box<dyn LbConfig>>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let ResolverUpdate::Data(u) = update {
            if let Some(e) = u.endpoints.into_iter().next() {
                if let Some(a) = e.addresses.into_iter().next() {
                    let a = Arc::new(a);
                    let sc = self.ch.new_subchannel(a.clone());
                    let _ = mem::replace(&mut *self.sc.lock().unwrap(), Some(sc.clone()));
                    sc.connect();
                    // TODO: return a picker that queues RPCs.
                    return Ok(());
                }
                return Err("no addresses".into());
            }
            return Err("no endpoints".into());
        }
        Err("unhandled".into())
    }

    fn subchannel_update(&mut self, update: &super::SubchannelUpdate) {
        if let Some(sc) = self.sc.lock().unwrap().clone() {
            if update
                .get(&sc)
                .is_some_and(|ss| ss.connectivity_state == ConnectivityState::Ready)
            {
                self.ch.update_picker(LbState {
                    connectivity_state: ConnectivityState::Ready,
                    picker: Box::new(OneSubchannelPicker { sc }),
                });
            }
        }
    }
}

struct OneSubchannelPicker {
    sc: Subchannel,
}

impl Picker for OneSubchannelPicker {
    fn pick(&self, request: &Request) -> Result<Pick, Box<dyn Error>> {
        Ok(Pick {
            subchannel: self.sc.clone(),
            on_complete: None,
            metadata: None,
        })
    }
}
