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
    sc: Arc<Mutex<Option<Arc<dyn Subchannel>>>>,
}

impl LbPolicy for Policy {
    fn update(
        &self,
        update: ResolverUpdate,
        config: Option<Box<dyn LbConfig>>,
    ) -> Result<(), Box<dyn Error>> {
        if let ResolverUpdate::Data(u) = update {
            if let Some(e) = u.endpoints.into_iter().next() {
                if let Some(a) = e.addresses.into_iter().next() {
                    let a = Arc::new(a);
                    let sc = self.ch.new_subchannel(a.clone());
                    let old_sc = mem::replace(&mut *self.sc.lock().unwrap(), Some(sc.clone()));
                    if let Some(o) = old_sc {
                        o.shutdown();
                    };
                    let slf = self.clone();
                    let sc2 = sc.clone();
                    sc.listen(Box::new(move |s| {
                        if s == ConnectivityState::Ready {
                            let sc = sc2.clone();
                            slf.ch.update(LbState {
                                connectivity_state: s,
                                picker: Box::new(OneSubchannelPicker { sc }),
                            });
                        }
                    }));
                    sc.connect();
                    // TODO: return a picker that queues RPCs.
                    self.ch.request_resolution();
                    return Ok(());
                }
                return Err("no addresses".into());
            }
            return Err("no endpoints".into());
        }
        Err("unhandled".into())
    }
}

struct OneSubchannelPicker {
    sc: Arc<dyn Subchannel>,
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
