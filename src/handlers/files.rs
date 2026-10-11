use super::*;
use crate::{
    crypto::{random, random_id},
    store::File,
    tl::bytes,
};
use std::{io::Read, path::Path};
/// Parse a bounded TGS stream and validate its Lottie animation dimensions/timing.
pub fn validate_tgs(data: &[u8]) -> Result<Value> {
    ensure!(data.len() <= 64 * 1024, "TGS_TOO_LARGE");
    ensure!(data.starts_with(&[0x1f, 0x8b]), "TGS_GZIP_INVALID");
    let mut decoded = vec![];
    flate2::read::GzDecoder::new(data)
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut decoded)?;
    ensure!(
        decoded.len() <= 4 * 1024 * 1024,
        "TGS_DECOMPRESSED_TOO_LARGE"
    );
    let v: Value = serde_json::from_slice(&decoded)?;
    ensure!(
        int(&v, "w") == 512 && int(&v, "h") == 512,
        "TGS_DIMENSIONS_INVALID"
    );
    let fps = v["fr"].as_f64().context("TGS_FPS_INVALID")?;
    let start = v["ip"].as_f64().context("TGS_TIMING_INVALID")?;
    let end = v["op"].as_f64().context("TGS_TIMING_INVALID")?;
    ensure!(
        fps.is_finite()
            && (fps == 30.0 || fps == 60.0)
            && end > start
            && (end - start) / fps <= 3.0,
        "TGS_TIMING_INVALID"
    );
    ensure!(v["layers"].is_array(), "TGS_LAYERS_INVALID");
    ensure!(
        v["assets"]
            .as_array()
            .is_none_or(|a| a.iter().all(|x| x["p"].is_null())),
        "TGS_EXTERNAL_ASSET"
    );
    Ok(v)
}
fn path(dir: &Path, id: i64) -> std::path::PathBuf {
    dir.join("files").join(format!("{id}.bin"))
}
pub fn handle(s: &mut State, uid: i64, v: &Value, dir: &Path) -> Result<Value> {
    match text(v, "_") {
        "upload.saveFilePart" | "upload.saveBigFilePart" => {
            let id = int(v, "file_id");
            let part = int(v, "file_part");
            let data = bytes(&v["bytes"])?;
            ensure!(
                (0..=2047).contains(&part) && !data.is_empty() && data.len() <= 512 * 1024,
                "FILE_PART_INVALID"
            );
            let total = int(v, "file_total_parts");
            ensure!(
                total == 0 || (total > part && total <= 2048),
                "FILE_PARTS_INVALID"
            );
            let folder = dir
                .join("uploads")
                .join(uid.to_string())
                .join(id.to_string());
            std::fs::create_dir_all(&folder)?;
            let mut used = 0u64;
            for e in std::fs::read_dir(dir.join("uploads").join(uid.to_string()))? {
                for f in std::fs::read_dir(e?.path())? {
                    used = used.saturating_add(f?.metadata()?.len());
                }
            }
            ensure!(
                used + data.len() as u64 <= 1024 * 1024 * 1024,
                "UPLOAD_QUOTA_EXCEEDED"
            );
            let dest = folder.join(format!("{part}.part"));
            let temp = folder.join(format!("{part}.tmp"));
            std::fs::write(&temp, data)?;
            std::fs::rename(temp, dest)?;
            Ok(json!(true))
        }
        "upload.getFile" => {
            let loc = &v["location"];
            let id = int(loc, "id");
            let f = s.files.get(&id).context("FILE_ID_INVALID")?;
            ensure!(
                int(loc, "access_hash") == f.hash && bytes(&loc["file_reference"])? == f.reference,
                "FILE_REFERENCE_EXPIRED"
            );
            let offset = int(v, "offset");
            let limit = int(v, "limit");
            ensure!(
                offset >= 0 && (1..=1024 * 1024).contains(&limit),
                "OFFSET_INVALID"
            );
            let mut file = std::fs::File::open(path(dir, id))?;
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(offset as u64))?;
            let mut data = vec![];
            file.take(limit as u64).read_to_end(&mut data)?;
            Ok(
                json!({"_":"upload.file","type":{"_":"storage.fileUnknown"},"mtime":f.date,"bytes":data}),
            )
        }
        _ => bail!("METHOD_NOT_IMPLEMENTED"),
    }
}
pub fn materialize(s: &mut State, uid: i64, media: &Value, dir: &Path) -> Result<Value> {
    let name = text(media, "_");
    if name != "inputMediaUploadedDocument" && name != "inputMediaUploadedPhoto" {
        return Ok(media.clone());
    }

    let f = &media["file"];
    let id = int(f, "id");
    let parts = int(f, "parts");
    ensure!((1..=2048).contains(&parts), "FILE_PARTS_INVALID");
    let folder = dir
        .join("uploads")
        .join(uid.to_string())
        .join(id.to_string());
    let mut data = vec![];
    for n in 0..parts {
        let b = std::fs::read(folder.join(format!("{n}.part"))).context("FILE_PART_MISSING")?;
        ensure!(data.len() + b.len() <= 64 * 1024 * 1024, "FILE_TOO_BIG");
        data.extend(b)
    }
    let is_photo = name == "inputMediaUploadedPhoto";
    let (mime, attrs) = if is_photo {
        let (w, h, mime) = photo_size(&data)?;
        (
            mime,
            vec![json!({"_":"documentAttributeImageSize","w":w,"h":h})],
        )
    } else {
        (text(media, "mime_type"), list(media, "attributes"))
    };
    if mime == "application/x-tgsticker" || text(f, "name").ends_with(".tgs") {
        validate_tgs(&data)?;
    }
    let file_id = random_id();
    std::fs::create_dir_all(dir.join("files"))?;
    std::fs::write(path(dir, file_id), &data)?;
    s.files.insert(
        file_id,
        File {
            id: file_id,
            hash: random_id(),
            reference: random(24),
            size: data.len() as i64,
            mime: if text(f, "name").ends_with(".tgs") {
                "application/x-tgsticker".into()
            } else {
                mime.into()
            },
            attrs,
            owner: uid,
            date: now(),
        },
    );
    let f = &s.files[&file_id];
    Ok(
        json!({"_":if is_photo{"inputMediaPhoto"}else{"inputMediaDocument"},"id":{"_":if is_photo{"inputPhoto"}else{"inputDocument"},"id":file_id,"access_hash":f.hash,"file_reference":f.reference}}),
    )
}
pub fn media(s: &State, uid: i64, v: &Value) -> Result<Value> {
    match text(v, "_") {
        "messageMediaDocument" | "messageMediaPhoto" => Ok(v.clone()),
        "inputMediaPhoto" => {
            let id = int(&v["id"], "id");
            let f = s.files.get(&id).context("FILE_ID_INVALID")?;
            ensure!(
                int(&v["id"], "access_hash") == f.hash
                    && bytes(&v["id"]["file_reference"])? == f.reference,
                "FILE_REFERENCE_EXPIRED"
            );
            let attr = f
                .attrs
                .iter()
                .find(|a| a["_"] == "documentAttributeImageSize")
                .context("PHOTO_INVALID")?;
            Ok(
                json!({"_":"messageMediaPhoto","photo":{"_":"photo","id":id,"access_hash":f.hash,"file_reference":f.reference,"date":f.date,"dc_id":2,"sizes":[{"_":"photoSize","type":"x","w":attr["w"],"h":attr["h"],"size":f.size}]}}),
            )
        }
        "inputMediaDocument" => {
            let id = int(&v["id"], "id");
            let f = s.files.get(&id).context("FILE_ID_INVALID")?;
            ensure!(
                int(&v["id"], "access_hash") == f.hash
                    && bytes(&v["id"]["file_reference"])? == f.reference,
                "FILE_REFERENCE_EXPIRED"
            );
            Ok(json!({"_":"messageMediaDocument","document":s.doc(id)?}))
        }
        "inputMediaEmpty" => Ok(json!({"_":"messageMediaEmpty"})),
        _ => {
            let _ = uid;
            bail!("MEDIA_INVALID")
        }
    }
}

