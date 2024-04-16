pub struct TODO;

pub trait Runtime {
    fn todo(&self) -> TODO;
}
