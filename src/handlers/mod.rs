pub mod auctions;
pub mod channels;
pub mod craft;
pub mod files;
pub mod fragment;
pub mod gifts;
pub mod messages;
pub mod rooms;
pub mod topics;
pub mod users;
use crate::{crypto::now, store::State, tl::Schema};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
pub fn int(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}
pub fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub fn list(v: &Value, k: &str) -> Vec<Value> {
    v[k].as_array().cloned().unwrap_or_default()
}
pub fn updates(chats: Vec<Value>, users: Vec<Value>, items: Vec<Value>) -> Value {
    json!({"_":"updates","updates":items,"chats":chats,"users":users,"date":now(),"seq":0})
}
pub fn peer(v: &Value, uid: i64) -> Result<(String, i64)> {
    match text(v, "_") {
        "inputPeerSelf" | "inputUserSelf" => Ok(("user".into(), uid)),
        "inputPeerUser" | "inputUser" | "peerUser" => Ok(("user".into(), int(v, "user_id"))),
        "inputPeerChat" | "peerChat" => Ok(("chat".into(), int(v, "chat_id"))),
        "inputPeerChannel" | "inputChannel" | "peerChannel" => {
            Ok(("channel".into(), int(v, "channel_id")))
        }
        _ => bail!("PEER_ID_INVALID"),
    }
}
pub fn access(s: &State, kind: &str, id: i64, uid: i64, write: bool) -> Result<()> {
    if kind == "user" {
        ensure!(s.users.contains_key(&id), "USER_ID_INVALID");
        return Ok(());
    }
    let r = s.rooms.get(&id).context("CHANNEL_INVALID")?;
    ensure!(r.channel == (kind == "channel"), "PEER_ID_INVALID");
    ensure!(!banned(r, uid, "view_messages"), "CHANNEL_PRIVATE");
    let member = r.members.get(&uid);
    ensure!(
        member.is_some() || (!write && channels::public(s, r)),
        "CHANNEL_PRIVATE"
    );
    if write {
        ensure!(!banned(r, uid, "send_messages"), "CHAT_WRITE_FORBIDDEN");
        let m = member.context("CHAT_WRITE_FORBIDDEN")?;
        let is_admin = r.owner == uid
            || m.rights
                .as_object()
                .is_some_and(|a| a.values().any(|v| v == true));
        if !is_admin {
            ensure!(
                !channels::enabled(r, "channels.convertToGigagroup"),
                "CHAT_WRITE_FORBIDDEN"
            );
            if let Some(default) = r.settings.get("default_banned_rights") {
                ensure!(default["send_messages"] != true, "CHAT_WRITE_FORBIDDEN");
            }
        }
        if r.channel && !r.mega {
            ensure!(
                r.owner == uid || m.rights["post_messages"] == true,
                "CHAT_WRITE_FORBIDDEN"
            )
        }
    }
    Ok(())
}
pub fn admin(s: &State, id: i64, uid: i64, right: &str) -> Result<()> {
    let r = s.rooms.get(&id).context("CHANNEL_INVALID")?;
    ensure!(
        r.owner == uid || r.members.get(&uid).is_some_and(|m| m.rights[right] == true),
        "CHAT_ADMIN_REQUIRED"
    );
    Ok(())
}
pub fn dispatch(
    s: &mut State,
    uid: i64,
    key: i64,
    v: &Value,
    schema: &Schema,
    dir: &std::path::Path,
) -> Result<Value> {
    let name = text(v, "_");
    if name.starts_with("fragment.")
        || [
            "account.toggleUsername",
            "account.reorderUsernames",
            "channels.toggleUsername",
            "channels.reorderUsernames",
            "channels.deactivateAllUsernames",
        ]
        .contains(&name)
    {
        return fragment::handle(s, uid, v);
    }
    if name.starts_with("auth.")
        || name.starts_with("users.")
        || name.starts_with("account.")
        || name.starts_with("contacts.")
        || name.starts_with("help.")
    {
        return users::handle(s, uid, key, v, schema);
    }
    ensure!(uid > 0, "AUTH_KEY_UNREGISTERED");
    if topics::METHODS.contains(&name) {
        return topics::handle(s, uid, v);
    }
    if name.starts_with("upload.") {
        return files::handle(s, uid, v, dir);
    }
    if name.starts_with("payments.") {
        return gifts::handle(s, uid, v);
    }
    if name.starts_with("channels.") || rooms::METHODS.contains(&name) {
        if v["channel"]["_"] == "inputChannel" {
            let room = s
                .rooms
                .get(&int(&v["channel"], "channel_id"))
                .context("CHANNEL_INVALID")?;
            ensure!(
                room.hash == int(&v["channel"], "access_hash"),
                "CHANNEL_INVALID"
            );
        }
        let before = s.rooms.clone();
        let mut request = v.clone();
        if matches!(name, "channels.editPhoto" | "messages.editChatPhoto") {
            let channel = if name.starts_with("channels") {
                int(&v["channel"], "channel_id")
            } else {
                int(v, "chat_id")
            };
            admin(s, channel, uid, "change_info")?;
            request["photo"] = match text(&v["photo"], "_") {
                "inputChatPhotoEmpty" => json!({"_":"photoEmpty","id":0}),
                "inputChatPhoto" => files::media(
                    s,
                    uid,
                    &json!({"_":"inputMediaPhoto","id":v["photo"]["id"]}),
                )?["photo"]
                    .clone(),
                "inputChatUploadedPhoto" => {
                    ensure!(
                        v["photo"]["video"].is_null() && v["photo"]["video_emoji_markup"].is_null(),
                        "PHOTO_VIDEO_NOT_SUPPORTED"
                    );
                    let media = files::materialize(
                        s,
                        uid,
                        &json!({"_":"inputMediaUploadedPhoto","file":v["photo"]["file"]}),
                        dir,
                    )?;
                    files::media(s, uid, &media)?["photo"].clone()
                }
                _ => bail!("PHOTO_INVALID"),
            };
        }
        let response = rooms::handle(s, uid, &request)?;
        let changed_rooms: Vec<_> = s
            .rooms
            .iter()
            .filter_map(|(id, r)| before.get(id).map(|old| (old.clone(), r.clone())))
            .collect();
        for (old, new) in changed_rooms {
            if new.channel {
                channels::audit(s, uid, &old, &new);
            }
        }
        let mut events = vec![];
        for (id, r) in &s.rooms {
            if before
                .get(id)
                .is_none_or(|old| serde_json::to_value(old).ok() != serde_json::to_value(r).ok())
            {
                let mut recipients: Vec<_> = r.members.keys().copied().collect();
                if let Some(old) = before.get(id) {
                    recipients.extend(old.members.keys().copied());
                }
                let update = if r.channel {
                    json!({"_":"updateChannel","channel_id":id})
                } else {
                    json!({"_":"updateChatParticipants","participants":{"_":"chatParticipants","chat_id":id,"version":s.pts+1,"participants":r.members.keys().map(|u|if *u==r.owner{json!({"_":"chatParticipantCreator","user_id":u})}else{json!({"_":"chatParticipant","user_id":u,"inviter_id":r.owner,"date":r.date})}).collect::<Vec<_>>()}})
                };
                events.push((recipients, update));
            }
        }
        for (id, r) in before {
            if !s.rooms.contains_key(&id) && r.channel {
                events.push((
                    r.members.keys().copied().collect(),
                    json!({"_":"updateChannel","channel_id":id}),
                ));
            }
        }
        for (users, update) in events {
            s.record(users, update);
        }
        return Ok(response);
    }
    if name.starts_with("messages.") || name.starts_with("updates.") {
        let mut request = v.clone();
        if !v["media"].is_null() {
            request["media"] = files::materialize(s, uid, &v["media"], dir)?;
        }
        return messages::handle(s, uid, &request);
    }
    bail!("METHOD_NOT_IMPLEMENTED")
}

pub fn banned(r: &crate::store::Room, uid: i64, right: &str) -> bool {
    r.bans.get(&uid).is_some_and(|b| {
        b[right] == true && (int(b, "until_date") == 0 || int(b, "until_date") > now())
    })
}
