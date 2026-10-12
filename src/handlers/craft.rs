//! Craft two to four upgraded gifts, with a fixed 999/1000 success probability.
use super::*;
use crate::{
    crypto::random_id,
    store::{Message, OwnedGift},
};

pub fn eligible(s: &State, o: &OwnedGift) -> bool {
    o.upgraded
        && !o.converted
        && o.resale == 0
        && !s.crafted_gifts.contains_key(&o.id)
        && s.gift_styles.get(&o.gift).is_some_and(|a| {
            a.iter()
                .any(|v| v["_"] == "starGiftAttributeModel" && v["crafted"] == true)
        })
}
fn result(s: &State, uid: i64, id: i64) -> Result<Value> {
    if id == 0 {
        return Ok(updates(
            vec![],
            vec![],
            vec![json!({"_":"updateStarGiftCraftFail"})],
        ));
    }
    let message = s
        .messages
        .iter()
        .find(|m| m.kind == "user" && m.peer == uid && m.id == id)
        .context("CRAFT_RESULT_MISSING")?;
    let pts = s.user_pts.get(&uid).copied().unwrap_or(0);
    Ok(updates(
        vec![],
        vec![s.user(uid, uid)],
        vec![
            json!({"_":"updateNewMessage","message":s.message(message,uid),"pts":pts,"pts_count":0}),
        ],
    ))
}
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    if text(v, "_") == "payments.getCraftStarGifts" {
        let gift = int(v, "gift_id");
        ensure!(s.gifts.contains_key(&gift), "GIFT_NOT_FOUND");
        let offset = text(v, "offset").parse::<usize>().unwrap_or(0);
        let limit = int(v, "limit").clamp(1, 100) as usize;
        let all: Vec<_> = s
            .owned
            .values()
            .filter(|o| o.owner == uid && o.gift == gift && eligible(s, o))
            .collect();
        let mut out = json!({"_":"payments.savedStarGifts","count":all.len(),"gifts":all.iter().skip(offset).take(limit).map(|o|super::gifts::saved(s,o)).collect::<Result<Vec<_>>>()?,"chats":[],"users":[s.user(uid,uid)]});
        if offset.saturating_add(limit) < all.len() {
            out["next_offset"] = json!((offset + limit).to_string());
        }
        return Ok(out);
    }
    let inputs = list(v, "stargift");
    ensure!(
        (2..=4).contains(&inputs.len()),
        "CRAFT_REQUIRES_TWO_TO_FOUR_GIFTS"
    );
    let mut ids = vec![];
    for input in inputs {
        let id = match text(&input, "_") {
            "inputSavedStarGiftUser" => int(&input, "msg_id"),
            "inputSavedStarGiftSlug" => text(&input, "slug")
                .strip_prefix("IonGift-")
                .and_then(|s| s.parse::<i64>().ok())
                .context("STARGIFT_INVALID")?,
            "inputSavedStarGiftChat" => {
                let (kind, owner) = peer(&input["peer"], uid)?;
                ensure!(kind == "user" && owner == uid, "STARGIFT_OWNER_INVALID");
                int(&input, "saved_id")
            }
            _ => bail!("STARGIFT_INVALID"),
        };
        ensure!(id > 0 && !ids.contains(&id), "STARGIFT_INVALID");
        ids.push(id);
    }
    ids.sort_unstable();
    let token = format!(
        "{uid}:{}",
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(":")
    );
    if let Some(id) = s.craft_receipts.get(&token) {
        return result(s, uid, *id);
    }
    let mut gift = 0;
    for id in &ids {
        let o = s.owned.get(id).context("GIFT_NOT_FOUND")?;
        ensure!(
            o.owner == uid && eligible(s, o),
            "STARGIFT_CRAFT_UNAVAILABLE"
        );
        if gift == 0 {
            gift = o.gift;
        }
        ensure!(gift == o.gift, "STARGIFT_COLLECTION_MISMATCH");
    }
    let models: Vec<_> = s.gift_styles[&gift]
        .iter()
        .filter(|a| a["_"] == "starGiftAttributeModel" && a["crafted"] == true)
        .cloned()
        .collect();
    ensure!(!models.is_empty(), "STARGIFT_CRAFT_UNAVAILABLE");
    if random_id() % 1000 == 999 {
        for id in &ids {
            let o = s.owned.get_mut(id).unwrap();
            o.converted = true;
            o.hidden = true;
            o.pinned = false;
        }
        s.craft_receipts.insert(token, 0);
        s.record(vec![uid], json!({"_":"updateStarGiftCraftFail"}));
        return result(s, uid, 0);
    }
    let model = models[random_id() as usize % models.len()].clone();
    let id = s.id();
    s.owned.insert(
        id,
        OwnedGift {
            id,
            gift,
            owner: uid,
            sender: uid,
            date: now(),
            upgraded: true,
            hidden: false,
            price: 0,
            converted: false,
            resale: 0,
            pinned: false,
        },
    );
    super::gifts::assign_style(s, id)?;
    let attrs = s
        .owned_styles
        .get_mut(&id)
        .context("GIFT_ATTRIBUTES_INVALID")?;
    attrs.retain(|a| a["_"] != "starGiftAttributeModel");
    attrs.insert(0, model);
    for old in &ids {
        let o = s.owned.get_mut(old).unwrap();
        o.converted = true;
        o.hidden = true;
        o.pinned = false;
    }
    // Inputs are consumed, not refunded; the collection's mint supply is not replenished.
    s.crafted_gifts.insert(id, ids);
    s.craft_receipts.insert(token, id);
    let action = json!({"_":"messageActionStarGiftUnique","craft":true,"gift":super::gifts::unique(s,&s.owned[&id])?,"from_id":{"_":"peerUser","user_id":uid},"peer":{"_":"peerUser","user_id":uid},"saved_id":id});
    let message = Message {
        id,
        from: uid,
        kind: "user".into(),
        peer: uid,
        date: now(),
        text: String::new(),
        media: None,
        entities: vec![],
        reply: None,
        forward: None,
        edit: None,
        action: Some(action),
        pinned: false,
    };
    s.messages.push(message.clone());
    Ok(super::messages::publish(s, uid, &message, false))
}
