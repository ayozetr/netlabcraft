//! Pictures in a lab: the background of the logical workspace and of every place (the Intercity
//! map, a city, a building, a wiring closet), the icon a place wears on the map above it, and
//! the size of a place in metres.
//!
//! The pictures live in one store, [`Images`], keyed by the hash of their bytes, so a picture used
//! twice is kept once. The store holds only PNG and JPEG, already checked and limited by whoever
//! put them in (the engine decodes, strips and downscales; see `netlabcraft_engine::images`); the
//! model enforces the limits again ([`MAX_IMAGE_BYTES`], [`MAX_IMAGE_SIDE`], [`MAX_IMAGES`],
//! [`MAX_LAB_IMAGE_BYTES`]) and drops anything in a hostile file that breaks them or whose key is
//! not the hash of its bytes. A lab file never names a path: the bytes are inside it.
//!
//! A place's size gives its map a scale, in metres per workspace unit. The scale replaces the
//! built-in one ([`CITY_UNIT_M`] on a city, [`INTERCITY_UNIT_M`] on the Intercity map) wherever the
//! lab turns a distance on that map into metres: cable lengths, the reach of a cell tower, the
//! range of a Wi-Fi radio and of the smart things' proximity sensors.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use netlabcraft_wire::sha1::sha1;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::physical::{CITY_UNIT_M, INTERCITY_UNIT_M, MAX_PLACES};
use crate::{ModelError, Result, Topology};

/// Largest stored picture, in bytes.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Largest total of all stored pictures, in bytes.
pub const MAX_LAB_IMAGE_BYTES: usize = 32 * 1024 * 1024;
/// Longest side of a stored picture, in pixels.
pub const MAX_IMAGE_SIDE: u32 = 4096;
/// Most pictures in a lab.
pub const MAX_IMAGES: usize = 64;
/// Largest custom width or height of a background, in workspace units.
pub const MAX_CUSTOM: u32 = 32_768;
/// Largest size of a place, in metres (20 000 km, the longest cable).
pub const MAX_CONTAINER_M: f32 = 20_000_000.0;
/// The part of a map a background that resizes to fit covers, in workspace units: the map area
/// of a place does not change with the window, so a size in metres means the same thing on every
/// screen.
pub const VIEW_W: f32 = 1000.0;
pub const VIEW_H: f32 = 700.0;
/// The Intercity map has no place number of its own: 0 stands for it.
pub const INTERCITY: u32 = 0;

