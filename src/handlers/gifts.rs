//! Internal Stars ledger and gifts. All mutations commit with the enclosing RPC transaction.
use super::*;
use crate::crypto::random_id;
use crate::store::{Form, Gift, OwnedGift};
fn amount(n: i64) -> Value {
    json!({"_":"starsAmount","amount":n,"nanos":0})
}
pub(super) fn gift(s: &State, g: &Gift) -> Result<Value> {
    let mut v = json!({"_":"starGift","id":g.id,"title":g.title,"sticker":s.doc(g.file)?,"stars":g.price,"convert_stars":g.price,"limited":true,"availability_remains":g.supply-g.sold,"availability_total":g.supply,"upgrade_stars":g.upgrade});
    if let Some(a) = s.auctions.get(&g.id) {
        v["auction"] = json!(true);
        v["auction_slug"] = json!(format!("IonAuction-{}", g.id));
        v["gifts_per_round"] = json!(a.per_round);
        v["auction_start_date"] = json!(a.start);
    }
    Ok(v)
}
fn style_catalog(s: &State, g: &Gift) -> Result<Vec<Value>> {
    let mut attrs = vec![
        json!({"_":"starGiftAttributeModel","name":"Original","document":s.doc(g.file)?,"rarity":{"_":"starGiftAttributeRarity","permille":1000}}),
    ];
    if s.gift_styles.get(&g.id).is_some_and(|a| {
        a.iter()
            .any(|v| v["_"] == "starGiftAttributeModel" && v["crafted"] != true)
    }) {
        attrs.clear();
    }
    attrs.extend(s.gift_styles.get(&g.id).cloned().unwrap_or_default());
    Ok(attrs)
}
pub fn assign_style(s: &mut State, id: i64) -> Result<()> {
    let o = s.owned.get(&id).context("GIFT_NOT_FOUND")?;
    if !o.upgraded || s.owned_styles.contains_key(&id) {
        return Ok(());
    }
    let catalog = style_catalog(s, &s.gifts[&o.gift])?;
    let mut selected = vec![];
    for kind in [
        "starGiftAttributeModel",
        "starGiftAttributePattern",
        "starGiftAttributeBackdrop",
    ] {
        let choices: Vec<_> = catalog
            .iter()
            .filter(|a| text(a, "_") == kind && a["crafted"] != true)
            .collect();
        if !choices.is_empty() {
            let total: i64 = choices.iter().map(|a| int(&a["rarity"], "permille")).sum();
            ensure!(total > 0, "GIFT_ATTRIBUTES_INVALID");
            let mut draw = random_id() % total;
            for a in choices {
                let weight = int(&a["rarity"], "permille");
                if draw < weight {
                    selected.push(a.clone());
                    break;
                }
                draw -= weight;
            }
        }
    }
    s.owned_styles.insert(id, selected);
    Ok(())
}
pub(super) fn unique(s: &State, o: &OwnedGift) -> Result<Value> {
    let g = &s.gifts[&o.gift];
    let mut value = json!({"_":"starGiftUnique","id":o.id,"gift_id":g.id,"title":g.title,"slug":format!("IonGift-{}",o.id),"num":o.id,"owner_id":{"_":"peerUser","user_id":o.owner},"attributes":s.owned_styles.get(&o.id).cloned().unwrap_or(style_catalog(s,g)?.into_iter().take(1).collect()),"availability_issued":g.sold,"availability_total":g.supply});
    value["crafted"] = json!(s.crafted_gifts.contains_key(&o.id));
    if super::craft::eligible(s, o) {
        value["craft_chance_permille"] = json!(334);
    }
    if o.resale > 0 {
        value["resell_amount"] = json!([amount(o.resale)]);
    }
    Ok(value)
}

