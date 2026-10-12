//! Forum topics use channel-local message IDs and persist with the room.
use super::*;
use crate::store::Message;
pub const METHODS: &[&str] = &[
    "messages.createForumTopic",
    "messages.editForumTopic",
    "messages.getForumTopics",
    "messages.getForumTopicsByID",
    "messages.updatePinnedForumTopic",
    "messages.reorderPinnedForumTopics",
    "messages.deleteTopicHistory",
];
fn key(id: i64) -> String {
    format!("topic:{id}")
}
pub fn topic(s: &State, channel: i64, id: i64) -> Option<&Value> {
    s.rooms.get(&channel)?.settings.get(&key(id))
}
pub fn ensure_general(s: &mut State, id: i64) {
    let r = s.rooms.get_mut(&id).unwrap();
    r.settings.entry(key(1)).or_insert_with(||json!({"_":"forumTopic","id":1,"date":r.date,"peer":{"_":"peerChannel","channel_id":id},"title":"General","icon_color":0x6fb9f0,"top_message":1,"read_inbox_max_id":0,"read_outbox_max_id":0,"unread_count":0,"unread_mentions_count":0,"unread_reactions_count":0,"unread_poll_votes_count":0,"from_id":{"_":"peerUser","user_id":r.owner},"notify_settings":{"_":"peerNotifySettings"}}));
}
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    let (kind, id) = peer(&v["peer"], uid)?;
    ensure!(kind == "channel", "CHANNEL_INVALID");
    access(s, &kind, id, uid, false)?;
    ensure!(
        super::channels::enabled(&s.rooms[&id], "channels.toggleForum"),
        "CHANNEL_FORUM_MISSING"
    );
    ensure_general(s, id);
    let name = text(v, "_");
    if name == "messages.getForumTopics" || name == "messages.getForumTopicsByID" {
        let ids = list(v, "topics");
        let q = text(v, "q").to_lowercase();
        let mut topics: Vec<_> = s.rooms[&id]
            .settings
            .iter()
            .filter(|(k, t)| {
                k.starts_with("topic:")
                    && text(t, "title").to_lowercase().contains(&q)
                    && (name.ends_with("ByID") == false
                        || ids.iter().any(|id| id.as_i64() == Some(int(t, "id"))))
            })
            .map(|(_, t)| {
                let mut t = t.clone();
                t["my"] = json!(int(&t["from_id"], "user_id") == uid);
                t
            })
            .collect();
        topics.sort_by_key(|t| {
            (
                t["pinned"] != true,
                std::cmp::Reverse(int(t, "top_message")),
            )
        });
        let count = topics.len();
        if int(v, "offset_topic") > 0 {
            if let Some(pos) = topics
                .iter()
                .position(|t| int(t, "id") == int(v, "offset_topic"))
            {
                topics.drain(..=pos);
            }
        }
        topics.truncate(if name.ends_with("ByID") {
            100
        } else {
            int(v, "limit").clamp(1, 100) as usize
        });
        let messages = s
            .messages
            .iter()
            .filter(|m| {
                m.kind == kind
                    && m.peer == id
                    && topics.iter().any(|t| int(t, "top_message") == m.id)
            })
            .map(|m| s.message(m, uid))
            .collect::<Vec<_>>();
        return Ok(
            json!({"_":"messages.forumTopics","count":count,"topics":topics,"messages":messages,"chats":[s.room(id,uid)],"users":s.rooms[&id].members.keys().map(|u|s.user(*u,uid)).collect::<Vec<_>>(),"pts":s.rooms[&id].pts}),
        );
    }
    let tid = int(v, "topic_id");
    if name == "messages.createForumTopic" {
        access(s, &kind, id, uid, true)?;
        let r = &s.rooms[&id];
        ensure!(
            super::channels::administrator(r, uid)
                || r.settings
                    .get("default_banned_rights")
                    .is_none_or(|b| b["manage_topics"] != true),
            "CHAT_ADMIN_REQUIRED"
        );
        let title = text(v, "title").trim();
        ensure!(
            !title.is_empty() && title.chars().count() <= 128,
            "TOPIC_TITLE_INVALID"
        );
        let token = format!("topic:{uid}:{}", int(v, "random_id"));
        if let Some(mid) = s.randoms.get(&token) {
            return Ok(updates(
                vec![s.room(id, uid)],
                vec![],
                vec![json!({"_":"updateMessageID","id":mid,"random_id":int(v,"random_id")})],
            ));
        }
        let r = s.rooms.get_mut(&id).unwrap();
        r.last += 1;
        let mid = r.last;
        let color = if v["icon_color"].is_null() {
            0x6fb9f0
        } else {
            int(v, "icon_color")
        };
        ensure!((0..=0xffffff).contains(&color), "TOPIC_ICON_COLOR_INVALID");
        let mut topic = json!({"_":"forumTopic","id":mid,"date":now(),"peer":{"_":"peerChannel","channel_id":id},"title":title,"icon_color":color,"top_message":mid,"read_inbox_max_id":0,"read_outbox_max_id":0,"unread_count":0,"unread_mentions_count":0,"unread_reactions_count":0,"unread_poll_votes_count":0,"from_id":{"_":"peerUser","user_id":uid},"notify_settings":{"_":"peerNotifySettings"}});
        let mut action = json!({"_":"messageActionTopicCreate","title":title,"icon_color":color});
        if !v["icon_emoji_id"].is_null() {
            topic["icon_emoji_id"] = v["icon_emoji_id"].clone();
            action["icon_emoji_id"] = v["icon_emoji_id"].clone();
        }
        r.settings.insert(key(mid), topic);
        let m = Message {
            id: mid,
            from: uid,
            kind,
            peer: id,
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
        s.messages.push(m.clone());
        s.message_topics.insert(format!("{id}:{mid}"), mid);
        s.randoms.insert(token, mid);
        let mut out = super::messages::publish(s, uid, &m, false);
        out["updates"]
            .as_array_mut()
            .unwrap()
            .push(json!({"_":"updateMessageID","id":mid,"random_id":int(v,"random_id")}));
        return Ok(out);
    }
    if name == "messages.reorderPinnedForumTopics" {
        admin(s, id, uid, "manage_topics")?;
        let order = list(v, "order");
        ensure!(order.len() <= 5, "PINNED_TOO_MUCH");
        for t in &order {
            ensure!(
                topic(s, id, t.as_i64().unwrap_or(0)).is_some(),
                "TOPIC_ID_INVALID"
            );
        }
        for (k, t) in &mut s.rooms.get_mut(&id).unwrap().settings {
            if k.starts_with("topic:") {
                t["pinned"] = json!(order.iter().any(|n| n.as_i64() == Some(int(t, "id"))));
            }
        }
        let update = json!({"_":"updatePinnedForumTopics","peer":{"_":"peerChannel","channel_id":id},"order":order});
        let users = s.rooms[&id].members.keys().copied().collect();
        s.record(users, update.clone());
        return Ok(updates(vec![s.room(id, uid)], vec![], vec![update]));
    }
    let tid = if name == "messages.deleteTopicHistory" {
        int(v, "top_msg_id")
    } else {
        tid
    };
    let old = topic(s, id, tid).context("TOPIC_ID_INVALID")?.clone();
    ensure!(
        int(&old["from_id"], "user_id") == uid || admin(s, id, uid, "manage_topics").is_ok(),
        "CHAT_ADMIN_REQUIRED"
    );
    if name == "messages.deleteTopicHistory" {
        ensure!(tid != 1, "TOPIC_ID_INVALID");
        let ids: Vec<_> = s
            .messages
            .iter()
            .filter(|m| {
                m.kind == "channel"
                    && m.peer == id
                    && s.message_topics.get(&format!("{id}:{}", m.id)) == Some(&tid)
            })
            .map(|m| m.id)
            .collect();
        s.messages
            .retain(|m| !(m.kind == "channel" && m.peer == id && ids.contains(&m.id)));
        let r = s.rooms.get_mut(&id).unwrap();
        r.settings.remove(&key(tid));
        r.pts += 1;
        let pts = r.pts;
        let users = r.members.keys().copied().collect();
        s.record(users,json!({"_":"updateDeleteChannelMessages","channel_id":id,"messages":ids,"pts":pts,"pts_count":1}));
        return Ok(json!({"_":"messages.affectedHistory","pts":pts,"pts_count":1,"offset":0}));
    }
    if name == "messages.updatePinnedForumTopic" {
        admin(s, id, uid, "manage_topics")?;
        s.rooms
            .get_mut(&id)
            .unwrap()
            .settings
            .get_mut(&key(tid))
            .unwrap()["pinned"] = v["pinned"].clone();
        let update = json!({"_":"updatePinnedForumTopic","peer":{"_":"peerChannel","channel_id":id},"topic_id":tid,"pinned":v["pinned"]});
        let users = s.rooms[&id].members.keys().copied().collect();
        s.record(users, update.clone());
        return Ok(updates(vec![s.room(id, uid)], vec![], vec![update]));
    }
    let mut action = json!({"_":"messageActionTopicEdit"});
    let r = s.rooms.get_mut(&id).unwrap();
    let t = r.settings.get_mut(&key(tid)).unwrap();
    for field in ["title", "icon_emoji_id", "closed", "hidden"] {
        if !v[field].is_null() {
            if field == "title" {
                ensure!(
                    !text(v, field).trim().is_empty() && text(v, field).chars().count() <= 128,
                    "TOPIC_TITLE_INVALID"
                );
            }
            if field == "hidden" {
                ensure!(tid == 1, "TOPIC_ID_INVALID");
            }
            t[field] = v[field].clone();
            action[field] = v[field].clone();
        }
    }
    r.last += 1;
    let mid = r.last;
    let m = Message {
        id: mid,
        from: uid,
        kind,
        peer: id,
        date: now(),
        text: String::new(),
        media: None,
        entities: vec![],
        reply: Some(tid),
        forward: None,
        edit: None,
        action: Some(action),
        pinned: false,
    };
    s.message_topics.insert(format!("{id}:{mid}"), tid);
    s.messages.push(m.clone());
    Ok(super::messages::publish(s, uid, &m, false))
}
