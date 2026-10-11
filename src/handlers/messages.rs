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
fn response(s: &State, uid: i64, kind: &str, id: i64, msgs: Vec<&Message>, count: usize) -> Value {
    json!({"_":if kind=="channel"{"messages.channelMessages"}else{"messages.messagesSlice"},"pts":s.rooms.get(&id).map(|r|r.pts).unwrap_or(s.pts),"count":count,"messages":msgs.iter().map(|m|s.message(m,uid)).collect::<Vec<_>>(),"topics":[],"chats":if kind=="user"{vec![]}else{vec![s.room(id,uid)]},"users":s.users.keys().filter(|u|msgs.iter().any(|m|m.from==**u)).map(|u|s.user(*u,uid)).collect::<Vec<_>>()})
}
pub fn publish(s: &mut State, uid: i64, m: &Message, edit: bool) -> Value {
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
        "messages.getPinnedDialogs" => Ok(
            json!({"_":"messages.peerDialogs","dialogs":[],"messages":[],"chats":[],"users":[],"state":state(s,uid)}),
        ),
        "messages.sendMessage" | "messages.sendMedia" => send(s, uid, v, None),
        "messages.forwardMessages" => {
            let (k, id) = peer(&v["from_peer"], uid)?;
            access(s, &k, id, uid, false)?;
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
                    .find(|m| m.id == mid.as_i64().unwrap_or(0) && visible(m, &k, id, uid))
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
                        && (name != "messages.getReplies" || m.reply == Some(thread))
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
            Ok(publish(s, uid, &m, true))
        }
        "messages.deleteMessages" | "channels.deleteMessages" => {
            let ids: Vec<i64> = list(v, "id").iter().filter_map(Value::as_i64).collect();
            ensure!(ids.len() <= 100, "INPUT_REQUEST_TOO_LONG");
            let channel = int(&v["channel"], "channel_id");
            if channel != 0 {
                admin(s, channel, uid, "delete_messages")?
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
            if k != "user" {
                admin(s, id, uid, "delete_messages")?
            }
            let max = int(v, "max_id");
            s.messages
                .retain(|m| !(visible(m, &k, id, uid) && (max == 0 || m.id <= max)));
            s.pts += 1;
            if name.starts_with("channels") {
                Ok(updates(vec![s.room(id, uid)], vec![], vec![]))
            } else {
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
                    let m = s.messages.iter().rev().find(|m| visible(m, &k, id, uid));
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
                .take(int(v, "limit").clamp(1, 100) as usize)
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
                    json!({"_":"updates.channelDifferenceTooLong","final":true,"dialog":{"_":"dialog","peer":{"_":"peerChannel","channel_id":id},"top_message":r.last,"read_inbox_max_id":s.read.get(&format!("{uid}:channel:{id}")).copied().unwrap_or(0),"read_outbox_max_id":0,"unread_count":0,"unread_mentions_count":0,"unread_reactions_count":0,"unread_poll_votes_count":0,"notify_settings":{"_":"peerNotifySettings"},"pts":r.pts},"messages":s.messages.iter().rev().filter(|m|m.kind=="channel"&&m.peer==id).take(100).map(|m|s.message(m,uid)).collect::<Vec<_>>(),"chats":[s.room(id,uid)],"users":r.members.keys().map(|u|s.user(*u,uid)).collect::<Vec<_>>()}),
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