/// What a stored picture is encoded as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    /// The kind the first bytes announce (`None`: neither).
    pub fn sniff(bytes: &[u8]) -> Option<ImageKind> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(ImageKind::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(ImageKind::Jpeg)
        } else {
            None
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            ImageKind::Png => "png",
            ImageKind::Jpeg => "jpeg",
        }
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 with padding (RFC 4648 section 4).
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= c.len() {
                out.push(char::from(B64.get(((n >> (18 - 6 * i)) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes base64, ignoring white space and padding; `None` for anything that is not base64 or
/// that would decode to more than `max` bytes.
pub fn base64_decode(text: &str, max: usize) -> Option<Vec<u8>> {
    let mut table = [255u8; 256];
    for (i, c) in B64.iter().enumerate() {
        if let Some(slot) = table.get_mut(usize::from(*c)) {
            *slot = i as u8;
        }
    }
    let digits = text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').count();
    if digits / 4 * 3 > max.saturating_add(3) {
        return None;
    }
    let mut out = Vec::with_capacity(digits * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        if c.is_ascii_whitespace() || c == b'=' {
            continue;
        }
        let v = u32::from(table.get(usize::from(c)).copied().unwrap_or(255));
        if v == 255 {
            return None;
        }
        acc = ((acc << 6) | v) & 0x00ff_ffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    (out.len() <= max).then_some(out)
}

/// The key of a picture: the SHA-1 of its bytes, in lower-case hexadecimal.
pub fn content_id(bytes: &[u8]) -> String {
    sha1(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

mod b64 {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Arc<Vec<u8>>, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(v))
    }

    /// A bad string reads as no bytes, and [`Topology::sanitize`] drops the picture, so one corrupt
    /// entry never stops a lab from opening.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Arc<Vec<u8>>, D::Error> {
        let text = String::deserialize(d)?;
        Ok(Arc::new(base64_decode(&text, MAX_IMAGE_BYTES).unwrap_or_default()))
    }
}

/// One stored picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredImage {
    pub kind: ImageKind,
    pub w: u32,
    pub h: u32,
    /// The encoded file (base64 in the lab file).
    #[serde(with = "b64")]
    pub data: Arc<Vec<u8>>,
}

/// The pictures of a lab, by content id.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Images(pub BTreeMap<String, StoredImage>);

impl Images {
    pub fn is_default(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&StoredImage> {
        self.0.get(id)
    }

    /// The bytes held, all pictures together.
    pub fn total_bytes(&self) -> usize {
        self.0.values().fold(0usize, |n, i| n.saturating_add(i.data.len()))
    }

    /// Stores a picture (a second copy of one already held is not stored again) and returns its id.
    /// `w` and `h` are the size the caller read from the picture; they only describe it.
    pub fn add(&mut self, kind: ImageKind, w: u32, h: u32, data: Vec<u8>) -> Result<String> {
        if w == 0 || h == 0 || w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE {
            return Err(ModelError::Invalid(format!("a picture is 1 to {MAX_IMAGE_SIDE} pixels on each side")));
        }
        if data.is_empty() || data.len() > MAX_IMAGE_BYTES {
            return Err(ModelError::Invalid(format!("a picture is at most {} MiB", MAX_IMAGE_BYTES / (1024 * 1024))));
        }
        if ImageKind::sniff(&data) != Some(kind) {
            return Err(ModelError::Invalid("the bytes are not the kind of picture they are said to be".into()));
        }
        let id = content_id(&data);
        if self.0.contains_key(&id) {
            return Ok(id);
        }
        if self.0.len() >= MAX_IMAGES {
            return Err(ModelError::Invalid(format!("a lab holds at most {MAX_IMAGES} pictures")));
        }
        if self.total_bytes().saturating_add(data.len()) > MAX_LAB_IMAGE_BYTES {
            return Err(ModelError::Invalid(format!("the pictures of a lab take at most {} MiB together", MAX_LAB_IMAGE_BYTES / (1024 * 1024))));
        }
        self.0.insert(id.clone(), StoredImage { kind, w, h, data: Arc::new(data) });
        Ok(id)
    }
}

/// How a background picture is laid on its map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BgMode {
    /// Its own size, once, with its corner at the map's origin.
    #[default]
    Original,
    /// Its own size, repeated over the whole map.
    Tiled,
    /// Stretched over the map area ([`VIEW_W`] by [`VIEW_H`]).
    Fit,
    /// A width of `custom` units, the height following the picture's proportions.
    Width,
    /// A height of `custom` units, the width following the picture's proportions.
    Height,
}

impl BgMode {
    pub const ALL: [BgMode; 5] = [BgMode::Original, BgMode::Tiled, BgMode::Fit, BgMode::Width, BgMode::Height];

    pub fn id(self) -> &'static str {
        match self {
            BgMode::Original => "original",
            BgMode::Tiled => "tiled",
            BgMode::Fit => "fit",
            BgMode::Width => "width",
            BgMode::Height => "height",
        }
    }

    pub fn from_id(s: &str) -> Option<BgMode> {
        BgMode::ALL.into_iter().find(|m| m.id() == s.trim().to_ascii_lowercase())
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A background picture and how it is laid out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Background {
    /// The picture's id in the lab's [`Images`].
    pub image: String,
    #[serde(default)]
    pub mode: BgMode,
    /// The width or height asked for by [`BgMode::Width`] and [`BgMode::Height`].
    #[serde(default, skip_serializing_if = "is_zero")]
    pub custom: u32,
}

impl Background {
    /// The size one copy of the picture is drawn at, in workspace units, for a picture of
    /// `iw` by `ih` pixels.
    pub fn shown_size(&self, iw: u32, ih: u32) -> (f32, f32) {
        let (w, h) = (iw.max(1) as f32, ih.max(1) as f32);
        match self.mode {
            BgMode::Original | BgMode::Tiled => (w, h),
            BgMode::Fit => (VIEW_W, VIEW_H),
            BgMode::Width => {
                let c = self.custom.clamp(1, MAX_CUSTOM) as f32;
                (c, c * h / w)
            }
            BgMode::Height => {
                let c = self.custom.clamp(1, MAX_CUSTOM) as f32;
                (c * w / h, c)
            }
        }
    }
}

/// The size of a place in metres, and the scale of its map.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub length: f32,
    pub height: f32,
    /// Metres per workspace unit on the place's map.
    pub scale: f32,
}

impl Size {
    /// Cubic metres.
    pub fn volume(&self) -> f64 {
        f64::from(self.width) * f64::from(self.length) * f64::from(self.height)
    }

    fn sane(&self) -> bool {
        [self.width, self.length, self.height].iter().all(|v| v.is_finite() && *v > 0.0 && *v <= MAX_CONTAINER_M)
            && self.scale.is_finite()
            && self.scale > 0.0
            && self.scale <= MAX_CONTAINER_M
    }
}

/// What a place (or the Intercity map, place 0) shows and is.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Scene {
    /// The place number; [`INTERCITY`] (0) for the Intercity map.
    #[serde(default)]
    pub place: u32,
    /// The picture under the places, devices and cables of the map inside this place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
    /// The picture this place wears on the map above it, instead of the drawn city, building or closet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Its size in metres and the scale of its map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
}

impl Scene {
    fn is_empty(&self) -> bool {
        self.background.is_none() && self.icon.is_none() && self.size.is_none()
    }
}

/// What a picture is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UseKind {
    /// The background of the logical workspace.
    Logical,
    /// The background of a place's map.
    Background,
    /// The icon of a place.
    Icon,
}

impl UseKind {
    pub fn id(self) -> &'static str {
        match self {
            UseKind::Logical => "logical",
            UseKind::Background => "background",
            UseKind::Icon => "icon",
        }
    }

    pub fn from_id(s: &str) -> Option<UseKind> {
        [UseKind::Logical, UseKind::Background, UseKind::Icon].into_iter().find(|k| k.id() == s.trim().to_ascii_lowercase())
    }
}

/// One place a picture is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageUse {
    pub kind: UseKind,
    /// The place (0 for the Intercity map, and for the logical workspace).
    pub place: u32,
    pub image: String,
}

