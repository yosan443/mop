pub mod fake;
pub mod traits;

pub use fake::FakeResourceCollector;
pub use traits::{LogLine, ResourceCollector, ResourceDetail};
