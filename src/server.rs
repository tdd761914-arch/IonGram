use crate::{
    crypto::{self, Handshake, ServerKey},
    handlers,
    store::{Event, Key, Store},
    tl::{Reader, Schema},
    transport::{Sender, Transport},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::VecDeque, io::Read, path::PathBuf, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, Semaphore, broadcast},
    time::{Duration, timeout},
};
pub struct Server {
    pub store: Mutex<Store>,
    pub key: ServerKey,
    pub dir: PathBuf,
    pub events: broadcast::Sender<Event>,
    pub schemas: [Schema; 2],
}
impl Server {
    pub fn new(dir: PathBuf) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let key = ServerKey::load(&dir)?;
        let store = Store::open(&dir.join("iongram.sqlite"))?;
        let (events, _) = broadcast::channel(1024);
        Ok(Arc::new(Self {
            store: Mutex::new(store),
            key,
            dir,
            events,
            schemas: [Schema::for_layer(229)?, Schema::for_layer(231)?],
        }))
    }
    pub async fn serve(self: Arc<Self>, bind: &str) -> Result<()> {
        let listener = TcpListener::bind(bind).await?;
        tracing::info!(%bind,fingerprint=self.key.fingerprint,"IonGram listening; API layers 229, 231");
        let slots = Arc::new(Semaphore::new(128));
        loop {
            let (socket, addr) = listener.accept().await?;
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                continue;
            };
            let server = self.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Err(e) = server.connection(socket).await {
                    tracing::debug!(%addr,error=%e,"connection closed")
                }
            });
        }
    }
    async fn connection(self: Arc<Self>, socket: TcpStream) -> Result<()> {
        socket.set_nodelay(true)?;
        let (mut rx, mut tx) =
            timeout(Duration::from_secs(15), Transport::accept(socket)).await??;
        let (frames_tx, mut frames) = tokio::sync::mpsc::channel(8);
        let reader = tokio::spawn(async move {
            loop {
                let frame = timeout(Duration::from_secs(180), rx.recv()).await;
                let frame = match frame {
                    Ok(Ok(f)) => f,
                    _ => break,
                };
                if frames_tx.send((frame, rx.quick)).await.is_err() {
                    break;
                }
            }
        });
        let result = self.session(&mut tx, &mut frames).await;
        reader.abort();
        result
    }
    async fn session(
        &self,
        tx: &mut Sender,
        frames: &mut tokio::sync::mpsc::Receiver<(Vec<u8>, bool)>,
    ) -> Result<()> {
        let mut hs: Option<Handshake> = None;
        let mut session = Session::default();
        let mut pushes = self.events.subscribe();
        loop {
            tokio::select! {
                        frame=frames.recv()=>{let Some((packet,quick))=frame else{return Ok(())};ensure!(packet.len()>=20,"short MTProto packet");let key_id=i64::from_le_bytes(packet[..8].try_into()?);if key_id==0{let mut r=Reader::new(&packet[8..]);let mid=r.long()?;ensure!(mid&3==0,"client msg_id");let n=r.int()?;ensure!(n>=0,"negative body length");let obj=self.schemas[1].decode(r.take(n as usize)?)?;let response=match handlers::text(&obj,"_"){
                        "msgs_ack"=>continue,
                        "req_pq"|"req_pq_multi"=>{hs=Some(Handshake::new(obj["nonce"].clone()));hs.as_ref().unwrap().pq(&self.key)},
                        "req_DH_params"=>hs.as_mut().context("DH state missing")?.dh(&obj,&self.key,&self.schemas[1])?,
                        "set_client_DH_params"=>{let(resp,data,salt)=hs.as_mut().context("DH state missing")?.finish(&obj,&self.schemas[1])?;let mut store=self.store.lock().await;let mut state=store.state.clone();state.keys.insert(crypto::auth_id(&data),Key{data,salt,user:None});store.save(state)?;hs=None;resp},_=>anyhow::bail!("unexpected unencrypted request: {}", handlers::text(&obj,"_"))};let body=self.schemas[1].encode(&response)?;let mut out=vec![0;8];out.extend(session.msg_id(true).to_le_bytes());out.extend((body.len() as i32).to_le_bytes());out.extend(body);tx.send(out).await?;continue}
                        let key={self.store.lock().await.state.keys.get(&key_id).cloned()};let Some(key)=key else{tx.send((-404i32).to_le_bytes().to_vec()).await?;return Ok(())};ensure!(session.key_id==0||session.key_id==key_id,"auth key changed");let(plain,ack)=crypto::decrypt(&key.data,&packet)?;if quick{tx.ack(ack).await?}let mut r=Reader::new(&plain);let salt=r.long()?;let sid=r.long()?;let mid=r.long()?;let seq=r.int()?;let n=r.int()?;let body=r.take(n as usize)?.to_vec();ensure!(mid&3==0,"invalid client msg_id");let seconds=mid>>32;ensure!((crypto::now()-seconds).abs()<=300,"msg_id time window");ensure!(session.session_id==0||session.session_id==sid,"session changed");session.key_id=key_id;session.session_id=sid;session.auth=key.data.clone();session.salt=key.salt;session.uid=key.user.unwrap_or(0);
                        if salt!=key.salt{session.send(tx,&self.schemas[session.layer],json!({"_":"bad_server_salt","bad_msg_id":mid,"bad_msg_seqno":seq,"error_code":48,"new_server_salt":key.salt}),false).await?;continue}
                        if !session.started{session.started=true;session.send(tx,&self.schemas[session.layer],json!({"_":"new_session_created","first_msg_id":mid,"unique_id":crypto::random_id(),"server_salt":key.salt}),true).await?}
                        let mut queue=VecDeque::from([(mid,seq,body)]);let mut processed=0;let mut acks=vec![];
                        while let Some((mid,seq,body))=queue.pop_front(){processed+=1;ensure!(processed<=1024,"container limit");ensure!(seq>=0&&mid&3==0,"invalid message envelope");if session.seen.contains(&mid){continue}
            if body.len()>=4&&u32::from_le_bytes(body[..4].try_into()?)==0x73f1f8dc{let mut r=Reader::new(&body[4..]);let count=r.int()?;ensure!((0..=1024).contains(&count),"container count");for _ in 0..count{let id=r.long()?;let sq=r.int()?;let n=r.int()?;ensure!(n>=0,"container size");queue.push_back((id,sq,r.take(n as usize)?.to_vec()))}ensure!(r.pos==body.len()-4,"container trailing bytes");continue}
                        session.seen.push_back(mid);if session.seen.len()>4096{session.seen.pop_front();}
            if seq&1==1{acks.push(mid)}let obj=match decode_request(&self.schemas,&mut session.layer,&body){Ok(v)=>v,Err(e)=>{tracing::debug!(error=%e,"TL request rejected");session.rpc(tx,&self.schemas[session.layer],mid,json!({"_":"rpc_error","error_code":400,"error_message":"INPUT_CONSTRUCTOR_INVALID"})).await?;continue}};let name=handlers::text(&obj,"_");match name{
                        "msgs_ack"|"http_wait"=>continue,
                        "ping"|"ping_delay_disconnect"=>{session.send(tx,&self.schemas[session.layer],json!({"_":"pong","msg_id":mid,"ping_id":obj["ping_id"]}),false).await?;continue},
                        "get_future_salts"=>{session.send(tx,&self.schemas[session.layer],json!({"_":"future_salts","req_msg_id":mid,"now":crypto::now(),"salts":[{"_":"future_salt","valid_since":crypto::now()-300,"valid_until":crypto::now()+1800,"salt":key.salt}]}),false).await?;continue},
                        _=>{let mut store=self.store.lock().await;let mut state=store.state.clone();let before=state.pts;let uid=state.keys.get(&key_id).and_then(|k|k.user).unwrap_or(0);let result=handlers::dispatch(&mut state,uid,key_id,&obj,&self.schemas[session.layer],&self.dir);let result=match result{
                        Ok(v)=>{if let Err(e)=encode_result(&self.schemas[session.layer],&v){tracing::error!(method=name,error=%e,"response schema mismatch");json!({"_":"rpc_error","error_code":500,"error_message":"INTERNAL_SERVER_ERROR"})}else{let events=state.events.iter().filter(|e|e.pts>before).cloned().collect::<Vec<_>>();store.save(state)?;session.uid=store.state.keys.get(&key_id).and_then(|k|k.user).unwrap_or(0);for e in events{let _=self.events.send(e);}v}},
                        Err(e)=>{if name=="auth.signIn"{store.save(state)?;}let error=e.to_string();let safe=error.len()<100&&error.bytes().all(|b|b.is_ascii_uppercase()||b.is_ascii_digit()||b==b'_');if !safe{tracing::error!(method=name,error=%e,"handler failed")}json!({"_":"rpc_error","error_code":if safe{400}else{500},"error_message":if safe{error.as_str()}else{"INTERNAL_SERVER_ERROR"}})}};drop(store);session.rpc(tx,&self.schemas[session.layer],mid,result).await?;}}
                        }
            if !acks.is_empty(){session.send(tx,&self.schemas[session.layer],json!({"_":"msgs_ack","msg_ids":acks}),false).await?}
                        },
                        event=pushes.recv(),if session.uid>0=>{match event{Ok(e)if e.users.contains(&session.uid)=>{let update=handlers::messages::event_view(&e,session.uid);let u=handlers::updates(vec![],vec![],vec![update]);session.send(tx,&self.schemas[session.layer],u,true).await?},Err(broadcast::error::RecvError::Lagged(_))=>{session.send(tx,&self.schemas[session.layer],json!({"_":"updatesTooLong"}),true).await?},_=>{}}}
                        }
        }
    }
}
fn decode_request(s: &[Schema; 2], layer: &mut usize, body: &[u8]) -> Result<Value> {
    let mut data = body.to_vec();
    for _ in 0..16 {
        ensure!(data.len() >= 4, "TL body");
        let id = u32::from_le_bytes(data[..4].try_into()?);
        if id == 0xda9b0d0d {
            ensure!(data.len() >= 8, "layer header");
            *layer = match i32::from_le_bytes(data[4..8].try_into()?) {
                229 => 0,
                231 => 1,
                _ => anyhow::bail!("LAYER_INVALID"),
            };
            data = data[8..].to_vec();
            continue;
        }
        if id == 0x3072cfa1 {
            let packed = Reader::new(&data[4..]).bytes()?;
            let mut out = vec![];
            flate2::read::GzDecoder::new(&packed[..])
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut out)?;
            ensure!(out.len() <= 4 * 1024 * 1024, "gzip limit");
            data = out;
            continue;
        }
        let mut v = s[*layer].decode(&data)?;
        for _ in 0..16 {
            if [
                "initConnection",
                "invokeAfterMsg",
                "invokeAfterMsgs",
                "invokeWithoutUpdates",
            ]
            .contains(&handlers::text(&v, "_"))
            {
                v = v["query"].take()
            } else {
                return Ok(v);
            }
        }
        anyhow::bail!("wrapper limit")
    }
    anyhow::bail!("wrapper limit")
}
fn encode_result(schema: &Schema, v: &Value) -> Result<Vec<u8>> {
    if v["_"] == "vector" {
        let mut b = 0x1cb5c415u32.to_le_bytes().to_vec();
        let a = handlers::list(v, "data");
        b.extend((a.len() as i32).to_le_bytes());
        for x in a {
            b.extend(schema.encode(&x)?)
        }
        Ok(b)
    } else {
        schema.encode(v)
    }
}
#[derive(Default)]
struct Session {
    key_id: i64,
    session_id: i64,
    auth: Vec<u8>,
    salt: i64,
    seq: i32,
    last: i64,
    started: bool,
    uid: i64,
    layer: usize,
    seen: VecDeque<i64>,
}
impl Session {
    fn msg_id(&mut self, reply: bool) -> i64 {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let n = ((duration.as_secs() << 32) | ((duration.subsec_nanos() as u64) << 2)) as i64;
        self.last = (n.max(self.last + 4) & !3) | if reply { 1 } else { 3 };
        self.last
    }
    async fn bytes(&mut self, tx: &mut Sender, body: Vec<u8>, content: bool) -> Result<()> {
        let mut plain = self.salt.to_le_bytes().to_vec();
        plain.extend(self.session_id.to_le_bytes());
        plain.extend(self.msg_id(true).to_le_bytes());
        plain.extend((self.seq * 2 + i32::from(content)).to_le_bytes());
        if content {
            self.seq += 1
        }
        plain.extend((body.len() as i32).to_le_bytes());
        plain.extend(body);
        tx.send(crypto::encrypt(&self.auth, plain)).await
    }
    async fn send(&mut self, tx: &mut Sender, s: &Schema, v: Value, content: bool) -> Result<()> {
        self.bytes(tx, s.encode(&v)?, content).await
    }
    async fn rpc(&mut self, tx: &mut Sender, s: &Schema, mid: i64, v: Value) -> Result<()> {
        let mut b = 0xf35c6d01u32.to_le_bytes().to_vec();
        b.extend(mid.to_le_bytes());
        b.extend(encode_result(s, &v)?);
        self.bytes(tx, b, true).await
    }
}