impl Topology {
    /// What a place shows (place 0: the Intercity map).
    pub fn scene(&self, place: u32) -> Option<&Scene> {
        self.physical.scenes.iter().find(|s| s.place == place)
    }

    fn place_known(&self, place: u32) -> bool {
        place == INTERCITY || self.place(place).is_some()
    }

    fn scene_entry(&mut self, place: u32) -> Result<&mut Scene> {
        if place != INTERCITY {
            self.materialize_places();
        }
        if !self.place_known(place) {
            return Err(ModelError::Invalid(format!("no place {place}")));
        }
        if self.scene(place).is_none() {
            if self.physical.scenes.len() > MAX_PLACES {
                return Err(ModelError::Invalid("too many places with a picture".into()));
            }
            self.physical.scenes.push(Scene { place, ..Scene::default() });
        }
        self.physical.scenes.iter_mut().find(|s| s.place == place).ok_or_else(|| ModelError::Invalid(format!("no place {place}")))
    }

    fn check_image(&self, id: &str) -> Result<()> {
        if self.images.get(id).is_some() { Ok(()) } else { Err(ModelError::Invalid(format!("the lab holds no picture `{id}`"))) }
    }

    fn check_background(&self, bg: &Background) -> Result<()> {
        self.check_image(&bg.image)?;
        if matches!(bg.mode, BgMode::Width | BgMode::Height) && !(1..=MAX_CUSTOM).contains(&bg.custom) {
            return Err(ModelError::Invalid(format!("a custom width or height is 1 to {MAX_CUSTOM}")));
        }
        Ok(())
    }

    /// Sets (with `None` clears) the background of a place's map.
    pub fn set_place_background(&mut self, place: u32, bg: Option<Background>) -> Result<()> {
        if let Some(b) = &bg {
            self.check_background(b)?;
        }
        self.scene_entry(place)?.background = bg;
        self.tidy_scenes();
        Ok(())
    }

    /// Sets (with `None` clears) the icon a place wears on the map above it.
    pub fn set_place_icon(&mut self, place: u32, icon: Option<String>) -> Result<()> {
        if place == INTERCITY {
            return Err(ModelError::Invalid("the Intercity map is not on a map above it, so it has no icon".into()));
        }
        if let Some(i) = &icon {
            self.check_image(i)?;
        }
        self.scene_entry(place)?.icon = icon;
        self.tidy_scenes();
        Ok(())
    }

