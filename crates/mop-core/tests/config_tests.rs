use mop_core::config::{Config, RegistrationMode};

#[test]
fn test_default_config() {
    let cfg = Config::default();
    assert_eq!(cfg.server.bind, "127.0.0.1:8787");
    assert_eq!(cfg.auth.registration, RegistrationMode::FirstUser);
    assert_eq!(cfg.auth.min_password_len, 10);
    assert_eq!(cfg.auth.session_hours, 12);
}

#[test]
fn test_env_override() {
    std::env::set_var("MOP_SERVER_BIND", "0.0.0.0:9000");
    std::env::set_var("MOP_AUTH_REGISTRATION", "open");
    std::env::set_var("MOP_RESOURCES_FAKE", "true");

    let mut cfg = Config::default();
    cfg.apply_env_overrides();

    assert_eq!(cfg.server.bind, "0.0.0.0:9000");
    assert_eq!(cfg.auth.registration, RegistrationMode::Open);
    assert!(cfg.resources.fake);

    // Clean up env
    std::env::remove_var("MOP_SERVER_BIND");
    std::env::remove_var("MOP_AUTH_REGISTRATION");
    std::env::remove_var("MOP_RESOURCES_FAKE");
}
