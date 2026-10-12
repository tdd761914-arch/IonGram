//! Temporary-key identity and binding proof validation, following MTProto PFS
//! and gramsrv's handshake-authoritative expiry / immutable identity semantics.
use crate::{
    crypto,
    handlers::int,
    store::State,
    tl::{Schema, bytes},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub fn live(s: &State, key: i64) -> bool {
    s.keys.contains_key(&key)
        && s.temp_expiry.get(&key).is_none_or(|t| *t > crypto::now())
        && s.key_bindings
            .get(&key)
            .is_none_or(|p| s.keys.contains_key(p))
}
pub fn identity(s: &State, key: i64) -> i64 {
    s.key_bindings.get(&key).copied().unwrap_or(key)
}
pub fn bind(
    s: &mut State,
    key: i64,
    session: i64,
    mid: i64,
    v: &Value,
    schema: &Schema,
) -> Result<Value> {
    ensure!(int(v, "expires_at") > crypto::now(), "EXPIRES_AT_INVALID");
    ensure!(
        live(s, key) && s.temp_expiry.contains_key(&key),
        "TEMP_AUTH_KEY_EMPTY"
    );
    let perm = int(v, "perm_auth_key_id");
    ensure!(
        perm != key && !s.temp_expiry.contains_key(&perm),
        "ENCRYPTED_MESSAGE_INVALID"
    );
    let permanent = s.keys.get(&perm).context("ENCRYPTED_MESSAGE_INVALID")?;
    let inner = bytes(&v["encrypted_message"])
        .and_then(|packet| crypto::decrypt_binding(&permanent.data, &packet, mid, schema))
        .map_err(|_| anyhow::anyhow!("ENCRYPTED_MESSAGE_INVALID"))?;
    ensure!(
        int(&inner, "nonce") == int(v, "nonce")
            && int(&inner, "temp_auth_key_id") == key
            && int(&inner, "perm_auth_key_id") == perm
            && int(&inner, "temp_session_id") == session
            && int(&inner, "expires_at") == int(v, "expires_at"),
        "ENCRYPTED_MESSAGE_INVALID"
    );
    ensure!(
        s.key_bindings.get(&key).is_none_or(|old| *old == perm),
        "TEMP_AUTH_KEY_ALREADY_BOUND"
    );
    // Never extend/reclassify a key based on the client's expires_at assertion.
    ensure!(live(s, key), "TEMP_AUTH_KEY_EMPTY");
    s.key_bindings.insert(key, perm);
    Ok(json!({"_":"boolTrue"}))
}
pub fn prune(s: &mut State) {
    let expired: Vec<_> = s
        .temp_expiry
        .iter()
        .filter(|(_, expiry)| **expiry <= crypto::now())
        .map(|(key, _)| *key)
        .collect();
    for key in expired {
        s.keys.remove(&key);
        s.temp_expiry.remove(&key);
        s.key_bindings.remove(&key);
    }
}
