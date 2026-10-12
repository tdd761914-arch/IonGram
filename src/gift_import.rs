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
