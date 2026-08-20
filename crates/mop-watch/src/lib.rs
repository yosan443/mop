pub mod composite;
pub mod docker;
pub mod fake;
pub mod ring_buffer;
pub mod systemd;
pub mod traits;

pub use composite::CompositeCollector;
pub use docker::DockerCollector;
pub use fake::FakeResourceCollector;
pub use ring_buffer::ResourceLogBuffer;
pub use systemd::SystemdCollector;
pub use traits::{LogLine, ResourceCollector, ResourceDetail, ResourceEvent};
