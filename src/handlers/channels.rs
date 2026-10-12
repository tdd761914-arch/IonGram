//! Channel RPCs and persistent projections for the local IonGram service.
use super::*;
use crate::store::Room;
pub const METHODS: &[&str] = &[
    "channels.reportSpam",
    "channels.getMessages",
    "channels.editPhoto",
    "channels.exportMessageLink",
    "channels.getLeftChannels",
    "channels.editLocation",
    "channels.getInactiveChannels",
    "channels.convertToGigagroup",
    "channels.getSendAs",
    "channels.deleteParticipantHistory",
    "channels.toggleForum",
    "channels.toggleAntiSpam",
    "channels.reportAntiSpamFalsePositive",
    "channels.updateColor",
    "channels.toggleViewForumAsMessages",
    "channels.setBoostsToUnblockRestrictions",
    "channels.restrictSponsoredMessages",
    "channels.searchPosts",
    "channels.updatePaidMessagesPrice",
    "channels.toggleAutotranslation",
    "channels.getMessageAuthor",
    "channels.checkSearchPostsFlood",
    "channels.setMainProfileTab",
    "channels.getAdminLog",
    "channels.getChannelRecommendations",
];
pub fn enabled(r: &Room, key: &str) -> bool {
    r.settings.get(key).is_some_and(|v| v["enabled"] == true)
}
pub fn administrator(r: &Room, uid: i64) -> bool {
    r.owner == uid
        || r.members.get(&uid).is_some_and(|m| {
            m.rights
                .as_object()
                .is_some_and(|a| a.values().any(|v| v == true))
        })
}
fn changed(s: &State, id: i64, uid: i64) -> Value {
    updates(
        vec![s.room(id, uid)],
        vec![s.user(uid, uid)],
        vec![json!({"_":"updateChannel","channel_id":id})],
    )
}
pub fn public(s: &State, r: &Room) -> bool {
    !r.username.is_empty()
        || s.aliases
            .values()
            .any(|a| a.kind == "channel" && a.owner == r.id && a.active)
}
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    let name = text(v, "_");
    match name {
        "channels.getLeftChannels" => {
            let key = format!("left:{uid}");
            let chats = s
                .rooms
                .values()
                .filter(|r| {
                    r.channel
                        && r.settings.get(&key).is_some_and(|v| v == true)
                        && !r.members.contains_key(&uid)
                        && public(s, r)
                })
                .skip(int(v, "offset").max(0) as usize)
                .take(100)
                .map(|r| s.room(r.id, uid))
                .collect::<Vec<_>>();
            return Ok(json!({"_":"messages.chats","chats":chats}));
        }
        "channels.getInactiveChannels" => {
            let rows: Vec<_> = s
                .rooms
                .values()
                .filter(|r| r.channel && r.members.contains_key(&uid))
                .filter_map(|r| {
                    let date = s
                        .messages
                        .iter()
                        .filter(|m| m.kind == "channel" && m.peer == r.id)
                        .map(|m| m.date)
                        .max()
                        .unwrap_or(r.date);
                    (date < now() - 30 * 86400).then_some((r.id, date))
                })
                .collect();
            return Ok(
                json!({"_":"messages.inactiveChats","chats":rows.iter().map(|(id,_)|s.room(*id,uid)).collect::<Vec<_>>(),"dates":rows.iter().map(|(_,d)|*d).collect::<Vec<_>>(),"users":[]}),
            );
        }
        "channels.getChannelRecommendations" => {
            let source = int(&v["channel"], "channel_id");
            let chats = s
                .rooms
                .values()
                .filter(|r| {
                    r.channel
                        && !r.mega
                        && r.id != source
                        && public(s, r)
                        && !r.members.contains_key(&uid)
                        && !banned(r, uid, "view_messages")
                })
                .take(20)
                .map(|r| s.room(r.id, uid))
                .collect::<Vec<_>>();
            return Ok(json!({"_":"messages.chats","chats":chats}));
        }
        "channels.checkSearchPostsFlood" => {
            return Ok(
                json!({"_":"searchPostsFlood","query_is_free":true,"total_daily":2147483647,"remains":2147483647,"stars_amount":0}),
            );
        }
        "channels.searchPosts" => {
            let q = if !text(v, "hashtag").is_empty() {
                format!("#{}", text(v, "hashtag").trim_start_matches('#'))
            } else {
                text(v, "query").trim().to_lowercase()
            };
            ensure!(!q.is_empty(), "QUERY_EMPTY");
            let mut rows: Vec<_> = s
                .messages
                .iter()
                .filter(|m| {
                    m.kind == "channel"
                        && m.action.is_none()
                        && m.text.to_lowercase().contains(&q.to_lowercase())
                        && s.rooms
                            .get(&m.peer)
                            .is_some_and(|r| public(s, r) && !banned(r, uid, "view_messages"))
                })
                .collect();
            rows.sort_by_key(|m| std::cmp::Reverse((m.date, m.id, m.peer)));
            let count = rows.len();
            if let Ok((_, id)) = peer(&v["offset_peer"], uid) {
                if let Some(pos) = rows
                    .iter()
                    .position(|m| m.peer == id && m.id == int(v, "offset_id"))
                {
                    rows.drain(..=pos);
                } else if int(v, "offset_rate") > 0 {
                    rows.retain(|m| m.date < int(v, "offset_rate"));
                }
            }
            let limit = int(v, "limit").clamp(1, 100) as usize;
            let more = rows.len() > limit;
            rows.truncate(limit);
            let channels: std::collections::BTreeSet<_> = rows.iter().map(|m| m.peer).collect();
            let users: std::collections::BTreeSet<_> = rows.iter().map(|m| m.from).collect();
            let mut out = json!({"_":"messages.messagesSlice","count":count,"messages":rows.iter().map(|m|s.message(m,uid)).collect::<Vec<_>>(),"chats":channels.iter().map(|id|s.room(*id,uid)).collect::<Vec<_>>(),"users":users.iter().map(|id|s.user(*id,uid)).collect::<Vec<_>>(),"topics":[]});
            if more {
                out["next_rate"] = json!(rows.last().map(|m| m.date).unwrap_or(0));
            }
            return Ok(out);
        }
        "channels.getSendAs" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, false)?;
            let mut peers = vec![json!({"_":"sendAsPeer","peer":{"_":"peerUser","user_id":uid}})];
            let mut chats = vec![];
            for r in s.rooms.values().filter(|r| {
                r.channel
                    && (r.owner == uid
                        || r.members
                            .get(&uid)
                            .is_some_and(|m| m.rights["post_messages"] == true))
            }) {
                peers.push(json!({"_":"sendAsPeer","peer":{"_":"peerChannel","channel_id":r.id}}));
                chats.push(s.room(r.id, uid));
            }
            return Ok(
                json!({"_":"channels.sendAsPeers","peers":peers,"chats":chats,"users":[s.user(uid,uid)]}),
            );
        }
        _ => {}
    }
    let id = int(&v["channel"], "channel_id");
    access(s, "channel", id, uid, false)?;
    let r = s.rooms.get(&id).context("CHANNEL_INVALID")?.clone();
    if name == "channels.toggleViewForumAsMessages" {
        ensure!(enabled(&r, "channels.toggleForum"), "CHANNEL_FORUM_MISSING");
        s.rooms
            .get_mut(&id)
            .unwrap()
            .settings
            .insert(format!("view_forum:{uid}"), v["enabled"].clone());
        let update =
            json!({"_":"updateChannelViewForumAsMessages","channel_id":id,"enabled":v["enabled"]});
        s.record(vec![uid], update.clone());
        return Ok(updates(vec![s.room(id, uid)], vec![], vec![update]));
    }

    match name {
        "channels.getMessages" => {
            let mut messages = vec![];
            let mut users = std::collections::BTreeSet::new();
            for input in list(v, "id") {
                let mid = if input["_"] == "inputMessagePinned" {
                    r.pinned
                } else {
                    int(&input, "id")
                };
                if let Some(m) = s.messages.iter().find(|m| {
                    m.kind == "channel"
                        && m.peer == id
                        && m.id == mid
                        && super::messages::history_visible(s, m, uid)
                }) {
                    messages.push(s.message(m, uid));
                    users.insert(m.from);
                } else {
                    messages.push(json!({"_":"messageEmpty","id":mid}));
                }
            }
            Ok(
                json!({"_":"messages.channelMessages","pts":r.pts,"count":messages.len(),"messages":messages,"chats":[s.room(id,uid)],"users":users.iter().map(|id|s.user(*id,uid)).collect::<Vec<_>>(),"topics":[]}),
            )
        }
        "channels.getMessageAuthor" => {
            admin(s, id, uid, "edit_messages")?;
            let m = s
                .messages
                .iter()
                .find(|m| m.kind == "channel" && m.peer == id && m.id == int(v, "id"))
                .context("MESSAGE_ID_INVALID")?;
            Ok(s.user(m.from, uid))
        }
        "channels.exportMessageLink" => {
            let mid = int(v, "id");
            ensure!(
                s.messages
                    .iter()
                    .any(|m| m.kind == "channel" && m.peer == id && m.id == mid),
                "MESSAGE_ID_INVALID"
            );
            let path = if r.username.is_empty() {
                format!("c/{id}")
            } else {
                r.username.clone()
            };
            let suffix = if v["thread"] == true {
                format!("?thread={mid}")
            } else {
                String::new()
            };
            let link = format!("https://iongram.local/{path}/{mid}{suffix}");
            Ok(
                json!({"_":"exportedMessageLink","link":link,"html":format!("<a href=\"{link}\">Message</a>")}),
            )
        }
        "channels.reportSpam" | "channels.reportAntiSpamFalsePositive" => {
            ensure!(r.members.contains_key(&uid), "USER_NOT_PARTICIPANT");
            let ids = if name.ends_with("FalsePositive") {
                admin(s, id, uid, "ban_users")?;
                vec![json!(int(v, "msg_id"))]
            } else {
                list(v, "id")
            };
            ensure!(!ids.is_empty() && ids.len() <= 100, "MESSAGE_ID_INVALID");
            for mid in &ids {
                ensure!(
                    s.messages.iter().any(|m| m.kind == "channel"
                        && m.peer == id
                        && m.id == mid.as_i64().unwrap_or(0)),
                    "MESSAGE_ID_INVALID"
                );
            }
            let report_id = s.id();
            s.channel_reports.entry(id).or_default().push(json!({"id":report_id,"reporter":uid,"date":now(),"method":name,"messages":ids,"participant":v["participant"]}));
            Ok(json!(true))
        }
        "channels.deleteParticipantHistory" => {
            admin(s, id, uid, "delete_messages")?;
            let (_, target) = peer(&v["participant"], uid)?;
            ensure!(target != r.owner || target == uid, "USER_CREATOR");
            let ids: Vec<_> = s
                .messages
                .iter()
                .filter(|m| m.kind == "channel" && m.peer == id && m.from == target)
                .map(|m| m.id)
                .collect();
            s.messages
                .retain(|m| !(m.kind == "channel" && m.peer == id && m.from == target));
            if !ids.is_empty() {
                let room = s.rooms.get_mut(&id).unwrap();
                room.pts += 1;
                room.pinned = if ids.contains(&room.pinned) {
                    0
                } else {
                    room.pinned
                };
                let pts = room.pts;
                s.record(r.members.keys().copied().collect(),json!({"_":"updateDeleteChannelMessages","channel_id":id,"messages":ids,"pts":pts,"pts_count":1}));
            }
            Ok(
                json!({"_":"messages.affectedHistory","pts":s.rooms[&id].pts,"pts_count":if ids.is_empty(){0}else{1},"offset":0}),
            )
        }
        "channels.getAdminLog" => {
            ensure!(administrator(&r, uid), "CHAT_ADMIN_REQUIRED");
            let admins: Vec<_> = list(v, "admins")
                .iter()
                .filter_map(|u| peer(u, uid).ok().map(|(_, id)| id))
                .collect();
            let q = text(v, "q").to_lowercase();
            let events: Vec<_> = s
                .channel_logs
                .get(&id)
                .into_iter()
                .flatten()
                .rev()
                .filter(|e| {
                    (int(v, "max_id") == 0 || int(e, "id") < int(v, "max_id"))
                        && (int(v, "min_id") == 0 || int(e, "id") > int(v, "min_id"))
                        && (admins.is_empty() || admins.contains(&int(e, "user_id")))
                        && (q.is_empty() || e.to_string().to_lowercase().contains(&q))
                        && log_filter(&v["events_filter"], &e["action"])
                })
                .take(int(v, "limit").clamp(1, 100) as usize)
                .cloned()
                .collect();
            let users: std::collections::BTreeSet<_> =
                events.iter().map(|e| int(e, "user_id")).collect();
            Ok(
                json!({"_":"channels.adminLogResults","events":events,"chats":[s.room(id,uid)],"users":users.iter().map(|id|s.user(*id,uid)).collect::<Vec<_>>()}),
            )
        }
        "channels.editPhoto" => {
            admin(s, id, uid, "change_info")?;
            let photo = &v["photo"];
            ensure!(
                matches!(text(photo, "_"), "photo" | "photoEmpty"),
                "PHOTO_INVALID"
            );
            s.rooms
                .get_mut(&id)
                .unwrap()
                .settings
                .insert("photo".into(), photo.clone());
            Ok(changed(s, id, uid))
        }
        _ => {
            admin(s, id, uid, "change_info")?;
            let mut value = v.clone();
            match name {
                "channels.editLocation" => {
                    ensure!(r.mega, "MEGAGROUP_REQUIRED");
                    let g = &v["geo_point"];
                    let location = if g["_"] == "inputGeoPointEmpty" {
                        json!({"_":"channelLocationEmpty"})
                    } else {
                        let lat = g["lat"].as_f64().context("GEO_POINT_INVALID")?;
                        let long = g["long"].as_f64().context("GEO_POINT_INVALID")?;
                        ensure!(
                            lat.is_finite()
                                && long.is_finite()
                                && (-90.0..=90.0).contains(&lat)
                                && (-180.0..=180.0).contains(&long),
                            "GEO_POINT_INVALID"
                        );
                        ensure!(!text(v, "address").trim().is_empty(), "ADDRESS_INVALID");
                        json!({"_":"channelLocation","geo_point":{"_":"geoPoint","lat":lat,"long":long,"access_hash":0},"address":v["address"]})
                    };
                    s.rooms
                        .get_mut(&id)
                        .unwrap()
                        .settings
                        .insert("location".into(), location);
                }
                "channels.convertToGigagroup" => {
                    ensure!(r.owner == uid && r.mega, "CHAT_ADMIN_REQUIRED");
                    value = json!({"enabled":true});
                }
                "channels.toggleForum" => {
                    ensure!(r.owner == uid && r.mega, "CHAT_ADMIN_REQUIRED");
                    ensure!(r.linked.is_none(), "CHAT_DISCUSSION_UNALLOWED");
                }
                "channels.toggleViewForumAsMessages" => ensure!(
                    r.settings
                        .get("channels.toggleForum")
                        .is_some_and(|v| v["enabled"] == true),
                    "CHANNEL_FORUM_MISSING"
                ),
                "channels.toggleAntiSpam" | "channels.setBoostsToUnblockRestrictions" => {
                    ensure!(r.mega, "MEGAGROUP_REQUIRED");
                    if name.ends_with("Restrictions") {
                        ensure!((0..=100).contains(&int(v, "boosts")), "BOOSTS_INVALID");
                    }
                }
                "channels.updateColor" => {
                    if !v["color"].is_null() {
                        ensure!((0..=255).contains(&int(v, "color")), "COLOR_INVALID");
                    }
                    let mut color = json!({"_":"peerColor"});
                    if !v["color"].is_null() {
                        color["color"] = v["color"].clone();
                    }
                    if !v["background_emoji_id"].is_null() {
                        color["background_emoji_id"] = v["background_emoji_id"].clone();
                    }
                    s.rooms.get_mut(&id).unwrap().settings.insert(
                        if v["for_profile"] == true {
                            "profile_color"
                        } else {
                            "color"
                        }
                        .into(),
                        color,
                    );
                }
                "channels.updatePaidMessagesPrice" => ensure!(
                    (0..=1000000).contains(&int(v, "send_paid_messages_stars")),
                    "STARS_AMOUNT_INVALID"
                ),
                "channels.setMainProfileTab" => ensure!(
                    text(&v["tab"], "_").starts_with("profileTab"),
                    "TAB_INVALID"
                ),
                "channels.restrictSponsoredMessages" => ensure!(!r.mega, "BROADCAST_REQUIRED"),
                "channels.toggleAutotranslation" => {}
                _ => bail!("METHOD_NOT_IMPLEMENTED"),
            }
            s.rooms
                .get_mut(&id)
                .unwrap()
                .settings
                .insert(name.into(), value);
            if matches!(name, "channels.editLocation" | "channels.setMainProfileTab") {
                Ok(json!(true))
            } else {
                Ok(changed(s, id, uid))
            }
        }
    }
}