    /// Sets (with `None` clears) the size of a place.
    pub fn set_place_size(&mut self, place: u32, size: Option<Size>) -> Result<()> {
        if let Some(z) = &size
            && !z.sane()
        {
            return Err(ModelError::Invalid(format!("a size is a number of metres above 0 and up to {MAX_CONTAINER_M}")));
        }
        self.scene_entry(place)?.size = size;
        self.tidy_scenes();
        Ok(())
    }

    /// Sets (with `None` clears) the background of the logical workspace.
    pub fn set_logical_background(&mut self, bg: Option<Background>) -> Result<()> {
        if let Some(b) = &bg {
            self.check_background(b)?;
        }
        self.background = bg;
        self.gc_images();
        Ok(())
    }

    /// Metres per workspace unit on the map of a place (0: the Intercity map): the scale its size
    /// gives, else the built-in one.
    pub fn map_scale(&self, place: u32) -> f32 {
        match self.scene(place).and_then(|s| s.size) {
            Some(z) if z.sane() => z.scale,
            _ if place == INTERCITY => INTERCITY_UNIT_M,
            _ => CITY_UNIT_M,
        }
    }

    /// Whether the map of a place has a scale of its own (its size was set).
    pub fn map_scale_is_custom(&self, place: u32) -> bool {
        self.scene(place).and_then(|s| s.size).is_some_and(|z| z.sane())
    }

    /// Where every picture of the lab is used: the logical workspace first, then the places in
    /// the order they were given a picture.
    pub fn image_uses(&self) -> Vec<ImageUse> {
        let mut out = Vec::new();
        if let Some(b) = &self.background {
            out.push(ImageUse { kind: UseKind::Logical, place: INTERCITY, image: b.image.clone() });
        }
        for s in &self.physical.scenes {
            if let Some(b) = &s.background {
                out.push(ImageUse { kind: UseKind::Background, place: s.place, image: b.image.clone() });
            }
            if let Some(i) = &s.icon {
                out.push(ImageUse { kind: UseKind::Icon, place: s.place, image: i.clone() });
            }
        }
        out
    }

    /// Stops using a picture in one place; the picture itself goes when nothing else uses it.
    pub fn clear_image_use(&mut self, kind: UseKind, place: u32) -> Result<()> {
        match kind {
            UseKind::Logical => self.set_logical_background(None),
            UseKind::Background => self.set_place_background(place, None),
            UseKind::Icon => self.set_place_icon(place, None),
        }
    }

    /// Drops scenes that say nothing, and pictures nothing uses.
    fn tidy_scenes(&mut self) {
        self.physical.scenes.retain(|s| !s.is_empty());
        self.gc_images();
    }

    /// Drops the pictures nothing uses.
    pub fn gc_images(&mut self) {
        let used: BTreeSet<String> = self.image_uses().into_iter().map(|u| u.image).collect();
        self.images.0.retain(|id, _| used.contains(id));
    }

    /// Drops the scenes of places that are gone.
    pub(crate) fn prune_scenes(&mut self) {
        let ids: BTreeSet<u32> = self.places().iter().map(|p| p.id).collect();
        self.physical.scenes.retain(|s| s.place == INTERCITY || ids.contains(&s.place));
        self.tidy_scenes();
    }

