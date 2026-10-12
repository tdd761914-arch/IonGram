use super::*;
use crate::{
    crypto::{random, random_id},
    store::{Invite, Member, Room},
};
pub const METHODS: &[&str] = &[
    "messages.createChat",
    "messages.addChatUser",
    "messages.getFullChat",
    "messages.getChats",
    "messages.editChatAbout",
    "messages.exportChatInvite",
    "messages.checkChatInvite",
    "messages.importChatInvite",
    "messages.getExportedChatInvites",
    "messages.editExportedChatInvite",
    "messages.deleteExportedChatInvite",
    "messages.deleteRevokedExportedChatInvites",
    "messages.getChatInviteImporters",
    "messages.hideChatJoinRequest",
    "messages.hideAllChatJoinRequests",
];
fn member() -> Member {
    Member {
        date: now(),
        rights: json!({"_":"chatAdminRights"}),
        banned: json!({"_":"chatBannedRights","until_date":0}),
        rank: String::new(),
    }
}
fn changed(s: &State, id: i64, uid: i64) -> Value {
    updates(vec![s.room(id, uid)], vec![], vec![])
}
fn invite(hash: &str, i: &Invite) -> Value {
    let mut v = json!({"_":"chatInviteExported","link":format!("https://iongram.local/+{hash}"),"admin_id":i.creator,"date":i.date,"revoked":i.revoked,"permanent":i.permanent,"request_needed":i.request,"usage":i.used});
    if i.expires > 0 {
        v["expire_date"] = json!(i.expires)
    }
    if i.limit > 0 {
        v["usage_limit"] = json!(i.limit)
    }
    v
}
fn permanent_invite(s: &mut State, room: i64, creator: i64) -> Value {
    if let Some((hash, entry)) = s
        .invites
        .iter()
        .find(|(_, i)| i.room == room && i.creator == creator && i.permanent && !i.revoked)
    {
        return invite(hash, entry);
    }
    let hash = hex::encode(random(24));
    let entry = Invite {
        permanent: true,
        room,
        creator,
        date: now(),
        expires: 0,
        limit: 0,
        used: 0,
        revoked: false,
        request: false,
        pending: vec![],
    };
    let result = invite(&hash, &entry);
    s.invites.insert(hash, entry);
    result
}
fn participant(r: &Room, id: i64) -> Value {
    let m = &r.members[&id];
    if id == r.owner {
        json!({"_":"channelParticipantCreator","user_id":id,"admin_rights":{"_":"chatAdminRights","change_info":true,"post_messages":true,"edit_messages":true,"delete_messages":true,"ban_users":true,"invite_users":true,"pin_messages":true,"add_admins":true}})
    } else if m
        .rights
        .as_object()
        .is_some_and(|o| o.values().any(|v| v == true))
    {
        json!({"_":"channelParticipantAdmin","user_id":id,"inviter_id":r.owner,"promoted_by":r.owner,"date":m.date,"admin_rights":m.rights,"rank":m.rank})
    } else {
        json!({"_":"channelParticipant","user_id":id,"date":m.date})
    }
}
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    let name = text(v, "_");
    match name {
        "messages.createChat" | "channels.createChannel" => {
            let title = text(v, "title").trim();
            ensure!(
                !title.is_empty() && title.chars().count() <= 128,
                "CHAT_TITLE_EMPTY"
            );
            let id = s.id();
            let mut members = std::collections::BTreeMap::new();
            members.insert(uid, member());
            for u in list(v, "users") {
                let (_, id) = peer(&u, uid)?;
                ensure!(s.users.contains_key(&id), "USER_ID_INVALID");
                members.insert(id, member());
            }
            let channel = name.starts_with("channels");
            s.rooms.insert(
                id,
                Room {
                    id,
                    hash: random_id(),
                    channel,
                    mega: v["megagroup"] == true,
                    title: title.into(),
                    about: text(v, "about").into(),
                    username: String::new(),
                    owner: uid,
                    members,
                    bans: Default::default(),
                    date: now(),
                    pts: 1,
                    last: 0,
                    pinned: 0,
                    signatures: false,
                    linked: None,
                    settings: Default::default(),
                },
            );
            permanent_invite(s, id, uid);
            let u = super::messages::creation_message(s, id).unwrap_or_else(|| changed(s, id, uid));
            if channel {
                Ok(u)
            } else {
                Ok(json!({"_":"messages.invitedUsers","updates":u,"missing_invitees":[]}))
            }
        }
        "messages.getChats" | "channels.getChannels" => {
            let mut chats = vec![];
            for x in list(v, "id") {
                let id = x.as_i64().unwrap_or(int(&x, "channel_id"));
                let kind = if name.starts_with("channels") {
                    "channel"
                } else {
                    "chat"
                };
                access(s, kind, id, uid, false)?;
                chats.push(s.room(id, uid))
            }
            Ok(json!({"_":"messages.chats","chats":chats}))
        }
        "channels.getAdminedPublicChannels" | "channels.getGroupsForDiscussion" => {
            let chats = s
                .rooms
                .values()
                .filter(|r| {
                    r.owner == uid
                        && r.channel
                        && if name.ends_with("Discussion") {
                            r.mega && r.linked.is_none()
                        } else {
                            !r.username.is_empty()
                        }
                })
                .map(|r| s.room(r.id, uid))
                .collect::<Vec<_>>();
            Ok(json!({"_":"messages.chats","chats":chats}))
        }
        "messages.checkChatInvite" | "messages.importChatInvite" => {
            let hash = text(v, "hash");
            let i = s.invites.get(hash).context("INVITE_HASH_INVALID")?.clone();
            ensure!(!i.revoked, "INVITE_HASH_EXPIRED");
            ensure!(i.expires == 0 || now() < i.expires, "INVITE_HASH_EXPIRED");
            let r = s.rooms.get(&i.room).context("CHANNEL_INVALID")?;
            ensure!(
                !super::banned(r, uid, "view_messages"),
                "USER_BANNED_IN_CHANNEL"
            );
            if r.members.contains_key(&uid) {
                return if name.ends_with("checkChatInvite") {
                    Ok(json!({"_":"chatInviteAlready","chat":s.room(i.room,uid)}))
                } else {
                    Ok(changed(s, i.room, uid))
                };
            }
            ensure!(i.limit == 0 || i.used < i.limit, "USERS_TOO_MUCH");
            if name.ends_with("checkChatInvite") {
                return Ok(
                    json!({"_":"chatInvite","title":r.title,"about":r.about,"photo":{"_":"photoEmpty","id":0},"participants_count":r.members.len(),"color":0,"channel":r.channel,"broadcast":r.channel&&!r.mega,"megagroup":r.mega,"request_needed":i.request}),
                );
            }
            if i.request {
                let entry = s.invites.get_mut(hash).unwrap();
                if !entry.pending.contains(&uid) {
                    entry.pending.push(uid)
                }
                return Ok(changed(s, i.room, uid));
            }
            s.rooms
                .get_mut(&i.room)
                .unwrap()
                .members
                .insert(uid, member());
            s.invites.get_mut(hash).unwrap().used += 1;
            Ok(changed(s, i.room, uid))
        }
        "channels.setDiscussionGroup" => {
            let id = int(&v["broadcast"], "channel_id");
            let group = int(&v["group"], "channel_id");
            admin(s, id, uid, "change_info")?;
            ensure!(!s.rooms[&id].mega, "BROADCAST_REQUIRED");
            if group != 0 {
                admin(s, group, uid, "change_info")?;
                ensure!(s.rooms[&group].mega, "MEGAGROUP_REQUIRED")
            }
            s.rooms.get_mut(&id).unwrap().linked = (group != 0).then_some(group);
            Ok(json!(true))
        }
        "channels.getChannelRecommendations" => Ok(json!({"_":"messages.chats","chats":[]})),
        "channels.getSponsoredMessages" => {
            Ok(json!({"_":"messages.sponsoredMessages","messages":[],"chats":[],"users":[]}))
        }
        _ => {
            let id = if !v["channel"].is_null() {
                int(&v["channel"], "channel_id")
            } else if !v["peer"].is_null() {
                peer(&v["peer"], uid)?.1
            } else {
                int(v, "chat_id")
            };
            let r = s.rooms.get(&id).context("CHANNEL_INVALID")?.clone();
            let kind = if r.channel { "channel" } else { "chat" };
            if name != "channels.joinChannel" {
                access(s, kind, id, uid, false)?;
            }
            match name {
                "channels.getFullChannel" | "messages.getFullChat" => {
                    let mut f = if r.channel {
                        json!({"_":"channelFull","id":id,"about":r.about,"participants_count":r.members.len(),"admins_count":r.members.values().filter(|m|m.rights.as_object().is_some_and(|o|o.values().any(|v|v==true))).count()+1,"read_inbox_max_id":0,"read_outbox_max_id":0,"unread_count":0,"chat_photo":{"_":"photoEmpty","id":0},"notify_settings":{"_":"peerNotifySettings"},"bot_info":[],"pts":r.pts,"can_view_participants":true})
                    } else {
                        json!({"_":"chatFull","id":id,"about":r.about,"participants":{"_":"chatParticipants","chat_id":id,"version":1,"participants":r.members.keys().map(|u|if *u==r.owner{json!({"_":"chatParticipantCreator","user_id":u})}else{json!({"_":"chatParticipant","user_id":u,"inviter_id":r.owner,"date":r.date})}).collect::<Vec<_>>()},"notify_settings":{"_":"peerNotifySettings"}})
                    };
                    if admin(s, id, uid, "invite_users").is_ok() {
                        f["exported_invite"] = permanent_invite(s, id, uid);
                    }
                    if r.pinned > 0 {
                        f["pinned_msg_id"] = json!(r.pinned)
                    }
                    if let Some(link) = r.linked {
                        f["linked_chat_id"] = json!(link)
                    }
                    Ok(
                        json!({"_":"messages.chatFull","full_chat":f,"chats":[s.room(id,uid)],"users":r.members.keys().map(|u|s.user(*u,uid)).collect::<Vec<_>>()}),
                    )
                }
                "channels.checkUsername" | "channels.updateUsername" => {
                    admin(s, id, uid, "change_info")?;
                    let n = text(v, "username");
                    ensure!(
                        !s.aliases.contains_key(&n.to_lowercase()),
                        "USERNAME_OCCUPIED"
                    );
                    ensure!(
                        n.is_empty() || super::users::valid_username(n),
                        "USERNAME_INVALID"
                    );
                    ensure!(
                        !s.rooms.values().any(|r| r.id != id
                            && !n.is_empty()
                            && r.username.eq_ignore_ascii_case(n))
                            && !s
                                .users
                                .values()
                                .any(|u| !n.is_empty() && u.username.eq_ignore_ascii_case(n)),
                        "USERNAME_OCCUPIED"
                    );
                    if name.ends_with("updateUsername") {
                        s.rooms.get_mut(&id).unwrap().username = n.into()
                    }
                    Ok(json!(true))
                }
                "channels.joinChannel" => {
                    ensure!(
                        !super::banned(&r, uid, "view_messages"),
                        "USER_BANNED_IN_CHANNEL"
                    );
                    ensure!(
                        !r.username.is_empty() || r.members.contains_key(&uid),
                        "CHANNEL_PRIVATE"
                    );
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .members
                        .entry(uid)
                        .or_insert_with(member);
                    Ok(changed(s, id, uid))
                }
                "channels.leaveChannel" => {
                    ensure!(uid != r.owner, "USER_CREATOR");
                    s.rooms.get_mut(&id).unwrap().members.remove(&uid);
                    Ok(changed(s, id, uid))
                }
                "channels.editTitle"
                | "channels.editAbout"
                | "messages.editChatAbout"
                | "channels.toggleSignatures" => {
                    admin(s, id, uid, "change_info")?;
                    let r = s.rooms.get_mut(&id).unwrap();
                    if name.ends_with("editTitle") {
                        ensure!(!text(v, "title").trim().is_empty(), "CHAT_TITLE_EMPTY");
                        r.title = text(v, "title").into()
                    } else if name.ends_with("About") {
                        r.about = text(v, "about").into()
                    } else {
                        r.signatures = v["enabled"] == true
                    }
                    if name.ends_with("About") {
                        Ok(json!(true))
                    } else {
                        Ok(changed(s, id, uid))
                    }
                }
                "channels.deleteChannel" => {
                    ensure!(uid == r.owner, "CHAT_ADMIN_REQUIRED");
                    s.rooms.remove(&id);
                    s.messages.retain(|m| m.kind == "user" || m.peer != id);
                    s.invites.retain(|_, i| i.room != id);
                    Ok(updates(vec![], vec![], vec![]))
                }
                "channels.inviteToChannel" | "messages.addChatUser" => {
                    admin(s, id, uid, "invite_users")?;
                    let us = if name == "messages.addChatUser" {
                        vec![v["user_id"].clone()]
                    } else {
                        list(v, "users")
                    };
                    ensure!(us.len() <= 100, "USERS_TOO_MUCH");
                    for u in us {
                        let (_, id2) = peer(&u, uid)?;
                        ensure!(s.users.contains_key(&id2), "USER_ID_INVALID");
                        s.rooms
                            .get_mut(&id)
                            .unwrap()
                            .members
                            .entry(id2)
                            .or_insert_with(member);
                    }
                    Ok(
                        json!({"_":"messages.invitedUsers","updates":changed(s,id,uid),"missing_invitees":[]}),
                    )
                }
                "channels.getParticipants" => {
                    let offset = int(v, "offset").max(0) as usize;
                    let limit = int(v, "limit").clamp(1, 200) as usize;
                    let ids: Vec<_> = r
                        .members
                        .keys()
                        .copied()
                        .filter(|i| {
                            text(&v["filter"], "_") != "channelParticipantsAdmins"
                                || *i == r.owner
                                || r.members[i]
                                    .rights
                                    .as_object()
                                    .is_some_and(|o| o.values().any(|v| v == true))
                        })
                        .collect();
                    Ok(
                        json!({"_":"channels.channelParticipants","count":ids.len(),"participants":ids.iter().skip(offset).take(limit).map(|i|participant(&r,*i)).collect::<Vec<_>>(),"chats":[s.room(id,uid)],"users":ids.iter().skip(offset).take(limit).map(|i|s.user(*i,uid)).collect::<Vec<_>>()}),
                    )
                }
                "channels.getParticipant" => {
                    let (_, u) = peer(&v["participant"], uid)?;
                    ensure!(r.members.contains_key(&u), "USER_NOT_PARTICIPANT");
                    Ok(
                        json!({"_":"channels.channelParticipant","participant":participant(&r,u),"chats":[s.room(id,uid)],"users":[s.user(u,uid)]}),
                    )
                }
                "channels.editAdmin" => {
                    admin(s, id, uid, "add_admins")?;
                    let (_, u) = peer(&v["user_id"], uid)?;
                    ensure!(u != r.owner, "USER_CREATOR");
                    ensure!(s.users.contains_key(&u), "USER_ID_INVALID");
                    let rights = &v["admin_rights"];
                    if uid != r.owner {
                        let mine = &r.members[&uid].rights;
                        for (k, val) in rights.as_object().context("RIGHTS_INVALID")? {
                            ensure!(*val != true || mine[k] == true, "CHAT_ADMIN_REQUIRED")
                        }
                    }
                    let m = s
                        .rooms
                        .get_mut(&id)
                        .unwrap()
                        .members
                        .entry(u)
                        .or_insert_with(member);
                    m.rights = rights.clone();
                    m.rank = text(v, "rank").into();
                    Ok(changed(s, id, uid))
                }
                "channels.editBanned" => {
                    admin(s, id, uid, "ban_users")?;
                    let (_, u) = peer(&v["participant"], uid)?;
                    ensure!(u != r.owner, "USER_CREATOR");
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .bans
                        .insert(u, v["banned_rights"].clone());
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .bans
                        .insert(u, v["banned_rights"].clone());
                    if v["banned_rights"]["view_messages"] == true {
                        s.rooms.get_mut(&id).unwrap().members.remove(&u);
                    } else if let Some(m) = s.rooms.get_mut(&id).unwrap().members.get_mut(&u) {
                        m.banned = v["banned_rights"].clone()
                    }
                    Ok(changed(s, id, uid))
                }
                "channels.togglePreHistoryHidden"
                | "channels.toggleJoinToSend"
                | "channels.toggleJoinRequest"
                | "channels.toggleParticipantsHidden"
                | "channels.toggleSlowMode"
                | "channels.setStickers"
                | "channels.setEmojiStickers"
                | "channels.updateEmojiStatus" => {
                    admin(s, id, uid, "change_info")?;
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert(name.into(), v.clone());
                    if name.ends_with("Stickers") || name.ends_with("EmojiStatus") {
                        Ok(json!(true))
                    } else {
                        Ok(changed(s, id, uid))
                    }
                }
                "messages.exportChatInvite" => {
                    admin(s, id, uid, "invite_users")?;
                    ensure!(
                        v["subscription_pricing"].is_null(),
                        "PAID_INVITES_NOT_SUPPORTED"
                    );
                    let hash = hex::encode(random(24));
                    let i = Invite {
                        permanent: false,
                        room: id,
                        creator: uid,
                        date: now(),
                        expires: int(v, "expire_date"),
                        limit: int(v, "usage_limit"),
                        used: 0,
                        revoked: false,
                        request: v["request_needed"] == true,
                        pending: vec![],
                    };
                    let result = invite(&hash, &i);
                    s.invites.insert(hash, i);
                    Ok(result)
                }
                "messages.getExportedChatInvites" => {
                    admin(s, id, uid, "invite_users")?;
                    let (_, creator) = peer(&v["admin_id"], uid)?;
                    if creator != uid {
                        ensure!(r.owner == uid, "CHAT_ADMIN_REQUIRED");
                    }
                    if v["revoked"] != true {
                        permanent_invite(s, id, creator);
                    }
                    let mut inv: Vec<_> = s
                        .invites
                        .iter()
                        .filter(|(_, i)| {
                            i.room == id
                                && i.creator == creator
                                && i.revoked == (v["revoked"] == true)
                        })
                        .map(|(h, i)| invite(h, i))
                        .collect();
                    inv.sort_by_key(|i| (i["permanent"] != true, -int(i, "date")));
                    let count = inv.len();
                    inv.truncate(int(v, "limit").clamp(1, 100) as usize);
                    Ok(
                        json!({"_":"messages.exportedChatInvites","count":count,"invites":inv,"users":[s.user(creator,uid)]}),
                    )
                }
                "messages.editExportedChatInvite" | "messages.deleteExportedChatInvite" => {
                    admin(s, id, uid, "invite_users")?;
                    let hash = text(v, "link").rsplit('+').next().unwrap_or("");
                    let i = s.invites.get_mut(hash).context("INVITE_HASH_INVALID")?;
                    ensure!(i.room == id, "INVITE_HASH_INVALID");
                    if name.ends_with("deleteExportedChatInvite") {
                        s.invites.remove(hash);
                        Ok(json!(true))
                    } else {
                        if v["revoked"] == true {
                            i.revoked = true
                        }
                        if v["expire_date"].is_number() {
                            i.expires = int(v, "expire_date")
                        }
                        if v["usage_limit"].is_number() {
                            i.limit = int(v, "usage_limit")
                        }
                        Ok(
                            json!({"_":"messages.exportedChatInvite","invite":invite(hash,i),"users":[]}),
                        )
                    }
                }
                "messages.deleteRevokedExportedChatInvites" => {
                    admin(s, id, uid, "invite_users")?;
                    s.invites.retain(|_, i| i.room != id || !i.revoked);
                    Ok(json!(true))
                }
                "messages.getChatInviteImporters" => {
                    admin(s, id, uid, "invite_users")?;
                    let ids: std::collections::BTreeSet<_> = s
                        .invites
                        .values()
                        .filter(|i| i.room == id)
                        .flat_map(|i| i.pending.iter().copied())
                        .collect();
                    Ok(
                        json!({"_":"messages.chatInviteImporters","count":ids.len(),"importers":ids.iter().map(|u|json!({"_":"chatInviteImporter","user_id":u,"date":now(),"requested":true})).collect::<Vec<_>>(),"users":ids.iter().map(|u|s.user(*u,uid)).collect::<Vec<_>>()}),
                    )
                }
                "messages.hideChatJoinRequest" | "messages.hideAllChatJoinRequests" => {
                    admin(s, id, uid, "invite_users")?;
                    let target = if name.ends_with("hideChatJoinRequest") {
                        Some(peer(&v["user_id"], uid)?.1)
                    } else {
                        None
                    };
                    let mut approved = vec![];
                    for i in s.invites.values_mut().filter(|i| i.room == id) {
                        i.pending.retain(|u| {
                            if target.is_none_or(|t| t == *u) {
                                approved.push(*u);
                                false
                            } else {
                                true
                            }
                        })
                    }
                    if v["approved"] == true {
                        for u in approved {
                            s.rooms
                                .get_mut(&id)
                                .unwrap()
                                .members
                                .entry(u)
                                .or_insert_with(member);
                        }
                    }
                    Ok(changed(s, id, uid))
                }
                "channels.readHistory" | "channels.readMessageContents" => {
                    s.read
                        .insert(format!("{uid}:{kind}:{id}"), int(v, "max_id"));
                    Ok(json!(true))
                }
                "channels.editMessage" | "channels.deleteMessages" | "channels.deleteHistory" => {
                    super::messages::handle(s, uid, v)
                }
                "channels.getAdminLog" => {
                    Ok(json!({"_":"channels.adminLogResults","events":[],"chats":[],"users":[]}))
                }
                _ => bail!("METHOD_NOT_IMPLEMENTED"),
            }
        }
    }
}
