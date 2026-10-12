use super::*;
use crate::{
    crypto::{random, random_id},
    store::Code,
};
pub fn handle(s: &mut State, uid: i64, key: i64, v: &Value, schema: &Schema) -> Result<Value> {
    let name = text(v, "_");
    match name {
        "auth.sendCode" => {
            let phone: String = text(v, "phone_number")
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            ensure!((5..=15).contains(&phone.len()), "PHONE_NUMBER_INVALID");
            if let Some(c) = s.pending.get(&phone) {
                if c.key == key && c.expires > now() && c.attempts < 5 {
                    return Ok(
                        json!({"_":"auth.sentCode","type":{"_":"auth.sentCodeTypeApp","length":5},"phone_code_hash":c.hash,"timeout":c.expires-now()}),
                    );
                }
                ensure!(c.expires - now() < 240, "FLOOD_WAIT_60")
            }
            let code = format!("{:05}", random_id() % 100000);
            let hash = hex::encode(random(16));
            tracing::warn!(phone=%phone,code=%code,"Local development login code (no SMS provider)");
            s.pending.insert(
                phone,
                Code {
                    code,
                    hash: hash.clone(),
                    expires: now() + 300,
                    attempts: 0,
                    key,
                },
            );
            Ok(
                json!({"_":"auth.sentCode","type":{"_":"auth.sentCodeTypeApp","length":5},"phone_code_hash":hash,"timeout":300}),
            )
        }
        "auth.signIn" => {
            let phone: String = text(v, "phone_number")
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            let c = s.pending.get_mut(&phone).context("PHONE_CODE_EXPIRED")?;
            ensure!(
                c.expires > now() && c.key == key && c.attempts < 5,
                "PHONE_CODE_EXPIRED"
            );
            c.attempts += 1;
            ensure!(
                c.hash == text(v, "phone_code_hash") && c.code == text(v, "phone_code"),
                "PHONE_CODE_INVALID"
            );
            let id = s
                .users
                .values()
                .find(|u| u.phone == phone)
                .map(|u| u.id)
                .unwrap_or_else(|| s.new_user(phone.clone()));
            s.pending.remove(&phone);
            s.keys.get_mut(&key).context("AUTH_KEY_UNREGISTERED")?.user = Some(id);
            Ok(json!({"_":"auth.authorization","user":s.user(id,id)}))
        }
        "help.getConfig" => {
            let mut c = schema.defaults("config")?;
            c["date"] = json!(now());
            c["expires"] = json!(now() + 3600);
            c["this_dc"] = json!(2);
            c["dc_options"] = json!([{"_":"dcOption","id":2,"ip_address":std::env::var("IONGRAM_PUBLIC_HOST").unwrap_or("127.0.0.1".into()),"port":std::env::var("IONGRAM_PORT").ok().and_then(|s|s.parse::<i32>().ok()).unwrap_or(4430),"this_port_only":true}]);
            for (k, n) in [
                ("chat_size_max", 200),
                ("megagroup_size_max", 200000),
                ("forwarded_count_max", 100),
                ("online_update_period_ms", 30000),
                ("offline_blur_timeout_ms", 30000),
                ("offline_idle_timeout_ms", 30000),
                ("online_cloud_timeout_ms", 30000),
                ("notify_cloud_delay_ms", 30000),
                ("notify_default_delay_ms", 10000),
                ("edit_time_limit", 172800),
                ("revoke_time_limit", 2147483647),
                ("revoke_pm_time_limit", 2147483647),
                ("rating_e_decay", 2419200),
                ("stickers_recent_limit", 20),
                ("channels_read_media_period", 86400),
                ("call_receive_timeout_ms", 20000),
                ("call_ring_timeout_ms", 90000),
                ("call_connect_timeout_ms", 30000),
                ("call_packet_timeout_ms", 10000),
                ("caption_length_max", 2048),
                ("message_length_max", 4096),
                ("webfile_dc_id", 2),
            ] {
                c[k] = json!(n)
            }
            c["me_url_prefix"] = json!("https://iongram.local/");
            Ok(c)
        }
        "help.getNearestDc" => Ok(json!({"_":"nearestDc","country":"","this_dc":2,"nearest_dc":2})),
        "help.getAppConfig" => {
            Ok(json!({"_":"help.appConfig","hash":1,"config":{"_":"jsonObject","value":[]}}))
        }
        "help.getTermsOfServiceUpdate" => {
            Ok(json!({"_":"help.termsOfServiceUpdateEmpty","expires":now()+86400}))
        }
        _ => {
            ensure!(uid > 0, "AUTH_KEY_UNREGISTERED");
            match name {
                "auth.logOut" => {
                    s.keys.get_mut(&key).context("AUTH_KEY_UNREGISTERED")?.user = None;
                    Ok(json!({"_":"auth.loggedOut"}))
                }
                "users.getUsers" => {
                    let mut out = vec![];
                    for u in list(v, "id") {
                        let (_, id) = peer(&u, uid)?;
                        out.push(s.user(id, uid))
                    }
                    Ok(json!({"_":"vector","data":out}))
                }
                "users.getFullUser" => {
                    let (_, id) = peer(&v["id"], uid)?;
                    let u = s.users.get(&id).context("USER_ID_INVALID")?;
                    Ok(
                        json!({"_":"users.userFull","full_user":{"_":"userFull","id":id,"about":u.about,"settings":{"_":"peerSettings"},"notify_settings":{"_":"peerNotifySettings"},"common_chats_count":0,"stargifts_count":s.owned.values().filter(|g|g.owner==id&&!g.converted).count()},"chats":[],"users":[s.user(id,uid)]}),
                    )
                }
                "account.updateProfile" => {
                    let u = s.users.get_mut(&uid).unwrap();
                    for (key, target, max) in [
                        ("first_name", &mut u.first, 64),
                        ("last_name", &mut u.last, 64),
                        ("about", &mut u.about, 255),
                    ] {
                        if let Some(t) = v[key].as_str() {
                            ensure!(t.chars().count() <= max, "FIELD_TOO_LONG");
                            *target = t.into()
                        }
                    }
                    ensure!(!u.first.trim().is_empty(), "FIRSTNAME_INVALID");
                    Ok(s.user(uid, uid))
                }
                "account.checkUsername" | "account.updateUsername" => {
                    let username = text(v, "username");
                    ensure!(
                        !s.aliases.contains_key(&username.to_lowercase()),
                        "USERNAME_OCCUPIED"
                    );
                    ensure!(
                        username.is_empty() || valid_username(username),
                        "USERNAME_INVALID"
                    );
                    ensure!(
                        !s.users.values().any(|u| u.id != uid
                            && !username.is_empty()
                            && u.username.eq_ignore_ascii_case(username))
                            && !s.rooms.values().any(|r| !username.is_empty()
                                && r.username.eq_ignore_ascii_case(username)),
                        "USERNAME_OCCUPIED"
                    );
                    if name == "account.updateUsername" {
                        s.users.get_mut(&uid).unwrap().username = username.into();
                        Ok(s.user(uid, uid))
                    } else {
                        Ok(json!(true))
                    }
                }
                "account.updateStatus" | "account.registerDevice" | "account.unregisterDevice" => {
                    Ok(json!(true))
                }
                "account.getNotifySettings" => Ok(json!({"_":"peerNotifySettings"})),
                "contacts.resolveUsername" => {
                    let n = text(v, "username").trim_start_matches('@');
                    if let Some(a) = s.aliases.get(&n.to_lowercase()).filter(|a| a.active) {
                        return Ok(if a.kind == "user" {
                            json!({"_":"contacts.resolvedPeer","peer":{"_":"peerUser","user_id":a.owner},"users":[s.user(a.owner,uid)],"chats":[]})
                        } else {
                            json!({"_":"contacts.resolvedPeer","peer":{"_":"peerChannel","channel_id":a.owner},"users":[],"chats":[s.room(a.owner,uid)]})
                        });
                    }

                    if let Some(u) = s
                        .users
                        .values()
                        .find(|u| !n.is_empty() && u.username.eq_ignore_ascii_case(n))
                    {
                        return Ok(
                            json!({"_":"contacts.resolvedPeer","peer":{"_":"peerUser","user_id":u.id},"users":[s.user(u.id,uid)],"chats":[]}),
                        );
                    }
                    if let Some(r) = s
                        .rooms
                        .values()
                        .find(|r| !n.is_empty() && r.username.eq_ignore_ascii_case(n))
                    {
                        return Ok(
                            json!({"_":"contacts.resolvedPeer","peer":{"_":"peerChannel","channel_id":r.id},"users":[],"chats":[s.room(r.id,uid)]}),
                        );
                    }
                    bail!("USERNAME_NOT_OCCUPIED")
                }
                "contacts.search" => {
                    let q = text(v, "q").to_lowercase();
                    let limit = int(v, "limit").clamp(1, 100) as usize;
                    let ids: Vec<_> = s
                        .users
                        .values()
                        .filter(|u| {
                            !q.is_empty()
                                && (u.username.to_lowercase().contains(&q)
                                    || u.first.to_lowercase().contains(&q))
                        })
                        .take(limit)
                        .map(|u| u.id)
                        .collect();
                    Ok(
                        json!({"_":"contacts.found","my_results":[],"results":ids.iter().map(|id|json!({"_":"peerUser","user_id":id})).collect::<Vec<_>>(),"users":ids.iter().map(|id|s.user(*id,uid)).collect::<Vec<_>>(),"chats":[]}),
                    )
                }
                "contacts.getContacts" => {
                    Ok(json!({"_":"contacts.contacts","contacts":[],"saved_count":0,"users":[]}))
                }
                "help.getPremiumPromo" => Ok(
                    json!({"_":"help.premiumPromo","status_text":"IonGram Premium","status_entities":[],"video_sections":[],"videos":[],"period_options":[],"users":[]}),
                ),
                _ => bail!("METHOD_NOT_IMPLEMENTED"),
            }
        }
    }
}
pub fn valid_username(s: &str) -> bool {
    (5..=32).contains(&s.len())
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
