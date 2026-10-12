use super::*;
use crate::store::{Event, Message};
fn visible(m: &Message, kind: &str, id: i64, uid: i64) -> bool {
    m.kind == kind
        && if kind == "user" {
            (m.from == uid && m.peer == id) || (m.from == id && m.peer == uid)
        } else {
            m.peer == id
        }
}
pub fn history_visible(s: &State, m: &Message, uid: i64) -> bool {
    if m.kind == "user" {
        return m.from == uid || m.peer == uid;
    }
    let Some(r) = s.rooms.get(&m.peer) else {
        return false;
    };
    if banned(r, uid, "view_messages") {
        return false;
    }
    if m.id
        <= r.settings
            .get(&format!("history_min:{uid}"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    {
        return false;
    }
    if super::channels::enabled(r, "channels.togglePreHistoryHidden")
        && !super::channels::administrator(r, uid)
    {
        return r
            .members
            .get(&uid)
            .is_some_and(|member| m.date >= member.date);
    }
    true
}
fn response(s: &State, uid: i64, kind: &str, id: i64, msgs: Vec<&Message>, count: usize) -> Value {
    json!({"_":if kind=="channel"{"messages.channelMessages"}else{"messages.messagesSlice"},"pts":s.rooms.get(&id).map(|r|r.pts).unwrap_or(s.pts),"count":count,"messages":msgs.iter().map(|m|s.message(m,uid)).collect::<Vec<_>>(),"topics":[],"chats":if kind=="user"{vec![]}else{vec![s.room(id,uid)]},"users":s.users.keys().filter(|u|msgs.iter().any(|m|m.from==**u)).map(|u|s.user(*u,uid)).collect::<Vec<_>>()})
}
pub fn publish(s: &mut State, uid: i64, m: &Message, edit: bool) -> Value {
    if m.kind == "channel" && !edit && m.action.is_none() {
        let action =
            json!({"_":"channelAdminLogEventActionSendMessage","message":s.message(m,uid)});
        super::channels::log_action(s, m.peer, uid, action);
    }
    let pts = if m.kind == "channel" {
        let r = s.rooms.get_mut(&m.peer).unwrap();
        r.pts += 1;
        r.pts
    } else {
        s.user_pts.get(&uid).copied().unwrap_or(0) + 1
    };
    let users = if m.kind == "user" {
        vec![uid, m.peer]
    } else {
        s.rooms[&m.peer].members.keys().copied().collect()
    };
    let update = json!({"_":match(m.kind=="channel",edit){(true,true)=>"updateEditChannelMessage",(true,false)=>"updateNewChannelMessage",(false,true)=>"updateEditMessage",(false,false)=>"updateNewMessage"},"message":s.message(m,uid),"pts":pts,"pts_count":1});
    s.record(users, update.clone());
    let update = event_view(s.events.last().unwrap(), uid);
    updates(
        if m.kind == "user" {
            vec![]
        } else {
            vec![s.room(m.peer, uid)]
        },
        vec![s.user(uid, uid)],
        vec![update],
    )
}
pub fn creation_message(s: &mut State, room_id: i64) -> Option<Value> {
    let r = s.rooms.get(&room_id)?.clone();
    if r.last != 0
        || s.messages
            .iter()
            .any(|m| m.peer == room_id && m.kind != "user")
    {
        return None;
    }
    let id = if r.channel { 1 } else { s.id() };
    s.rooms.get_mut(&room_id)?.last = id;
    let action = if r.channel {
        json!({"_":"messageActionChannelCreate","title":r.title})
    } else {
        json!({"_":"messageActionChatCreate","title":r.title,"users":r.members.keys().copied().collect::<Vec<_>>()})
    };
    let message = Message {
        id,
        from: r.owner,
        kind: if r.channel {
            "channel".into()
        } else {
            "chat".into()
        },
        peer: room_id,
        date: r.date,
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
    Some(publish(s, r.owner, &message, false))
}
fn send(s: &mut State, uid: i64, v: &Value, forward: Option<Value>) -> Result<Value> {
    let (kind, id) = peer(&v["peer"], uid)?;
    access(s, &kind, id, uid, true)?;
    ensure!(
        text(v, "message").chars().count() <= 4096,
        "MESSAGE_TOO_LONG"
    );
    ensure!(
        !text(v, "message").is_empty() || !v["media"].is_null(),
        "MESSAGE_EMPTY"
    );
    ensure!(v["schedule_date"].is_null(), "SCHEDULE_NOT_SUPPORTED");
    let random = int(v, "random_id");
    let token = format!("{uid}:{random}");
    if let Some(mid) = s.randoms.get(&token) {
        let m = s
            .messages
            .iter()
            .find(|m| m.id == *mid && visible(m, &kind, id, uid))
            .context("RANDOM_ID_DUPLICATE")?;
        return Ok(updates(
            vec![],
            vec![],
            vec![json!({"_":"updateMessageID","id":m.id,"random_id":random})],
        ));
    }
    if kind != "user" {
        let r = &s.rooms[&id];
        if !super::channels::administrator(r, uid) {
            let right = match text(&v["media"], "_") {
                "inputMediaPhoto" | "messageMediaPhoto" => "send_photos",
                "inputMediaDocument" | "messageMediaDocument" => "send_docs",
                _ => "send_plain",
            };
            ensure!(
                !banned(r, uid, right)
                    && r.settings
                        .get("default_banned_rights")
                        .is_none_or(|b| b[right] != true),
                "CHAT_SEND_MEDIA_FORBIDDEN"
            );
            let slow = r
                .settings
                .get("channels.toggleSlowMode")
                .map(|v| int(v, "seconds"))
                .unwrap_or(0);
            if slow > 0 {
                if let Some(last) =
                    s.messages.iter().rev().find(|m| {
                        m.kind == kind && m.peer == id && m.from == uid && m.action.is_none()
                    })
                {
                    let remaining = last.date + slow - now();
                    if remaining > 0 {
                        bail!("SLOWMODE_WAIT_{}", remaining);
                    }
                }
            }
            if super::channels::enabled(r, "channels.toggleAntiSpam")
                && !text(v, "message").is_empty()
            {
                ensure!(
                    s.messages
                        .iter()
                        .rev()
                        .filter(|m| m.kind == kind
                            && m.peer == id
                            && m.from == uid
                            && m.date > now() - 60
                            && m.text == text(v, "message"))
                        .count()
                        < 3,
                    "FLOOD_WAIT_60"
                );
            }
            let price = r
                .settings
                .get("channels.updatePaidMessagesPrice")
                .map(|v| int(v, "send_paid_messages_stars"))
                .unwrap_or(0);
            let owner = r.owner;
            if price > 0 {
                ensure!(
                    int(v, "allow_paid_stars") >= price,
                    "ALLOW_PAYMENT_REQUIRED"
                );
                s.credit(uid, -price, "paid channel message")?;
                s.credit(owner, price, "paid channel message income")?;
            }
        }
    }
    let send_as = if !v["send_as"].is_null() {
        let (k, target) = peer(&v["send_as"], uid)?;
        if k == "user" {
            ensure!(target == uid, "SEND_AS_PEER_INVALID");
            None
        } else {
            ensure!(k == "channel", "SEND_AS_PEER_INVALID");
            admin(s, target, uid, "post_messages")?;
            Some(json!({"_":"peerChannel","channel_id":target}))
        }
    } else {
        None
    };
    let topic =
        if kind == "channel" && super::channels::enabled(&s.rooms[&id], "channels.toggleForum") {
            super::topics::ensure_general(s, id);
            let mut top = int(&v["reply_to"], "top_msg_id");
            if top == 0 {
                let reply = int(&v["reply_to"], "reply_to_msg_id");
                top = s
                    .message_topics
                    .get(&format!("{id}:{reply}"))
                    .copied()
                    .unwrap_or(1);
            }
            let t = super::topics::topic(s, id, top).context("TOPIC_ID_INVALID")?;
            ensure!(
                t["closed"] != true || super::channels::administrator(&s.rooms[&id], uid),
                "TOPIC_CLOSED"
            );
            Some(top)
        } else {
            None
        };
    let mid = if kind == "channel" {
        let r = s.rooms.get_mut(&id).unwrap();
        r.last += 1;
        r.last
    } else {
        s.id()
    };
    let media = if v["media"].is_null() {
        None
    } else {
        Some(super::files::media(s, uid, &v["media"])?)
    };
    let reply = int(&v["reply_to"], "reply_to_msg_id");
    if reply != 0 {
        ensure!(
            s.messages
                .iter()
                .any(|m| m.id == reply && visible(m, &kind, id, uid)),
            "REPLY_MESSAGE_ID_INVALID"
        )
    };
    let m = Message {
        id: mid,
        from: uid,
        kind,
        peer: id,
        date: now(),
        text: text(v, "message").into(),
        media,
        entities: list(v, "entities"),
        reply: (reply > 0).then_some(reply),
        forward,
        edit: None,
        action: None,
        pinned: false,
    };
    if let Some(top) = topic {
        s.message_topics.insert(format!("{id}:{mid}"), top);
        s.rooms
            .get_mut(&id)
            .unwrap()
            .settings
            .get_mut(&format!("topic:{top}"))
            .unwrap()["top_message"] = json!(mid);
    }
    if let Some(sender) = send_as {
        s.message_senders
            .insert(format!("{}:{}:{}", m.kind, m.peer, m.id), sender);
    }
    s.messages.push(m.clone());
    s.randoms.insert(token, mid);
    let mut out = publish(s, uid, &m, false);
    out["updates"]
        .as_array_mut()
        .unwrap()
        .push(json!({"_":"updateMessageID","id":mid,"random_id":random}));
    Ok(out)
}
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    let name = text(v, "_");
    match name {
        "messages.getMessagesViews" => {
            let (k, id) = peer(&v["peer"], uid)?;
            ensure!(k == "channel", "CHANNEL_INVALID");
            access(s, &k, id, uid, false)?;
            let ids = list(v, "id");
            ensure!(ids.len() <= 100, "INPUT_REQUEST_TOO_LONG");
            let mut views = vec![];
            for mid in ids {
                let mid = mid.as_i64().context("MESSAGE_ID_INVALID")?;
                ensure!(
                    s.messages.iter().any(|m| m.kind == k
                        && m.peer == id
                        && m.id == mid
                        && history_visible(s, m, uid)),
                    "MESSAGE_ID_INVALID"
                );
                let r = s.rooms.get_mut(&id).unwrap();
                let seen = r
                    .settings
                    .entry(format!("views:{mid}"))
                    .or_insert(json!([]))
                    .as_array_mut()
                    .unwrap();
                if v["increment"] == true && !seen.contains(&json!(uid)) {
                    seen.push(json!(uid));
                }
                views.push(json!({"_":"messageViews","views":seen.len(),"forwards":0}));
            }
            Ok(
                json!({"_":"messages.messageViews","views":views,"chats":[s.room(id,uid)],"users":[]}),
            )
        }
        "messages.unpinAllMessages" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, false)?;
            if k != "user" {
                admin(s, id, uid, "pin_messages")?;
            }
            let topic = int(v, "top_msg_id");
            let mut ids = vec![];
            for m in &mut s.messages {
                if visible(m, &k, id, uid)
                    && m.pinned
                    && (topic == 0
                        || s.message_topics.get(&format!("{}:{}", m.peer, m.id)) == Some(&topic))
                {
                    m.pinned = false;
                    ids.push(m.id);
                }
            }
            let pts = if k == "channel" {
                let r = s.rooms.get_mut(&id).unwrap();
                if ids.contains(&r.pinned) {
                    r.pinned = 0;
                }
                if !ids.is_empty() {
                    r.pts += 1;
                }
                r.pts
            } else {
                s.user_pts.get(&uid).copied().unwrap_or(0)
            };
            if k == "channel" && !ids.is_empty() {
                let users = s.rooms[&id].members.keys().copied().collect();
                s.record(users,json!({"_":"updatePinnedChannelMessages","channel_id":id,"messages":ids,"pinned":false,"pts":pts,"pts_count":1}));
            }
            Ok(
                json!({"_":"messages.affectedHistory","pts":pts,"pts_count":if ids.is_empty(){0}else{1},"offset":0}),
            )
        }
        "messages.readMentions" | "messages.readReactions" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, false)?;
            if k != "user" {
                let last = s.rooms[&id].last;
                s.rooms.get_mut(&id).unwrap().settings.insert(
                    format!("{}:{uid}:{}", name, int(v, "top_msg_id")),
                    json!(last),
                );
            }
            Ok(
                json!({"_":"messages.affectedHistory","pts":s.user_pts.get(&uid).copied().unwrap_or(0),"pts_count":0,"offset":0}),
            )
        }
        "messages.getPinnedDialogs" => Ok(
            json!({"_":"messages.peerDialogs","dialogs":[],"messages":[],"chats":[],"users":[],"state":state(s,uid)}),
        ),
        "messages.sendMessage" | "messages.sendMedia" => send(s, uid, v, None),
        "messages.forwardMessages" => {
            let (k, id) = peer(&v["from_peer"], uid)?;
            access(s, &k, id, uid, false)?;
            if k != "user" {
                ensure!(
                    !s.rooms[&id]
                        .settings
                        .get("noforwards")
                        .is_some_and(|v| v == true),
                    "CHAT_FORWARDS_RESTRICTED"
                );
            }
            let mids = list(v, "id");
            let randoms = list(v, "random_id");
            ensure!(
                mids.len() == randoms.len() && mids.len() <= 100,
                "INPUT_REQUEST_TOO_LONG"
            );
            let mut out = updates(vec![], vec![], vec![]);
            for (mid, random) in mids.iter().zip(randoms) {
                let m = s
                    .messages
                    .iter()
                    .find(|m| {
                        m.id == mid.as_i64().unwrap_or(0)
                            && visible(m, &k, id, uid)
                            && history_visible(s, m, uid)
                    })
                    .context("MESSAGE_ID_INVALID")?
                    .clone();
                let req = json!({"peer":v["to_peer"],"message":m.text,"random_id":random,"media":m.media,"entities":m.entities});
                let r = send(
                    s,
                    uid,
                    &req,
                    Some(
                        json!({"_":"messageFwdHeader","date":m.date,"from_id":{"_":"peerUser","user_id":m.from}}),
                    ),
                )?;
                out["updates"]
                    .as_array_mut()
                    .unwrap()
                    .extend(list(&r, "updates"));
                out["chats"] = r["chats"].clone();
                out["users"] = r["users"].clone();
            }
            Ok(out)
        }
        "messages.uploadMedia" => super::files::media(s, uid, &v["media"]),
        "messages.searchGlobal" => {
            ensure!(v["community"].is_null(), "COMMUNITY_INVALID");
            ensure!(int(v, "folder_id") == 0, "FOLDER_ID_INVALID");
            ensure!(
                matches!(text(&v["filter"], "_"), "" | "inputMessagesFilterEmpty"),
                "SEARCH_FILTER_NOT_SUPPORTED"
            );
            let q = text(v, "q").trim().to_lowercase();
            let offset = peer(&v["offset_peer"], uid).ok();
            let rate = int(v, "offset_rate");
            let offset_id = int(v, "offset_id");
            let mut all: Vec<_> = s
                .messages
                .iter()
                .filter(|m| {
                    let allowed = if m.kind == "user" {
                        m.from == uid || m.peer == uid
                    } else {
                        s.rooms.get(&m.peer).is_some_and(|r| {
                            r.members.contains_key(&uid) && !banned(r, uid, "view_messages")
                        })
                    };
                    let room = s.rooms.get(&m.peer).filter(|_| m.kind != "user");
                    allowed
                        && history_visible(s, m, uid)
                        && m.action.is_none()
                        && (q.is_empty() || m.text.to_lowercase().contains(&q))
                        && (int(v, "min_date") == 0 || m.date >= int(v, "min_date"))
                        && (int(v, "max_date") == 0 || m.date <= int(v, "max_date"))
                        && (v["users_only"] != true || m.kind == "user")
                        && (v["broadcasts_only"] != true
                            || room.is_some_and(|r| r.channel && !r.mega))
                        && (v["groups_only"] != true || room.is_some_and(|r| !r.channel || r.mega))
                })
                .collect();
            all.sort_by_key(|m| std::cmp::Reverse((m.date, m.id, m.peer)));
            let count = all.len();
            if let Some((kind, id)) = offset {
                if let Some(index) = all
                    .iter()
                    .position(|m| m.id == offset_id && visible(m, &kind, id, uid))
                {
                    all.drain(..=index);
                } else if rate > 0 {
                    all.retain(|m| m.date < rate);
                }
            } else if rate > 0 {
                all.retain(|m| m.date < rate);
            }
            let limit = int(v, "limit").clamp(1, 100) as usize;
            let more = all.len() > limit;
            all.truncate(limit);
            let mut user_ids = std::collections::BTreeSet::from([uid]);
            let mut room_ids = std::collections::BTreeSet::new();
            for m in &all {
                user_ids.insert(m.from);
                if m.kind == "user" {
                    user_ids.insert(m.peer);
                } else {
                    room_ids.insert(m.peer);
                }
            }
            let mut out = json!({"_":"messages.messagesSlice","count":count,"messages":all.iter().map(|m|s.message(m,uid)).collect::<Vec<_>>(),"chats":room_ids.iter().map(|id|s.room(*id,uid)).collect::<Vec<_>>(),"users":user_ids.iter().map(|id|s.user(*id,uid)).collect::<Vec<_>>(),"topics":[]});
            if more {
                if let Some(m) = all.last() {
                    out["next_rate"] = json!(m.date);
                }
            }
            Ok(out)
        }
        "messages.getHistory" | "messages.search" | "messages.getReplies" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, false)?;
            let q = text(v, "q").to_lowercase();
            let offset = int(v, "offset_id");
            let min = int(v, "min_id");
            let max = int(v, "max_id");
            let date = int(v, "offset_date");
            let thread = int(v, "msg_id");
            let all: Vec<_> = s
                .messages
                .iter()
                .rev()
                .filter(|m| {
                    visible(m, &k, id, uid)
                        && history_visible(s, m, uid)
                        && (name != "messages.getReplies"
                            || m.reply == Some(thread)
                            || s.message_topics.get(&format!("{}:{}", m.peer, m.id))
                                == Some(&thread))
                        && (q.is_empty() || m.text.to_lowercase().contains(&q))
                        && (offset == 0 || m.id < offset)
                        && (min == 0 || m.id > min)
                        && (max == 0 || m.id < max)
                        && (date == 0 || m.date < date)
                })
                .collect();
            let count = all.len();
            let page = all
                .into_iter()
                .skip(int(v, "add_offset").max(0) as usize)
                .take(int(v, "limit").clamp(1, 100) as usize)
                .collect();
            Ok(response(s, uid, &k, id, page, count))
        }
        "messages.editMessage" | "channels.editMessage" => {
            let (k, id) = if name.starts_with("channels") {
                ("channel".into(), int(&v["channel"], "channel_id"))
            } else {
                peer(&v["peer"], uid)?
            };
            access(s, &k, id, uid, true)?;
            let idx = s
                .messages
                .iter()
                .position(|m| m.id == int(v, "id") && visible(m, &k, id, uid))
                .context("MESSAGE_ID_INVALID")?;
            if s.messages[idx].from != uid {
                admin(s, id, uid, "edit_messages")?
            }
            let media = if v["media"].is_null() {
                None
            } else {
                Some(super::files::media(s, uid, &v["media"])?)
            };
            let previous = s.message(&s.messages[idx], uid);
            let m = &mut s.messages[idx];
            if let Some(t) = v["message"].as_str() {
                ensure!(t.chars().count() <= 4096, "MESSAGE_TOO_LONG");
                m.text = t.into()
            }
            if media.is_some() {
                m.media = media
            }
            if v["entities"].is_array() {
                m.entities = list(v, "entities")
            }
            m.edit = Some(now());
            let m = m.clone();
            if k == "channel" {
                let action = json!({"_":"channelAdminLogEventActionEditMessage","prev_message":previous,"new_message":s.message(&m,uid)});
                super::channels::log_action(s, id, uid, action);
            }
            Ok(publish(s, uid, &m, true))
        }
        "messages.updatePinnedMessage" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, false)?;
            if k != "user" {
                admin(s, id, uid, "pin_messages")?
            }
            let mid = int(v, "id");
            let m = s
                .messages
                .iter_mut()
                .find(|m| m.id == mid && visible(m, &k, id, uid))
                .context("MESSAGE_ID_INVALID")?;
            m.pinned = v["unpin"] != true;
            if k != "user" {
                s.rooms.get_mut(&id).unwrap().pinned = if m.pinned { mid } else { 0 }
            }
            let m = m.clone();
            if k == "channel" {
                let r = s.rooms.get_mut(&id).unwrap();
                r.pts += 1;
                let update = json!({"_":"updatePinnedChannelMessages","channel_id":id,"messages":[mid],"pinned":v["unpin"]!=true,"pts":r.pts,"pts_count":1});
                let users = r.members.keys().copied().collect();
                s.record(users, update.clone());
                Ok(updates(vec![s.room(id, uid)], vec![], vec![update]))
            } else {
                Ok(publish(s, uid, &m, true))
            }
        }
        "messages.deleteMessages" | "channels.deleteMessages" => {
            let ids: Vec<i64> = list(v, "id").iter().filter_map(Value::as_i64).collect();
            ensure!(ids.len() <= 100, "INPUT_REQUEST_TOO_LONG");
            let channel = int(&v["channel"], "channel_id");
            if channel != 0 {
                access(s, "channel", channel, uid, false)?;
                let may_delete = admin(s, channel, uid, "delete_messages").is_ok();
                ensure!(
                    s.messages
                        .iter()
                        .filter(|m| m.kind == "channel" && m.peer == channel && ids.contains(&m.id))
                        .all(|m| may_delete || m.from == uid),
                    "MESSAGE_DELETE_FORBIDDEN"
                );
                let deleted: Vec<_> = s
                    .messages
                    .iter()
                    .filter(|m| m.kind == "channel" && m.peer == channel && ids.contains(&m.id))
                    .cloned()
                    .collect();
                for m in deleted {
                    let action = json!({"_":"channelAdminLogEventActionDeleteMessage","message":s.message(&m,uid)});
                    super::channels::log_action(s, channel, uid, action);
                }
            }
            let mut recipients = vec![uid];
            let before = s.messages.len();
            s.messages.retain(|m| {
                let delete = ids.contains(&m.id)
                    && if channel != 0 {
                        m.kind == "channel" && m.peer == channel
                    } else {
                        m.kind == "user" && (m.from == uid || m.peer == uid)
                            || m.kind == "chat" && m.from == uid
                    };
                if delete {
                    if m.kind == "user" {
                        recipients.extend([m.from, m.peer])
                    } else if let Some(r) = s.rooms.get(&m.peer) {
                        recipients.extend(r.members.keys().copied())
                    }
                }
                !delete
            });
            let count = before - s.messages.len();

            let pts = if channel != 0 {
                let r = s.rooms.get_mut(&channel).unwrap();
                r.pts += count as i32;
                r.pts
            } else {
                s.user_pts.get(&uid).copied().unwrap_or(0) + count as i32
            };
            let u = if channel != 0 {
                json!({"_":"updateDeleteChannelMessages","channel_id":channel,"messages":ids,"pts":pts,"pts_count":count})
            } else {
                json!({"_":"updateDeleteMessages","messages":ids,"pts":pts,"pts_count":count})
            };
            if count > 0 {
                s.record(recipients, u);
            }
            Ok(json!({"_":"messages.affectedMessages","pts":pts,"pts_count":count}))
        }
        "messages.deleteHistory" | "channels.deleteHistory" => {
            let (k, id) = if name.starts_with("channels") {
                ("channel".into(), int(&v["channel"], "channel_id"))
            } else {
                peer(&v["peer"], uid)?
            };
            access(s, &k, id, uid, false)?;
            let max = if int(v, "max_id") == 0 {
                s.rooms.get(&id).map(|r| r.last).unwrap_or(i64::MAX)
            } else {
                int(v, "max_id")
            };
            if k == "channel" && v["for_everyone"] != true {
                let room = s.rooms.get_mut(&id).unwrap();
                let key = format!("history_min:{uid}");
                let old = room.settings.get(&key).and_then(Value::as_i64).unwrap_or(0);
                room.settings.insert(key, json!(max.max(old)));
                s.record(vec![uid], json!({"_":"updateChannel","channel_id":id}));
                return Ok(updates(
                    vec![s.room(id, uid)],
                    vec![],
                    vec![json!({"_":"updateChannel","channel_id":id})],
                ));
            }
            if k != "user" {
                admin(s, id, uid, "delete_messages")?;
            }
            let ids: Vec<_> = s
                .messages
                .iter()
                .filter(|m| visible(m, &k, id, uid) && m.id <= max)
                .map(|m| m.id)
                .collect();
            s.messages
                .retain(|m| !(visible(m, &k, id, uid) && m.id <= max));
            if k == "channel" {
                let r = s.rooms.get_mut(&id).unwrap();
                r.pts += 1;
                if ids.contains(&r.pinned) {
                    r.pinned = 0;
                }
                let update = json!({"_":"updateDeleteChannelMessages","channel_id":id,"messages":ids,"pts":r.pts,"pts_count":1});
                let recipients = r.members.keys().copied().collect();
                s.record(recipients, update.clone());
                Ok(updates(vec![s.room(id, uid)], vec![], vec![update]))
            } else {
                s.pts += 1;
                Ok(json!({"_":"messages.affectedHistory","pts":s.pts,"pts_count":1,"offset":0}))
            }
        }

        "messages.readHistory" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, false)?;
            s.read.insert(format!("{uid}:{k}:{id}"), int(v, "max_id"));
            Ok(json!({"_":"messages.affectedMessages","pts":s.pts,"pts_count":0}))
        }
        "messages.getDialogs" | "messages.getPeerDialogs" => {
            let mut pairs = std::collections::BTreeSet::new();
            if name == "messages.getPeerDialogs" {
                for p in list(v, "peers") {
                    let (k, id) = peer(&p["peer"], uid)?;
                    access(s, &k, id, uid, false)?;
                    pairs.insert((k, id));
                }
            } else {
                for m in &s.messages {
                    if m.kind == "user" {
                        if m.from == uid {
                            pairs.insert((m.kind.clone(), m.peer));
                        } else if m.peer == uid {
                            pairs.insert((m.kind.clone(), m.from));
                        }
                    } else if s
                        .rooms
                        .get(&m.peer)
                        .is_some_and(|r| r.members.contains_key(&uid))
                    {
                        pairs.insert((m.kind.clone(), m.peer));
                    }
                }
                for r in s.rooms.values().filter(|r| r.members.contains_key(&uid)) {
                    pairs.insert((
                        if r.channel {
                            "channel".into()
                        } else {
                            "chat".into()
                        },
                        r.id,
                    ));
                }
            }
            let mut rows: Vec<_> = pairs
                .into_iter()
                .map(|(k, id)| {
                    let m = s
                        .messages
                        .iter()
                        .rev()
                        .find(|m| visible(m, &k, id, uid) && history_visible(s, m, uid));
                    (k, id, m)
                })
                .collect();
            rows.sort_by_key(|(_, _, m)| std::cmp::Reverse(m.map(|m| m.date).unwrap_or(0)));
            let mut dialogs = vec![];
            let mut msgs = vec![];
            let mut chats = vec![];
            let mut users = vec![s.user(uid, uid)];
            for (k, id, m) in rows
                .into_iter()
                .filter(|(_, _, m)| {
                    int(v, "offset_date") == 0 || m.is_none_or(|m| m.date < int(v, "offset_date"))
                })
                .take(if name == "messages.getPeerDialogs" {
                    100
                } else {
                    int(v, "limit").clamp(1, 100) as usize
                })
            {
                let p = json!({"_":match k.as_str(){"channel"=>"peerChannel","chat"=>"peerChat",_=>"peerUser"},match k.as_str(){"channel"=>"channel_id","chat"=>"chat_id",_=>"user_id"}:id});
                let read = s.read.get(&format!("{uid}:{k}:{id}")).copied().unwrap_or(0);
                let mut d = json!({"_":"dialog","peer":p,"top_message":m.map(|m|m.id).unwrap_or(0),"read_inbox_max_id":read,"read_outbox_max_id":0,"unread_count":s.messages.iter().filter(|m|visible(m,&k,id,uid)&&m.id>read&&m.from!=uid).count(),"unread_mentions_count":0,"unread_reactions_count":0,"unread_poll_votes_count":0,"notify_settings":{"_":"peerNotifySettings"}});
                if k == "channel" {
                    d["pts"] = json!(s.rooms[&id].pts)
                }
                dialogs.push(d);
                if let Some(m) = m {
                    msgs.push(s.message(m, uid));
                    users.push(s.user(m.from, uid))
                }
                if k == "user" {
                    users.push(s.user(id, uid))
                } else {
                    chats.push(s.room(id, uid))
                }
            }
            if name == "messages.getDialogs" {
                Ok(
                    json!({"_":"messages.dialogs","dialogs":dialogs,"messages":msgs,"chats":chats,"users":users}),
                )
            } else {
                Ok(
                    json!({"_":"messages.peerDialogs","dialogs":dialogs,"messages":msgs,"chats":chats,"users":users,"state":state(s,uid)}),
                )
            }
        }
        "updates.getState" => Ok(state(s, uid)),
        "updates.getDifference" => {
            let pts = int(v, "pts");
            let current = s.user_pts.get(&uid).copied().unwrap_or(0);
            ensure!(
                pts >= 0 && pts <= current as i64,
                "PERSISTENT_TIMESTAMP_INVALID"
            );
            if pts == current as i64 {
                return Ok(json!({"_":"updates.differenceEmpty","date":now(),"seq":0}));
            }
            let retained: Vec<_> = s.events.iter().filter(|e| e.users.contains(&uid)).collect();
            if retained.first().is_none_or(|e| {
                pts < (e.per_user.get(&uid).copied().unwrap_or(0)
                    - int(&e.update, "pts_count").max(1) as i32) as i64
            }) {
                return Ok(json!({"_":"updates.differenceTooLong","pts":current}));
            }
            let limit = if int(v, "pts_limit") > 0 {
                int(v, "pts_limit").min(100) as usize
            } else {
                100
            };
            let after: Vec<_> = retained
                .into_iter()
                .filter(|e| e.per_user.get(&uid).copied().unwrap_or(0) as i64 > pts)
                .collect();
            let slice = after.len() > limit;
            let items: Vec<_> = after
                .iter()
                .take(limit)
                .map(|e| event_view(e, uid))
                .collect();
            let mut st = state(s, uid);
            if slice {
                st["pts"] = json!(after[limit - 1].per_user[&uid]);
            }
            let users: Vec<_> = s
                .users
                .keys()
                .filter(|u| {
                    **u == uid
                        || items
                            .iter()
                            .any(|e| int(&e["message"]["from_id"], "user_id") == **u)
                })
                .map(|u| s.user(*u, uid))
                .collect();
            Ok(
                json!({"_":if slice{"updates.differenceSlice"}else{"updates.difference"},"new_messages":[],"new_encrypted_messages":[],"other_updates":items,"users":users,"chats":s.rooms.values().filter(|r|r.members.contains_key(&uid)).map(|r|s.room(r.id,uid)).collect::<Vec<_>>(),if slice{"intermediate_state"}else{"state"}:st}),
            )
        }
        "updates.getChannelDifference" => {
            let id = int(&v["channel"], "channel_id");
            access(s, "channel", id, uid, false)?;
            let r = &s.rooms[&id];
            let pts = int(v, "pts");
            if pts >= r.pts as i64 {
                return Ok(json!({"_":"updates.channelDifferenceEmpty","final":true,"pts":r.pts}));
            }
            let events: Vec<_> = s
                .events
                .iter()
                .filter(|e| {
                    e.users.contains(&uid)
                        && (int(&e.update, "channel_id") == id
                            || int(&e.update["message"]["peer_id"], "channel_id") == id)
                        && int(&e.update, "pts") > pts
                })
                .take(100)
                .collect();
            if events
                .first()
                .is_none_or(|e| pts < int(&e.update, "pts") - int(&e.update, "pts_count"))
                && r.pts > 1
            {
                return Ok(
                    json!({"_":"updates.channelDifferenceTooLong","final":true,"dialog":{"_":"dialog","peer":{"_":"peerChannel","channel_id":id},"top_message":r.last,"read_inbox_max_id":s.read.get(&format!("{uid}:channel:{id}")).copied().unwrap_or(0),"read_outbox_max_id":0,"unread_count":0,"unread_mentions_count":0,"unread_reactions_count":0,"unread_poll_votes_count":0,"notify_settings":{"_":"peerNotifySettings"},"pts":r.pts},"messages":s.messages.iter().rev().filter(|m|m.kind=="channel"&&m.peer==id&&history_visible(s,m,uid)).take(100).map(|m|s.message(m,uid)).collect::<Vec<_>>(),"chats":[s.room(id,uid)],"users":r.members.keys().map(|u|s.user(*u,uid)).collect::<Vec<_>>()}),
                );
            }
            let end = events
                .last()
                .map(|e| int(&e.update, "pts"))
                .unwrap_or(r.pts as i64);
            Ok(
                json!({"_":"updates.channelDifference","final":end>=r.pts as i64,"pts":end,"new_messages":[],"other_updates":events.iter().map(|e|event_view(e,uid)).collect::<Vec<_>>(),"chats":[s.room(id,uid)],"users":r.members.keys().map(|u|s.user(*u,uid)).collect::<Vec<_>>()}),
            )
        }
        "messages.setTyping" => {
            let (k, id) = peer(&v["peer"], uid)?;
            access(s, &k, id, uid, true)?;
            Ok(json!(true))
        }
        "messages.getPeerSettings" => Ok(
            json!({"_":"messages.peerSettings","settings":{"_":"peerSettings"},"chats":[],"users":[]}),
        ),
        "messages.getDialogFilters" => Ok(json!({"_":"messages.dialogFilters","filters":[]})),
        "messages.getScheduledHistory" => {
            Ok(json!({"_":"messages.messages","messages":[],"chats":[],"users":[],"topics":[]}))
        }
        "messages.getAllDrafts" => Ok(updates(vec![], vec![], vec![])),
        _ => bail!("METHOD_NOT_IMPLEMENTED"),
    }
}
pub fn state(s: &State, uid: i64) -> Value {
    json!({"_":"updates.state","pts":s.user_pts.get(&uid).copied().unwrap_or(0),"qts":0,"date":now(),"seq":0,"unread_count":0})
}
pub fn for_user(mut v: Value, uid: i64) -> Value {
    if v["message"].is_object() {
        let m = &mut v["message"];
        let from = int(&m["from_id"], "user_id");
        m["out"] = json!(from == uid);
        if m["peer_id"]["_"] == "peerUser" && from != uid {
            m["peer_id"]["user_id"] = json!(from)
        }
    }
    v
}

pub fn event_view(e: &Event, uid: i64) -> Value {
    let mut v = for_user(e.update.clone(), uid);
    let name = text(&v, "_").to_owned();
    if !name.contains("Channel") && !v["pts"].is_null() {
        v["pts"] = json!(e.per_user.get(&uid).copied().unwrap_or(0));
    }
    v
}
