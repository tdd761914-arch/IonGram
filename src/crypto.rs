//! Server-side MTProto 2.0 directionality, using grammers-crypto AES and hashes.
use crate::tl::{Reader, Schema, bytes, put_bytes};
use anyhow::{Result, ensure};
use grammers_crypto::{
    aes::{ige_decrypt, ige_encrypt},
    sha1, sha256,
};
use rand::{RngCore, rngs::OsRng};
use rsa::{
    BigUint, RsaPrivateKey,
    pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey, EncodeRsaPublicKey, LineEnding},
    traits::PublicKeyParts,
};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
pub fn random(n: usize) -> Vec<u8> {
    let mut v = vec![0; n];
    OsRng.fill_bytes(&mut v);
    v
}
pub fn random_id() -> i64 {
    i64::from_le_bytes(random(8).try_into().unwrap()) & i64::MAX
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn padded(n: &BigUint) -> Vec<u8> {
    let b = n.to_bytes_be();
    let mut out = vec![0; 256 - b.len()];
    out.extend(b);
    out
}
pub struct ServerKey {
    key: RsaPrivateKey,
    pub fingerprint: i64,
}
impl ServerKey {
    pub fn load(dir: &std::path::Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("private.pem");
        let key = if path.exists() {
            RsaPrivateKey::from_pkcs1_pem(&std::fs::read_to_string(&path)?)?
        } else {
            let key = RsaPrivateKey::new(&mut OsRng, 2048)?;
            std::fs::write(&path, key.to_pkcs1_pem(LineEnding::LF)?.as_bytes())?;
            key
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::write(
            dir.join("public.pem"),
            key.to_public_key().to_pkcs1_pem(LineEnding::LF)?,
        )?;
        let mut b = vec![];
        put_bytes(&mut b, &key.n().to_bytes_be())?;
        put_bytes(&mut b, &key.e().to_bytes_be())?;
        let h = sha1!(&b);
        Ok(Self {
            key,
            fingerprint: i64::from_le_bytes(h[12..].try_into()?),
        })
    }
    fn decrypt(&self, data: &[u8], schema: &Schema) -> Result<Value> {
        ensure!(data.len() == 256, "RSA length");
        let n = BigUint::from_bytes_be(data);
        ensure!(&n < self.key.n(), "RSA modulus");
        let raw = padded(&rsa::hazmat::rsa_decrypt_and_check(
            &self.key,
            Some(&mut OsRng),
            &n,
        )?);
        let h = sha256!(&raw[32..]);
        let mut key = [0; 32];
        for i in 0..32 {
            key[i] = raw[i] ^ h[i]
        }
        let mut dec = raw[32..].to_vec();
        ige_decrypt(&mut dec, &key, &[0; 32]);
        let rev: Vec<u8> = dec[..192].iter().rev().copied().collect();
        if bool::from(sha256!(&key, &rev).ct_eq(&dec[192..])) {
            return schema.read_object(&mut Reader::new(&rev));
        }
        ensure!(raw[0] == 0, "RSA_PAD hash");
        let mut r = Reader::new(&raw[21..]);
        let v = schema.read_object(&mut r)?;
        ensure!(
            bool::from(sha1!(&raw[21..21 + r.pos]).ct_eq(&raw[1..21])),
            "RSA legacy hash"
        );
        Ok(v)
    }
}
// Telegram's published 2048-bit safe prime, generator 3.
const PRIME: &str = "C71CAEB9C6B1C9048E6C522F70F13F73980D40238E3E21C14934D037563D930F48198A0AA7C14058229493D22530F4DBFA336F6E0AC925139543AED44CCE7C3720FD51F69458705AC68CD4FE6B6B13ABDC9746512969328454F18FAF8C595F642477FE96BB2A941D5BCD1D4AC8CC49880708FA9B378E3C4F3A9060BEE67CF9A4A4A695811051907E162753B56B0F6B410DBA74D8A84B2A14B3144E0EF1284754FD17ED950D5965B4B9DD46582DB1178D169C6BC465B0D6FF9CA3928FEF5B9AE4E418FC15E83EBEA0F87FA9FF5EED70050DED2849F47BF959D956850CE929851F0D8115F635B105EE2E4E15D04B2454BF6F4FADF034B10403119CD8E3B92FCC5B";
pub struct Handshake {
    nonce: Value,
    server: Vec<u8>,
    new: Vec<u8>,
    secret: BigUint,
    key: [u8; 32],
    iv: [u8; 32],
    stage: u8,
    created: i64,
}
impl Handshake {
    pub fn new(nonce: Value) -> Self {
        Self {
            nonce,
            server: random(16),
            new: vec![],
            secret: BigUint::from(0u32),
            key: [0; 32],
            iv: [0; 32],
            stage: 0,
            created: now(),
        }
    }
    pub fn pq(&self, key: &ServerKey) -> Value {
        json!({"_":"resPQ","nonce":self.nonce,"server_nonce":self.server,"pq":((1000000007u64)*(1000000009u64)).to_be_bytes(),"server_public_key_fingerprints":[key.fingerprint]})
    }
    pub fn dh(&mut self, v: &Value, key: &ServerKey, s: &Schema) -> Result<Value> {
        ensure!(self.stage == 0 && now() - self.created < 60, "DH state");
        ensure!(
            v["nonce"] == self.nonce && bytes(&v["server_nonce"])? == self.server,
            "DH nonce"
        );
        ensure!(
            v["public_key_fingerprint"].as_i64() == Some(key.fingerprint),
            "RSA fingerprint"
        );
        ensure!(
            bytes(&v["p"])? == 1000000007u32.to_be_bytes()
                && bytes(&v["q"])? == 1000000009u32.to_be_bytes(),
            "PQ factors"
        );
        let inner = key.decrypt(&bytes(&v["encrypted_data"])?, s)?;
        ensure!(
            inner["_"]
                .as_str()
                .unwrap_or("")
                .starts_with("p_q_inner_data"),
            "DH inner type"
        );
        ensure!(
            inner["nonce"] == self.nonce && bytes(&inner["server_nonce"])? == self.server,
            "inner nonce"
        );
        ensure!(
            inner["p"] == v["p"]
                && inner["q"] == v["q"]
                && bytes(&inner["pq"])? == ((1000000007u64) * (1000000009u64)).to_be_bytes(),
            "inner factors"
        );
        self.new = bytes(&inner["new_nonce"])?;
        ensure!(self.new.len() == 32, "new nonce");
        self.secret = BigUint::from_bytes_be(&random(256));
        let prime = BigUint::parse_bytes(PRIME.as_bytes(), 16).unwrap();
        let ga = BigUint::from(3u32).modpow(&self.secret, &prime);
        let body=s.encode(&json!({"_":"server_DH_inner_data","nonce":self.nonce,"server_nonce":self.server,"g":3,"dh_prime":padded(&prime),"g_a":padded(&ga),"server_time":now()}))?;
        let mut answer = sha1!(&body).to_vec();
        answer.extend(body);
        answer.extend(random((16 - answer.len() % 16) % 16));
        (self.key, self.iv) = grammers_crypto::generate_key_data_from_nonce(
            self.server.as_slice().try_into()?,
            self.new.as_slice().try_into()?,
        );
        ige_encrypt(&mut answer, &self.key, &self.iv);
        self.stage = 1;
        Ok(
            json!({"_":"server_DH_params_ok","nonce":self.nonce,"server_nonce":self.server,"encrypted_answer":answer}),
        )
    }
    pub fn finish(&mut self, v: &Value, s: &Schema) -> Result<(Value, Vec<u8>, i64)> {
        ensure!(self.stage == 1 && now() - self.created < 60, "DH state");
        ensure!(
            v["nonce"] == self.nonce && bytes(&v["server_nonce"])? == self.server,
            "DH nonce"
        );
        let mut b = bytes(&v["encrypted_data"])?;
        ensure!(b.len() >= 32 && b.len() % 16 == 0, "DH ciphertext");
        ige_decrypt(&mut b, &self.key, &self.iv);
        let mut r = Reader::new(&b[20..]);
        let inner = s.read_object(&mut r)?;
        ensure!(
            inner["_"] == "client_DH_inner_data"
                && inner["nonce"] == self.nonce
                && bytes(&inner["server_nonce"])? == self.server,
            "DH inner"
        );
        ensure!(
            bool::from(sha1!(&b[20..20 + r.pos]).ct_eq(&b[..20])),
            "DH hash"
        );
        let gb = BigUint::from_bytes_be(&bytes(&inner["g_b"])?);
        let p = BigUint::parse_bytes(PRIME.as_bytes(), 16).unwrap();
        let bound = BigUint::from(1u32) << 1984;
        ensure!(
            gb > bound && gb < (&p - &bound),
            "DH public value out of range"
        );
        let auth = padded(&gb.modpow(&self.secret, &p));
        let h = sha1!(&auth);
        let nonce_hash = sha1!(&self.new, &[1u8], &h[..8]);
        let salt = i64::from_le_bytes(self.new[..8].try_into()?)
            ^ i64::from_le_bytes(self.server[..8].try_into()?);
        self.stage = 2;
        Ok((
            json!({"_":"dh_gen_ok","nonce":self.nonce,"server_nonce":self.server,"new_nonce_hash1":&nonce_hash[4..]}),
            auth,
            salt,
        ))
    }
}
fn kdf(auth: &[u8], msg: &[u8], x: usize) -> ([u8; 32], [u8; 32]) {
    let a = sha256!(msg, &auth[x..x + 36]);
    let b = sha256!(&auth[40 + x..76 + x], msg);
    let mut key = [0; 32];
    key[..8].copy_from_slice(&a[..8]);
    key[8..24].copy_from_slice(&b[8..24]);
    key[24..].copy_from_slice(&a[24..]);
    let mut iv = [0; 32];
    iv[..8].copy_from_slice(&b[..8]);
    iv[8..24].copy_from_slice(&a[8..24]);
    iv[24..].copy_from_slice(&b[24..]);
    (key, iv)
}
pub fn auth_id(auth: &[u8]) -> i64 {
    i64::from_le_bytes(sha1!(auth)[12..].try_into().unwrap())
}
pub fn encrypt(auth: &[u8], mut plain: Vec<u8>) -> Vec<u8> {
    plain.extend(random(12 + (16 - (plain.len() + 12) % 16) % 16));
    let h = sha256!(&auth[96..128], &plain);
    let msg = &h[8..24];
    let (key, iv) = kdf(auth, msg, 8);
    ige_encrypt(&mut plain, &key, &iv);
    let mut out = auth_id(auth).to_le_bytes().to_vec();
    out.extend(msg);
    out.extend(plain);
    out
}
pub fn decrypt(auth: &[u8], packet: &[u8]) -> Result<(Vec<u8>, u32)> {
    ensure!(auth.len() == 256 && packet.len() >= 72, "encrypted length");
    let end = 24 + (packet.len() - 24) / 16 * 16;
    let msg = &packet[8..24];
    let (key, iv) = kdf(auth, msg, 0);
    let mut plain = packet[24..end].to_vec();
    ige_decrypt(&mut plain, &key, &iv);
    let h = sha256!(&auth[88..120], &plain);
    ensure!(bool::from(h[8..24].ct_eq(msg)), "message key mismatch");
    let n = i32::from_le_bytes(plain[28..32].try_into()?) as usize;
    ensure!(
        n.is_multiple_of(4) && n <= plain.len().saturating_sub(44),
        "message length/padding"
    );
    ensure!(plain.len() - 32 - n <= 1024, "excess padding");
    let ack = u32::from_le_bytes(h[..4].try_into()?);
    plain.truncate(32 + n);
    Ok((plain, ack))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rsa_pad_interoperates_with_grammers_and_rejects_tampering() {
        let key = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
        let client =
            grammers_crypto::rsa::Key::new(&key.n().to_str_radix(10), &key.e().to_str_radix(10))
                .unwrap();
        let server = ServerKey {
            key,
            fingerprint: 0,
        };
        let schema = Schema::load().unwrap();
        let value = json!({"_":"p_q_inner_data","pq":[1,2],"p":[1],"q":[2],"nonce":vec![3;16],"server_nonce":vec![4;16],"new_nonce":vec![5;32]});
        let body = schema.encode(&value).unwrap();
        let mut encrypted =
            grammers_crypto::rsa::encrypt_hashed(&body, &client, &random(224).try_into().unwrap());
        assert_eq!(server.decrypt(&encrypted, &schema).unwrap(), value);
        encrypted[25] ^= 1;
        assert!(server.decrypt(&encrypted, &schema).is_err());
    }
}
