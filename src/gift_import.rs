//! Administrator import of an external collection manifest; assets stay outside the source tree.
use crate::{
    crypto::{now, random, random_id},
    handlers::{auctions::Auction, files::validate_tgs},
    store::{File, Gift, State},
    tl::Schema,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
fn document(s: &mut State, data: &[u8], dir: &Path) -> Result<i64> {
    validate_tgs(data)?;
    let id = random_id();
    std::fs::create_dir_all(dir.join("files"))?;
    std::fs::write(dir.join("files").join(format!("{id}.bin")), data)?;
    s.files.insert(id,File{id,hash:random_id(),reference:random(24),size:data.len() as i64,mime:"application/x-tgsticker".into(),attrs:vec![json!({"_":"documentAttributeAnimated"}),json!({"_":"documentAttributeImageSize","w":512,"h":512}),json!({"_":"documentAttributeSticker","alt":"⭐","stickerset":{"_":"inputStickerSetEmpty"}})],owner:0,date:now()});
    Ok(id)
}
pub fn import(s: &mut State, manifest: &Path, dir: &Path, schemas: &[Schema; 2]) -> Result<i64> {
    let v: Value = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let n = |k: &str| v[k].as_i64().with_context(|| format!("missing {k}"));
    let id = n("id")?;
    let price = n("price")?;
    let supply = n("supply")?;
    let upgrade = n("upgrade")?;
    let rounds = n("rounds")?;
    let duration = n("duration")?;
    let per_round = n("per_round")?;
    ensure!(
        id > 0 && !s.gifts.contains_key(&id),
        "gift ID already exists or invalid"
    );
    ensure!(
        price > 0
            && upgrade >= 0
            && (1..=1000).contains(&rounds)
            && (1..=10000).contains(&per_round)
            && (10..=86400).contains(&duration)
            && rounds * per_round == supply,
        "invalid auction configuration"
    );
    let title = v["title"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .context("title missing")?;
    let base = manifest
        .parent()
        .context("manifest parent missing")?
        .canonicalize()?;
    let read = |name: &Value| -> Result<Vec<u8>> {
        let path = base
            .join(name.as_str().context("file missing")?)
            .canonicalize()?;
        ensure!(path.starts_with(&base), "file outside manifest directory");
        Ok(std::fs::read(path)?)
    };
    let original = read(&v["original"])?;
    validate_tgs(&original)?;
    let models = v["models"].as_array().context("models missing")?;
    ensure!(
        !models.is_empty() && models.len() <= 1000,
        "invalid model count"
    );
    let mut prepared = vec![];
    let mut names = BTreeSet::new();
    for m in models {
        let name = m["name"].as_str().context("model name missing")?;
        ensure!(
            !name.trim().is_empty() && names.insert(name.to_owned()),
            "duplicate or empty model name"
        );
        let weight = m["permille"].as_i64().context("model rarity missing")?;
        ensure!((0..=1000).contains(&weight), "invalid rarity");
        let data = read(&m["file"])?;
        validate_tgs(&data)?;
        prepared.push((name.to_owned(), weight, data));
    }
    ensure!(prepared.iter().any(|(_, w, _)| *w > 0), "no upgrade models");
    let file = document(s, &original, dir)?;
    s.gifts.insert(
        id,
        Gift {
            id,
            title: title.into(),
            file,
            price,
            supply,
            sold: 0,
            upgrade,
        },
    );
    let mut attrs = vec![];
    for (name, weight, data) in prepared {
        let file = document(s, &data, dir)?;
        let attr = json!({"_":"starGiftAttributeModel","crafted":weight==0,"name":name,"document":s.doc(file)?,"rarity":{"_":"starGiftAttributeRarity","permille":weight}});
        for schema in schemas {
            schema.encode(&attr)?;
        }
        attrs.push(attr);
    }
    s.gift_styles.insert(id, attrs);
    s.auctions.insert(
        id,
        Auction {
            start: 0,
            duration,
            rounds,
            per_round,
            minimum: price,
            completed: 0,
            version: 1,
            bids: Default::default(),
            acquired: Default::default(),
            refunded: vec![],
        },
    );
    Ok(id)
}

pub fn styles(
    s: &mut State,
    id: i64,
    manifest: &Path,
    dir: &Path,
    schemas: &[Schema; 2],
) -> Result<()> {
    ensure!(s.gifts.contains_key(&id), "gift not found");
    let v: Value = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let base = manifest
        .parent()
        .context("manifest directory missing")?
        .canonicalize()?;
    let patterns = v["patterns"].as_array().context("patterns missing")?;
    let backdrops = v["backdrops"].as_array().context("backdrops missing")?;
    ensure!(
        !patterns.is_empty()
            && patterns.len() <= 1000
            && !backdrops.is_empty()
            && backdrops.len() <= 1000,
        "invalid style count"
    );
    let mut attrs = vec![];
    let mut total = 0;
    for p in patterns {
        let weight = p["permille"].as_i64().context("pattern rarity missing")?;
        ensure!((1..=1000).contains(&weight), "invalid rarity");
        total += weight;
        let path = base
            .join(p["file"].as_str().context("pattern file missing")?)
            .canonicalize()?;
        ensure!(
            path.starts_with(&base),
            "pattern outside manifest directory"
        );
        let name = p["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("pattern name missing")?;
        let file = document(s, &std::fs::read(path)?, dir)?;
        attrs.push(json!({"_":"starGiftAttributePattern","name":name,"document":s.doc(file)?,"rarity":{"_":"starGiftAttributeRarity","permille":weight}}));
    }
    ensure!(total == 1000, "pattern rarities must total 1000");
    total = 0;
    let mut ids = BTreeSet::new();
    for b in backdrops {
        ensure!(
            b["_"] == "starGiftAttributeBackdrop"
                && b["name"].as_str().is_some_and(|s| !s.is_empty()),
            "invalid backdrop"
        );
        let id = b["backdrop_id"].as_i64().context("backdrop id missing")?;
        ensure!(
            (0..=i32::MAX as i64).contains(&id) && ids.insert(id),
            "invalid backdrop id"
        );
        for key in ["center_color", "edge_color", "pattern_color", "text_color"] {
            ensure!(
                b[key].as_i64().is_some_and(|n| (0..=0xffffff).contains(&n)),
                "invalid backdrop color"
            );
        }
        let weight = b["rarity"]["permille"]
            .as_i64()
            .context("backdrop rarity missing")?;
        ensure!((1..=1000).contains(&weight), "invalid rarity");
        total += weight;
        attrs.push(b.clone());
    }
    ensure!(total == 1000, "backdrop rarities must total 1000");
    if v["uniform"] == true {
        for a in &mut attrs {
            a["selection_weight"] = json!(1);
        }
    }
    for a in &attrs {
        for schema in schemas {
            schema.encode(a)?;
        }
    }
    let catalog = s.gift_styles.entry(id).or_default();
    catalog
        .retain(|a| a["_"] != "starGiftAttributePattern" && a["_"] != "starGiftAttributeBackdrop");
    catalog.extend(attrs);
    // Preserve previously selected models while filling missing style categories.
    let owned: Vec<_> = s
        .owned
        .values()
        .filter(|o| o.gift == id && o.upgraded && !o.converted)
        .map(|o| o.id)
        .collect();
    for oid in owned {
        let old = s.owned_styles.remove(&oid).unwrap_or_default();
        crate::handlers::gifts::assign_style(s, oid)?;
        let assigned = s.owned_styles.get_mut(&oid).unwrap();
        for previous in old {
            assigned.retain(|a| a["_"] != previous["_"]);
            assigned.push(previous);
        }
    }
    Ok(())
}