fn photo_size(data: &[u8]) -> Result<(u32, u32, &'static str)> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") && data.len() >= 24 {
        let w = u32::from_be_bytes(data[16..20].try_into()?);
        let h = u32::from_be_bytes(data[20..24].try_into()?);
        ensure!(w > 0 && h > 0 && w <= 16384 && h <= 16384, "PHOTO_INVALID");
        return Ok((w, h, "image/png"));
    }
    if data.starts_with(&[255, 216]) {
        let mut p = 2;
        while p + 4 <= data.len() {
            ensure!(data[p] == 255, "PHOTO_INVALID");
            let marker = data[p + 1];
            p += 2;
            if marker == 255 {
                p -= 1;
                continue;
            }
            if marker == 0xd9 || marker == 0xda {
                break;
            }
            let len = u16::from_be_bytes(data[p..p + 2].try_into()?) as usize;
            ensure!(len >= 2 && p + len <= data.len(), "PHOTO_INVALID");
            if [0xc0, 0xc1, 0xc2].contains(&marker) && len >= 7 {
                let h = u16::from_be_bytes(data[p + 3..p + 5].try_into()?) as u32;
                let w = u16::from_be_bytes(data[p + 5..p + 7].try_into()?) as u32;
                ensure!(w > 0 && h > 0 && w <= 16384 && h <= 16384, "PHOTO_INVALID");
                return Ok((w, h, "image/jpeg"));
            }
            p += len;
        }
    }
    bail!("PHOTO_INVALID")
}