    /// Repairs the pictures of a hostile file: bad or oversized pictures, keys that are not the
    /// hash of the bytes, references to pictures or places that are not there.
    pub(crate) fn sanitize_backdrops(&mut self) {
        let mut total = 0usize;
        let mut keep = BTreeMap::new();
        for (id, img) in std::mem::take(&mut self.images.0) {
            let ok = !img.data.is_empty()
                && img.data.len() <= MAX_IMAGE_BYTES
                && (1..=MAX_IMAGE_SIDE).contains(&img.w)
                && (1..=MAX_IMAGE_SIDE).contains(&img.h)
                && ImageKind::sniff(&img.data) == Some(img.kind)
                && content_id(&img.data) == id;
            if !ok || keep.len() >= MAX_IMAGES || total.saturating_add(img.data.len()) > MAX_LAB_IMAGE_BYTES {
                continue;
            }
            total += img.data.len();
            keep.insert(id, img);
        }
        self.images = Images(keep);

        let clean = |bg: &mut Option<Background>, images: &Images| {
            if bg.as_ref().is_some_and(|b| images.get(&b.image).is_none()) {
                *bg = None;
            } else if let Some(b) = bg {
                b.custom = if matches!(b.mode, BgMode::Width | BgMode::Height) { b.custom.clamp(1, MAX_CUSTOM) } else { 0 };
            }
        };
        clean(&mut self.background, &self.images);

        let ids: BTreeSet<u32> = self.places().iter().map(|p| p.id).collect();
        let mut seen = BTreeSet::new();
        self.physical.scenes.truncate(MAX_PLACES + 1);
        let images = &self.images;
        self.physical.scenes.retain(|s| (s.place == INTERCITY || ids.contains(&s.place)) && seen.insert(s.place));
        for s in &mut self.physical.scenes {
            clean(&mut s.background, images);
            if s.icon.as_ref().is_some_and(|i| images.get(i).is_none()) || s.place == INTERCITY {
                s.icon = None;
            }
            if s.size.is_some_and(|z| !z.sane()) {
                s.size = None;
            }
        }
        self.tidy_scenes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{from_json, to_json};

    /// The smallest bytes `sniff` and the store accept as a PNG (the store does not decode).
    fn png_bytes(tag: u8) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&[0, 0, 0, tag]);
        v
    }

    fn jpeg_bytes(tag: u8) -> Vec<u8> {
        vec![0xFF, 0xD8, 0xFF, 0xE0, tag, 0xFF, 0xD9]
    }

    fn lab_with_picture() -> (Topology, String) {
        let mut t = Topology::new();
        let id = t.images.add(ImageKind::Png, 40, 30, png_bytes(1)).unwrap();
        (t, id)
    }

    #[test]
    fn base64_round_trips_and_refuses_what_is_not_base64() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0u8, 255, 128, 7, 1]] {
            let text = base64_encode(data);
            assert_eq!(base64_decode(&text, 100).unwrap(), data);
        }
        assert_eq!(base64_decode("YW Jj\nZA==", 100).unwrap(), b"abcd");
        assert_eq!(base64_decode("@@@@", 100), None);
        assert_eq!(base64_decode(&base64_encode(&[1u8; 100]), 10), None);
    }

    #[test]
    fn a_picture_used_twice_is_stored_once_under_its_hash() {
        let (mut t, id) = lab_with_picture();
        assert_eq!(id.len(), 40);
        assert_eq!(id, content_id(&png_bytes(1)));
        let again = t.images.add(ImageKind::Png, 40, 30, png_bytes(1)).unwrap();
        assert_eq!(again, id);
        assert_eq!(t.images.0.len(), 1);
        let other = t.images.add(ImageKind::Jpeg, 10, 10, jpeg_bytes(2)).unwrap();
        assert_ne!(other, id);
        assert_eq!(t.images.0.len(), 2);

        // Two places and the logical workspace use the first one: still one copy.
        let city = t.default_place();
        t.set_place_background(city, Some(Background { image: id.clone(), mode: BgMode::Tiled, custom: 0 })).unwrap();
        t.set_place_icon(city, Some(id.clone())).unwrap();
        t.set_logical_background(Some(Background { image: id.clone(), mode: BgMode::Original, custom: 0 })).unwrap();
        t.gc_images();
        assert_eq!(t.images.0.len(), 1, "the unused JPEG goes");
        assert_eq!(t.image_uses().len(), 3);
        // The file holds it once.
        let text = to_json(&t).unwrap();
        assert_eq!(text.matches(&base64_encode(&png_bytes(1))).count(), 1);
    }

    #[test]
    fn the_store_enforces_its_limits() {
        let mut t = Topology::new();
        assert!(t.images.add(ImageKind::Png, 0, 5, png_bytes(1)).is_err());
        assert!(t.images.add(ImageKind::Png, MAX_IMAGE_SIDE + 1, 5, png_bytes(1)).is_err());
        assert!(t.images.add(ImageKind::Png, 5, 5, Vec::new()).is_err());
        assert!(t.images.add(ImageKind::Jpeg, 5, 5, png_bytes(1)).is_err(), "the kind must match the bytes");
        let mut big = png_bytes(1);
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        assert!(t.images.add(ImageKind::Png, 5, 5, big).is_err());
        for i in 0..MAX_IMAGES {
            let mut b = png_bytes(1);
            b.extend_from_slice(&(i as u32).to_be_bytes());
            t.images.add(ImageKind::Png, 1, 1, b).unwrap();
        }
        let mut b = png_bytes(1);
        b.extend_from_slice(&u32::MAX.to_be_bytes());
        assert!(t.images.add(ImageKind::Png, 1, 1, b).is_err(), "at most {MAX_IMAGES} pictures");
        // The total is capped too.
        let mut t = Topology::new();
        for i in 0..4u8 {
            let mut b = png_bytes(i);
            b.resize(MAX_IMAGE_BYTES, i);
            let r = t.images.add(ImageKind::Png, 1, 1, b);
            assert_eq!(r.is_ok(), i < 4, "{i}");
        }
        let mut b = png_bytes(9);
        b.resize(MAX_IMAGE_BYTES, 9);
        assert!(t.images.add(ImageKind::Png, 1, 1, b).is_err(), "32 MiB is the most");
    }

    #[test]
    fn backgrounds_icons_and_sizes_round_trip_and_older_files_open_without_them() {
        let (mut t, id) = lab_with_picture();
        let b = t.default_place();
        t.set_place_background(b, Some(Background { image: id.clone(), mode: BgMode::Width, custom: 500 })).unwrap();
        t.set_place_icon(b, Some(id.clone())).unwrap();
        t.set_place_size(b, Some(Size { width: 50.0, length: 30.0, height: 3.0, scale: 0.05 })).unwrap();
        t.set_place_size(INTERCITY, Some(Size { width: 1000.0, length: 700.0, height: 1.0, scale: 1.0 })).unwrap();
        t.set_logical_background(Some(Background { image: id, mode: BgMode::Tiled, custom: 0 })).unwrap();
        let text = to_json(&t).unwrap();
        assert_eq!(from_json(&text).unwrap(), t);
        assert!(!to_json(&Topology::new()).unwrap().contains("images"));
        // A lab saved before pictures existed has none.
        let old = r#"{"format":"netlabcraft","version":11,"topology":{"devices":[],"links":[],"next_id":1}}"#;
        let t = from_json(old).unwrap();
        assert!(t.images.is_default() && t.background.is_none() && t.physical.scenes.is_empty());
    }

    #[test]
    fn a_picture_is_set_only_when_it_is_stored_and_a_place_when_it_exists() {
        let (mut t, id) = lab_with_picture();
        let place = t.default_place();
        let bg = |image: &str, mode, custom| Some(Background { image: image.into(), mode, custom });
        assert!(t.set_place_background(place, bg("0123", BgMode::Original, 0)).is_err());
        assert!(t.set_place_background(999, bg(&id, BgMode::Original, 0)).is_err());
        assert!(t.set_place_background(place, bg(&id, BgMode::Width, 0)).is_err(), "a custom width is needed");
        assert!(t.set_place_background(place, bg(&id, BgMode::Height, MAX_CUSTOM + 1)).is_err());
        assert!(t.set_place_icon(INTERCITY, Some(id.clone())).is_err());
        let size = |w: f32, s: f32| Some(Size { width: w, length: 1.0, height: 1.0, scale: s });
        for bad in [size(0.0, 1.0), size(-1.0, 1.0), size(f32::NAN, 1.0), size(1.0, 0.0), size(1.0, f32::INFINITY), size(MAX_CONTAINER_M * 2.0, 1.0)]
        {
            assert!(t.set_place_size(place, bad).is_err());
        }
        assert!(t.physical.scenes.is_empty(), "a refused change leaves nothing behind");
        t.set_place_background(place, bg(&id, BgMode::Original, 0)).unwrap();
        t.set_place_background(place, None).unwrap();
        assert!(t.physical.scenes.is_empty() && t.images.is_default(), "an empty scene and its unused picture go");
    }

    #[test]
    fn the_shown_size_follows_the_mode() {
        let bg = |mode, custom| Background { image: String::new(), mode, custom };
        assert_eq!(bg(BgMode::Original, 0).shown_size(200, 100), (200.0, 100.0));
        assert_eq!(bg(BgMode::Tiled, 0).shown_size(200, 100), (200.0, 100.0));
        assert_eq!(bg(BgMode::Fit, 0).shown_size(200, 100), (VIEW_W, VIEW_H));
        assert_eq!(bg(BgMode::Width, 400).shown_size(200, 100), (400.0, 200.0));
        assert_eq!(bg(BgMode::Height, 400).shown_size(200, 100), (800.0, 400.0));
        assert_eq!(bg(BgMode::Width, 0).shown_size(0, 0), (1.0, 1.0));
        assert_eq!(BgMode::from_id("Tiled"), Some(BgMode::Tiled));
        assert_eq!(BgMode::from_id("sideways"), None);
    }

    #[test]
    fn a_size_gives_the_map_its_scale() {
        let (mut t, _) = lab_with_picture();
        let place = t.default_place();
        assert_eq!(t.map_scale(INTERCITY), INTERCITY_UNIT_M);
        assert_eq!(t.map_scale(place), CITY_UNIT_M);
        assert!(!t.map_scale_is_custom(place));
        t.set_place_size(place, Some(Size { width: 100.0, length: 50.0, height: 3.0, scale: 0.25 })).unwrap();
        assert_eq!(t.map_scale(place), 0.25);
        assert!(t.map_scale_is_custom(place));
        assert_eq!(t.scene(place).and_then(|s| s.size).map(|z| z.volume()), Some(15_000.0));
        // Removing the place takes the scale away.
        t.set_place_size(place, None).unwrap();
        assert_eq!(t.map_scale(place), CITY_UNIT_M);
    }

    #[test]
    fn a_scale_moves_the_distance_between_buildings_and_cities() {
        let mut t = Topology::new();
        let c1 = t.add_place(crate::physical::PlaceKind::City, Some("A"), None, 0.0, 0.0).unwrap();
        t.add_place(crate::physical::PlaceKind::City, Some("B"), None, 300.0, 400.0).unwrap();
        let b1 = t.add_place(crate::physical::PlaceKind::Building, Some("B1"), Some(c1), 0.0, 0.0).unwrap();
        let b2 = t.add_place(crate::physical::PlaceKind::Building, Some("B2"), Some(c1), 30.0, 40.0).unwrap();
        let k1 = t.add_place(crate::physical::PlaceKind::Closet, Some("K1"), Some(b1), 0.0, 0.0).unwrap();
        let k2 = t.add_place(crate::physical::PlaceKind::Closet, Some("K2"), Some(b2), 0.0, 0.0).unwrap();
        let plain = t.distance(k1, k2);
        assert_eq!(plain, 50 + crate::physical::SAME_BUILDING_M);
        // Ten times as many metres per unit on the first city's map: ten times as far.
        let img = t.images.add(ImageKind::Png, 4, 4, png_bytes(3)).unwrap();
        t.set_place_background(c1, Some(Background { image: img, mode: BgMode::Original, custom: 0 })).unwrap();
        t.set_place_size(c1, Some(Size { width: 100.0, length: 100.0, height: 10.0, scale: 10.0 })).unwrap();
        assert_eq!(t.distance(k1, k2), 500 + crate::physical::SAME_BUILDING_M);
    }

    #[test]
    fn two_devices_in_one_scaled_place_are_cabled_by_the_map() {
        use crate::{DeviceKind, End};
        let mut t = Topology::new();
        let a = t.add_device(DeviceKind::Router, None, 0.0, 0.0).unwrap();
        let b = t.add_device(DeviceKind::Switch, None, 0.0, 0.0).unwrap();
        t.connect(End { device: a, port: 0 }, End { device: b, port: 25 }).unwrap();
        t.materialize_places();
        let closet = crate::physical::DEFAULT_CLOSET;
        t.set_home(a, closet, 0.0, 0.0).unwrap();
        t.set_home(b, closet, 30.0, 40.0).unwrap();
        let link = t.links[0];
        assert_eq!(t.cable_length(&link).meters, crate::physical::SAME_PLACE_M);
        t.set_place_size(closet, Some(Size { width: 100.0, length: 70.0, height: 3.0, scale: 0.5 })).unwrap();
        assert_eq!(t.cable_length(&link).meters, 25, "50 units at half a metre each");
        t.set_place_size(closet, None).unwrap();
        assert_eq!(t.cable_length(&link).meters, crate::physical::SAME_PLACE_M);
    }

    #[test]
    fn hostile_files_lose_their_bad_pictures_and_references_but_still_open() {
        let (mut t, id) = lab_with_picture();
        let place = t.default_place();
        t.set_place_background(place, Some(Background { image: id.clone(), mode: BgMode::Tiled, custom: 0 })).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&to_json(&t).unwrap()).unwrap();

        // A key that is not the hash of the bytes, bytes that are not base64, a wrong kind, a size of 0.
        let good = v["topology"]["images"][&id].clone();
        v["topology"]["images"]["deadbeef"] = good.clone();
        let mut broken = good.clone();
        broken["data"] = serde_json::json!("@@ not base64 @@");
        v["topology"]["images"]["cafe"] = broken;
        let mut zero = good.clone();
        zero["w"] = serde_json::json!(0);
        v["topology"]["images"][&id] = zero;
        // A picture that is not there, an icon on the Intercity map, a place that is not there.
        v["topology"]["physical"]["scenes"] = serde_json::json!([
            {"place": place, "background": {"image": "nope", "mode": "fit"}, "icon": "nope"},
            {"place": 0, "icon": id, "size": {"width": -5, "length": 1, "height": 1, "scale": 1}},
            {"place": 4242, "size": {"width": 5, "length": 1, "height": 1, "scale": 1}},
        ]);
        v["topology"]["background"] = serde_json::json!({"image": "nope"});
        let t = from_json(&v.to_string()).unwrap();
        assert!(t.images.is_default(), "{:?}", t.images.0.keys().collect::<Vec<_>>());
        assert!(t.physical.scenes.is_empty() && t.background.is_none());
    }

    #[test]
    fn a_lab_full_of_pictures_is_cut_to_the_limits() {
        let mut t = Topology::new();
        let mut entries = serde_json::Map::new();
        for i in 0..(MAX_IMAGES as u32 + 20) {
            let mut b = png_bytes(1);
            b.extend_from_slice(&i.to_be_bytes());
            entries.insert(content_id(&b), serde_json::json!({"kind": "png", "w": 1, "h": 1, "data": base64_encode(&b)}));
        }
        // Each is a use, so garbage collection keeps them.
        let ids: Vec<String> = entries.keys().cloned().collect();
        t.images = serde_json::from_value(serde_json::Value::Object(entries)).unwrap();
        t.materialize_places();
        let places: Vec<u32> = t.places().iter().map(|p| p.id).collect();
        for (i, id) in ids.iter().enumerate() {
            t.physical.scenes.push(Scene { place: places.get(i).copied().unwrap_or(1000 + i as u32), icon: Some(id.clone()), ..Scene::default() });
        }
        t.sanitize_backdrops();
        assert!(t.images.0.len() <= MAX_IMAGES);
        assert!(t.physical.scenes.len() <= places.len());
    }

    #[test]
    fn removing_a_place_removes_its_scene_and_the_picture_nothing_uses() {
        let mut t = Topology::new();
        let city = t.add_place(crate::physical::PlaceKind::City, Some("Spare"), None, 0.0, 0.0).unwrap();
        t.add_place(crate::physical::PlaceKind::City, Some("Other"), None, 100.0, 0.0).unwrap();
        let img = t.images.add(ImageKind::Png, 4, 4, png_bytes(5)).unwrap();
        t.set_place_icon(city, Some(img)).unwrap();
        assert_eq!(t.image_uses().len(), 1);
        t.remove_place(city).unwrap();
        assert!(t.physical.scenes.is_empty() && t.images.is_default());
    }

    #[test]
    fn clearing_one_use_keeps_a_picture_another_use_holds() {
        let (mut t, id) = lab_with_picture();
        let place = t.default_place();
        t.set_place_background(place, Some(Background { image: id.clone(), mode: BgMode::Fit, custom: 0 })).unwrap();
        t.set_logical_background(Some(Background { image: id, mode: BgMode::Tiled, custom: 0 })).unwrap();
        t.clear_image_use(UseKind::Background, place).unwrap();
        assert_eq!(t.images.0.len(), 1);
        t.clear_image_use(UseKind::Logical, 0).unwrap();
        assert!(t.images.is_default());
        assert_eq!(UseKind::from_id("Icon"), Some(UseKind::Icon));
    }
}
