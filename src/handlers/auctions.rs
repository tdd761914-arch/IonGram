//! Persistent Stars auctions. Bids are escrowed; losing bids carry to the next round.
use super::*;
use crate::store::{Message, OwnedGift};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Serialize, Deserialize)]
pub struct Auction {
    pub start: i64,
    pub duration: i64,
    pub rounds: i64,
    pub per_round: i64,
    pub minimum: i64,
    pub completed: i64,
    pub version: i64,
    pub bids: BTreeMap<i64, Bid>,
    pub acquired: BTreeMap<i64, Vec<Value>>,
    pub refunded: Vec<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Bid {
    pub amount: i64,
    pub date: i64,
    pub order: i64,
    pub peer: i64,
    pub hidden: bool,
    pub message: Option<Value>,
}
fn ranking(a: &Auction) -> Vec<(i64, Bid)> {
    let mut bids: Vec<_> = a.bids.iter().map(|(u, b)| (*u, b.clone())).collect();
    bids.sort_by_key(|(u, b)| (std::cmp::Reverse(b.amount), b.order, *u));
    bids
}
pub fn state(a: &Auction, sold: i64) -> Value {
    let end = a.start + a.rounds * a.duration;
    if a.completed == a.rounds {
        let prices: Vec<_> = a
            .acquired
            .values()
            .flatten()
            .map(|v| int(v, "bid_amount"))
            .collect();
        let average = if prices.is_empty() {
            0
        } else {
            (prices.iter().map(|p| *p as i128).sum::<i128>() / prices.len() as i128) as i64
        };
        return json!({"_":"starGiftAuctionStateFinished","start_date":a.start,"end_date":end,"average_price":average});
    }
    let ranked = ranking(a);
    json!({"_":"starGiftAuctionState","version":a.version,"start_date":a.start,"end_date":end,"min_bid_amount":a.minimum,
      "bid_levels":ranked.iter().take(a.per_round as usize).enumerate().map(|(i,(_,b))|json!({"_":"auctionBidLevel","pos":i+1,"amount":b.amount,"date":b.date})).collect::<Vec<_>>(),
      "top_bidders":ranked.iter().filter(|(_,b)|!b.hidden).take(3).map(|(u,_)|*u).collect::<Vec<_>>(),
      "next_round_at":a.start+(a.completed+1)*a.duration,"last_gift_num":sold,"gifts_left":a.per_round*a.rounds-sold,"current_round":a.completed+1,"total_rounds":a.rounds,
      "rounds":(1..=a.rounds).map(|n|json!({"_":"starGiftAuctionRound","num":n,"duration":a.duration})).collect::<Vec<_>>()})
}
pub fn user_state(a: &Auction, uid: i64) -> Value {
    let mut v = json!({"_":"starGiftAuctionUserState","acquired_count":a.acquired.get(&uid).map_or(0,Vec::len),"returned":a.refunded.contains(&uid)});
    if let Some(b) = a.bids.get(&uid) {
        v["bid_amount"] = json!(b.amount);
        v["bid_date"] = json!(b.date);
        v["min_bid_amount"] = json!(b.amount.saturating_add(1));
        v["bid_peer"] = json!({"_":"peerUser","user_id":b.peer});
    }
    v
}
pub fn bid_price(s: &State, uid: i64, v: &Value) -> Result<i64> {
    let a = s
        .auctions
        .get(&int(v, "gift_id"))
        .context("STARGIFT_AUCTION_INVALID")?;
    ensure!(
        a.start > 0
            && now() >= a.start
            && now() < a.start + (a.completed + 1) * a.duration
            && a.completed < a.rounds,
        "STARGIFT_AUCTION_CLOSED"
    );
    let amount = int(v, "bid_amount");
    let old = a.bids.get(&uid).map_or(0, |b| b.amount);
    ensure!(
        (v["update_bid"] == true) == (old > 0),
        "STARGIFT_AUCTION_BID_INVALID"
    );
    ensure!(
        amount >= a.minimum && amount > old && amount <= i64::MAX / 10000,
        "STARGIFT_AUCTION_BID_TOO_LOW"
    );
    let (kind, target) = if v["peer"].is_null() {
        ("user".into(), uid)
    } else {
        peer(&v["peer"], uid)?
    };
    ensure!(
        kind == "user" && s.users.contains_key(&target),
        "PEER_ID_INVALID"
    );
    Ok(amount - old)
}
pub fn place_bid(s: &mut State, uid: i64, v: &Value) -> Result<()> {
    bid_price(s, uid, v)?;
    let target = if v["peer"].is_null() {
        uid
    } else {
        peer(&v["peer"], uid)?.1
    };
    let order = s.id();
    let a = s.auctions.get_mut(&int(v, "gift_id")).unwrap();
    a.bids.insert(
        uid,
        Bid {
            amount: int(v, "bid_amount"),
            date: now(),
            order,
            peer: target,
            hidden: v["hide_name"] == true,
            message: (!v["message"].is_null()).then(|| v["message"].clone()),
        },
    );
    a.version += 1;
    notify(s, int(v, "gift_id"));
    Ok(())
}
fn notify(s: &mut State, id: i64) {
    let a = &s.auctions[&id];
    let view = state(a, s.gifts[&id].sold);
    let users: Vec<_> = a
        .bids
        .keys()
        .chain(a.acquired.keys())
        .chain(a.refunded.iter())
        .copied()
        .collect();
    let personal: Vec<_> = users.iter().map(|u| (*u, user_state(a, *u))).collect();
    s.record(
        users,
        json!({"_":"updateStarGiftAuctionState","gift_id":id,"state":view}),
    );
    for (u, v) in personal {
        s.record(
            vec![u],
            json!({"_":"updateStarGiftAuctionUserState","gift_id":id,"user_state":v}),
        );
    }
}
/// Called once a second, also catches up after a restart. Saved as one transaction.
pub fn tick(s: &mut State, at: i64) -> Result<bool> {
    let due: Vec<_> = s
        .auctions
        .iter()
        .filter(|(_, a)| {
            a.start > 0 && a.completed < a.rounds && at >= a.start + (a.completed + 1) * a.duration
        })
        .map(|(id, _)| *id)
        .collect();
    for gift_id in &due {
        let mut a = s.auctions.remove(gift_id).unwrap();
        while a.completed < a.rounds && at >= a.start + (a.completed + 1) * a.duration {
            let date = a.start + (a.completed + 1) * a.duration;
            for (pos, (uid, b)) in ranking(&a)
                .into_iter()
                .take(a.per_round as usize)
                .enumerate()
            {
                let id = s.id();
                let g = s.gifts.get_mut(gift_id).context("GIFT_NOT_FOUND")?;
                ensure!(g.sold < g.supply, "STARGIFT_USAGE_LIMITED");
                g.sold += 1;
                let num = g.sold;
                s.owned.insert(
                    id,
                    OwnedGift {
                        id,
                        gift: *gift_id,
                        owner: b.peer,
                        sender: uid,
                        date,
                        upgraded: false,
                        hidden: false,
                        price: g.price,
                        converted: false,
                        resale: 0,
                        pinned: false,
                    },
                );
                let mut acquired = json!({"_":"starGiftAuctionAcquiredGift","name_hidden":b.hidden,"peer":{"_":"peerUser","user_id":b.peer},"date":date,"bid_amount":b.amount,"round":a.completed+1,"pos":pos+1,"gift_num":num});
                if let Some(m) = &b.message {
                    acquired["message"] = m.clone();
                }
                a.acquired.entry(uid).or_default().push(acquired);
                let mut action = json!({"_":"messageActionStarGift","auction_acquired":true,"saved":true,"can_upgrade":true,"name_hidden":b.hidden,"gift":super::gifts::gift(s,&s.gifts[gift_id])?,"from_id":{"_":"peerUser","user_id":uid},"peer":{"_":"peerUser","user_id":b.peer},"saved_id":id,"gift_num":num,"upgrade_stars":s.gifts[gift_id].upgrade,"convert_stars":s.gifts[gift_id].price});
                if let Some(m) = &b.message {
                    action["message"] = m.clone();
                }
                let m = Message {
                    id,
                    from: uid,
                    kind: "user".into(),
                    peer: b.peer,
                    date,
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
                super::messages::publish(s, uid, &m, false);
                a.bids.remove(&uid);
            }
            a.completed += 1;
            a.version += 1;
        }
        if a.completed == a.rounds {
            for (u, b) in std::mem::take(&mut a.bids) {
                s.credit(u, b.amount, "auction refund")?;
                a.refunded.push(u);
            }
        }
        s.auctions.insert(*gift_id, a);
        notify(s, *gift_id);
    }
    Ok(!due.is_empty())
}
pub fn handle(s: &State, uid: i64, v: &Value) -> Result<Value> {
    if text(v, "_") == "payments.getStarGiftActiveAuctions" {
        let auctions=s.auctions.iter().filter(|(_,a)|a.start>0 && a.completed<a.rounds).map(|(id,a)|Ok(json!({"_":"starGiftActiveAuctionState","gift":super::gifts::gift(s,&s.gifts[id])?,"state":state(a,s.gifts[id].sold),"user_state":user_state(a,uid)}))).collect::<Result<Vec<_>>>()?;
        let users = s
            .auctions
            .values()
            .flat_map(|a| {
                ranking(a)
                    .into_iter()
                    .filter(|(_, b)| !b.hidden)
                    .take(3)
                    .map(|(u, _)| s.user(u, uid))
            })
            .collect::<Vec<_>>();
        return Ok(
            json!({"_":"payments.starGiftActiveAuctions","auctions":auctions,"users":users,"chats":[]}),
        );
    }
    let id = if text(&v["auction"], "_") == "inputStarGiftAuctionSlug" {
        text(&v["auction"], "slug")
            .strip_prefix("IonAuction-")
            .and_then(|x| x.parse().ok())
            .context("STARGIFT_AUCTION_INVALID")?
    } else if !v["auction"].is_null() {
        int(&v["auction"], "gift_id")
    } else {
        int(v, "gift_id")
    };
    let a = s
        .auctions
        .get(&id)
        .filter(|a| a.start > 0)
        .context("STARGIFT_AUCTION_INVALID")?;
    if text(v, "_") == "payments.getStarGiftAuctionAcquiredGifts" {
        let gifts = a.acquired.get(&uid).cloned().unwrap_or_default();
        let users = gifts
            .iter()
            .map(|g| s.user(int(&g["peer"], "user_id"), uid))
            .collect::<Vec<_>>();
        return Ok(
            json!({"_":"payments.starGiftAuctionAcquiredGifts","gifts":gifts,"users":users,"chats":[]}),
        );
    }
    let users = ranking(a)
        .into_iter()
        .filter(|(_, b)| !b.hidden)
        .take(3)
        .map(|(u, _)| s.user(u, uid))
        .collect::<Vec<_>>();
    Ok(
        json!({"_":"payments.starGiftAuctionState","gift":super::gifts::gift(s,&s.gifts[&id])?,"state":state(a,s.gifts[&id].sold),"user_state":user_state(a,uid),"timeout":1,"users":users,"chats":[]}),
    )
}
