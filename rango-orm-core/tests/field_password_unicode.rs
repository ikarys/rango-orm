//! C3: argon2 0.6 changed the hash for non-ASCII passwords (hashes the whole
//! UTF-8 string instead of a byte prefix). These tests verify the round-trip
//! (new → verify) still works for accented and unicode inputs after the bump.

use rango_orm_core::FieldPassword;

#[test]
fn field_password_verify_non_ascii_accents() {
    let password = FieldPassword::<8, 128>::new("passe àccéntué").unwrap();
    assert!(password.verify("passe àccéntué"));
    assert!(!password.verify("passe accentue"));
    assert!(password.hash().starts_with("$argon2id$"));
}

#[test]
fn field_password_verify_emoji() {
    let password = FieldPassword::<8, 128>::new("s3cr3t 🔐🔑").unwrap();
    assert!(password.verify("s3cr3t 🔐🔑"));
    assert!(!password.verify("s3cr3t no emoji"));
}

#[test]
fn field_password_verify_cjk() {
    let password = FieldPassword::<8, 128>::new("密码很长的密码啊").unwrap();
    assert!(password.verify("密码很长的密码啊"));
    assert!(!password.verify("password long"));
}