pub(super) fn saved(s: &State, o: &OwnedGift) -> Result<Value> {
    let mut value = json!({"_":"savedStarGift","date":o.date,"gift":if o.upgraded{unique(s,o)?}else{gift(s,&s.gifts[&o.gift])?},"msg_id":o.id,"saved_id":o.id,"from_id":{"_":"peerUser","user_id":o.sender},"unsaved":o.hidden,"can_upgrade":!o.upgraded,"pinned_to_top":o.pinned,"convert_stars":o.price,"upgrade_stars":s.gifts[&o.gift].upgrade});
    if super::craft::eligible(s, o) {
        value["can_craft_at"] = json!(o.date);
    }
    Ok(value)
}
fn owned_id(v: &Value) -> i64 {
    let g = &v["stargift"];
    let id = int(g, "msg_id");
    if id != 0 { id } else { int(g, "saved_id") }
}
fn price(s: &State, v: &Value, uid: i64) -> Result<i64> {
    match text(v, "_") {
        "inputInvoiceStarGiftAuctionBid" => super::auctions::bid_price(s, uid, v),
        "inputInvoiceStarGift" => {
            let g = s.gifts.get(&int(v, "gift_id")).context("GIFT_NOT_FOUND")?;
            ensure!(!s.auctions.contains_key(&g.id), "STARGIFT_AUCTION_ONLY");
            ensure!(g.sold < g.supply, "STARGIFT_USAGE_LIMITED");
            let (_, u) = peer(&v["peer"], uid)?;
            ensure!(s.users.contains_key(&u), "USER_ID_INVALID");
            g.price
                .checked_add(if v["include_upgrade"] == true {
                    g.upgrade
                } else {
                    0
                })
                .context("PRICE_INVALID")
        }
        "inputInvoicePremiumGiftStars" => {
            let (_, u) = peer(&v["user_id"], uid)?;
            ensure!(s.users.contains_key(&u), "USER_ID_INVALID");
            Ok(match int(v, "months") {
                3 => 1000,
                6 => 1800,
                12 => 3000,
                _ => bail!("MONTHS_INVALID"),
            })
        }
        "inputInvoiceStarGiftResale" => {
            let id = text(v, "slug")
                .strip_prefix("IonGift-")
                .context("GIFT_NOT_FOUND")?
                .parse::<i64>()?;
            let g = s.owned.get(&id).context("GIFT_NOT_FOUND")?;
            ensure!(
                g.resale > 0 && g.upgraded && !g.converted && g.owner != uid,
                "GIFT_NOT_FOR_SALE"
            );
            Ok(g.resale)
        }
        _ => bail!("INVOICE_INVALID"),
    }
}
pub fn handle(s: &mut State, uid: i64, v: &Value) -> Result<Value> {
    let name = text(v, "_");
    match name {
        "payments.getStarGiftAuctionState"
        | "payments.getStarGiftAuctionAcquiredGifts"
        | "payments.getStarGiftActiveAuctions" => super::auctions::handle(s, uid, v),
        "payments.getCraftStarGifts" | "payments.craftStarGift" => super::craft::handle(s, uid, v),
        "payments.getStarsStatus"
        | "payments.getStarsTransactions"
        | "payments.getStarsTransactionsByID"
        | "payments.getStarsSubscriptions" => {
            let target = if v["peer"].is_null() {
                uid
            } else {
                peer(&v["peer"], uid)?.1
            };
            ensure!(target == uid, "PEER_ID_INVALID");
            // TON uses the same RPC family but a distinct balance constructor.
            // This server only maintains Stars; never expose its ledger as TON.
            if v["ton"] == true {
                return Ok(
                    json!({"_":"payments.starsStatus","balance":{"_":"starsTonAmount","amount":0},"history":[],"chats":[],"users":[]}),
                );
            }

            let cursor = text(v, "offset").parse::<i64>().unwrap_or(i64::MAX);
            let all: Vec<_> = s
                .ledger
                .iter()
                .rev()
                .filter(|e| {
                    e.user == uid
                        && e.id < cursor
                        && (v["inbound"] != true || e.delta > 0)
                        && (v["outbound"] != true || e.delta < 0)
                        && (name != "payments.getStarsTransactionsByID"
                            || list(v, "id")
                                .iter()
                                .any(|x| text(x, "id") == e.id.to_string()))
                })
                .collect();
            let history:Vec<_>=all.iter().take(100).map(|e|json!({"_":"starsTransaction","id":e.id.to_string(),"amount":amount(e.delta),"date":e.date,"peer":{"_":"starsTransactionPeerAPI"},"title":e.reason})).collect();
            let mut result = json!({"_":"payments.starsStatus","balance":amount(s.users[&uid].stars),"history":history,"chats":[],"users":[]});
            if all.len() > 100 {
                result["next_offset"] = json!(all[99].id.to_string())
            }
            Ok(result)
        }
        "payments.getStarGifts" => {
            let gifts: Vec<_> = s
                .gifts
                .values()
                .filter(|g| g.sold < g.supply && s.auctions.get(&g.id).is_none_or(|a| a.start > 0))
                .map(|g| gift(s, g))
                .collect::<Result<_>>()?;
            let hash = (s.gifts.len() as i32)
                .wrapping_add(s.gifts.values().map(|g| g.sold as i32).sum::<i32>())
                .wrapping_add(1);
            if int(v, "hash") == hash as i64 {
                Ok(json!({"_":"payments.starGiftsNotModified"}))
            } else {
                Ok(
                    json!({"_":"payments.starGifts","hash":hash,"gifts":gifts,"chats":[],"users":[]}),
                )
            }
        }
        "payments.getSavedStarGift" => {
            let mut gifts = vec![];
            for g in list(v, "stargift") {
                let id = if int(&g, "msg_id") != 0 {
                    int(&g, "msg_id")
                } else {
                    int(&g, "saved_id")
                };
                let o = s.owned.get(&id).context("GIFT_NOT_FOUND")?;
                ensure!(o.owner == uid && !o.converted, "GIFT_NOT_FOUND");
                gifts.push(saved(s, o)?);
            }
            Ok(
                json!({"_":"payments.savedStarGifts","count":gifts.len(),"gifts":gifts,"chats":[],"users":[]}),
            )
        }
        "payments.getStarGiftUpgradePreview" | "payments.getStarGiftUpgradeAttributes" => {
            let g = s.gifts.get(&int(v, "gift_id")).context("GIFT_NOT_FOUND")?;
            let attrs = style_catalog(s, g)?;
            if name.ends_with("Preview") {
                Ok(
                    json!({"_":"payments.starGiftUpgradePreview","sample_attributes":attrs,"prices":[{"_":"starGiftUpgradePrice","date":now(),"upgrade_stars":g.upgrade}],"next_prices":[]}),
                )
            } else {
                Ok(json!({"_":"payments.starGiftUpgradeAttributes","attributes":attrs}))
            }
        }
        "payments.toggleStarGiftsPinnedToTop" => {
            let (_, owner) = peer(&v["peer"], uid)?;
            ensure!(owner == uid, "PEER_ID_INVALID");
            let ids: Vec<_> = list(v, "stargift")
                .iter()
                .map(|g| {
                    let id = int(g, "msg_id");
                    if id != 0 { id } else { int(g, "saved_id") }
                })
                .collect();
            ensure!(ids.len() <= 6, "STARGIFTS_TOO_MUCH");
            for id in &ids {
                ensure!(
                    s.owned
                        .get(id)
                        .is_some_and(|o| o.owner == uid && !o.converted),
                    "GIFT_NOT_FOUND"
                );
            }
            for o in s.owned.values_mut().filter(|o| o.owner == uid) {
                o.pinned = ids.contains(&o.id);
            }
            Ok(json!(true))
        }
        "payments.getSavedStarGifts" => {
            let target = if v["peer"].is_null() {
                uid
            } else {
                peer(&v["peer"], uid)?.1
            };
            let offset = text(v, "offset").parse::<usize>().unwrap_or(0);
            let limit = int(v, "limit").clamp(1, 100) as usize;
            let all: Vec<_> = s
                .owned
                .values()
                .rev()
                .filter(|o| o.owner == target && !o.converted && (target == uid || !o.hidden))
                .collect();
            let count = all.len();
            let mut out = json!({"_":"payments.savedStarGifts","count":count,"gifts":all.into_iter().skip(offset).take(limit).map(|o|saved(s,o)).collect::<Result<Vec<_>>>()?,"chats":[],"users":[s.user(target,uid)]});
            if offset + limit < count {
                out["next_offset"] = json!((offset + limit).to_string())
            }
            Ok(out)
        }
        "payments.getPaymentForm" => {
            let inv = &v["invoice"];
            let price = price(s, inv, uid)?;
            let id = random_id();
            s.forms.insert(
                id,
                Form {
                    user: uid,
                    invoice: inv.clone(),
                    price,
                    expires: now() + 600,
                    paid: false,
                },
            );
            Ok(
                json!({"_":"payments.paymentFormStarGift","form_id":id,"invoice":{"_":"invoice","currency":"XTR","prices":[{"_":"labeledPrice","label":"IonGram","amount":price}]}}),
            )
        }
        "payments.sendStarsForm" => {
            let id = int(v, "form_id");
            let form = s.forms.get(&id).context("FORM_ID_INVALID")?.clone();
            ensure!(
                form.user == uid && form.invoice == v["invoice"],
                "FORM_ID_INVALID"
            );
            if form.paid {
                return Ok(
                    json!({"_":"payments.paymentResult","updates":updates(vec![],vec![],vec![])}),
                );
            }
            ensure!(form.expires > now(), "FORM_EXPIRED");
            ensure!(price(s, &form.invoice, uid)? == form.price, "FORM_EXPIRED");
            s.credit(uid, -form.price, "purchase")?;
            let inv = &form.invoice;
            match text(inv, "_") {
                "inputInvoiceStarGiftAuctionBid" => super::auctions::place_bid(s, uid, inv)?,
                "inputInvoiceStarGift" => {
                    let (_, owner) = peer(&inv["peer"], uid)?;
                    let gift_id = int(inv, "gift_id");
                    let id = s.id();
                    let g = s.gifts.get_mut(&gift_id).unwrap();
                    g.sold += 1;
                    s.owned.insert(
                        id,
                        OwnedGift {
                            id,
                            gift: gift_id,
                            owner,
                            sender: uid,
                            date: now(),
                            upgraded: inv["include_upgrade"] == true,
                            hidden: false,
                            price: g.price,
                            converted: false,
                            resale: 0,
                            pinned: false,
                        },
                    );
                    assign_style(s, id)?;
                }
                "inputInvoicePremiumGiftStars" => {
                    let (_, u) = peer(&inv["user_id"], uid)?;
                    let u = s.users.get_mut(&u).unwrap();
                    u.premium_until = u.premium_until.max(now()) + int(inv, "months") * 30 * 86400;
                }
                "inputInvoiceStarGiftResale" => {
                    let id = text(inv, "slug")
                        .strip_prefix("IonGift-")
                        .unwrap()
                        .parse::<i64>()?;
                    let (_, to) = peer(&inv["to_id"], uid)?;
                    ensure!(s.users.contains_key(&to), "USER_ID_INVALID");
                    let seller = s.owned[&id].owner;
                    s.credit(seller, form.price, "gift resale")?;
                    let o = s.owned.get_mut(&id).unwrap();
                    o.owner = to;
                    o.resale = 0;
                }
                _ => bail!("INVOICE_INVALID"),
            };
            s.forms.get_mut(&int(v, "form_id")).unwrap().paid = true;
            Ok(
                json!({"_":"payments.paymentResult","updates":updates(vec![],vec![s.user(uid,uid)],vec![])}),
            )
        }
        "payments.getUniqueStarGift" => {
            let id = text(v, "slug")
                .strip_prefix("IonGift-")
                .context("GIFT_NOT_FOUND")?
                .parse::<i64>()?;
            let o = s.owned.get(&id).context("GIFT_NOT_FOUND")?;
            ensure!(o.upgraded && !o.converted, "GIFT_NOT_FOUND");
            Ok(
                json!({"_":"payments.uniqueStarGift","gift":unique(s,o)?,"chats":[],"users":[s.user(o.owner,uid)]}),
            )
        }
        "payments.getResaleStarGifts" => {
            let gifts: Vec<_> = s
                .owned
                .values()
                .filter(|o| {
                    o.resale > 0
                        && !o.converted
                        && (int(v, "gift_id") == 0 || o.gift == int(v, "gift_id"))
                })
                .map(|o| unique(s, o))
                .collect::<Result<_>>()?;
            Ok(
                json!({"_":"payments.resaleStarGifts","count":gifts.len(),"gifts":gifts,"chats":[],"users":[]}),
            )
        }
        "payments.getStarsTopupOptions"
        | "payments.getStarsGiftOptions"
        | "payments.getPremiumGiftCodeOptions"
        | "payments.getStarsGiveawayOptions" => Ok(json!({"_":"vector","data":[]})),
        "payments.checkCanSendGift" => Ok(json!(true)),
        "payments.saveStarGift"
        | "payments.convertStarGift"
        | "payments.upgradeStarGift"
        | "payments.transferStarGift"
        | "payments.updateStarGiftPrice" => {
            let id = owned_id(v);
            let o = s.owned.get(&id).context("GIFT_NOT_FOUND")?.clone();
            ensure!(o.owner == uid && !o.converted, "GIFT_NOT_FOUND");
            match name {
                "payments.saveStarGift" => {
                    s.owned.get_mut(&id).unwrap().hidden = v["unsave"] == true;
                    Ok(json!(true))
                }
                "payments.convertStarGift" => {
                    ensure!(!o.upgraded, "STARGIFT_UPGRADED");
                    s.credit(uid, o.price, "gift conversion")?;
                    s.owned.get_mut(&id).unwrap().converted = true;
                    s.gifts.get_mut(&o.gift).unwrap().sold -= 1;
                    Ok(json!(true))
                }
                "payments.upgradeStarGift" => {
                    ensure!(!o.upgraded, "STARGIFT_ALREADY_UPGRADED");
                    let cost = s.gifts[&o.gift].upgrade;
                    s.credit(uid, -cost, "gift upgrade")?;
                    s.owned.get_mut(&id).unwrap().upgraded = true;
                    assign_style(s, id)?;
                    Ok(updates(vec![], vec![], vec![]))
                }
                "payments.transferStarGift" => {
                    let (_, to) = peer(&v["to_id"], uid)?;
                    ensure!(to != uid && s.users.contains_key(&to), "PEER_ID_INVALID");
                    let o = s.owned.get_mut(&id).unwrap();
                    o.owner = to;
                    o.resale = 0;
                    Ok(updates(vec![], vec![], vec![]))
                }
                _ => {
                    ensure!(o.upgraded, "STARGIFT_NOT_UPGRADED");
                    let n = int(&v["resell_amount"], "amount");
                    ensure!((0..=1000000000).contains(&n), "PRICE_INVALID");
                    s.owned.get_mut(&id).unwrap().resale = n;
                    Ok(updates(vec![], vec![], vec![]))
                }
            }
        }
        _ => bail!("METHOD_NOT_IMPLEMENTED"),
    }
}
