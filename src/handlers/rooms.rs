use super::*;
use crate::{
    crypto::{random, random_id},
    store::{Invite, Member, Room},
};
pub const METHODS: &[&str] = &[
    "messages.createChat",
    "messages.editChatPhoto",
    "messages.editChatTitle",
    "messages.editChatCreator",
    "messages.editChatDefaultBannedRights",
    "messages.toggleNoForwards",
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
pub(super) fn participant(r: &Room, id: i64) -> Value {
    if let Some(rights) = r.bans.get(&id).filter(|b| {
        (int(b, "until_date") == 0 || int(b, "until_date") > now())
            && b.as_object().is_some_and(|o| o.values().any(|v| v == true))
    }) {
        return json!({"_":"channelParticipantBanned","peer":{"_":"peerUser","user_id":id},"left":!r.members.contains_key(&id),"kicked_by":r.owner,"date":r.date,"banned_rights":rights});
    }
    let Some(m) = r.members.get(&id) else {
        return json!({"_":"channelParticipantLeft","peer":{"_":"peerUser","user_id":id}});
    };
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
    if super::channels::METHODS.contains(&name) {
        return super::channels::handle(s, uid, v);
    }
    match name {
        "messages.createChat" | "channels.createChannel" => {
            let title = text(v, "title").trim();
            ensure!(
                !title.is_empty() && title.chars().count() <= 128,
                "CHAT_TITLE_EMPTY"
            );
            if name == "channels.createChannel" {
                ensure!(
                    (v["megagroup"] == true) != (v["broadcast"] == true),
                    "CHAT_TYPE_INVALID"
                );
                ensure!(
                    v["forum"] != true || v["megagroup"] == true,
                    "MEGAGROUP_REQUIRED"
                );
            }
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
            if v["forum"] == true {
                s.rooms.get_mut(&id).unwrap().settings.insert(
                    "channels.toggleForum".into(),
                    json!({"enabled":true,"tabs":false}),
                );
            }
            if !v["ttl_period"].is_null() {
                ensure!(
                    (0..=31536000).contains(&int(v, "ttl_period")),
                    "TTL_PERIOD_INVALID"
                );
                s.rooms
                    .get_mut(&id)
                    .unwrap()
                    .settings
                    .insert("ttl_period".into(), v["ttl_period"].clone());
            }
            if !v["geo_point"].is_null() {
                super::channels::handle(
                    s,
                    uid,
                    &json!({"_":"channels.editLocation","channel":{"_":"inputChannel","channel_id":id,"access_hash":s.rooms[&id].hash},"geo_point":v["geo_point"],"address":v["address"]}),
                )?;
            }
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
                if let Some(r) = s.deleted_rooms.get(&id) {
                    ensure!(r.members.contains_key(&uid), "CHANNEL_PRIVATE");
                    chats.push(s.room(id, uid));
                    continue;
                }
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
                            super::channels::public(s, r)
                                && (v["for_personal"] != true || !r.mega)
                                && (v["by_location"] != true
                                    || r.settings
                                        .get("location")
                                        .is_some_and(|p| p["_"] == "channelLocation"))
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
            if let Some(old) = s.rooms[&id].linked {
                if let Some(r) = s.rooms.get_mut(&old) {
                    r.linked = None;
                }
            }
            if group != 0 {
                if let Some(old) = s.rooms[&group].linked {
                    if let Some(r) = s.rooms.get_mut(&old) {
                        r.linked = None;
                    }
                }
                s.rooms.get_mut(&group).unwrap().linked = Some(id);
            }
            s.rooms.get_mut(&id).unwrap().linked = (group != 0).then_some(group);
            Ok(json!(true))
        }
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
                "messages.editChatCreator" => {
                    ensure!(r.owner == uid, "CHAT_ADMIN_REQUIRED");
                    ensure!(
                        v["password"]["_"] == "inputCheckPasswordEmpty",
                        "PASSWORD_HASH_INVALID"
                    );
                    let (kind, target) = peer(&v["user_id"], uid)?;
                    ensure!(
                        kind == "user" && target != uid && r.members.contains_key(&target),
                        "USER_NOT_PARTICIPANT"
                    );
                    let room = s.rooms.get_mut(&id).unwrap();
                    room.owner = target;
                    room.members.get_mut(&uid).unwrap().rights = json!({"_":"chatAdminRights","change_info":true,"post_messages":true,"edit_messages":true,"delete_messages":true,"ban_users":true,"invite_users":true,"pin_messages":true,"add_admins":true,"manage_topics":true});
                    room.bans.remove(&target);
                    Ok(changed(s, id, uid))
                }
                "messages.editChatPhoto" => {
                    admin(s, id, uid, "change_info")?;
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert("photo".into(), v["photo"].clone());
                    Ok(changed(s, id, uid))
                }
                "messages.editChatDefaultBannedRights" => {
                    admin(s, id, uid, "ban_users")?;
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert("default_banned_rights".into(), v["banned_rights"].clone());
                    Ok(changed(s, id, uid))
                }
                "messages.toggleNoForwards" => {
                    admin(s, id, uid, "change_info")?;
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert("noforwards".into(), v["enabled"].clone());
                    Ok(changed(s, id, uid))
                }
                "channels.getFullChannel" | "messages.getFullChat" => {
                    let mut f = if r.channel {
                        json!({"_":"channelFull","id":id,"about":r.about,"participants_count":r.members.len(),"admins_count":r.members.values().filter(|m|m.rights.as_object().is_some_and(|o|o.values().any(|v|v==true))).count()+1,"read_inbox_max_id":0,"read_outbox_max_id":0,"unread_count":0,"chat_photo":{"_":"photoEmpty","id":0},"notify_settings":{"_":"peerNotifySettings"},"bot_info":[],"pts":r.pts,"can_view_participants":true})
                    } else {
                        json!({"_":"chatFull","id":id,"about":r.about,"participants":{"_":"chatParticipants","chat_id":id,"version":1,"participants":r.members.keys().map(|u|if *u==r.owner{json!({"_":"chatParticipantCreator","user_id":u})}else{json!({"_":"chatParticipant","user_id":u,"inviter_id":r.owner,"date":r.date})}).collect::<Vec<_>>()},"notify_settings":{"_":"peerNotifySettings"}})
                    };
                    if admin(s, id, uid, "invite_users").is_ok() {
                        f["exported_invite"] = permanent_invite(s, id, uid);
                    }
                    if let Some(photo) = r.settings.get("photo") {
                        f["chat_photo"] = photo.clone();
                    }
                    if r.channel {
                        super::channels::project_full(&r, &mut f);
                        if let Some(mode) = r.settings.get(&format!("view_forum:{uid}")) {
                            f["view_forum_as_messages"] = mode.clone();
                        }
                        let read = s
                            .read
                            .get(&format!("{uid}:channel:{id}"))
                            .copied()
                            .unwrap_or(0);
                        f["read_inbox_max_id"] = json!(read);
                        f["unread_count"] = json!(
                            s.messages
                                .iter()
                                .filter(|m| m.kind == "channel"
                                    && m.peer == id
                                    && m.id > read
                                    && m.from != uid
                                    && super::messages::history_visible(s, m, uid))
                                .count()
                        );
                        f["banned_count"] = json!(
                            r.bans
                                .keys()
                                .filter(|u| super::banned(&r, **u, "view_messages"))
                                .count()
                        );
                        if super::channels::enabled(&r, "channels.toggleParticipantsHidden")
                            && !super::channels::administrator(&r, uid)
                        {
                            f["can_view_participants"] = json!(false);
                        }
                        if let Some(period) = r.settings.get("ttl_period") {
                            f["ttl_period"] = period.clone();
                        }
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
                        super::channels::public(s, &r) || r.members.contains_key(&uid),
                        "CHANNEL_PRIVATE"
                    );
                    if r.settings
                        .get("channels.toggleJoinRequest")
                        .is_some_and(|v| v["enabled"] == true)
                        && !r.members.contains_key(&uid)
                    {
                        let link = permanent_invite(s, id, r.owner);
                        let hash = text(&link, "link").rsplit('+').next().unwrap_or("");
                        let inv = s.invites.get_mut(hash).unwrap();
                        if !inv.pending.contains(&uid) {
                            inv.pending.push(uid);
                        }
                        return Ok(
                            json!({"_":"messages.chatInviteJoinResultOk","updates":updates(vec![],vec![],vec![])}),
                        );
                    }
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .members
                        .entry(uid)
                        .or_insert_with(member);
                    Ok(json!({"_":"messages.chatInviteJoinResultOk","updates":changed(s,id,uid)}))
                }
                "channels.leaveChannel" => {
                    ensure!(uid != r.owner, "USER_CREATOR");
                    s.rooms.get_mut(&id).unwrap().members.remove(&uid);
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert(format!("left:{uid}"), json!(true));
                    Ok(changed(s, id, uid))
                }
                "channels.editTitle"
                | "messages.editChatTitle"
                | "channels.editAbout"
                | "messages.editChatAbout"
                | "channels.toggleSignatures" => {
                    admin(s, id, uid, "change_info")?;
                    let r = s.rooms.get_mut(&id).unwrap();
                    if name.ends_with("editTitle") || name.ends_with("editChatTitle") {
                        ensure!(!text(v, "title").trim().is_empty(), "CHAT_TITLE_EMPTY");
                        r.title = text(v, "title").into()
                    } else if name.ends_with("About") {
                        r.about = text(v, "about").into()
                    } else {
                        r.signatures = v["signatures_enabled"] == true;
                        r.settings.insert(name.into(), v.clone());
                    }
                    if name.ends_with("About") {
                        Ok(json!(true))
                    } else {
                        Ok(changed(s, id, uid))
                    }
                }
                "channels.deleteChannel" => {
                    ensure!(uid == r.owner, "CHAT_ADMIN_REQUIRED");
                    if let Some(deleted) = s.rooms.remove(&id) {
                        s.deleted_rooms.insert(id, deleted);
                    }
                    if let Some(link) = r.linked {
                        if let Some(other) = s.rooms.get_mut(&link) {
                            other.linked = None;
                        }
                    }
                    s.aliases
                        .retain(|_, a| a.kind != "channel" || a.owner != id);
                    s.messages.retain(|m| m.kind == "user" || m.peer != id);
                    s.invites.retain(|_, i| i.room != id);
                    Ok(updates(
                        vec![s.room(id, uid)],
                        vec![],
                        vec![json!({"_":"updateChannel","channel_id":id})],
                    ))
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
                    let filter = text(&v["filter"], "_");
                    let admin = super::channels::administrator(&r, uid);
                    if matches!(
                        filter,
                        "channelParticipantsBanned" | "channelParticipantsKicked"
                    ) || super::channels::enabled(&r, "channels.toggleParticipantsHidden")
                    {
                        ensure!(admin, "CHAT_ADMIN_REQUIRED");
                    }
                    let q = text(&v["filter"], "q").to_lowercase();
                    let pool: std::collections::BTreeSet<_> =
                        r.members.keys().chain(r.bans.keys()).copied().collect();
                    let ids: Vec<_> = pool
                        .into_iter()
                        .filter(|u| {
                            let member = r.members.contains_key(u);
                            let banned = super::banned(&r, *u, "view_messages");
                            let restricted = r.bans.get(u).is_some_and(|b| {
                                (int(b, "until_date") == 0 || int(b, "until_date") > now())
                                    && b.as_object().is_some_and(|o| o.values().any(|v| v == true))
                            });
                            let matches = match filter {
                                "channelParticipantsAdmins" => {
                                    member && super::channels::administrator(&r, *u)
                                }
                                "channelParticipantsBanned" => restricted && !banned,
                                "channelParticipantsKicked" => banned,
                                "channelParticipantsBots" => false,
                                "channelParticipantsContacts" => {
                                    member
                                        && *u != uid
                                        && s.messages.iter().any(|m| {
                                            m.kind == "user"
                                                && ((m.from == uid && m.peer == *u)
                                                    || (m.from == *u && m.peer == uid))
                                        })
                                }
                                _ => member && !banned,
                            };
                            matches
                                && (q.is_empty()
                                    || s.users.get(u).is_some_and(|p| {
                                        format!("{} {} {}", p.first, p.last, p.username)
                                            .to_lowercase()
                                            .contains(&q)
                                    }))
                        })
                        .collect();
                    Ok(
                        json!({"_":"channels.channelParticipants","count":ids.len(),"participants":ids.iter().skip(offset).take(limit).map(|i|participant(&r,*i)).collect::<Vec<_>>(),"chats":[s.room(id,uid)],"users":ids.iter().skip(offset).take(limit).map(|i|s.user(*i,uid)).collect::<Vec<_>>()}),
                    )
                }
                "channels.getParticipant" => {
                    let (_, u) = peer(&v["participant"], uid)?;
                    ensure!(s.users.contains_key(&u), "USER_ID_INVALID");
                    ensure!(
                        u == uid
                            || !super::channels::enabled(&r, "channels.toggleParticipantsHidden")
                            || super::channels::administrator(&r, uid),
                        "CHAT_ADMIN_REQUIRED"
                    );
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
                    ensure!(s.users.contains_key(&u), "USER_ID_INVALID");
                    if uid != r.owner {
                        ensure!(
                            !r.members.get(&u).is_some_and(|m| m
                                .rights
                                .as_object()
                                .is_some_and(|a| a.values().any(|v| v == true))),
                            "CHAT_ADMIN_REQUIRED"
                        );
                    }
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
                    if name == "channels.toggleSlowMode" {
                        ensure!(r.mega, "MEGAGROUP_REQUIRED");
                        ensure!(
                            [0, 10, 30, 60, 300, 900, 3600].contains(&int(v, "seconds")),
                            "SECONDS_INVALID"
                        );
                    }
                    if matches!(
                        name,
                        "channels.toggleJoinToSend"
                            | "channels.toggleJoinRequest"
                            | "channels.toggleParticipantsHidden"
                    ) {
                        ensure!(r.mega, "MEGAGROUP_REQUIRED");
                    }
                    if name.ends_with("Stickers") {
                        ensure!(
                            v["stickerset"]["_"] == "inputStickerSetEmpty",
                            "STICKERSET_INVALID"
                        );
                    }
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert(name.into(), v.clone());
                    if name.ends_with("Stickers") {
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
                "channels.readHistory" => {
                    let max = int(v, "max_id").min(r.last).max(0);
                    let key = format!("{uid}:{kind}:{id}");
                    let old = s.read.get(&key).copied().unwrap_or(0);
                    s.read.insert(key, max.max(old));
                    s.record(vec![uid],json!({"_":"updateReadChannelInbox","channel_id":id,"max_id":max.max(old),"still_unread_count":s.messages.iter().filter(|m|m.kind=="channel"&&m.peer==id&&m.id>max&&m.from!=uid).count(),"pts":r.pts}));
                    Ok(json!(true))
                }
                "channels.readMessageContents" => {
                    let ids = list(v, "id");
                    ensure!(ids.len() <= 100, "INPUT_REQUEST_TOO_LONG");
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert(format!("content_read:{uid}"), json!(ids));
                    s.record(vec![uid],json!({"_":"updateChannelReadMessagesContents","channel_id":id,"messages":ids}));
                    Ok(json!(true))
                }
                "channels.editMessage" | "channels.deleteMessages" | "channels.deleteHistory" => {
                    super::messages::handle(s, uid, v)
                }
                _ => bail!("METHOD_NOT_IMPLEMENTED"),
            }
        }
    }
}
