//! Game files built into the executable by `build.rs`.

static PACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded.bin"));

/// Every embedded file as (archive path, contents). Empty when the build
/// found no disk images.
pub fn files() -> Vec<(String, &'static [u8])> {
    parse(PACK).unwrap_or_default()
}

fn parse(d: &'static [u8]) -> Option<Vec<(String, &'static [u8])>> {
    let rest = d.strip_prefix(b"SKIDPAK1")?;
    let (count, mut rest) = rest.split_at_checked(4)?;
    let count = u32::from_le_bytes(count.try_into().ok()?);
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (len, r) = rest.split_at_checked(2)?;
        let (name, r) = r.split_at_checked(u16::from_le_bytes(len.try_into().ok()?) as usize)?;
        let (len, r) = r.split_at_checked(4)?;
        let (data, r) = r.split_at_checked(u32::from_le_bytes(len.try_into().ok()?) as usize)?;
        out.push((String::from_utf8_lossy(name).into_owned(), data));
        rest = r;
    }
    Some(out)
}
