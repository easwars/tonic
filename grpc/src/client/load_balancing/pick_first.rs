use std::{
    error::Error,
    mem,
    sync::{Arc, Mutex},
};

use tonic::async_trait;

use crate::client::{
    load_balancing::{self as lb, State},
    name_resolution, ConnectivityState,
};

use super::{Pick, Subchannel};

pub static POLICY_NAME: &str = "pick_first";

struct Builder {}

impl lb::Builder for Builder {
    fn build(
        &self,
        channel: Arc<dyn lb::SubchannelPool>,
        options: lb::TODO,
    ) -> Box<dyn lb::Policy> {
        Box::new(Policy {
            ch: channel,
            sc: Arc::new(Mutex::new(None)),
        })
    }

    fn name(&self) -> &'static str {
        POLICY_NAME
    }
}

pub fn reg() {
    super::GLOBAL_REGISTRY.add_builder(Builder {})
}

#[derive(Clone)]
struct Policy {
    ch: Arc<dyn lb::SubchannelPool>,
    sc: Arc<Mutex<Option<Arc<dyn Subchannel>>>>,
}

#[async_trait]
impl lb::Policy for Policy {
    async fn update(&self, update: lb::PolicyUpdate) -> Result<(), Box<dyn Error>> {
        if let name_resolution::ResolverUpdate::Data(u) = update.update {
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
                            slf.ch.update_state(Ok(Box::new(State {
                                connectivity_state: s,
                                picker: Box::new(move |_v| {
                                    Ok(Pick {
                                        subchannel: sc.clone(),
                                        on_complete: None,
                                        metadata: None,
                                    })
                                }),
                            })));
                        }
                    }));
                    sc.connect();
                    return Ok(());
                }
                return Err("no addresses".into());
            }
            return Err("no endpoints".into());
        }
        Err("unhandled".into())
    }
}
