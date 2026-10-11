use iongram::{
    handlers::{self, fragment},
    store::{File, Gift, State, Store},
    tl::Schema,
};
use serde_json::{Value, json};
fn fixture() -> (State, i64, i64, i64) {
    let mut s = State::default();
    let a = s.new_user("15550000001".into());
    let b = s.new_user("15550000002".into());
    let c = s.new_user("15550000003".into());
    (s, a, b, c)
}
fn rpc(s: &mut State, uid: i64, v: Value, layer: i64) -> anyhow::Result<Value> {
    let schema = Schema::for_layer(layer)?;
    let mut candidate = s.clone();
    let result = handlers::dispatch(
        &mut candidate,
        uid,
        0,
        &v,
        &schema,
        std::path::Path::new("unused"),
    )?;
    if result["_"] != "vector" {
        let bytes = schema.encode(&result)?;
        schema.decode(&bytes)?;
    }
    *s = candidate;
    Ok(result)
}
#[test]
fn both_layers_codec_and_user_scoped_updates() {
    for layer in [229, 231] {
        let (mut s, a, b, c) = fixture();
        for (to, n) in [(b, 1), (b, 2), (c, 3)] {
            rpc(&mut s,a,json!({"_":"messages.sendMessage","peer":{"_":"inputPeerUser","user_id":to,"access_hash":0},"message":"hello","random_id":n}),layer).unwrap();
        }
        assert_eq!(s.user_pts[&a], 3);
        assert_eq!(s.user_pts[&b], 2);
        assert_eq!(s.user_pts[&c], 1);
        let d = rpc(
            &mut s,
            c,
            json!({"_":"updates.getDifference","pts":0,"date":0,"qts":0}),
            layer,
        )
        .unwrap();
        assert_eq!(d["other_updates"].as_array().unwrap().len(), 1);
        assert_eq!(d["other_updates"][0]["pts"], 1);
        assert_eq!(d["other_updates"][0]["message"]["out"], false);
        assert_eq!(d["other_updates"][0]["message"]["peer_id"]["user_id"], a);
    }
}
#[test]
fn room_permissions_ban_and_invite_revocation() {
    let (mut s, a, b, _) = fixture();
    let ch = rpc(
        &mut s,
        a,
        json!({"_":"channels.createChannel","title":"private","about":"","broadcast":true}),
        231,
    )
    .unwrap()["chats"][0]["id"]
        .as_i64()
        .unwrap();
    let p = json!({"_":"inputPeerChannel","channel_id":ch,"access_hash":s.rooms[&ch].hash});
    assert!(
        rpc(
            &mut s,
            b,
            json!({"_":"messages.getHistory","peer":p,"limit":10}),
            231
        )
        .is_err()
    );
    let link = rpc(
        &mut s,
        a,
        json!({"_":"messages.exportChatInvite","peer":p}),
        231,
    )
    .unwrap()["link"]
        .as_str()
        .unwrap()
        .to_string();
    let h = link.split('+').next_back().unwrap();
    rpc(
        &mut s,
        b,
        json!({"_":"messages.importChatInvite","hash":h}),
        231,
    )
    .unwrap();
    assert!(
        rpc(
            &mut s,
            b,
            json!({"_":"messages.sendMessage","peer":p,"message":"not admin","random_id":4}),
            231
        )
        .is_err()
    );
    rpc(
        &mut s,
        a,
        json!({"_":"messages.editExportedChatInvite","peer":p,"link":link,"revoked":true}),
        231,
    )
    .unwrap();
    assert!(
        rpc(
            &mut s,
            b,
            json!({"_":"messages.checkChatInvite","hash":h}),
            231
        )
        .is_err()
    );
    rpc(&mut s,a,json!({"_":"channels.editBanned","channel":{"_":"inputChannel","channel_id":ch,"access_hash":0},"participant":{"_":"inputPeerUser","user_id":b,"access_hash":0},"banned_rights":{"_":"chatBannedRights","view_messages":true,"until_date":0}}),231).unwrap();
    assert!(s.rooms[&ch].bans.contains_key(&b));
}
#[test]
fn atomic_stars_idempotent_purchase_and_exact_gift_ownership() {
    for layer in [229, 231] {
        let (mut s, a, b, _) = fixture();
        s.credit(a, 200, "test grant").unwrap();
        s.files.insert(
            900,
            File {
                id: 900,
                hash: 1,
                reference: vec![1],
                size: 10,
                mime: "application/x-tgsticker".into(),
                attrs: vec![],
                owner: a,
                date: 1,
            },
        );
        s.gifts.insert(
            800,
            Gift {
                id: 800,
                title: "Test".into(),
                file: 900,
                price: 100,
                supply: 2,
                sold: 0,
                upgrade: 50,
            },
        );
        let invoice = json!({"_":"inputInvoiceStarGift","peer":{"_":"inputPeerUser","user_id":b,"access_hash":0},"gift_id":800});
        let form = rpc(
            &mut s,
            a,
            json!({"_":"payments.getPaymentForm","invoice":invoice}),
            layer,
        )
        .unwrap();
        let pay = json!({"_":"payments.sendStarsForm","invoice":invoice,"form_id":form["form_id"]});
        rpc(&mut s, a, pay.clone(), layer).unwrap();
        rpc(&mut s, a, pay, layer).unwrap();
        assert_eq!(s.users[&a].stars, 100);
        assert_eq!(s.owned.len(), 1);
        assert_eq!(s.gifts[&800].sold, 1);
        let id = *s.owned.keys().next().unwrap();
        assert!(rpc(&mut s,a,json!({"_":"payments.convertStarGift","stargift":{"_":"inputSavedStarGiftUser","msg_id":id}}),layer).is_err());
        rpc(&mut s,b,json!({"_":"payments.convertStarGift","stargift":{"_":"inputSavedStarGiftUser","msg_id":id}}),layer).unwrap();
        assert_eq!(s.users[&b].stars, 100);
        assert!(rpc(&mut s,b,json!({"_":"payments.convertStarGift","stargift":{"_":"inputSavedStarGiftUser","msg_id":id}}),layer).is_err());
        assert_eq!(s.users[&b].stars, 100);
    }
}
#[test]
fn collectible_visibility_order_and_global_collision() {
    let (mut s, a, b, _) = fixture();
    fragment::mint(&mut s, "user", a, "RareName").unwrap();
    fragment::mint(&mut s, "user", a, "AnotherName").unwrap();
    assert!(fragment::mint(&mut s, "user", b, "rarename").is_err());
    let user = s.user(a, b);
    assert!(user.get("username").is_none());
    assert_eq!(user["usernames"].as_array().unwrap().len(), 2);
    rpc(
        &mut s,
        a,
        json!({"_":"account.toggleUsername","username":"RareName","active":false}),
        229,
    )
    .unwrap();
    let query = json!({"_":"fragment.getCollectibleInfo","collectible":{"_":"inputCollectibleUsername","username":"rarename"}});
    assert!(rpc(&mut s, b, query.clone(), 229).is_err());
    rpc(&mut s, a, query, 229).unwrap();
    assert!(
        rpc(
            &mut s,
            a,
            json!({"_":"account.reorderUsernames","order":["RareName","RareName"]}),
            231
        )
        .is_err()
    );
    rpc(
        &mut s,
        a,
        json!({"_":"account.reorderUsernames","order":["AnotherName","RareName"]}),
        231,
    )
    .unwrap();
    assert_eq!(s.user(a, a)["usernames"][0]["username"], "AnotherName");
}
#[test]
fn sqlite_atomic_restart_and_exclusive_writer() {
    let dir = std::env::temp_dir().join(format!("iongram-test-{}", iongram::crypto::random_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.sqlite");
    let (mut s, a, _, _) = fixture();
    s.credit(a, 100, "grant").unwrap();
    fragment::mint(&mut s, "user", a, "SampleName").unwrap();
    {
        let mut store = Store::open(&path).unwrap();
        assert!(Store::open(&path).is_err());
        store.save(s.clone()).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.state.users[&a].stars, 100);
    assert_eq!(store.state.ledger.len(), 1);
    assert!(store.state.aliases.contains_key("samplename"));
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn malformed_tl_and_tgs_are_rejected() {
    for layer in [229, 231] {
        let s = Schema::for_layer(layer).unwrap();
        assert!(s.decode(&[0; 3]).is_err());
        assert!(s.decode(&[0xff; 64]).is_err());
    }
    assert!(handlers::files::validate_tgs(b"not gzip").is_err());
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
    gz.write_all(br#"{"w":512,"h":512,"fr":60,"ip":0,"op":120,"layers":[]}"#)
        .unwrap();
    let tgs = gz.finish().unwrap();
    assert!(handlers::files::validate_tgs(&tgs).is_ok());
}

#[test]
fn premium_purchase_and_collectible_editable_order() {
    let (mut s, a, b, _) = fixture();
    s.credit(a, 5000, "grant").unwrap();
    for layer in [229, 231] {
        let invoice = json!({"_":"inputInvoicePremiumGiftStars","user_id":{"_":"inputUser","user_id":b,"access_hash":0},"months":3});
        let form = rpc(
            &mut s,
            a,
            json!({"_":"payments.getPaymentForm","invoice":invoice}),
            layer,
        )
        .unwrap();
        rpc(
            &mut s,
            a,
            json!({"_":"payments.sendStarsForm","form_id":form["form_id"],"invoice":invoice}),
            layer,
        )
        .unwrap();
        assert!(s.user(b, b)["premium"] == true);
    }
    assert_eq!(s.users[&a].stars, 3000);
    rpc(
        &mut s,
        a,
        json!({"_":"account.updateUsername","username":"EditableName"}),
        231,
    )
    .unwrap();
    fragment::mint(&mut s, "user", a, "CollectibleName").unwrap();
    rpc(
        &mut s,
        a,
        json!({"_":"account.reorderUsernames","order":["CollectibleName","EditableName"]}),
        231,
    )
    .unwrap();
    assert_eq!(s.user(a, a)["usernames"][0]["username"], "CollectibleName");
}
