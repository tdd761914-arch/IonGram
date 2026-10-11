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
            ensure!((1..=3650).contains(&days), "days out of range");
            let mut store = server.store.lock().await;
            let mut state = store.state.clone();
            let u = state.users.get_mut(&uid).context("unknown user")?;
            u.premium_until = u.premium_until.max(now()) + days * 86400;
            store.save(state)?;
            println!("Premium granted")
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
