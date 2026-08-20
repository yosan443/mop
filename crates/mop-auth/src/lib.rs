pub mod backend;
pub mod middleware;
pub mod password;
pub mod rate_limit;
pub mod rbac;
pub mod service;

pub use backend::{AuthUserRecord, Credentials, MopAuthBackend};
pub use middleware::csrf_protection_middleware;
pub use password::{hash_password, verify_password};
pub use rate_limit::{IpRateLimiter, KeyRateLimiter};
pub use rbac::{RequireAdmin, RequireAuth, RequireOperator};
pub use service::AuthService;

pub type AuthSession = axum_login::AuthSession<MopAuthBackend>;
