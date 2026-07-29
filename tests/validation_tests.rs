use gugle_rag::auth::{validate_password, validate_username};

#[test]
fn username_accepts_documented_characters() {
    assert!(validate_username("team_admin-1").is_ok());
}

#[test]
fn username_rejects_invalid_length_and_characters() {
    assert!(validate_username("ab").is_err());
    assert!(validate_username("team admin").is_err());
    assert!(validate_username("用户名").is_err());
}

#[test]
fn password_requires_eight_characters() {
    assert!(validate_password("1234567").is_err());
    assert!(validate_password("12345678").is_ok());
}