pub fn project_room(r: &Room, v: &mut Value) {
    for (method, field) in [
        ("channels.toggleForum", "forum"),
        ("channels.convertToGigagroup", "gigagroup"),
        ("channels.toggleJoinToSend", "join_to_send"),
        ("channels.toggleJoinRequest", "join_request"),
        ("channels.toggleAutotranslation", "autotranslation"),
    ] {
        if let Some(setting) = r.settings.get(method) {
            v[field] = setting["enabled"].clone();
        }
    }
    if let Some(setting) = r.settings.get("channels.toggleForum") {
        v["forum_tabs"] = setting["tabs"].clone();
    }
    if let Some(setting) = r.settings.get("channels.toggleSignatures") {
        v["signature_profiles"] = setting["profiles_enabled"].clone();
    }
    for field in ["color", "profile_color"] {
        if let Some(value) = r.settings.get(field) {
            v[field] = value.clone();
        }
    }
    if let Some(value) = r.settings.get("channels.updateEmojiStatus") {
        v["emoji_status"] = value["emoji_status"].clone();
    }
    if let Some(value) = r.settings.get("channels.updatePaidMessagesPrice") {
        v["send_paid_messages_stars"] = value["send_paid_messages_stars"].clone();
        v["broadcast_messages_allowed"] = value["broadcast_messages_allowed"].clone();
    }
    if let Some(photo) = r.settings.get("photo").filter(|p| p["_"] == "photo") {
        v["photo"] = json!({"_":"chatPhoto","photo_id":photo["id"],"dc_id":2});
    }
}
pub fn project_full(r: &Room, v: &mut Value) {
    for (method, field) in [
        ("channels.togglePreHistoryHidden", "hidden_prehistory"),
        ("channels.toggleAntiSpam", "antispam"),
        ("channels.toggleParticipantsHidden", "participants_hidden"),
        (
            "channels.toggleViewForumAsMessages",
            "view_forum_as_messages",
        ),
    ] {
        if let Some(value) = r.settings.get(method) {
            v[field] = value["enabled"].clone();
        }
    }
    if let Some(value) = r.settings.get("channels.restrictSponsoredMessages") {
        v["restricted_sponsored"] = value["restricted"].clone();
    }
    if let Some(value) = r.settings.get("channels.toggleSlowMode") {
        v["slowmode_seconds"] = value["seconds"].clone();
    }
    if let Some(value) = r.settings.get("channels.setBoostsToUnblockRestrictions") {
        v["boosts_unrestrict"] = value["boosts"].clone();
    }
    if let Some(value) = r.settings.get("channels.setMainProfileTab") {
        v["main_tab"] = value["tab"].clone();
    }
    if let Some(value) = r.settings.get("channels.updatePaidMessagesPrice") {
        v["send_paid_messages_stars"] = value["send_paid_messages_stars"].clone();
    }
    if let Some(value) = r.settings.get("location") {
        v["location"] = value.clone();
    }
    if let Some(value) = r.settings.get("photo") {
        v["chat_photo"] = value.clone();
    }
}
pub fn audit(s: &mut State, uid: i64, old: &Room, new: &Room) {
    let mut actions = vec![];
    for (a, b, kind) in [
        (
            &old.title,
            &new.title,
            "channelAdminLogEventActionChangeTitle",
        ),
        (
            &old.about,
            &new.about,
            "channelAdminLogEventActionChangeAbout",
        ),
        (
            &old.username,
            &new.username,
            "channelAdminLogEventActionChangeUsername",
        ),
    ] {
        if a != b {
            actions.push(json!({"_":kind,"prev_value":a,"new_value":b}));
        }
    }
    for (method, kind) in [
        (
            "channels.toggleForum",
            "channelAdminLogEventActionToggleForum",
        ),
        (
            "channels.toggleAntiSpam",
            "channelAdminLogEventActionToggleAntiSpam",
        ),
        (
            "channels.togglePreHistoryHidden",
            "channelAdminLogEventActionTogglePreHistoryHidden",
        ),
        (
            "channels.toggleAutotranslation",
            "channelAdminLogEventActionToggleAutotranslation",
        ),
    ] {
        if old.settings.get(method) != new.settings.get(method) {
            actions.push(json!({"_":kind,"new_value":new.settings.get(method).is_some_and(|v|v["enabled"]==true)}));
        }
    }
    if old.signatures != new.signatures {
        actions.push(
            json!({"_":"channelAdminLogEventActionToggleSignatures","new_value":new.signatures}),
        );
    }
    if old.linked != new.linked {
        actions.push(json!({"_":"channelAdminLogEventActionChangeLinkedChat","prev_value":old.linked.unwrap_or(0),"new_value":new.linked.unwrap_or(0)}));
    }
    if old.settings.get("channels.toggleSlowMode") != new.settings.get("channels.toggleSlowMode") {
        actions.push(json!({"_":"channelAdminLogEventActionToggleSlowMode","prev_value":old.settings.get("channels.toggleSlowMode").map(|v|int(v,"seconds")).unwrap_or(0),"new_value":new.settings.get("channels.toggleSlowMode").map(|v|int(v,"seconds")).unwrap_or(0)}));
    }
    if old.settings.get("photo") != new.settings.get("photo") {
        actions.push(json!({"_":"channelAdminLogEventActionChangePhoto","prev_photo":old.settings.get("photo").cloned().unwrap_or(json!({"_":"photoEmpty","id":0})),"new_photo":new.settings.get("photo").cloned().unwrap_or(json!({"_":"photoEmpty","id":0}))}));
    }
    let people: std::collections::BTreeSet<_> = old
        .members
        .keys()
        .chain(new.members.keys())
        .chain(old.bans.keys())
        .chain(new.bans.keys())
        .copied()
        .collect();
    for person in people {
        let prev = super::rooms::participant(old, person);
        let next = super::rooms::participant(new, person);
        if prev == next {
            continue;
        }
        let action = if old.bans.get(&person) != new.bans.get(&person) {
            json!({"_":"channelAdminLogEventActionParticipantToggleBan","prev_participant":prev,"new_participant":next})
        } else if !old.members.contains_key(&person) && new.members.contains_key(&person) {
            json!({"_":"channelAdminLogEventActionParticipantInvite","participant":next})
        } else if old.members.contains_key(&person) && !new.members.contains_key(&person) {
            json!({"_":"channelAdminLogEventActionParticipantLeave"})
        } else {
            json!({"_":"channelAdminLogEventActionParticipantToggleAdmin","prev_participant":prev,"new_participant":next})
        };
        actions.push(action);
    }
    for action in actions {
        let event_id = s.id();
        s.channel_logs.entry(new.id).or_default().push(json!({"_":"channelAdminLogEvent","id":event_id,"date":now(),"user_id":uid,"action":action}));
    }
}

