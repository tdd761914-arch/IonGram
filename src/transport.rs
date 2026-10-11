use anyhow::{Result, bail, ensure};
use grammers_crypto::ObfuscatedCipher;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
#[derive(Clone, Copy)]
enum Mode {
    Abridged,
    Intermediate,
    Padded,
    Full,
}
pub struct Transport;
pub struct Receiver {
    stream: tokio::net::tcp::OwnedReadHalf,
    mode: Mode,
    cipher: Option<ObfuscatedCipher>,
    rx: u32,
    first: Option<[u8; 4]>,
    pub quick: bool,
    prefix: Vec<u8>,
}
pub struct Sender {
    stream: tokio::net::tcp::OwnedWriteHalf,
    mode: Mode,
    cipher: Option<ObfuscatedCipher>,
    tx: u32,
}
impl Transport {
    pub async fn accept(mut stream: TcpStream) -> Result<(Receiver, Sender)> {
        let first = stream.read_u8().await?;
        let mut head = [0; 4];
        head[0] = first;
        let mut cipher = None;
        let mut tx_cipher = None;
        let mut leftover = None;
        let mut prefix = vec![];
        let mode = if first == 0xef {
            Mode::Abridged
        } else {
            stream.read_exact(&mut head[1..]).await?;
            match head {
                [0xee, 0xee, 0xee, 0xee] => Mode::Intermediate,
                [0xdd, 0xdd, 0xdd, 0xdd] => Mode::Padded,
                _ => {
                    let n = u32::from_le_bytes(head) as usize;
                    let mut second = [0; 4];
                    stream.read_exact(&mut second).await?;
                    if (12..=MAX_FRAME).contains(&n) && n.is_multiple_of(4) && second == [0; 4] {
                        leftover = Some(head);
                        prefix = second.to_vec();
                        Mode::Full
                    } else {
                        let mut init = [0; 64];
                        init[..4].copy_from_slice(&head);
                        init[4..8].copy_from_slice(&second);
                        stream.read_exact(&mut init[8..]).await?;
                        tx_cipher = Some(ObfuscatedCipher::new(&init));
                        let mut c = ObfuscatedCipher::new(&init);
                        let mut decrypted = init;
                        c.encrypt(&mut decrypted);
                        let tag = &decrypted[56..60];
                        let mode = match tag {
                            [0xef, 0xef, 0xef, 0xef] => Mode::Abridged,
                            [0xee, 0xee, 0xee, 0xee] => Mode::Intermediate,
                            [0xdd, 0xdd, 0xdd, 0xdd] => Mode::Padded,
                            _ => bail!("invalid obfuscated transport"),
                        };
                        cipher = Some(c);
                        mode
                    }
                }
            }
        };
        let (reader, writer) = stream.into_split();
        Ok((
            Receiver {
                stream: reader,
                mode,
                cipher,
                rx: 0,
                first: leftover,
                quick: false,
                prefix,
            },
            Sender {
                stream: writer,
                mode,
                cipher: tx_cipher,
                tx: 0,
            },
        ))
    }
}
impl Receiver {
    async fn read(&mut self, n: usize) -> Result<Vec<u8>> {
        ensure!(n <= MAX_FRAME, "frame limit");
        let mut b = vec![0; n];
        let ready = n.min(self.prefix.len());
        b[..ready].copy_from_slice(&self.prefix[..ready]);
        self.prefix.drain(..ready);
        self.stream.read_exact(&mut b[ready..]).await?;
        if let Some(c) = &mut self.cipher {
            c.encrypt(&mut b)
        }
        Ok(b)
    }
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        match self.mode {
            Mode::Abridged => {
                let b = self.read(1).await?[0];
                self.quick = b & 0x80 != 0;
                let n = if b & 0x7f == 0x7f {
                    let a = self.read(3).await?;
                    u32::from_le_bytes([a[0], a[1], a[2], 0]) as usize
                } else {
                    (b & 0x7f) as usize
                };
                ensure!(n > 0 && n <= MAX_FRAME / 4, "frame limit");
                self.read(n * 4).await
            }
            Mode::Intermediate | Mode::Padded => {
                let n = u32::from_le_bytes(self.read(4).await?.try_into().unwrap());
                self.quick = n & 0x80000000 != 0;
                let n = (n & 0x7fffffff) as usize;
                ensure!((1..=MAX_FRAME).contains(&n), "frame limit");
                self.read(n).await
            }
            Mode::Full => {
                let head = if let Some(h) = self.first.take() {
                    h
                } else {
                    self.read(4).await?.try_into().unwrap()
                };
                let n = u32::from_le_bytes(head) as usize;
                ensure!((12..=MAX_FRAME).contains(&n), "frame limit");
                let rest = self.read(n - 4).await?;
                ensure!(
                    u32::from_le_bytes(rest[..4].try_into()?) == self.rx,
                    "transport sequence"
                );
                let mut all = head.to_vec();
                all.extend(&rest[..rest.len() - 4]);
                ensure!(
                    crc32fast::hash(&all) == u32::from_le_bytes(rest[rest.len() - 4..].try_into()?),
                    "CRC mismatch"
                );
                self.rx += 1;
                Ok(rest[4..rest.len() - 4].to_vec())
            }
        }
    }
}
impl Sender {
    async fn write(&mut self, mut b: Vec<u8>) -> Result<()> {
        if let Some(c) = &mut self.cipher {
            c.decrypt(&mut b)
        }
        self.stream.write_all(&b).await?;
        Ok(())
    }
    pub async fn send(&mut self, mut data: Vec<u8>) -> Result<()> {
        let mut b = vec![];
        match self.mode {
            Mode::Abridged => {
                ensure!(data.len().is_multiple_of(4), "abridged alignment");
                let n = data.len() / 4;
                if n < 127 {
                    b.push(n as u8)
                } else {
                    b.push(127);
                    b.extend_from_slice(&(n as u32).to_le_bytes()[..3])
                }
            }
            Mode::Intermediate => b.extend_from_slice(&(data.len() as u32).to_le_bytes()),
            Mode::Padded => {
                data.extend(crate::crypto::random(
                    (crate::crypto::random_id() % 16) as usize,
                ));
                b.extend_from_slice(&(data.len() as u32).to_le_bytes())
            }
            Mode::Full => {
                b.extend_from_slice(&((data.len() + 12) as u32).to_le_bytes());
                b.extend_from_slice(&self.tx.to_le_bytes());
                self.tx += 1;
            }
        }
        b.extend(data);
        if matches!(self.mode, Mode::Full) {
            b.extend_from_slice(&crc32fast::hash(&b).to_le_bytes())
        }
        self.write(b).await
    }
    pub async fn ack(&mut self, token: u32) -> Result<()> {
        let n = token | 0x80000000;
        match self.mode {
            Mode::Abridged => self.write(n.to_be_bytes().to_vec()).await,
            Mode::Intermediate | Mode::Padded => self.write(n.to_le_bytes().to_vec()).await,
            Mode::Full => Ok(()),
        }
    }
}
