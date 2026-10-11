//! Independent implementation of gramsrv's username registry behavior.
use super::*;
use crate::store::Alias;
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    ensure!(uid > 0, "AUTH_KEY_UNREGISTERED");
    let name = text(v, "_");
    if name == "fragment.getCollectibleInfo" {
        let c = &v["collectible"];
        let phone = text(c, "_") == "inputCollectiblePhone";
        let key = if phone {
            format!("phone:{}", text(c, "phone").trim_start_matches('+'))
        } else {
            text(c, "username").trim_start_matches('@').to_lowercase()
        };
        ensure!(!key.is_empty(), "COLLECTIBLE_INVALID");
        let a = s.aliases.get(&key).context("COLLECTIBLE_NOT_FOUND")?;
        let mine = if a.kind == "channel" {
            s.rooms.get(&a.owner).is_some_and(|r| r.owner == uid)
        } else {
            a.owner == uid
        };
        ensure!(mine || (!phone && a.active), "COLLECTIBLE_NOT_FOUND");
        return Ok(
            json!({"_":"fragment.collectibleInfo","purchase_date":a.date,"currency":a.currency,"amount":a.amount,"crypto_currency":a.crypto_currency,"crypto_amount":a.crypto_amount,"url":a.url}),
        );
    }
    let (kind, id) = if name.starts_with("channels.") {
        let id = int(&v["channel"], "channel_id");
        ensure!(
            s.rooms.get(&id).is_some_and(|r| r.owner == uid),
            "CHAT_ADMIN_REQUIRED"
        );
        ("channel", id)
    } else {
        ("user", uid)
    };
    match name {
        "account.toggleUsername" | "channels.toggleUsername" => {
            let key = text(v, "username").to_lowercase();
            let a = s.aliases.get_mut(&key).context("USERNAME_NOT_OCCUPIED")?;
            ensure!(a.kind == kind && a.owner == id, "USERNAME_NOT_OCCUPIED");
            let active = v["active"] == true;
            ensure!(a.active != active, "USERNAME_NOT_MODIFIED");
            a.active = active;
        }
        "account.reorderUsernames" | "channels.reorderUsernames" => {
            let order: Vec<_> = list(v, "order")
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_lowercase())
                .collect();
            let current = projection(s, kind, id);
            let owned: std::collections::BTreeSet<_> = current
                .iter()
                .map(|n| text(n, "username").to_lowercase())
                .collect();
            let active: std::collections::BTreeSet<_> = current
                .iter()
                .filter(|n| n["active"] == true)
                .map(|n| text(n, "username").to_lowercase())
                .collect();
            let input: std::collections::BTreeSet<_> = order.iter().cloned().collect();
            ensure!(
                input.len() == order.len() && input.is_subset(&owned) && active.is_subset(&input),
                "ORDER_INVALID"
            );
            let mut full = order;
            for n in &current {
                let n = text(n, "username").to_lowercase();
                if !input.contains(&n) {
                    full.push(n)
                }
            }
            let previous: Vec<_> = current
                .iter()
                .map(|n| text(n, "username").to_lowercase())
                .collect();
            ensure!(full != previous, "USERNAME_NOT_MODIFIED");
            s.username_orders.insert(format!("{kind}:{id}"), full);
        }
        "channels.deactivateAllUsernames" => {
            for a in s
                .aliases
                .values_mut()
                .filter(|a| a.kind == kind && a.owner == id)
            {
                a.active = false;
            }
        }
        _ => bail!("METHOD_NOT_IMPLEMENTED"),
    };
    let update = if kind == "user" {
        json!({"_":"updateUserName","user_id":id,"first_name":s.users[&id].first,"last_name":s.users[&id].last,"usernames":projection(s,kind,id)})
    } else {
        json!({"_":"updateChannel","channel_id":id})
    };
    let recipients = if kind == "user" {
        vec![id]
    } else {
        s.rooms[&id].members.keys().copied().collect()
    };
    s.record(recipients, update);
    Ok(json!(true))
}
pub fn projection(s: &State, kind: &str, id: i64) -> Vec<Value> {
    let mut list: Vec<_> = s
        .aliases
        .values()
        .filter(|a| a.kind == kind && a.owner == id)
        .collect();
    list.sort_by_key(|a| a.position);
    let mut out: Vec<Value> = list
        .iter()
        .map(|a| json!({"_":"username","username":a.name,"active":a.active,"editable":false}))
        .collect();
    let main = if kind == "user" {
        s.users.get(&id).map(|u| u.username.as_str())
    } else {
        s.rooms.get(&id).map(|r| r.username.as_str())
    }
    .unwrap_or("");
    if !main.is_empty() {
        out.insert(
            0,
            json!({"_":"username","username":main,"active":true,"editable":true}),
        );
    }
    if let Some(order) = s.username_orders.get(&format!("{kind}:{id}")) {
        out.sort_by_key(|n| {
            order
                .iter()
                .position(|x| x.eq_ignore_ascii_case(text(n, "username")))
                .unwrap_or(usize::MAX)
        });
    }
    out
}
pub fn mint(s: &mut State, kind: &str, owner: i64, name: &str) -> Result<()> {
    let name = name.trim_start_matches('@');
    ensure!(
        (1..=32).contains(&name.len())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "USERNAME_INVALID"
    );
    ensure!(matches!(kind, "user" | "channel"), "PEER_ID_INVALID");
    ensure!(
        if kind == "user" {
            s.users.contains_key(&owner)
        } else {
            s.rooms.contains_key(&owner)
        },
        "PEER_ID_INVALID"
    );
    ensure!(
        !s.aliases.contains_key(&name.to_lowercase())
            && !s
                .users
                .values()
                .any(|u| u.username.eq_ignore_ascii_case(name))
            && !s
                .rooms
                .values()
                .any(|r| r.username.eq_ignore_ascii_case(name)),
        "USERNAME_OCCUPIED"
    );
    let position = s
        .aliases
        .values()
        .filter(|a| a.owner == owner && a.kind == kind)
        .count() as i32;
    s.aliases.insert(
        name.to_lowercase(),
        Alias {
            name: name.into(),
            kind: kind.into(),
            owner,
            active: true,
            position,
            date: now(),
            currency: "XTR".into(),
            amount: 0,
            crypto_currency: String::new(),
            crypto_amount: 0,
            url: format!("https://iongram.local/collectible/{name}"),
        },
    );
    Ok(())
}
