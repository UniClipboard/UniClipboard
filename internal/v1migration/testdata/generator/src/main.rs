//! 独立 Rust oracle；只生成固定测试数据，禁止读取个人目录或钥匙串。
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Serialize)]
struct Catalog {
    format_version: u16,
    revision: u64,
    groups: Vec<Group>,
}
#[derive(Serialize)]
struct Group {
    protection_group_id: String,
    space_id: String,
    entries: Vec<Entry>,
    catalog_digest: [u8; 32],
}
#[derive(Serialize)]
struct Entry {
    content_key_id: String,
    epoch: u64,
    key: Vec<u8>,
}
#[derive(Serialize)]
struct Manifest {
    format_version: u16,
    space_id: String,
    keyslot_generation: [u8; 16],
    profile_data_generation: [u8; 16],
    space_control_generation: [u8; 16],
    manifest_digest: [u8; 32],
}
#[derive(Serialize)]
struct Lifecycle {
    marker_format_version: u16,
    profile_generation: [u8; 16],
    factory_reset_phase: u32,
}
fn field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u64).to_be_bytes());
    out.extend(bytes);
}
fn raw(key: &[u8], nonce: &[u8], bytes: &[u8], aad: &[u8]) -> Vec<u8> {
    XChaCha20Poly1305::new_from_slice(key)
        .unwrap()
        .encrypt(XNonce::from_slice(nonce), Payload { msg: bytes, aad })
        .unwrap()
}
fn sealed(key: &[u8], bytes: &[u8], aad: &[u8], seed: u8) -> Value {
    let nonce = [seed; 24];
    json!({"version":"V1","aead":"XChaCha20Poly1305","nonce":nonce.to_vec(),
        "ciphertext":raw(key,&nonce,bytes,aad)})
}
fn protect(bytes: &[u8], business: &[u8], seed: u8) -> Vec<u8> {
    let mut info = Vec::new();
    field(&mut info, b"uniclipboard/content-protection/key/v1");
    field(&mut info, b"content");
    let mut key = [0; 32];
    Hkdf::<Sha256>::new(Some(b"group-fixture"), &[7; 32])
        .expand(&info, &mut key)
        .unwrap();
    let mut aad = Vec::new();
    for part in [
        b"uniclipboard/content-protection/aad/v1".as_slice(),
        &[0, 3],
        b"XChaCha20Poly1305",
        b"group-fixture",
        b"fixture-key",
        &7u64.to_be_bytes(),
        b"content",
        business,
    ] {
        field(&mut aad, part);
    }
    let nonce = [seed; 24];
    let mut out = b"UCP3".to_vec();
    out.extend(3u16.to_be_bytes());
    out.push(1);
    out.extend(11u16.to_be_bytes());
    out.extend(7u64.to_be_bytes());
    out.extend(nonce);
    out.extend(b"fixture-key");
    out.extend(raw(&key, &nonce, bytes, &aad));
    out
}
fn main() {
    let passphrase = b"fixture-passphrase";
    let salt = [1u8; 16];
    let mut kek = [0; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(8192, 1, 1, Some(32)).unwrap())
        .hash_password_into(passphrase, &salt, &mut kek)
        .unwrap();
    let master = [4u8; 32];
    let keyslot = json!({"version":"V1","scope":{"profile_id":"default"},
        "salt":salt.to_vec(),"kdf":{"alg":"Argon2id","params":{"mem_kib":8192,"iters":1,"parallelism":1}},
        "wrapped_master_key":sealed(&kek,&master,b"",1)});
    let lifecycle = postcard::to_stdvec(&Lifecycle {
        marker_format_version: 1, profile_generation: [0x11;16], factory_reset_phase: 0,
    }).unwrap();
    let mut secrets = BTreeMap::new();
    secrets.insert("profile_lifecycle_marker:v1", lifecycle);
    secrets.insert("profile_admission_master_key:v1", vec![5;32]);
    secrets.insert("profile_content_vault_key:v1", vec![6;32]);
    let payload = serde_json::to_vec(&json!({"format_version":1,"cleanup_authorized":false,"secrets":secrets})).unwrap();
    let secret_file = json!({"format_version":1,"wrapped_root":sealed(&kek,&[3;32],b"",2),
        "encrypted_payload":sealed(&[3;32],&payload,b"uniclipboard/profile-secret-payload/v1",3)});
    let mut manifest = Manifest {
        format_version:3,space_id:"space-fixture".into(),keyslot_generation:[0x22;16],
        profile_data_generation:[0x33;16],space_control_generation:[0x44;16],manifest_digest:[0;32],
    };
    let mut digest = b"uniclipboard/active-runtime-manifest/v3\0".to_vec();
    digest.extend(3u16.to_be_bytes());
    field(&mut digest,manifest.space_id.as_bytes());
    digest.extend(manifest.keyslot_generation);
    digest.extend(manifest.profile_data_generation);
    digest.extend(manifest.space_control_generation);
    manifest.manifest_digest=Sha256::digest(&digest).into();
    let mut aad=b"uniclipboard/admission-profile-payload/v1\0".to_vec();
    aad.extend([0x11;16]);field(&mut aad,b"active-space-manifest-v2");
    let manifest_file=sealed(&[5;32],&postcard::to_stdvec(&manifest).unwrap(),&aad,4);
    let mut group=Group {protection_group_id:"group-fixture".into(),space_id:"space-fixture".into(),
        entries:vec![Entry{content_key_id:"fixture-key".into(),epoch:7,key:vec![7;32]}],catalog_digest:[0;32]};
    let mut digest=b"uniclipboard/protected-group-catalog/v1\0".to_vec();
    field(&mut digest,group.protection_group_id.as_bytes());field(&mut digest,group.space_id.as_bytes());
    digest.extend(1u64.to_be_bytes());field(&mut digest,b"fixture-key");digest.extend(7u64.to_be_bytes());field(&mut digest,&[7;32]);
    group.catalog_digest=Sha256::digest(&digest).into();
    let catalog=postcard::to_stdvec(&Catalog{format_version:1,revision:1,groups:vec![group]}).unwrap();
    let mut aad=b"uniclipboard/profile-content-key-vault/v1\0".to_vec();aad.extend([0x11;16]);
    let catalog_file=sealed(&[6;32],&catalog,&aad,5);
    let mut generation=b"uniclipboard/profile-upgrade-generation-path/v1\0".to_vec();generation.extend([0x33;16]);
    let generation=blake3::hash(&generation).to_hex().to_string();
    let file=b"binary\0fixture";
    let compressed=zstd::bulk::compress(file,3).unwrap();
    let mut blob=b"UCBL\x03".to_vec();blob.extend(protect(&compressed,b"uc:blob:v3|blob-file",7));
    let pixel=include_bytes!("../../pixel.png");
    let mut image_blob=b"UCBL\x03".to_vec();
    image_blob.extend(protect(&zstd::bulk::compress(pixel,3).unwrap(),b"uc:blob:v3|blob-image",16));
    let output=json!({
        "generation":generation,"passphrase":"fixture-passphrase","kek":kek.to_vec(),
        "vault":{"keyslot.json":keyslot,"profile-secrets-v1":secret_file,
            ".active-space-manifest-v2":manifest_file,"profile-content-key-vault-v1.json":catalog_file},
        "text":protect(b"fixture text",b"uc:inline:v1|event-text|rep-text",8),
        "html":protect(b"<b>fixture text</b>",b"uc:inline:v1|event-text|rep-html",9),
        "repaired":protect(b"recovered",b"uc:inline:v1|event-bad|rep-bad",14),
        "image":protect(include_bytes!("../../pixel.png"),b"uc:inline:v1|event-image|rep-image",15),
        "image_blob":image_blob,"image_hash":format!("blake3v1:{}",blake3::hash(pixel).to_hex()),
        "file_blob":blob,"file_hash":format!("blake3v1:{}",blake3::hash(file).to_hex()),
        "file_original":protect(b"/fixture/original.bin",b"uc:file_set_line:v1|entry-file|0|original-text",10),
        "tag":protect(serde_json::to_vec(&json!({"name":"\u{6807}\u{7b7e}","created_at_ms":1000})).unwrap().as_slice(),b"uc:history_tag:v1|tag-one",11),
        "unused_tag":protect(serde_json::to_vec(&json!({"name":"unused","created_at_ms":1000})).unwrap().as_slice(),b"uc:history_tag:v1|tag-unused",12),
        "assignment":protect(br#"["tag-one"]"#,b"uc:history_tag_assignment:v1|entry-text",13)
    });
    println!("{}",serde_json::to_string_pretty(&output).unwrap());
}
