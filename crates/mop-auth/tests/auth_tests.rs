use mop_auth::{hash_password, verify_password};

#[test]
fn test_password_hash_and_verify() {
    let password = "SuperSecretPassword123!";
    let hash = hash_password(password).expect("Hashing should succeed");

    assert!(verify_password(password, &hash).expect("Verification should succeed"));
    assert!(
        !verify_password("WrongPassword", &hash).expect("Verification should succeed with false")
    );
}
