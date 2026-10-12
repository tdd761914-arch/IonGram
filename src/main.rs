use anyhow::{Context, Result, ensure};
use iongram::{
    crypto::{now, random, random_id},
    handlers::files::validate_tgs,
    server::Server,
    store::{File, Gift},
};
use serde_json::json;
use std::path::PathBuf;
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "iongram=info".into()),
        )
        .init();
    let dir = PathBuf::from(std::env::var("IONGRAM_DATA").unwrap_or("data".into()));
    let args: Vec<_> = std::env::args().collect();
    let server = Server::new(dir.clone())?;
    match args.get(1).map(String::as_str).unwrap_or("serve") {
        "serve" => {
            let bind = std::env::var("IONGRAM_BIND").unwrap_or("127.0.0.1:4430".into());
            server.serve(&bind).await?
        }
        "key" => {
            println!(
                "Fingerprint: {} ({:016x})\nPublic key: {}",
                server.key.fingerprint,
                server.key.fingerprint,
                dir.join("public.pem").display()
            )
        }
        "credit" => {
            let uid = args
                .get(2)
                .context("credit USER_ID AMOUNT")?
                .parse::<i64>()?;
            let amount = args
                .get(3)
                .context("credit USER_ID AMOUNT")?
                .parse::<i64>()?;
            ensure!(amount > 0, "amount must be positive");
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            state.credit(uid, amount, "administrator topup")?;
            store.save(state)?;
            println!("Stars credited")
        }
        "premium" => {
            let uid = args
                .get(2)
                .context("premium USER_ID DAYS")?
                .parse::<i64>()?;
            let days = args
                .get(3)
                .context("premium USER_ID DAYS")?
                .parse::<i64>()?;
            ensure!((1..=36525).contains(&days), "days out of range");
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            let u = state.users.get_mut(&uid).context("unknown user")?;
            u.premium_until = u.premium_until.max(now()) + days * 86400;
            store.save(state)?;
            println!("Premium granted")
        }
        "gift-styles-import" => {
            let id = args
                .get(2)
                .context("gift-styles-import GIFT_ID MANIFEST.json")?
                .parse::<i64>()?;
            let manifest = PathBuf::from(args.get(3).context("manifest missing")?);
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            iongram::gift_import::styles(&mut state, id, &manifest, &dir, &server.schemas)?;
            store.save(state)?;
            println!("Gift {id} styles imported");
        }
        "import-auction" => {
            let manifest = PathBuf::from(args.get(2).context("import-auction MANIFEST.json")?);
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            let id = iongram::gift_import::import(&mut state, &manifest, &dir, &server.schemas)?;
            store.save(state)?;
            println!("Auction gift {id} imported; waiting for auction-start");
        }
        "auction-start" => {
            let id = args
                .get(2)
                .context("auction-start GIFT_ID")?
                .parse::<i64>()?;
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            let a = state.auctions.get_mut(&id).context("auction not found")?;
            ensure!(a.start == 0, "auction already started");
            a.start = now();
            a.version += 1;
            println!(
                "Auction {id} starts at {}, ends at {}",
                a.start,
                a.start + a.rounds * a.duration
            );
            store.save(state)?;
        }
        "import-gift" => {
            let path = args
                .get(2)
                .context("import-gift FILE.tgs TITLE PRICE SUPPLY")?;
            let title = args.get(3).context("title missing")?;
            let price = args.get(4).context("price missing")?.parse::<i64>()?;
            let supply = args.get(5).context("supply missing")?.parse::<i64>()?;
            ensure!(
                price > 0 && supply > 0 && supply <= i32::MAX as i64,
                "invalid price or supply"
            );
            let data = std::fs::read(path)?;
            validate_tgs(&data)?;
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            let id = state.id();
            let fid = random_id();
            std::fs::create_dir_all(dir.join("files"))?;
            std::fs::write(dir.join("files").join(format!("{fid}.bin")), &data)?;
            state.files.insert(fid,File{id:fid,hash:random_id(),reference:random(24),size:data.len()as i64,mime:"application/x-tgsticker".into(),attrs:vec![json!({"_":"documentAttributeAnimated"}),json!({"_":"documentAttributeImageSize","w":512,"h":512}),json!({"_":"documentAttributeSticker","alt":"⭐","stickerset":{"_":"inputStickerSetEmpty"}})],owner:0,date:now()});
            state.gifts.insert(
                id,
                Gift {
                    id,
                    title: title.clone(),
                    file: fid,
                    price,
                    supply,
                    sold: 0,
                    upgrade: 50,
                },
            );
            store.save(state)?;
            println!("Gift {id} imported")
        }
        "craft" => {
            let uid = args
                .get(2)
                .context("craft USER_ID GIFT_ID GIFT_ID GIFT_ID")?
                .parse::<i64>()?;
            ensure!(args.len() == 6, "exactly three gift instance IDs required");
            let ids: Vec<i64> = args[3..]
                .iter()
                .map(|s| s.parse())
                .collect::<std::result::Result<_, _>>()?;
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            ensure!(state.users.contains_key(&uid), "user not found");
            let result = iongram::handlers::craft::handle(
                &mut state,
                uid,
                &json!({"_":"payments.craftStarGift","stargift":ids.iter().map(|id|json!({"_":"inputSavedStarGiftUser","msg_id":id})).collect::<Vec<_>>()}),
            )?;
            let id = result["updates"][0]["message"]["id"].clone();
            store.save(state)?;
            println!("Crafted gift {id}");
        }
        "gift-craft-model" => {
            let gift_id = args
                .get(2)
                .context("gift-craft-model GIFT_ID MODEL.tgs NAME")?
                .parse::<i64>()?;
            let data = std::fs::read(args.get(3).context("model missing")?)?;
            validate_tgs(&data)?;
            let name = args.get(4).context("name missing")?;
            ensure!(!name.trim().is_empty(), "empty model name");
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            ensure!(state.gifts.contains_key(&gift_id), "gift not found");
            ensure!(
                !state.gift_styles.get(&gift_id).is_some_and(|a| a
                    .iter()
                    .any(|a| a["_"] == "starGiftAttributeModel" && a["name"] == *name)),
                "model already registered"
            );
            let fid = random_id();
            state.files.insert(fid,File{id:fid,hash:random_id(),reference:random(24),size:data.len()as i64,mime:"application/x-tgsticker".into(),attrs:vec![json!({"_":"documentAttributeAnimated"}),json!({"_":"documentAttributeImageSize","w":512,"h":512}),json!({"_":"documentAttributeSticker","alt":"⬡","stickerset":{"_":"inputStickerSetEmpty"}})],owner:0,date:now()});
            let model = json!({"_":"starGiftAttributeModel","crafted":true,"name":name,"document":state.doc(fid)?,"rarity":{"_":"starGiftAttributeRarity","permille":1000}});
            let attrs = state.gift_styles.entry(gift_id).or_default();
            attrs.push(model);
            let count = attrs
                .iter()
                .filter(|a| a["_"] == "starGiftAttributeModel" && a["crafted"] == true)
                .count() as i64;
            let mut index = 0;
            for a in attrs
                .iter_mut()
                .filter(|a| a["_"] == "starGiftAttributeModel" && a["crafted"] == true)
            {
                a["rarity"]["permille"] =
                    json!(1000 / count + if index < 1000 % count { 1 } else { 0 });
                index += 1;
            }
            std::fs::create_dir_all(dir.join("files"))?;
            std::fs::write(dir.join("files").join(format!("{fid}.bin")), data)?;
            store.save(state)?;
            println!("Craft model {name} registered for gift {gift_id}");
        }
        "gift-style" => {
            let gift_id = args
                .get(2)
                .context("gift-style GIFT_ID PATTERN.tgs BACKDROPS.json")?
                .parse::<i64>()?;
            let data = std::fs::read(args.get(3).context("pattern file missing")?)?;
            validate_tgs(&data)?;
            let backdrops: Vec<serde_json::Value> =
                serde_json::from_slice(&std::fs::read(args.get(4).context("backdrops missing")?)?)?;
            ensure!(
                !backdrops.is_empty() && backdrops.len() <= 100,
                "invalid backdrops"
            );
            let mut ids = std::collections::BTreeSet::new();
            let mut total = 0;
            for a in &backdrops {
                ensure!(
                    a["_"] == "starGiftAttributeBackdrop"
                        && a["name"].as_str().is_some_and(|n| !n.is_empty()),
                    "invalid backdrop"
                );
                ensure!(
                    a["backdrop_id"]
                        .as_i64()
                        .is_some_and(|n| n > 0 && n <= i32::MAX as i64 && ids.insert(n)),
                    "invalid backdrop id"
                );
                for color in ["center_color", "edge_color", "pattern_color", "text_color"] {
                    ensure!(
                        a[color]
                            .as_i64()
                            .is_some_and(|c| (0..=0xffffff).contains(&c)),
                        "invalid RGB color"
                    );
                }
                let rarity = a["rarity"]["permille"].as_i64().context("rarity missing")?;
                ensure!((1..=1000).contains(&rarity), "invalid rarity");
                total += rarity;
                server.schemas[0].encode(a)?;
                server.schemas[1].encode(a)?;
            }
            ensure!(total == 1000, "backdrop rarity must total 1000");
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            let title = state
                .gifts
                .get(&gift_id)
                .context("gift not found")?
                .title
                .clone();
            let fid = random_id();
            state.files.insert(fid,File{id:fid,hash:random_id(),reference:random(24),size:data.len() as i64,mime:"application/x-tgsticker".into(),attrs:vec![json!({"_":"documentAttributeAnimated"}),json!({"_":"documentAttributeImageSize","w":512,"h":512}),json!({"_":"documentAttributeSticker","alt":"⬡","stickerset":{"_":"inputStickerSetEmpty"}})],owner:0,date:now()});
            let mut attrs = vec![
                json!({"_":"starGiftAttributePattern","name":title,"document":state.doc(fid)?,"rarity":{"_":"starGiftAttributeRarity","permille":1000}}),
            ];
            attrs.extend(backdrops);
            state.gift_styles.insert(gift_id, attrs);
            let owned: Vec<_> = state
                .owned
                .values()
                .filter(|o| o.gift == gift_id && o.upgraded)
                .map(|o| o.id)
                .collect();
            for id in owned {
                if state
                    .owned_styles
                    .get(&id)
                    .is_some_and(|a| a.iter().any(|v| v["_"] == "starGiftAttributeBackdrop"))
                {
                    continue;
                }
                state.owned_styles.remove(&id);
                iongram::handlers::gifts::assign_style(&mut state, id)?;
            }
            std::fs::create_dir_all(dir.join("files"))?;
            std::fs::write(dir.join("files").join(format!("{fid}.bin")), data)?;
            store.save(state)?;
            println!("Gift {gift_id}: pattern and backgrounds configured");
        }
        "collectible" => {
            let kind = args
                .get(2)
                .context("collectible user|channel OWNER_ID USERNAME")?;
            let owner = args.get(3).context("owner missing")?.parse::<i64>()?;
            let name = args.get(4).context("username missing")?;
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            iongram::handlers::fragment::mint(&mut state, kind, owner, name)?;
            store.save(state)?;
            println!("Collectible username assigned");
        }
        "users" => {
            let store = server.store.lock().await;
            for u in store.state.users.values() {
                println!(
                    "{} {} stars={} premium_until={}",
                    u.id, u.first, u.stars, u.premium_until
                )
            }
        }
        _ => anyhow::bail!("commands: serve, key, users, credit, premium, import-gift"),
    };
    Ok(())
}
