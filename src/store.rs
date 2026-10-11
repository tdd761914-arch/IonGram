use crate::crypto::{now, random_id};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub aliases: BTreeMap<String, Alias>,
    #[serde(default)]
    pub username_orders: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub user_pts: BTreeMap<i64, i32>,
    pub next: i64,
    pub pts: i32,
    pub users: BTreeMap<i64, User>,
    pub rooms: BTreeMap<i64, Room>,
    pub messages: Vec<Message>,
    pub keys: BTreeMap<i64, Key>,
    pub pending: BTreeMap<String, Code>,
    pub randoms: BTreeMap<String, i64>,
    pub invites: BTreeMap<String, Invite>,
    pub files: BTreeMap<i64, File>,
    pub gifts: BTreeMap<i64, Gift>,
    pub owned: BTreeMap<i64, OwnedGift>,
    pub ledger: Vec<Entry>,
    pub forms: BTreeMap<i64, Form>,
    pub read: BTreeMap<String, i64>,
    pub events: Vec<Event>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Key {
    pub data: Vec<u8>,
    pub salt: i64,
    pub user: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Code {
    pub code: String,
    pub hash: String,
    pub expires: i64,
    pub attempts: u8,
    pub key: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub hash: i64,
    pub phone: String,
    pub first: String,
    pub last: String,
    pub username: String,
    pub about: String,
    pub stars: i64,
    pub premium_until: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: i64,
    pub hash: i64,
    pub channel: bool,
    pub mega: bool,
    pub title: String,
    pub about: String,
    pub username: String,
    pub owner: i64,
    pub members: BTreeMap<i64, Member>,
    #[serde(default)]
    pub bans: BTreeMap<i64, Value>,
    pub date: i64,
    pub pts: i32,
    pub last: i64,
    pub pinned: i64,
    pub signatures: bool,
    pub linked: Option<i64>,
    pub settings: BTreeMap<String, Value>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Member {
    pub date: i64,
    pub rights: Value,
    pub banned: Value,
    pub rank: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub from: i64,
    pub kind: String,
    pub peer: i64,
    pub date: i64,
    pub text: String,
    pub media: Option<Value>,
    pub entities: Vec<Value>,
    pub reply: Option<i64>,
    pub forward: Option<Value>,
    pub edit: Option<i64>,
    pub action: Option<Value>,
    pub pinned: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Invite {
    pub room: i64,
    pub creator: i64,
    pub date: i64,
    pub expires: i64,
    pub limit: i64,
    pub used: i64,
    pub revoked: bool,
    pub request: bool,
    pub pending: Vec<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct File {
    pub id: i64,
    pub hash: i64,
    pub reference: Vec<u8>,
    pub size: i64,
    pub mime: String,
    pub attrs: Vec<Value>,
    pub owner: i64,
    pub date: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Gift {
    pub id: i64,
    pub title: String,
    pub file: i64,
    pub price: i64,
    pub supply: i64,
    pub sold: i64,
    pub upgrade: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct OwnedGift {
    pub id: i64,
    pub gift: i64,
    pub owner: i64,
    pub sender: i64,
    pub date: i64,
    pub upgraded: bool,
    pub hidden: bool,
    pub price: i64,
    pub converted: bool,
    pub resale: i64,
    pub pinned: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub user: i64,
    pub delta: i64,
    pub date: i64,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Form {
    pub user: i64,
    pub invoice: Value,
    pub price: i64,
    pub expires: i64,
    pub paid: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub per_user: BTreeMap<i64, i32>,
    pub pts: i32,
    pub users: Vec<i64>,
    pub update: Value,
}
pub struct Store {
    _lock: std::fs::File,
    conn: Connection,
    pub state: State,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.try_lock()
            .context("database already in use; stop the server before administrative commands")?;
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS state(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,data TEXT NOT NULL);")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS stars_ledger(id INTEGER PRIMARY KEY,user_id INTEGER NOT NULL,delta INTEGER NOT NULL CHECK(delta<>0),date INTEGER NOT NULL,reason TEXT NOT NULL); CREATE TABLE IF NOT EXISTS stars_balances(user_id INTEGER PRIMARY KEY,balance INTEGER NOT NULL CHECK(balance>=0)); CREATE TABLE IF NOT EXISTS user_updates(user_id INTEGER NOT NULL,pts INTEGER NOT NULL,event TEXT NOT NULL,PRIMARY KEY(user_id,pts)); CREATE TABLE IF NOT EXISTS peer_usernames(name TEXT PRIMARY KEY COLLATE NOCASE,kind TEXT NOT NULL,owner_id INTEGER NOT NULL,active INTEGER NOT NULL,editable INTEGER NOT NULL,position INTEGER NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS one_editable_username ON peer_usernames(kind,owner_id) WHERE editable=1;")?;
        let row: Option<(i32, String)> = conn
            .query_row("SELECT version,data FROM state WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let state = if let Some((v, data)) = row {
            anyhow::ensure!(v == 1, "unsupported database version");
            serde_json::from_str(&data)?
        } else {
            State {
                next: 100,
                pts: 1,
                ..Default::default()
            }
        };
        Ok(Self {
            _lock: lock,
            conn,
            state,
        })
    }
    pub fn save(&mut self, state: State) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO state VALUES(1,1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            [serde_json::to_string(&state)?],
        )?;
        for e in &state.ledger {
            tx.execute(
                "INSERT OR IGNORE INTO stars_ledger VALUES(?,?,?,?,?)",
                rusqlite::params![e.id, e.user, e.delta, e.date, e.reason],
            )?;
        }
        for u in state.users.values() {
            tx.execute("INSERT INTO stars_balances VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET balance=excluded.balance",rusqlite::params![u.id,u.stars])?;
        }
        for e in &state.events {
            for (uid, pts) in &e.per_user {
                tx.execute(
                    "INSERT OR IGNORE INTO user_updates VALUES(?,?,?)",
                    rusqlite::params![
                        uid,
                        pts,
                        serde_json::to_string(&crate::handlers::messages::event_view(e, *uid))?
                    ],
                )?;
            }
        }
        tx.execute("DELETE FROM peer_usernames", [])?;
        for u in state.users.values().filter(|u| !u.username.is_empty()) {
            tx.execute(
                "INSERT INTO peer_usernames VALUES(?,'user',?,1,1,-1)",
                rusqlite::params![u.username, u.id],
            )?;
        }
        for r in state.rooms.values().filter(|r| !r.username.is_empty()) {
            tx.execute(
                "INSERT INTO peer_usernames VALUES(?,'channel',?,1,1,-1)",
                rusqlite::params![r.username, r.id],
            )?;
        }
        for a in state.aliases.values() {
            tx.execute(
                "INSERT INTO peer_usernames VALUES(?,?,?,?,0,?)",
                rusqlite::params![a.name, a.kind, a.owner, a.active, a.position],
            )?;
        }
        tx.commit()?;
        self.state = state;
        Ok(())
    }
}
impl State {
    pub fn id(&mut self) -> i64 {
        self.next += 1;
        self.next
    }
    pub fn credit(&mut self, user: i64, delta: i64, reason: &str) -> Result<()> {
        anyhow::ensure!(delta != 0, "AMOUNT_INVALID");
        let u = self.users.get_mut(&user).context("USER_ID_INVALID")?;
        let balance = u.stars.checked_add(delta).context("BALANCE_OVERFLOW")?;
        anyhow::ensure!(balance >= 0, "STARS_NOT_ENOUGH");
        u.stars = balance;
        let id = self.id();
        self.ledger.push(Entry {
            id,
            user,
            delta,
            date: now(),
            reason: reason.into(),
        });
        self.record(vec![user],json!({"_":"updateStarsBalance","balance":{"_":"starsAmount","amount":balance,"nanos":0}}));
        Ok(())
    }
    pub fn user(&self, id: i64, viewer: i64) -> Value {
        self.users.get(&id).map(|u|{let mut v=json!({"_":"user","id":id,"access_hash":u.hash,"first_name":u.first,"last_name":u.last,"self":id==viewer,"premium":u.premium_until>now(),"photo":{"_":"userProfilePhotoEmpty"},"status":{"_":"userStatusRecently"}});if id==viewer{v["phone"]=json!(u.phone)}
if !u.username.is_empty(){v["username"]=json!(u.username)}
if self.aliases.values().any(|a|a.kind=="user"&&a.owner==id){v.as_object_mut().unwrap().remove("username");v["usernames"]=json!(crate::handlers::fragment::projection(self,"user",id));}v}).unwrap_or(json!({"_":"userEmpty","id":id}))
    }
    pub fn room(&self, id: i64, viewer: i64) -> Value {
        let Some(r) = self.rooms.get(&id) else {
            return json!({"_":"chatEmpty","id":id});
        };
        let mut v = json!({"_":if r.channel{"channel"}else{"chat"},"id":id,"title":r.title,"photo":{"_":"chatPhotoEmpty"},"date":r.date,"creator":r.owner==viewer,"left":!r.members.contains_key(&viewer),"participants_count":r.members.len()});
        if r.channel {
            v["access_hash"] = json!(r.hash);
            v["broadcast"] = json!(!r.mega);
            v["megagroup"] = json!(r.mega);
            v["signatures"] = json!(r.signatures);
            if !r.username.is_empty() {
                v["username"] = json!(r.username)
            }
        } else {
            v["version"] = json!(1)
        }
        if self
            .aliases
            .values()
            .any(|a| a.kind == "channel" && a.owner == id)
        {
            v.as_object_mut().unwrap().remove("username");
            v["usernames"] = json!(crate::handlers::fragment::projection(self, "channel", id));
        }
        v
    }
    pub fn doc(&self, id: i64) -> Result<Value> {
        let f = self.files.get(&id).context("FILE_ID_INVALID")?;
        Ok(
            json!({"_":"document","id":f.id,"access_hash":f.hash,"file_reference":f.reference,"date":f.date,"mime_type":f.mime,"size":f.size,"dc_id":2,"attributes":f.attrs}),
        )
    }
    pub fn message(&self, m: &Message, viewer: i64) -> Value {
        let peer = if m.kind == "user" {
            json!({"_":"peerUser","user_id":if m.from==viewer{m.peer}else{m.from}})
        } else {
            json!({"_":if m.kind=="channel"{"peerChannel"}else{"peerChat"},if m.kind=="channel"{"channel_id"}else{"chat_id"}:m.peer})
        };
        let mut v = json!({"_":if m.action.is_some(){"messageService"}else{"message"},"id":m.id,"out":m.from==viewer,"from_id":{"_":"peerUser","user_id":m.from},"peer_id":peer,"date":m.date,"message":m.text,"entities":m.entities,"pinned":m.pinned});
        if let Some(a) = &m.action {
            v["action"] = a.clone()
        }
        if let Some(a) = &m.media {
            v["media"] = a.clone()
        }
        if let Some(a) = m.reply {
            v["reply_to"] = json!({"_":"messageReplyHeader","reply_to_msg_id":a})
        }
        if let Some(a) = &m.forward {
            v["fwd_from"] = a.clone()
        }
        if let Some(a) = m.edit {
            v["edit_date"] = json!(a)
        }
        if m.kind == "channel" && self.rooms.get(&m.peer).is_some_and(|r| !r.mega) {
            v["post"] = json!(true)
        }
        v
    }
    pub fn new_user(&mut self, phone: String) -> i64 {
        let id = self.id();
        self.users.insert(
            id,
            User {
                id,
                hash: random_id(),
                phone,
                first: "IonGram".into(),
                last: String::new(),
                username: String::new(),
                about: String::new(),
                stars: 0,
                premium_until: 0,
            },
        );
        id
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Alias {
    pub name: String,
    pub kind: String,
    pub owner: i64,
    pub active: bool,
    pub position: i32,
    pub date: i64,
    pub currency: String,
    pub amount: i64,
    pub crypto_currency: String,
    pub crypto_amount: i64,
    pub url: String,
}
impl State {
    pub fn record(&mut self, users: Vec<i64>, update: Value) {
        self.pts += 1;
        let users: Vec<_> = users
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let count = update["pts_count"].as_i64().unwrap_or(1).max(1) as i32;
        let mut per_user = BTreeMap::new();
        for uid in &users {
            let p = self.user_pts.entry(*uid).or_insert(0);
            *p += count;
            per_user.insert(*uid, *p);
        }
        self.events.push(Event {
            pts: self.pts,
            users,
            update,
            per_user,
        });
        if self.events.len() > 10000 {
            self.events.drain(..1000);
        }
    }
}