fn log_filter(filter: &Value, action: &Value) -> bool {
    if filter.is_null() {
        return true;
    }
    let name = text(action, "_");
    let keys: &[&str] = if name.contains("ToggleBan") {
        &["ban", "unban", "kick", "unkick"]
    } else if name.contains("ToggleAdmin") {
        &["promote", "demote"]
    } else if name.contains("ParticipantInvite") {
        &["invite", "join"]
    } else if name.contains("ParticipantLeave") {
        &["leave"]
    } else if name.contains("Forum") || name.contains("Topic") {
        &["forums"]
    } else if name.contains("ChangeTitle")
        || name.contains("ChangeAbout")
        || name.contains("ChangePhoto")
        || name.contains("ChangeUsername")
    {
        &["info"]
    } else if name.contains("DeleteMessage") {
        &["delete"]
    } else if name.contains("EditMessage") {
        &["edit"]
    } else if name.contains("SendMessage") {
        &["send"]
    } else {
        &["settings"]
    };
    keys.iter().any(|k| filter[*k] == true)
}

pub fn log_action(s: &mut State, channel: i64, uid: i64, action: Value) {
    let id = s.id();
    let events = s.channel_logs.entry(channel).or_default();
    events.push(
        json!({"_":"channelAdminLogEvent","id":id,"date":now(),"user_id":uid,"action":action}),
    );
}
