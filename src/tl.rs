//! Schema-driven TL codec. The schema is the pinned TGScheme API, not a JSON API.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::HashMap;
#[derive(Clone, Debug)]
struct Field {
    name: String,
    ty: String,
    flag: Option<(String, u32)>,
}
#[derive(Clone, Debug)]
struct Constructor {
    id: u32,
    name: String,
    fields: Vec<Field>,
}
#[derive(Clone)]
pub struct Schema {
    ids: HashMap<u32, Constructor>,
    names: HashMap<String, u32>,
}
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(
            n <= self.data.len().saturating_sub(self.pos),
            "truncated TL"
        );
        let p = self.pos;
        self.pos += n;
        Ok(&self.data[p..p + n])
    }
    pub fn int(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub fn long(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into()?))
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        let first = self.take(1)?[0];
        let (n, h) = if first == 254 {
            let b = self.take(3)?;
            (
                (b[0] as usize) | ((b[1] as usize) << 8) | ((b[2] as usize) << 16),
                4,
            )
        } else {
            ensure!(first < 254, "invalid TL length");
            (first as usize, 1)
        };
        let b = self.take(n)?.to_vec();
        self.take((4 - (n + h) % 4) % 4)?;
        Ok(b)
    }
}
pub fn put_bytes(out: &mut Vec<u8>, b: &[u8]) -> Result<()> {
    ensure!(b.len() < 1 << 24, "TL string too long");
    let h = if b.len() < 254 {
        out.push(b.len() as u8);
        1
    } else {
        out.push(254);
        out.extend_from_slice(&(b.len() as u32).to_le_bytes()[..3]);
        4
    };
    out.extend_from_slice(b);
    out.resize(out.len() + (4 - (b.len() + h) % 4) % 4, 0);
    Ok(())
}
pub fn bytes(v: &Value) -> Result<Vec<u8>> {
    v.as_array()
        .context("expected byte array")?
        .iter()
        .map(|v| {
            let n = v.as_u64().context("invalid byte")?;
            ensure!(n < 256, "invalid byte");
            Ok(n as u8)
        })
        .collect()
}
impl Schema {
    pub fn load() -> Result<Self> {
        Self::for_layer(231)
    }
    pub fn for_layer(layer: i64) -> Result<Self> {
        ensure!(layer == 229 || layer == 231, "unsupported API layer");
        let mut s = Self {
            ids: HashMap::new(),
            names: HashMap::new(),
        };
        s.parse(
            if layer == 229 {
                include_str!("../schema/api_229.tl")
            } else {
                include_str!("../schema/api.tl")
            },
            false,
        )?;
        s.parse(include_str!("../schema/mtproto.tl"), true)?;
        s.parse("rpc_result#f35c6d01 req_msg_id:long result:Object = RpcResult;\ngzip_packed#3072cfa1 packed_data:bytes = Object;",false)?;
        Ok(s)
    }
    fn parse(&mut self, src: &str, core: bool) -> Result<()> {
        for line in src.lines() {
            let line = line.split("//").next().unwrap().trim();
            let Some((left, _)) = line.split_once(" = ") else {
                continue;
            };
            let mut words = left.split_whitespace();
            let Some((name, id)) = words.next().and_then(|s| s.split_once('#')) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let id = u32::from_str_radix(id, 16)?;
            if name == "vector" {
                continue;
            }
            let mut fields = vec![];
            for word in words {
                if word.starts_with('{') {
                    continue;
                }
                let Some((n, t)) = word.split_once(':') else {
                    continue;
                };
                let (flag, t) = if let Some((f, t)) = t.split_once('?') {
                    let (f, b) = f.split_once('.').context("flag")?;
                    (Some((f.to_owned(), b.parse()?)), t)
                } else {
                    (None, t)
                };
                let t = if core && t == "string" && n != "error_message" {
                    "bytes"
                } else {
                    t
                };
                fields.push(Field {
                    name: n.into(),
                    ty: t.trim_start_matches('!').into(),
                    flag,
                });
            }
            self.names.insert(name.into(), id);
            self.ids.insert(
                id,
                Constructor {
                    id,
                    name: name.into(),
                    fields,
                },
            );
        }
        Ok(())
    }
    pub fn encode(&self, v: &Value) -> Result<Vec<u8>> {
        let mut b = vec![];
        self.write("Object", v, &mut b, 0)?;
        Ok(b)
    }
    pub fn decode(&self, b: &[u8]) -> Result<Value> {
        let mut r = Reader::new(b);
        let v = self.read("Object", &mut r, 0)?;
        ensure!(r.pos == b.len(), "trailing TL bytes");
        Ok(v)
    }
    pub fn read_object(&self, r: &mut Reader) -> Result<Value> {
        self.read("Object", r, 0)
    }
    fn read(&self, t: &str, r: &mut Reader, depth: usize) -> Result<Value> {
        ensure!(depth < 64, "TL nesting limit");
        Ok(match t {
            "#" | "int" => json!(r.int()?),
            "long" => json!(r.long()?),
            "double" => json!(f64::from_le_bytes(r.take(8)?.try_into()?)),
            "int128" => json!(r.take(16)?),
            "int256" => json!(r.take(32)?),
            "bytes" => json!(r.bytes()?),
            "string" => json!(String::from_utf8(r.bytes()?)?),
            "true" => json!(true),
            _ if t.starts_with("Vector<") || t.starts_with("vector<") => {
                if t.starts_with('V') {
                    ensure!(r.int()? as u32 == 0x1cb5c415, "vector constructor")
                }
                let n = r.int()?;
                ensure!((0..=100000).contains(&n), "vector limit");
                let inner = &t[7..t.len() - 1];
                let mut a = vec![];
                for _ in 0..n {
                    a.push(self.read(inner, r, depth + 1)?)
                }
                json!(a)
            }
            _ => {
                let c = if let Some(id) = self.names.get(t) {
                    self.ids.get(id).unwrap()
                } else {
                    let id = r.int()? as u32;
                    self.ids
                        .get(&id)
                        .with_context(|| format!("unknown TL constructor {id:08x}"))?
                };
                if c.name == "boolTrue" {
                    return Ok(json!(true));
                }
                if c.name == "boolFalse" {
                    return Ok(json!(false));
                }
                let mut v = json!({"_":c.name});
                for f in &c.fields {
                    if let Some((ref name, bit)) = f.flag
                        && (v[name].as_i64().unwrap_or(0) as u32) & (1 << bit) == 0
                    {
                        continue;
                    }
                    let x = self.read(&f.ty, r, depth + 1)?;
                    v[&f.name] = x;
                }
                v
            }
        })
    }
    fn write(&self, t: &str, v: &Value, b: &mut Vec<u8>, depth: usize) -> Result<()> {
        ensure!(depth < 64, "TL nesting limit");
        match t {
            "#" | "int" => b.extend_from_slice(&(v.as_i64().context("int")? as i32).to_le_bytes()),
            "long" => b.extend_from_slice(&v.as_i64().context("long")?.to_le_bytes()),
            "double" => b.extend_from_slice(&v.as_f64().context("double")?.to_le_bytes()),
            "int128" | "int256" => {
                let x = bytes(v)?;
                ensure!(
                    x.len() == if t == "int128" { 16 } else { 32 },
                    "fixed integer size"
                );
                b.extend(x)
            }
            "bytes" => put_bytes(b, &bytes(v)?)?,
            "string" => put_bytes(b, v.as_str().context("string")?.as_bytes())?,
            "true" => {}
            _ if t.starts_with("Vector<") || t.starts_with("vector<") => {
                let a = v.as_array().context("vector")?;
                ensure!(a.len() <= 100000, "vector limit");
                if t.starts_with('V') {
                    b.extend_from_slice(&0x1cb5c415u32.to_le_bytes())
                }
                b.extend_from_slice(&(a.len() as i32).to_le_bytes());
                for x in a {
                    self.write(&t[7..t.len() - 1], x, b, depth + 1)?
                }
            }
            _ => {
                let name = if v == true {
                    "boolTrue"
                } else if v == false {
                    "boolFalse"
                } else {
                    v["_"].as_str().context("constructor name")?
                };
                let c = self
                    .ids
                    .get(
                        self.names
                            .get(name)
                            .with_context(|| format!("unknown constructor {name}"))?,
                    )
                    .unwrap();
                if !self.names.contains_key(t) {
                    b.extend_from_slice(&c.id.to_le_bytes())
                }
                let mut flags: HashMap<&str, u32> = HashMap::new();
                for f in &c.fields {
                    if let Some((ref flag, bit)) = f.flag
                        && !v[&f.name].is_null()
                        && (f.ty != "true" || v[&f.name] == true)
                    {
                        *flags.entry(flag).or_default() |= 1 << bit;
                    }
                }
                for f in &c.fields {
                    if f.ty == "#" {
                        b.extend_from_slice(
                            &flags
                                .get(f.name.as_str())
                                .copied()
                                .unwrap_or(0)
                                .to_le_bytes(),
                        );
                        continue;
                    }
                    if let Some((ref flag, bit)) = f.flag
                        && flags.get(flag.as_str()).copied().unwrap_or(0) & (1 << bit) == 0
                    {
                        continue;
                    }
                    self.write(&f.ty, &v[&f.name], b, depth + 1)
                        .with_context(|| format!("{}.{}", c.name, f.name))?;
                }
            }
        }
        Ok(())
    }
    pub fn defaults(&self, name: &str) -> Result<Value> {
        let c = self
            .ids
            .get(self.names.get(name).context("constructor")?)
            .unwrap();
        let mut v = json!({"_":name});
        for f in &c.fields {
            if f.flag.is_some() || f.ty == "#" {
                continue;
            }
            v[&f.name] = match f.ty.as_str() {
                "int" | "long" => json!(0),
                "double" => json!(0.0),
                "string" => json!(""),
                "bytes" => json!([]),
                "Bool" => json!(false),
                t if t.starts_with("Vector<") => json!([]),
                _ => bail!("no default for {name}.{}", f.name),
            }
        }
        Ok(v)
    }
}
