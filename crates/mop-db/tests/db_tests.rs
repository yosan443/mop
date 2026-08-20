use chrono::Utc;
use mop_core::models::{Role, User};
use mop_db::{create_sqlite_pool, repos::UserRepo, run_migrations};
use tempfile::NamedTempFile;
use ulid::Ulid;

#[tokio::test]
async fn test_db_init_and_user_repo() {
    let tmp = NamedTempFile::new().unwrap();
    let db_path = tmp.path();

    let pool = create_sqlite_pool(db_path)
        .await
        .expect("Pool creation failed");
    run_migrations(&pool).await.expect("Migration failed");

    let count = UserRepo::count(&pool).await.unwrap();
    assert_eq!(count, 0);

    let user = User {
        id: Ulid::new().to_string(),
        username: "admin".to_string(),
        password_hash: "dummyhash".to_string(),
        role: Role::Admin,
        disabled: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    UserRepo::create(&pool, &user)
        .await
        .expect("User creation should succeed");

    let count_after = UserRepo::count(&pool).await.unwrap();
    assert_eq!(count_after, 1);

    let fetched = UserRepo::find_by_username(&pool, "admin").await.unwrap();
    assert!(fetched.is_some());
    let fetched_user = fetched.unwrap();
    assert_eq!(fetched_user.username, "admin");
    assert_eq!(fetched_user.role, Role::Admin);
}
